//! Song library and score store.
//!
//! Stored as JSON in `cache/songs.json` (rebuildable scan cache),
//! `scores.json` (your play records) and `player.json` (totals over all
//! plays) so it works identically on every platform, including the web
//! build. Scanning runs in the background.

use crate::platform::vfs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use vibebms_core::score::{ClearType, Score};
use vibebms_core::{LnProfile, LnType};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Song {
    pub path: String,
    /// Group folder shown in song select (root/<first sub directory>).
    pub folder: String,
    pub md5: String,
    pub sha256: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub subartist: String,
    pub genre: String,
    pub level: String,
    pub difficulty: i32,
    pub mode: String,
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub main_bpm: f64,
    /// 0 in libraries scanned before it was stored (then the main BPM).
    pub start_bpm: f64,
    pub notes: i64,
    pub length_ms: i64,
    pub total: f64,
    pub has_ln: bool,
    pub has_mine: bool,
    pub has_random: bool,
    pub has_bga: bool,
    pub has_stop: bool,
    pub stagefile: String,
    pub banner: String,
    pub preview: String,
    /// Notes by kind: normal, long, scratch, long on the scratch, mines
    /// (zeros in libraries scanned before they were stored).
    pub note_kinds: [i64; 5],
    /// Judge rank as beatoraja keeps it: `#RANK` 0..4, else a percentage.
    pub judge: Option<i64>,
    /// Notes per second: average, peak and near the end, as beatoraja
    /// measures them.
    pub density: [f64; 3],
    /// The long notes by type (`LnProfile::bits`); `None` in libraries
    /// scanned before it was stored.
    pub ln_profile: Option<u8>,
}

impl Song {
    pub fn level_num(&self) -> i64 {
        self.level
            .trim()
            .parse::<f64>()
            .map(|v| v as i64)
            .unwrap_or(0)
    }
    /// The LN mode its scores, replays and IR entries are filed under
    /// with the player's mode `pref` (`LnProfile::score_ln`).
    pub fn score_ln(&self, pref: LnType) -> LnType {
        match self.ln_profile {
            Some(b) => LnProfile::from_bits(b).score_ln(pref),
            None if self.has_ln => pref,
            None => LnType::Ln,
        }
    }

    /// Its long notes all have their type: one LN mode whatever the
    /// player's (records from before were filed under the player's).
    pub fn fixed_ln(&self) -> bool {
        self.has_ln
            && self
                .ln_profile
                .is_some_and(|b| !LnProfile::from_bits(b).undefined)
    }

