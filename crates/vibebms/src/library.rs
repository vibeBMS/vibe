//! Song library and score store.
//!
//! Stored as JSON in `cache/songs.json` (rebuildable scan cache) and
//! `scores.json` (your play records) so it works identically on every
//! platform, including the web build. Scanning runs in the background.

use crate::platform::vfs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use vibebms_core::score::{ClearType, Score};

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
}

impl Song {
    pub fn level_num(&self) -> i64 {
        self.level
            .trim()
            .parse::<f64>()
            .map(|v| v as i64)
            .unwrap_or(0)
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
    pub fn rate(&self) -> f64 {
        if self.notes == 0 {
            0.0
        } else {
            self.ex_score as f64 / (self.notes as f64 * 2.0)
        }
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
}

fn load_songs(file: &Path) -> Vec<SongRecord> {
    vfs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

impl Library {
    /// `cache` holds the scan cache, `data` the score file.
    pub fn open(cache: &Path, data: &Path) -> Library {
        let scores_file = data.join("scores.json");
        let scores = vfs::read(&scores_file)
            .ok()
            .and_then(|b| {
                serde_json::from_slice::<Vec<ScoreRecord>>(&b)
                    .map_err(|e| log::error!("scores.json: {e}"))
                    .ok()
            })
            .unwrap_or_default()
            .into_iter()
            .map(|r| ((r.sha256, r.ln), r.best))
            .collect();
        Library {
            songs_file: cache.join("songs.json"),
            scores_file,
            scores,
        }
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

    /// Merge a play into the best score. Returns the updated best.
    pub fn record_play(&mut self, sha256: &str, ln: i32, s: &Score, clear: ClearType) -> BestScore {
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

fn walk(dir: &Path, depth: usize, skip: &HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = vfs::read_dir(dir) else {
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
                walk(&e.path, depth + 1, skip, out);
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
        walk(r, 0, skip, &mut v);
    }
    v
}

pub fn song_from_chart(path: &Path, folder: String, c: &vibebms_core::Chart) -> Song {
    let m = &c.meta;
    Song {
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
    let known: HashMap<String, (i64, i64)> = known_records
        .iter()
        .map(|r| (r.song.path.clone(), (r.mtime, r.size)))
        .collect();
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    for root in &roots {
        *progress.current.lock().unwrap() = root.display().to_string();
        let mut found = Vec::new();
        walk(root, 0, &skip, &mut found);
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
    let seen: HashSet<String> = files
        .iter()
        .map(|(f, _)| f.to_string_lossy().into_owned())
        .collect();
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
        match vfs::read(f)
            .map_err(|e| e.to_string())
            .and_then(|b| vibebms_core::load_chart_bytes(f, &b, &opts).map_err(|e| e.to_string()))
        {
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
    use super::*;

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
        let b = lib.record_play(&s[0].sha256, 0, &sc, ClearType::Hard);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 1));
        let worse = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        let b = lib.record_play(&s[0].sha256, 0, &worse, ClearType::Failed);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 2));
        // persisted
        let lib2 = Library::open(&cache, &cache);
        assert_eq!(lib2.scores().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
