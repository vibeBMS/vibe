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
use vibebms_core::judge::Judge;
use vibebms_core::score::{ClearType, Score};
use vibebms_core::{LnProfile, LnType, PlayMode};

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
    #[serde(with = "vibebms_core::by_label")]
    pub mode: PlayMode,
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
    /// Not a playable chart (it does not parse, or has no notes) to this
    /// version of the game: kept so that it is read again only when it
    /// changes or another version scans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    skipped_by: Option<String>,
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
            .filter(|r| r.skipped_by.is_none())
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
        for (t, k) in j.iter_mut().zip(Judge::ALL) {
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
            b.judges = Judge::ALL.map(|j| s.count(j));
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

/// Notes by kind and the densities of a chart (see `Song`).
///
/// Kinds: normal notes and long notes off the scratch lanes, the same on
/// them, mines. Densities (notes per second): the average over the
/// seconds that are not much quieter than the chart's mean, the busiest
/// second, and the busiest stretch of up to 5 s from the point where the
/// notes left could still fill the gauge from empty. A second's count
/// includes the long notes held through it. Charts over an hour get no
/// densities.
///
/// Written from a behaviour specification, not from another program's
/// code.
pub fn chart_info(c: &vibebms_core::Chart) -> ([i64; 5], [f64; 3]) {
    use vibebms_core::chart::{LnType, NoteKind};
    const SEC: i64 = 1_000_000;

    let mode = c.mode();
    let lanes = mode.lane_count();
    let is_scratch = |lane: u8| (lane as usize) < lanes && mode.is_scratch(lane as usize);
    let mut kinds = [0i64; 5];
    for n in &c.notes {
        let slot = match n.kind {
            NoteKind::Normal => 0,
            NoteKind::Long { .. } => 1,
            NoteKind::Mine { .. } => {
                kinds[4] += 1;
                continue;
            }
            NoteKind::Invisible => continue,
        };
        kinds[slot + if is_scratch(n.lane) { 2 } else { 0 }] += 1;
    }

    let mut playable: Vec<&vibebms_core::Note> =
        c.notes.iter().filter(|n| n.is_playable()).collect();
    playable.sort_by_key(|n| (n.time, n.lane));
    let n_notes = playable.len() as i64;
    let last = playable
        .iter()
        .map(|n| n.end_time())
        .max()
        .unwrap_or(0)
        .max(0);
    if n_notes == 0 || last > 3_600 * SEC {
        return (kinds, [0.0; 3]);
    }

    let second = |t: i64| t.max(0) / SEC;
    let buckets_len = second(last) + 2;
    let mut buckets = vec![0i64; buckets_len as usize];
    for n in &playable {
        let from = second(n.time);
        let to = second(n.end_time()).min(buckets_len - 1).max(from);
        for b in &mut buckets[from as usize..=to as usize] {
            *b += 1;
        }
    }

    let threshold = n_notes / buckets_len / 4;
    let busy: Vec<i64> = buckets
        .iter()
        .copied()
        .filter(|b| *b >= threshold)
        .collect();
    let average = if busy.is_empty() {
        0.0
    } else {
        busy.iter().sum::<i64>() as f64 / busy.len() as f64
    };
    let peak = buckets.iter().copied().max().unwrap_or(0) as f64;

    let total = c.total(LnType::Ln);
    let k = if total > 0.0 {
        (n_notes as f64 * (1.0 - 100.0 / total)) as i64
    } else {
        0
    };
    let fill = if k >= 1 {
        second(playable[(k.min(n_notes) - 1) as usize].time)
    } else {
        0
    };
    let width = 5.min(buckets_len - 1 - fill);
    let end = if width > 0 {
        (fill..=buckets_len - 1 - width)
            .map(|s| {
                buckets[s as usize..(s + width) as usize]
                    .iter()
                    .sum::<i64>()
            })
            .max()
            .map_or(0.0, |sum| sum as f64 / width as f64)
    } else {
        0.0
    };
    (kinds, [average, peak, end])
}

pub fn song_from_chart(path: &Path, folder: String, c: &vibebms_core::Chart) -> Song {
    let m = &c.meta;
    let (note_kinds, density) = chart_info(c);
    Song {
        note_kinds,
        density,
        judge: Some(crate::scene::skinvals::judge_rank_number(m)),
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
        mode: c.mode(),
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

/// The game's version, which a chart it could not read is filed under.
const VERSION: &str = env!("CARGO_PKG_VERSION");

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
        .filter(|r| match &r.skipped_by {
            Some(v) => v == VERSION,
            None => r.song.judge.is_some() && (!r.song.has_ln || r.song.ln_profile.is_some()),
        })
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
        let opts = vibebms_core::bms::ParseOptions::default();
        // (a file that cannot be read now is tried again next time)
        let parsed = vfs::read(f).map(|b| {
            // a parser bug on one file skips that file
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                vibebms_core::load_chart_bytes(f, &b, &opts).map_err(|e| e.to_string())
            }))
            .unwrap_or_else(|_| Err("the parser panicked".into()))
        });
        let entry = |song, skipped_by| SongRecord {
            song,
            mtime: stamp.0,
            size: stamp.1,
            skipped_by,
        };
        let record = match parsed {
            Ok(Ok(c)) if c.note_count() > 0 => {
                Some(entry(song_from_chart(f, folder.clone(), &c), None))
            }
            Ok(r) => {
                if let Err(e) = r {
                    log::debug!("skip {}: {e}", f.display());
                }
                let song = Song {
                    path: f.to_string_lossy().into_owned(),
                    folder: folder.clone(),
                    ..Song::default()
                };
                Some(entry(song, Some(VERSION.to_string())))
            }
            Err(e) => {
                log::debug!("skip {}: {e}", f.display());
                None
            }
        };
        if let Some(r) = record {
            results.lock().unwrap().push(r);
        }
        progress.parsed.fetch_add(1, Ordering::Relaxed);
    });
    let new = results.into_inner().unwrap();
    // (records read again count too: their file may be gone)
    let removed = known_records
        .iter()
        .filter(|r| !seen.contains(&r.song.path))
        .count();
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
            all.iter().filter(|r| r.skipped_by.is_none()).count(),
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
        // buckets [0, 0, 3, 2, 1, 1, 1, 0]: all count (threshold 0); the
        // gauge could fill after the 2nd note (second 2): 8/5
        assert_eq!(d, [1.0, 3.0, 1.6]);
    }

    use super::*;
    use vibebms_core::chart::{Chart, ChartMeta, LnType, Note, NoteKind};
    use vibebms_core::PlayMode;

    fn info_note(lane: u8, time: i64, kind: NoteKind) -> Note {
        Note {
            lane,
            time,
            beat: 0.0,
            pos: 0.0,
            sound: 0,
            kind,
        }
    }

    fn info_long(lane: u8, time: i64, end_time: i64, ln_type: Option<LnType>) -> Note {
        info_note(
            lane,
            time,
            NoteKind::Long {
                end_time,
                end_beat: 0.0,
                end_pos: 0.0,
                end_sound: 0,
                ln_type,
            },
        )
    }

    fn info_chart(mode: PlayMode, total: Option<f64>, mut notes: Vec<Note>) -> Chart {
        notes.sort_by_key(|n| (n.time, n.lane));
        Chart {
            meta: ChartMeta {
                mode: Some(mode),
                total,
                ..Default::default()
            },
            notes,
            ..Default::default()
        }
    }

    /// The spec's larger example: 48 notes (two long notes adding a count
    /// to the next second each), buckets [0, 2, 6, 8, 7, 1, 0, 3, 9, 10,
    /// 4, 0], TOTAL 175 (the 20th note, in second 4, is where the gauge
    /// could fill).
    #[test]
    fn chart_info_synthetic() {
        let mut notes = Vec::new();
        let normals = [
            (1, 2),
            (2, 6),
            (3, 8),
            (4, 7),
            (5, 1),
            (7, 3),
            (8, 8),
            (9, 8),
            (10, 3),
        ];
        for (sec, n) in normals {
            for k in 0..n {
                notes.push(info_note(
                    (k % 7) as u8,
                    sec * 1_000_000 + k * 100_000,
                    NoteKind::Normal,
                ));
            }
        }
        notes.push(info_long(0, 8_950_000, 9_500_000, None));
        notes.push(info_long(1, 9_950_000, 10_500_000, None));
        notes.push(info_note(2, 6_500_000, NoteKind::Mine { damage: 1.0 }));
        let c = info_chart(PlayMode::Beat7K, Some(175.0), notes);
        let (k, d) = chart_info(&c);
        assert_eq!(k, [46, 2, 0, 0, 1]);
        assert!((d[0] - 50.0 / 9.0).abs() < 1e-9, "{d:?}");
        assert_eq!(d[1], 10.0);
        assert!((d[2] - 5.2).abs() < 1e-9, "{d:?}");
        // TOTAL 100 or less: from second 0, every 5-second window counts
        let mut low = c.clone();
        low.meta.total = Some(100.0);
        assert!((chart_info(&low).1[2] - 5.2).abs() < 1e-9);
        // an unset TOTAL is beatoraja's default for the judgement count
        // (here 260: k = trunc(48 × (1 − 100/260)) = 29; the 29th note is in
        // second 8; three buckets are left before the last: 23 / 3)
        let mut def = c.clone();
        def.meta.total = None;
        assert!((chart_info(&def).1[2] - 23.0 / 3.0).abs() < 1e-9);
    }

    /// The one-hour guard limits only the densities: all five kinds are
    /// counted.
    #[test]
    fn chart_info_counts_kinds_past_an_hour() {
        let c = info_chart(
            PlayMode::Beat14K,
            None,
            vec![
                info_note(7, 1_000_000, NoteKind::Normal),
                info_long(15, 2_000_000, 3_000_000, Some(LnType::Cn)),
                info_long(8, 3_000_000, 4_000_000, None),
                info_note(4, 5_000_000, NoteKind::Mine { damage: 5.0 }),
                info_note(0, 3_600_000_001, NoteKind::Normal),
            ],
        );
        assert_eq!(chart_info(&c), ([1, 1, 1, 1, 1], [0.0; 3]));
    }

    /// Kinds by scratch lane (any player's), mines; negative times in
    /// second 0; over an hour no densities; no notes nothing.
    #[test]
    fn chart_info_edges() {
        let c = info_chart(
            PlayMode::Beat14K,
            Some(300.0),
            vec![
                info_note(7, -2_000_000, NoteKind::Normal),
                info_note(15, 500_000, NoteKind::Normal),
                info_long(15, 600_000, 2_400_000, Some(LnType::Cn)),
                info_long(8, 700_000, 900_000, None),
                info_note(3, 800_000, NoteKind::Invisible),
                info_note(4, 800_000, NoteKind::Mine { damage: 5.0 }),
            ],
        );
        let (k, d) = chart_info(&c);
        assert_eq!(k, [0, 1, 2, 1, 1]);
        // buckets [4, 1, 1, 0]: threshold ⌊⌊4/4⌋/4⌋ = 0
        assert_eq!(d[0], 6.0 / 4.0);
        assert_eq!(d[1], 4.0);
        let mut long = c.clone();
        long.notes
            .push(info_note(0, 3_600_000_001, NoteKind::Normal));
        // over an hour: no densities (the arrays per second would be too
        // long), but the kinds are counted
        assert_eq!(chart_info(&long).1, [0.0; 3]);
        let mut hour = c.clone();
        hour.notes
            .push(info_note(0, 3_600_000_000, NoteKind::Normal));
        assert!(chart_info(&hour).1[1] > 0.0);
        let empty = info_chart(
            PlayMode::Beat7K,
            None,
            vec![info_note(1, 1_000_000, NoteKind::Mine { damage: 1.0 })],
        );
        assert_eq!(chart_info(&empty), ([0, 0, 0, 0, 1], [0.0; 3]));
    }

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
        sc.add(Judge::PGreat, 0);
        sc.add(Judge::PGreat, 1);
        sc.add(Judge::Poor, 0);
        sc.add(Judge::Miss, 0);
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
        // no notes: not a song, and not read again while it stays so
        std::fs::write(songs.join("empty.bme"), "#TITLE E\n").unwrap();
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
        assert_eq!(s[0].mode, PlayMode::Beat7K);
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
        sc.add(Judge::PGreat, 1);
        sc.add(Judge::Great, 1);
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
        great.add(Judge::PGreat, 1);
        great.add(Judge::PGreat, 1);
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

    #[test]
    fn old_records_of_deleted_charts_go() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-old-{}", std::process::id()));
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        // a record from before judge ranks were kept (read again when
        // seen), its chart deleted since
        let gone = songs.join("gone.bme").to_string_lossy().into_owned();
        let old = serde_json::json!([{ "path": gone, "title": "Gone", "mtime": 1, "size": 1 }]);
        std::fs::write(cache.join("songs.json"), old.to_string()).unwrap();
        assert_eq!(Library::open(&cache, &cache).songs().len(), 1);
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![songs],
            [cache.clone()].into_iter().collect(),
            p.clone(),
        );
        assert!(p.changed.load(Ordering::Relaxed));
        assert!(Library::open(&cache, &cache).songs().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