    pub fn full_title(&self) -> String {
        if self.subtitle.is_empty() {
            self.title.clone()
        } else {
            format!("{} {}", self.title, self.subtitle)
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BestScore {
    pub clear: ClearType,
    pub ex_score: u32,
    pub notes: u32,
    /// [PG, GR, GD, BD, PR, MS] of the best EX play.
    pub judges: [u32; 6],
    pub max_combo: u32,
    pub min_bp: u32,
    pub play_count: u32,
    pub clear_count: u32,
    pub last_played: i64,
}

impl BestScore {
    /// Two records of the same chart and LN mode as one.
    pub fn merge(&self, o: &BestScore) -> BestScore {
        let (hi, lo) = if o.ex_score > self.ex_score {
            (o, self)
        } else {
            (self, o)
        };
        BestScore {
            clear: hi.clear.max(lo.clear),
            ex_score: hi.ex_score,
            notes: hi.notes,
            judges: hi.judges,
            max_combo: hi.max_combo.max(lo.max_combo),
            min_bp: hi.min_bp.min(lo.min_bp),
            play_count: hi.play_count + lo.play_count,
            clear_count: hi.clear_count + lo.clear_count,
            last_played: hi.last_played.max(lo.last_played),
        }
    }

    pub fn rate(&self) -> f64 {
        if self.notes == 0 {
            0.0
        } else {
            self.ex_score as f64 / (self.notes as f64 * 2.0)
        }
    }
}

/// The player's totals over every recorded play (as beatoraja's
/// `PlayerData`; skin numbers 17-19, 33-37 and 333). Plays and clears are
/// the sums of the charts' counts instead.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Totals {
    /// [PG, GR, GD, BD, PR, MS] of every play. Missing in files from
    /// before vibe kept them: then seeded once from the best plays, so
    /// plays from before count with their best play only.
    pub judges: Option<[u64; 6]>,
    /// Time played (ms): up to the last judgement of each play, counted
    /// since vibe keeps it.
    pub play_ms: u64,
}

impl Totals {
    pub fn judge(&self, j: usize) -> u64 {
        self.judges.map_or(0, |v| v[j])
    }

    /// Notes hit (PG to BD), beatoraja's "total notes played".
    pub fn notes(&self) -> u64 {
        (0..4).map(|j| self.judge(j)).sum()
    }
}

#[derive(Serialize, Deserialize)]
struct SongRecord {
    #[serde(flatten)]
    song: Song,
    mtime: i64,
    size: i64,
}

#[derive(Serialize, Deserialize)]
struct ScoreRecord {
    sha256: String,
    ln: i32,
    #[serde(flatten)]
    best: BestScore,
}

pub struct Library {
    songs_file: PathBuf,
    scores_file: PathBuf,
    scores: HashMap<(String, i32), BestScore>,
    totals_file: PathBuf,
    totals: Totals,
}

fn load_songs(file: &Path) -> Vec<SongRecord> {
    vfs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The score records, one by one: a record that does not read is dropped,
/// and the file is copied to `scores.json.bak` first (the next save
/// rewrites it).
fn load_scores(file: &Path) -> Vec<ScoreRecord> {
    let Ok(b) = vfs::read(file) else {
        return Vec::new();
    };
    let (records, bad) = match serde_json::from_slice::<Vec<serde_json::Value>>(&b) {
        Ok(list) => {
            let n = list.len();
            let ok: Vec<ScoreRecord> = list
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect();
            let bad = n - ok.len();
            (ok, bad)
        }
        Err(_) => (Vec::new(), 1),
    };
    if bad > 0 {
        let bak = file.with_extension("json.bak");
        log::error!(
            "{}: {bad} unreadable record(s); the file is saved as {}",
            file.display(),
            bak.display()
        );
        let _ = vfs::write(&bak, &b);
    }
    records
}

impl Library {
    /// `cache` holds the scan cache, `data` the score file.
    pub fn open(cache: &Path, data: &Path) -> Library {
        let scores_file = data.join("scores.json");
        let scores: HashMap<_, _> = load_scores(&scores_file)
            .into_iter()
            .map(|r| ((r.sha256, r.ln), r.best))
            .collect();
        let totals_file = data.join("player.json");
        let mut totals: Totals = vfs::read(&totals_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if totals.judges.is_none() {
            let mut j = [0u64; 6];
            for b in scores.values() {
                for (t, n) in j.iter_mut().zip(b.judges) {
                    *t += n as u64;
                }
            }
            totals.judges = Some(j);
        }
        Library {
            songs_file: cache.join("songs.json"),
            scores_file,
            scores,
            totals_file,
            totals,
        }
    }

    pub fn totals(&self) -> &Totals {
        &self.totals
    }

    pub fn songs_file(&self) -> &Path {
        &self.songs_file
    }

    pub fn songs(&self) -> Vec<Song> {
        load_songs(&self.songs_file)
            .into_iter()
            .map(|r| r.song)
            .collect()
    }

    /// Best scores keyed by `(sha256, ln_mode)`.
    pub fn scores(&self) -> HashMap<(String, i32), BestScore> {
        self.scores.clone()
    }

    fn save_scores(&self) {
        let mut v: Vec<ScoreRecord> = self
            .scores
            .iter()
            .map(|((s, l), b)| ScoreRecord {
                sha256: s.clone(),
                ln: *l,
                best: b.clone(),
            })
            .collect();
        v.sort_by(|a, b| a.sha256.cmp(&b.sha256).then(a.ln.cmp(&b.ln)));
        match serde_json::to_vec_pretty(&v) {
            Ok(j) => {
                if let Err(e) = vfs::write(&self.scores_file, &j) {
                    log::error!("saving scores: {e}");
                }
            }
            Err(e) => log::error!("saving scores: {e}"),
        }
    }

    /// Records of charts whose long notes all have their type, filed
    /// under the player's LN mode before vibe filed them under the
    /// chart's: merged into the chart's (they were played as it).
    pub fn merge_fixed_ln(&mut self, songs: &[Song]) {
        let mut changed = false;
        for song in songs.iter().filter(|s| s.fixed_ln()) {
            let key = song.score_ln(LnType::Ln) as i32;
            for other in (0..3).filter(|k| *k != key) {
                let Some(old) = self.scores.remove(&(song.sha256.clone(), other)) else {
                    continue;
                };
                let k = (song.sha256.clone(), key);
                let merged = match self.scores.get(&k) {
                    Some(b) => b.merge(&old),
                    None => old,
                };
                self.scores.insert(k, merged);
                changed = true;
            }
        }
        if changed {
            self.save_scores();
        }
    }

    /// An assisted play (`AssistFacts`): its lamp, the play and clear
    /// counts and the totals, but no best score, combo or BP.
    pub fn record_assisted(
        &mut self,
        sha256: &str,
        ln: i32,
        s: &Score,
        clear: ClearType,
        play_ms: u64,
    ) -> BestScore {
        let key = (sha256.to_string(), ln);
        let mut b = self.scores.get(&key).cloned().unwrap_or(BestScore {
            min_bp: u32::MAX,
            ..Default::default()
        });
        b.clear = b.clear.max(clear);
        b.play_count += 1;
        b.clear_count += (clear > ClearType::Failed) as u32;
        b.last_played = crate::platform::unix_now() as i64;
        self.scores.insert(key, b.clone());
        self.save_scores();
        self.add_totals(s, play_ms);
        b
    }

    fn add_totals(&mut self, s: &Score, play_ms: u64) {
        let j = self.totals.judges.get_or_insert_default();
        for (k, t) in j.iter_mut().enumerate() {
            *t += s.count(k) as u64;
        }
        self.totals.play_ms += play_ms;
        match serde_json::to_vec_pretty(&self.totals) {
            Ok(v) => {
                if let Err(e) = vfs::write(&self.totals_file, &v) {
                    log::error!("saving totals: {e}");
                }
            }
            Err(e) => log::error!("saving totals: {e}"),
        }
    }

    /// Merge a play into the best score and the totals (`play_ms`: how
    /// long it was played). Returns the updated best.
    pub fn record_play(
        &mut self,
        sha256: &str,
        ln: i32,
        s: &Score,
        clear: ClearType,
        play_ms: u64,
    ) -> BestScore {
        let key = (sha256.to_string(), ln);
        let old = self.scores.get(&key).cloned();
        let mut b = old.clone().unwrap_or(BestScore {
            min_bp: u32::MAX,
            ..Default::default()
        });
        if s.ex_score() > b.ex_score || old.is_none() {
            b.ex_score = s.ex_score();
            b.judges = std::array::from_fn(|j| s.count(j));
        }
        b.clear = b.clear.max(clear);
        b.notes = s.total_notes;
        b.max_combo = b.max_combo.max(s.max_combo);
        b.min_bp = b.min_bp.min(s.miss_count());
        b.play_count += 1;
        b.clear_count += (clear > ClearType::Failed) as u32;
        b.last_played = crate::platform::unix_now() as i64;
        self.scores.insert(key, b.clone());
        self.save_scores();
        self.add_totals(s, play_ms);
        b
    }
}

#[derive(Default)]
pub struct ScanProgress {
    pub found: AtomicUsize,
    pub parsed: AtomicUsize,
    pub to_parse: AtomicUsize,
    pub done: AtomicBool,
    pub changed: AtomicBool,
    pub current: Mutex<String>,
}

fn is_chart(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            vibebms_core::CHART_EXTENSIONS
                .iter()
                .any(|x| x.eq_ignore_ascii_case(e))
        })
        .unwrap_or(false)
}

/// Chart files under `dir` into `out`; folders that cannot be read (a
/// drive not mounted, no permission) into `unreadable`.
fn walk(
    dir: &Path,
    depth: usize,
    skip: &HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
    unreadable: &mut Vec<PathBuf>,
) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = vfs::read_dir(dir) else {
        unreadable.push(dir.to_path_buf());
        return;
    };
    for e in entries {
        let name = e
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if e.is_dir {
            if !skip.contains(&e.path) {
                walk(&e.path, depth + 1, skip, out, unreadable);
            }
        } else if is_chart(&e.path) {
            out.push(e.path);
        }
    }
}

/// Chart files under `roots` (used by the web build to prefetch them).
pub fn find_charts(roots: &[PathBuf], skip: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for r in roots {
        walk(r, 0, skip, &mut v, &mut Vec::new());
    }
    v
}

/// Notes by kind and the densities of a chart (see `Song`). Densities
/// follow beatoraja's `SongInformation`: a second's count includes the
/// long notes held through it; the average leaves out the quietest
/// seconds; the end is the busiest 5 s after the gauge could last fill.
pub fn chart_info(c: &vibebms_core::Chart) -> ([i64; 5], [f64; 3]) {
    let mode = c.mode();
    let scratch = |l: u8| (0..mode.players()).any(|p| mode.scratch_lane(p) == Some(l as usize));
    let mut kinds = [0i64; 5];
    kinds[4] = c
        .notes
        .iter()
        .filter(|n| matches!(n.kind, vibebms_core::chart::NoteKind::Mine { .. }))
        .count() as i64;
    let notes: Vec<_> = c.notes.iter().filter(|n| n.is_playable()).collect();
    let last = notes.iter().map(|n| n.end_time()).max().unwrap_or(0).max(0);
    // a chart beatoraja would not analyse (over an hour): no densities
    if last > 3_600_000_000 {
        return (kinds, [0.0; 3]);
    }
    let mut secs = vec![0i64; (last / 1_000_000 + 2) as usize];
    let total = c.total(vibebms_core::LnType::Ln);
    let mut border = if total > 0.0 {
        (notes.len() as f64 * (1.0 - 100.0 / total)) as i64
    } else {
        0
    };
    let mut border_pos = 0usize;
    for n in &notes {
        let s = (n.time.max(0) / 1_000_000) as usize;
        kinds[usize::from(n.is_long()) + 2 * usize::from(scratch(n.lane))] += 1;
        let e = if n.is_long() {
            ((n.end_time().max(0) / 1_000_000) as usize).min(secs.len() - 1)
        } else {
            s
        };
        for v in &mut secs[s..=e.max(s)] {
            *v += 1;
        }
        border -= 1;
        if border == 0 {
            border_pos = s;
        }
    }
    let bd = notes.len() as i64 / secs.len() as i64 / 4;
    let peak = secs.iter().copied().max().unwrap_or(0) as f64;
    let busy: Vec<i64> = secs.iter().copied().filter(|v| *v >= bd).collect();
    let avg = if busy.is_empty() {
        0.0
    } else {
        busy.iter().sum::<i64>() as f64 / busy.len() as f64
    };
    let d = 5.min(secs.len().saturating_sub(border_pos + 1));
    let end = if d > 0 {
        (border_pos..secs.len() - d)
            .map(|i| secs[i..i + d].iter().sum::<i64>() as f64 / d as f64)
            .fold(0.0, f64::max)
    } else {
        0.0
    };
    (kinds, [avg, peak, end])
}

pub fn song_from_chart(path: &Path, folder: String, c: &vibebms_core::Chart) -> Song {
    let m = &c.meta;
    let (note_kinds, density) = chart_info(c);
    Song {
        note_kinds,
        density,
        judge: Some(crate::scene::skinned::judge_rank_number(m)),
        ln_profile: Some(c.ln_profile().bits()),
        path: path.to_string_lossy().into_owned(),
        folder,
        md5: m.md5.clone(),
        sha256: m.sha256.clone(),
        title: m.title.clone(),
        subtitle: m.subtitle.clone(),
        artist: m.artist.clone(),
        subartist: m.subartist.clone(),
        genre: m.genre.clone(),
        level: m.level.clone(),
        difficulty: m.difficulty,
        mode: c.mode().label().to_string(),
        min_bpm: m.min_bpm,
        max_bpm: m.max_bpm,
        main_bpm: m.main_bpm,
        start_bpm: m.initial_bpm,
        notes: c.note_count() as i64,
        length_ms: c.length() / 1000,
        total: c.total(vibebms_core::LnType::Ln),
        has_ln: m.has_long,
        has_mine: m.has_mine,
        has_random: m.has_random,
        has_bga: m.has_bga,
        has_stop: m.has_stop,
        stagefile: m.stagefile.clone(),
        banner: m.banner.clone(),
        preview: m.preview.clone(),
    }
}

/// Scan `roots` and update the song cache. Runs on a background thread.
pub fn scan(
    songs_file: PathBuf,
    roots: Vec<PathBuf>,
    skip: HashSet<PathBuf>,
    progress: Arc<ScanProgress>,
) {
    let known_records = load_songs(&songs_file);
    // charts read before the library kept their judge rank and note
    // kinds, or the types of their long notes, are read again (once)
    let known: HashMap<String, (i64, i64)> = known_records
        .iter()
        .filter(|r| r.song.judge.is_some() && (!r.song.has_ln || r.song.ln_profile.is_some()))
        .map(|r| (r.song.path.clone(), (r.mtime, r.size)))
        .collect();
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    for root in &roots {
        *progress.current.lock().unwrap() = root.display().to_string();
        let mut found = Vec::new();
        walk(root, 0, &skip, &mut found, &mut unreadable);
        for f in found {
            let first = f
                .strip_prefix(root)
                .ok()
                .and_then(|r| r.components().next())
                .map(|c| root.join(c.as_os_str()));
            let folder = match first {
                Some(p) if p != f => p.to_string_lossy().into_owned(),
                _ => root.to_string_lossy().into_owned(),
            };
            files.push((f, folder));
        }
        progress.found.store(files.len(), Ordering::Relaxed);
    }
    let mut seen: HashSet<String> = files
        .iter()
        .map(|(f, _)| f.to_string_lossy().into_owned())
        .collect();
    // charts in a folder that cannot be read now (a drive not mounted) are
    // kept, not removed with their records
    for r in &known_records {
        if unreadable
            .iter()
            .any(|d| Path::new(&r.song.path).starts_with(d))
        {
            seen.insert(r.song.path.clone());
        }
    }
    if !unreadable.is_empty() {
        log::warn!("scan: cannot read {}", unreadable[0].display());
    }
    let todo: Vec<(PathBuf, String, (i64, i64))> = files
        .into_iter()
        .filter_map(|(f, folder)| {
            let stamp = vfs::stamp(&f);
            let key = f.to_string_lossy().into_owned();
            (known.get(&key) != Some(&stamp)).then_some((f, folder, stamp))
        })
        .collect();
    progress.to_parse.store(todo.len(), Ordering::Relaxed);

    let results: Mutex<Vec<SongRecord>> = Mutex::new(Vec::new());
    crate::platform::parallel_for(todo.len(), |i| {
        let (f, folder, stamp) = &todo[i];
        let opts = vibebms_core::bms::ParseOptions {
            random_seed: Some(0),
            ..Default::default()
        };
        // a parser bug on one file skips that file
        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            vfs::read(f).map_err(|e| e.to_string()).and_then(|b| {
                vibebms_core::load_chart_bytes(f, &b, &opts).map_err(|e| e.to_string())
            })
        }))
        .unwrap_or_else(|_| Err("the parser panicked".into()));
        match parsed {
            Ok(c) if c.note_count() > 0 => {
                results.lock().unwrap().push(SongRecord {
                    song: song_from_chart(f, folder.clone(), &c),
                    mtime: stamp.0,
                    size: stamp.1,
                });
            }
            Ok(_) => {}
            Err(e) => log::debug!("skip {}: {e}", f.display()),
        }
        progress.parsed.fetch_add(1, Ordering::Relaxed);
    });
    let new = results.into_inner().unwrap();
    let removed = known.keys().filter(|k| !seen.contains(*k)).count();
    let changed = !new.is_empty() || removed > 0;
    if changed {
        let updated: HashSet<String> = new.iter().map(|r| r.song.path.clone()).collect();
        let mut all: Vec<SongRecord> = known_records
            .into_iter()
            .filter(|r| seen.contains(&r.song.path) && !updated.contains(&r.song.path))
            .collect();
        all.extend(new);
        all.sort_by(|a, b| a.song.path.cmp(&b.song.path));
        match serde_json::to_vec(&all) {
            Ok(j) => {
                if let Err(e) = vfs::write(&songs_file, &j) {
                    log::error!("saving song cache: {e}");
                }
            }
            Err(e) => log::error!("saving song cache: {e}"),
        }
        log::info!(
            "scan finished: {} charts ({} updated, {removed} removed)",
            all.len(),
            updated.len()
        );
    } else {
        log::info!("scan finished: {} charts, no changes", seen.len());
    }
    progress.changed.store(changed, Ordering::Release);
    progress.done.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {

    #[test]
    fn chart_info_counts() {
        // 120 BPM: a measure is 2 s. Scratch note, two keys, a 4 s LN.
        let bms = "#PLAYER 1\n#BPM 120\n#TOTAL 200\n#WAV01 a.wav\n\
            #00116:01\n#00111:0001\n#00112:01\n#00153:01\n#00353:01\n";
        let c = vibebms_core::bms::parse_bms(bms.as_bytes(), "bms", &Default::default()).unwrap();
        let (k, d) = chart_info(&c);
        assert_eq!(k, [2, 1, 1, 0, 0]);
        // the LN holds through seconds 2..=6; second 2 has two more notes
        assert_eq!(d[1], 3.0);
    }

    use super::*;

    #[test]
    fn totals_seeded_then_added() {
        let dir = std::env::temp_dir().join(format!("vibebms-totals-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // a library from before totals: two charts' best plays
        let rec = |sha: &str, j: [u32; 6]| serde_json::json!({"sha256": sha, "ln": 0, "judges": j, "play_count": 3});
        let old = serde_json::json!([rec("a", [10, 5, 2, 1, 3, 4]), rec("b", [1, 1, 1, 1, 1, 1])]);
        std::fs::write(dir.join("scores.json"), old.to_string()).unwrap();
        std::fs::write(dir.join("player.json"), r#"{"play_ms": 5000}"#).unwrap();
        let mut lib = Library::open(&dir, &dir);
        assert_eq!(lib.totals().judges, Some([11, 6, 3, 2, 4, 5]));
        assert_eq!((lib.totals().notes(), lib.totals().play_ms), (22, 5000));
        let mut sc = Score {
            total_notes: 3,
            passed: 3,
            ..Default::default()
        };
        sc.add(0, 0);
        sc.add(0, 1);
        sc.add(4, 0);
        sc.add(5, 0);
        // a better play of "a": the best changes, the totals only add up
        lib.record_play("a", 0, &sc, ClearType::Easy, 61_000);
        let want = Totals {
            judges: Some([13, 6, 3, 2, 5, 6]),
            play_ms: 66_000,
        };
        assert_eq!(*lib.totals(), want);
        // kept, not seeded again
        assert_eq!(*Library::open(&dir, &dir).totals(), want);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn records_of_typed_long_notes_merge_into_the_charts_mode() {
        let dir = std::env::temp_dir().join(format!("vibebms-lnkey-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rec = |ln: i32, ex: u32, clear: &str, plays: u32| {
            serde_json::json!({"sha256": "c", "ln": ln, "ex_score": ex, "clear": clear,
                "play_count": plays, "min_bp": 9})
        };
        let old = serde_json::json!([rec(0, 100, "Hard", 2), rec(2, 120, "Easy", 1)]);
        std::fs::write(dir.join("scores.json"), old.to_string()).unwrap();
        let mut lib = Library::open(&dir, &dir);
        // #LNMODE 2: every long note a CN
        let cn = Song {
            sha256: "c".into(),
            has_ln: true,
            ln_profile: Some(
                LnProfile {
                    undefined: false,
                    typed: [false, true, false],
                }
                .bits(),
            ),
            ..Default::default()
        };
        assert_eq!(cn.score_ln(LnType::Hcn), LnType::Cn);
        lib.merge_fixed_ln(std::slice::from_ref(&cn));
        let s = Library::open(&dir, &dir).scores();
        assert_eq!(s.len(), 1);
        let b = &s[&("c".to_string(), 1)];
        assert_eq!(
            (b.ex_score, b.clear, b.play_count),
            (120, ClearType::Hard, 3)
        );
        // a chart whose long notes take the player's mode keeps them apart
        let free = Song {
            sha256: "c".into(),
            has_ln: true,
            ln_profile: Some(
                LnProfile {
                    undefined: true,
                    typed: [false; 3],
                }
                .bits(),
            ),
            ..Default::default()
        };
        assert!(!free.fixed_ln());
        assert_eq!(free.score_ln(LnType::Hcn), LnType::Hcn);
        // no long notes: LN
        assert_eq!(Song::default().score_ln(LnType::Cn), LnType::Ln);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_score_records_are_kept_aside() {
        let dir = std::env::temp_dir().join(format!("vibebms-scores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("scores.json");
        std::fs::write(&file, r#"[{"sha256": "a", "ln": 0}, {"sha256": 5}]"#).unwrap();
        assert_eq!(load_scores(&file).len(), 1);
        assert!(dir.join("scores.json.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_and_scores() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-{}", std::process::id()));
        let songs = dir.join("pack").join("song");
        std::fs::create_dir_all(&songs).unwrap();
        std::fs::write(
            songs.join("a.bme"),
            "#TITLE A\n#PLAYLEVEL 5\n#BPM 150\n#00111:01\n#00118:01\n",
        )
        .unwrap();
        std::fs::write(songs.join("readme.txt"), "x").unwrap();
        let cache = dir.join("cache");
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.clone()],
            [cache.clone()].into_iter().collect(),
            p.clone(),
        );
        assert!(p.done.load(Ordering::Acquire));
        let mut lib = Library::open(&cache, &cache);
        let s = lib.songs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "A");
        assert_eq!(s[0].mode, "7KEYS");
        assert!(s[0].folder.ends_with("pack"));
        // rescan is incremental
        let p2 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.clone()],
            [cache.clone()].into_iter().collect(),
            p2.clone(),
        );
        assert_eq!(p2.to_parse.load(Ordering::Relaxed), 0);
        assert!(!p2.changed.load(Ordering::Relaxed));

        let mut sc = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        sc.add(0, 1);
        sc.add(1, 1);
        let b = lib.record_play(&s[0].sha256, 0, &sc, ClearType::Hard, 1500);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 1));
        let worse = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        let b = lib.record_play(&s[0].sha256, 0, &worse, ClearType::Failed, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 2));
        // an assisted play: lamp and counts only, never the best score
        let mut great = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        great.add(0, 1);
        great.add(0, 1);
        let b = lib.record_assisted(&s[0].sha256, 0, &great, ClearType::AssistEasy, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 3));
        let b = lib.record_assisted("other", 0, &great, ClearType::LightAssistEasy, 0);
        assert_eq!((b.ex_score, b.min_bp, b.max_combo), (0, u32::MAX, 0));
        assert_eq!((b.clear, b.clear_count), (ClearType::LightAssistEasy, 1));
        // persisted
        let lib2 = Library::open(&cache, &cache);
        assert_eq!(lib2.scores().len(), 2);
        // the library folder cannot be read (here: gone): its charts stay
        let moved = dir.with_extension("away");
        std::fs::rename(&dir, &moved).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::copy(moved.join("cache/songs.json"), cache.join("songs.json")).unwrap();
        let p3 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.join("pack")],
            [cache.clone()].into_iter().collect(),
            p3.clone(),
        );
        assert!(!p3.changed.load(Ordering::Relaxed));
        assert_eq!(Library::open(&cache, &cache).songs().len(), 1);
        // and go once the folder is there without them
        std::fs::create_dir_all(dir.join("pack")).unwrap();
        let p4 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.join("pack")],
            [cache.clone()].into_iter().collect(),
            p4.clone(),
        );
        assert_eq!(Library::open(&cache, &cache).songs().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&moved).ok();
    }
}
