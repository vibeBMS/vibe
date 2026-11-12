//! Song library (SQLite in `cache/library.db`) and background scanning.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use vibebms_core::score::{ClearType, Score};

#[derive(Clone, Debug, Default)]
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

#[derive(Clone, Debug, Default)]
pub struct BestScore {
    pub clear: ClearType,
    pub ex_score: u32,
    pub notes: u32,
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

pub struct Library {
    conn: Connection,
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS song(
  path TEXT PRIMARY KEY, folder TEXT, md5 TEXT, sha256 TEXT, title TEXT, subtitle TEXT, artist TEXT,
  subartist TEXT, genre TEXT, level TEXT, difficulty INTEGER, mode TEXT, min_bpm REAL, max_bpm REAL,
  main_bpm REAL, notes INTEGER, length_ms INTEGER, total REAL, flags INTEGER, stagefile TEXT, banner TEXT,
  preview TEXT, mtime INTEGER, size INTEGER);
CREATE INDEX IF NOT EXISTS song_md5 ON song(md5);
CREATE INDEX IF NOT EXISTS song_sha ON song(sha256);
CREATE TABLE IF NOT EXISTS score(
  sha256 TEXT, ln INTEGER, clear INTEGER, ex INTEGER, notes INTEGER, pg INTEGER, gr INTEGER, gd INTEGER,
  bd INTEGER, pr INTEGER, ms INTEGER, max_combo INTEGER, min_bp INTEGER, play_count INTEGER,
  clear_count INTEGER, last_played INTEGER, PRIMARY KEY(sha256, ln));
";

fn flags(s: &Song) -> i64 {
    (s.has_ln as i64)
        | (s.has_mine as i64) << 1
        | (s.has_random as i64) << 2
        | (s.has_bga as i64) << 3
        | (s.has_stop as i64) << 4
}

impl Library {
    pub fn open(path: &Path) -> rusqlite::Result<Library> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Library { conn })
    }

    pub fn songs(&self) -> Vec<Song> {
        let mut st = match self.conn.prepare(
            "SELECT path,folder,md5,sha256,title,subtitle,artist,subartist,genre,level,difficulty,mode,min_bpm,max_bpm,main_bpm,notes,length_ms,total,flags,stagefile,banner,preview FROM song",
        ) {
            Ok(s) => s,
            Err(e) => {
                log::error!("library: {e}");
                return Vec::new();
            }
        };
        let rows = st.query_map([], |r| {
            let f: i64 = r.get(18)?;
            Ok(Song {
                path: r.get(0)?,
                folder: r.get(1)?,
                md5: r.get(2)?,
                sha256: r.get(3)?,
                title: r.get(4)?,
                subtitle: r.get(5)?,
                artist: r.get(6)?,
                subartist: r.get(7)?,
                genre: r.get(8)?,
                level: r.get(9)?,
                difficulty: r.get(10)?,
                mode: r.get(11)?,
                min_bpm: r.get(12)?,
                max_bpm: r.get(13)?,
                main_bpm: r.get(14)?,
                notes: r.get(15)?,
                length_ms: r.get(16)?,
                total: r.get(17)?,
                has_ln: f & 1 != 0,
                has_mine: f & 2 != 0,
                has_random: f & 4 != 0,
                has_bga: f & 8 != 0,
                has_stop: f & 16 != 0,
                stagefile: r.get(19)?,
                banner: r.get(20)?,
                preview: r.get(21)?,
            })
        });
        match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Best scores keyed by `(sha256, ln_mode)`.
    pub fn scores(&self) -> HashMap<(String, i32), BestScore> {
        let mut m = HashMap::new();
        let Ok(mut st) = self.conn.prepare("SELECT sha256,ln,clear,ex,notes,max_combo,min_bp,play_count,clear_count,last_played FROM score") else { return m };
        let rows = st.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, i32>(1)?),
                BestScore {
                    clear: ClearType::from_id(r.get(2)?),
                    ex_score: r.get(3)?,
                    notes: r.get(4)?,
                    max_combo: r.get(5)?,
                    min_bp: r.get(6)?,
                    play_count: r.get(7)?,
                    clear_count: r.get(8)?,
                    last_played: r.get(9)?,
                },
            ))
        });
        if let Ok(rows) = rows {
            for (k, v) in rows.flatten() {
                m.insert(k, v);
            }
        }
        m
    }

    /// Merge a play into the best score. Returns the updated best.
    pub fn record_play(&self, sha256: &str, ln: i32, s: &Score, clear: ClearType) -> BestScore {
        let old: Option<BestScore> = self
            .conn
            .query_row(
                "SELECT clear,ex,notes,max_combo,min_bp,play_count,clear_count,last_played FROM score WHERE sha256=?1 AND ln=?2",
                params![sha256, ln],
                |r| {
                    Ok(BestScore {
                        clear: ClearType::from_id(r.get(0)?),
                        ex_score: r.get(1)?,
                        notes: r.get(2)?,
                        max_combo: r.get(3)?,
                        min_bp: r.get(4)?,
                        play_count: r.get(5)?,
                        clear_count: r.get(6)?,
                        last_played: r.get(7)?,
                    })
                },
            )
            .optional()
            .ok()
            .flatten();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let cleared = clear > ClearType::Failed;
        let mut b = old.clone().unwrap_or(BestScore {
            min_bp: u32::MAX,
            ..Default::default()
        });
        let better_ex = s.ex_score() > b.ex_score || old.is_none();
        b.clear = b.clear.max(clear);
        b.ex_score = b.ex_score.max(s.ex_score());
        b.notes = s.total_notes;
        b.max_combo = b.max_combo.max(s.max_combo);
        b.min_bp = b.min_bp.min(s.miss_count());
        b.play_count += 1;
        b.clear_count += cleared as u32;
        b.last_played = now;
        let c = |j: usize| s.count(j);
        let res = if better_ex || old.is_none() {
            self.conn.execute(
                "INSERT OR REPLACE INTO score VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![sha256, ln, b.clear as i64, b.ex_score, b.notes, c(0), c(1), c(2), c(3), c(4), c(5), b.max_combo, b.min_bp, b.play_count, b.clear_count, now],
            )
        } else {
            self.conn.execute(
                "UPDATE score SET clear=?3,max_combo=?4,min_bp=?5,play_count=?6,clear_count=?7,last_played=?8 WHERE sha256=?1 AND ln=?2",
                params![sha256, ln, b.clear as i64, b.max_combo, b.min_bp, b.play_count, b.clear_count, now],
            )
        };
        if let Err(e) = res {
            log::error!("saving score: {e}");
        }
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() || (ft.is_symlink() && p.is_dir()) {
            if !skip.contains(&p) {
                walk(&p, depth + 1, skip, out);
            }
        } else if is_chart(&p) {
            out.push(p);
        }
    }
}

fn file_stamp(p: &Path) -> (i64, i64) {
    std::fs::metadata(p)
        .map(|m| {
            let mt = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (mt, m.len() as i64)
        })
        .unwrap_or((0, 0))
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

/// Scan `roots` and update the database. Runs on a background thread.
pub fn scan(
    db_path: PathBuf,
    roots: Vec<PathBuf>,
    skip: HashSet<PathBuf>,
    progress: Arc<ScanProgress>,
) {
    let lib = match Library::open(&db_path) {
        Ok(l) => l,
        Err(e) => {
            log::error!("scan: cannot open db: {e}");
            progress.done.store(true, Ordering::Release);
            return;
        }
    };
    let mut known: HashMap<String, (i64, i64)> = HashMap::new();
    if let Ok(mut st) = lib.conn.prepare("SELECT path, mtime, size FROM song") {
        if let Ok(rows) = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?),
            ))
        }) {
            for (p, s) in rows.flatten() {
                known.insert(p, s);
            }
        }
    }
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
            let stamp = file_stamp(&f);
            let key = f.to_string_lossy().into_owned();
            (known.get(&key) != Some(&stamp)).then_some((f, folder, stamp))
        })
        .collect();
    progress.to_parse.store(todo.len(), Ordering::Relaxed);

    // parse in parallel
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(Song, (i64, i64))>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((f, folder, stamp)) = todo.get(i) else {
                    break;
                };
                let opts = vibebms_core::bms::ParseOptions {
                    random_seed: Some(0),
                    ..Default::default()
                };
                match vibebms_core::load_chart(f, &opts) {
                    Ok(c) if c.note_count() > 0 => {
                        results
                            .lock()
                            .unwrap()
                            .push((song_from_chart(f, folder.clone(), &c), *stamp));
                    }
                    Ok(_) => {}
                    Err(e) => log::debug!("skip {}: {e}", f.display()),
                }
                progress.parsed.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    let results = results.into_inner().unwrap();
    let removed: Vec<&String> = known.keys().filter(|k| !seen.contains(*k)).collect();
    let changed = !results.is_empty() || !removed.is_empty();
    let tx = lib.conn.unchecked_transaction();
    for (s, (mt, size)) in &results {
        let r = lib.conn.execute(
            "INSERT OR REPLACE INTO song VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
            params![
                s.path, s.folder, s.md5, s.sha256, s.title, s.subtitle, s.artist, s.subartist, s.genre, s.level, s.difficulty, s.mode,
                s.min_bpm, s.max_bpm, s.main_bpm, s.notes, s.length_ms, s.total, flags(s), s.stagefile, s.banner, s.preview, mt, size
            ],
        );
        if let Err(e) = r {
            log::error!("insert song: {e}");
        }
    }
    for p in removed {
        let _ = lib
            .conn
            .execute("DELETE FROM song WHERE path=?1", params![p]);
    }
    if let Ok(tx) = tx {
        let _ = tx.commit();
    }
    log::info!(
        "scan finished: {} charts, {} updated",
        seen.len(),
        results.len()
    );
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
        let db = dir.join("lib.db");
        let p = Arc::new(ScanProgress::default());
        scan(db.clone(), vec![dir.clone()], HashSet::new(), p.clone());
        assert!(p.done.load(Ordering::Acquire));
        let lib = Library::open(&db).unwrap();
        let s = lib.songs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "A");
        assert_eq!(s[0].mode, "7KEYS");
        assert!(s[0].folder.ends_with("pack"));
        // rescan is incremental
        let p2 = Arc::new(ScanProgress::default());
        scan(db.clone(), vec![dir.clone()], HashSet::new(), p2.clone());
        assert_eq!(p2.to_parse.load(Ordering::Relaxed), 0);

        let mut sc = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        sc.add(0, true);
        sc.add(1, true);
        let b = lib.record_play(&s[0].sha256, 0, &sc, ClearType::Hard);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 1));
        let worse = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        let b = lib.record_play(&s[0].sha256, 0, &worse, ClearType::Failed);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 2));
        assert_eq!(lib.scores().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
