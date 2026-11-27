//! The scan of the song folders into the song cache (`cache/songs.json`),
//! in the background, and what a scan before it left noted of the files
//! that stopped it.

use super::store::save_json;
use super::Song;
use crate::platform::{arcfs, vfs};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

/// The charts of the song cache at `file`, as the last scan left it (the
/// command line's `songs list` too). One in an archive that is also a
/// file of its own (the archive unpacked beside it) is left out: the file
/// is the one listed.
pub fn cached_songs(file: &Path) -> Vec<Song> {
    let songs: Vec<Song> = load_songs(file)
        .into_iter()
        .filter(|r| r.skipped_by.is_none())
        .map(|r| Song {
            added: r.mtime,
            ..r.song
        })
        .collect();
    if !songs.iter().any(Song::in_archive) {
        return songs;
    }
    let loose: HashSet<String> = songs
        .iter()
        .filter(|s| !s.in_archive())
        .map(|s| s.sha256.clone())
        .collect();
    songs
        .into_iter()
        .filter(|s| !s.in_archive() || !loose.contains(&s.sha256))
        .collect()
}

/// The song cache's records. When the file does not read as a whole they
/// are read one by one, and a record that does not read is left out: its
/// chart is read again by the next scan.
fn load_songs(file: &Path) -> Vec<SongRecord> {
    let Ok(b) = vfs::read(file) else {
        return Vec::new();
    };
    if let Ok(records) = serde_json::from_slice(&b) {
        return records;
    }
    let list: Vec<serde_json::Value> = serde_json::from_slice(&b).unwrap_or_default();
    let n = list.len();
    let records: Vec<SongRecord> = list
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    log::warn!("{}: {} of {n} records read", file.display(), records.len());
    records
}

#[derive(Default)]
pub struct ScanProgress {
    pub found: AtomicUsize,
    pub parsed: AtomicUsize,
    pub to_parse: AtomicUsize,
    pub done: AtomicBool,
    pub changed: AtomicBool,
    pub current: Mutex<String>,
    /// Charts the scan could not read (on the web: not in memory, as when
    /// the browser's storage swapped a stored file in after it was read).
    /// They got no record, so they are read again; the app reads them in
    /// and scans once more.
    pub unread: Mutex<Vec<PathBuf>>,
    /// The files left out for stopping the game twice are read again (the
    /// player asked for a rescan).
    pub forget: bool,
}

/// A file the scan reads: its path and its stamp then.
#[derive(Clone, Serialize, Deserialize)]
struct Mark(String, (i64, i64));

impl Mark {
    fn of(path: &Path, stamp: (i64, i64)) -> Mark {
        Mark(path.to_string_lossy().into_owned(), stamp)
    }
}

/// A file a scan was reading when the game stopped.
#[derive(Serialize, Deserialize)]
struct Suspect {
    stamp: (i64, i64),
    /// It was being read on its own, the second time: it is not read again.
    skip: bool,
}

/// One thread's note of the file it is reading.
struct Slot {
    path: PathBuf,
    /// Kept open: each file's note is written over the last one's.
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<std::fs::File>,
}

impl Slot {
    /// The note of `m` in work at `at`.
    fn line(m: &Mark, at: At) -> Vec<u8> {
        let mut line = serde_json::to_vec(&(m, at)).unwrap_or_default();
        line.push(b'\n');
        line
    }

    /// `m`'s note in the slot at `path`, noted at `from`, noted again as it
    /// is read on (at most once a second): how long it was in work counts
    /// from when it last moved.
    fn moving(path: &Path, m: &Mark, from: At) -> arcfs::Moved {
        let slot = Slot {
            path: path.to_path_buf(),
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
        };
        let m = m.clone();
        let last = Mutex::new((slot, from.ms));
        Arc::new(move || {
            let now = At::now();
            let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
            if now.ms >= last.1 + 1000 {
                last.1 = now.ms;
                last.0.put(&Slot::line(&m, now));
            }
        })
    }

    /// `line` (ending in a line break) in place of what it holds: the
    /// rest of a longer line before it stays behind, and is not read.
    #[cfg(not(target_arch = "wasm32"))]
    fn put(&mut self, line: &[u8]) {
        use std::io::{Seek, Write};
        if self.file.is_none() {
            if let Some(d) = self.path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let mut o = std::fs::OpenOptions::new();
            self.file = o
                .write(true)
                .create(true)
                .truncate(false)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = &mut self.file {
            let _ = f.rewind().and_then(|_| f.write_all(line));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn put(&mut self, line: &[u8]) {
        let _ = vfs::write(&self.path, line);
    }

    /// Nothing is being read.
    fn clear(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.file.is_some() {
            self.put(b"\n");
        }
        #[cfg(target_arch = "wasm32")]
        let _ = vfs::remove_file(&self.path);
    }

    /// The file it noted, if it was reading one, and when it was begun or
    /// last read on (not known in a note of an older version).
    fn left(path: &Path) -> Option<(Mark, Option<At>)> {
        let b = vfs::read(path).ok()?;
        let line = b.split(|c| *c == b'\n').next()?;
        let note = serde_json::from_slice(line)
            .ok()
            .map(|(m, at)| (m, Some(at)));
        // (older versions: begun in Unix seconds, or no time at all)
        let older = || {
            (serde_json::from_slice::<(Mark, u64)>(line).map(|(m, _)| m))
                .or_else(|_| serde_json::from_slice(line))
                .ok()
        };
        note.or_else(|| older().map(|m| (m, None)))
    }
}

/// A moment of this run of the game: the time since it began, which does
/// not jump as the wall clock may. Two moments compare only within a run.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct At {
    run: u64,
    ms: u64,
}

impl At {
    fn now() -> At {
        use crate::platform::{Instant, SystemTime, UNIX_EPOCH};
        static BEGUN: std::sync::OnceLock<(u64, Instant)> = std::sync::OnceLock::new();
        let (run, begun) = *BEGUN.get_or_init(|| {
            let wall = SystemTime::now().duration_since(UNIX_EPOCH);
            (
                wall.map(|d| d.as_nanos() as u64).unwrap_or(0),
                Instant::now(),
            )
        });
        At {
            run,
            ms: begun.elapsed().as_millis() as u64,
        }
    }
}

/// Beside the song cache: the game stopped in order ([`stopping`]) and has
/// not gone on since; it holds when, as an [`At`].
fn stop_file(songs_file: &Path) -> PathBuf {
    songs_file.with_file_name("scan-stopped")
}

/// The game stops in order (it is closed, or put away where the system may
/// end it unasked: a phone app sent to the background, a page left): until
/// it goes on ([`going_on`]) or starts again, what a scan reads is no
/// suspect if the game ends, unless it had not moved since before this for
/// longer than a file of its size takes ([`Marks::open`]). A scan goes on in
/// the background, and the system may end the game there at any time.
pub fn stopping(songs_file: &Path) {
    if vfs::is_file(&died_file(songs_file)) {
        return;
    }
    let f = stop_file(songs_file);
    let now = At::now();
    // (a stop of this run not gone on from stands: what was begun after it
    // was begun in the background)
    let held = vfs::read(&f).ok();
    if held
        .and_then(|b| serde_json::from_slice::<At>(&b).ok())
        .is_some_and(|a| a.run == now.run)
    {
        return;
    }
    let _ = vfs::write(&f, &serde_json::to_vec(&now).unwrap_or_default());
}

/// Beside the song cache: the scan's task died while the game went on (on
/// the web a panic ends the task, not the page): what it was reading counts
/// as at a crash at the next start, however the game stops meanwhile.
fn died_file(songs_file: &Path) -> PathBuf {
    songs_file.with_file_name("scan-died")
}

/// The scan of `songs_file` in work dies ([`died_file`]).
#[cfg(any(target_arch = "wasm32", test))]
fn died(songs_file: &Path) {
    let _ = vfs::remove_file(&stop_file(songs_file));
    let _ = vfs::write(&died_file(songs_file), b"");
}

/// The scan of `songs_file` is in work (`None`: none is): on the web, a
/// panic meanwhile is the scan's, and it [`died`].
#[cfg(target_arch = "wasm32")]
fn scanning(songs_file: Option<&Path>) {
    static IN_WORK: Mutex<Option<PathBuf>> = Mutex::new(None);
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(f) = IN_WORK.try_lock().ok().and_then(|f| f.clone()) {
                died(&f);
            }
            before(info);
        }));
    });
    *IN_WORK.lock().unwrap_or_else(|e| e.into_inner()) = songs_file.map(Path::to_path_buf);
}

/// The game goes on after [`stopping`] (back in the foreground).
pub fn going_on(songs_file: &Path) {
    let f = stop_file(songs_file);
    if vfs::is_file(&f) {
        let _ = vfs::remove_file(&f);
    }
}

/// How long a file of `size` bytes may be in work without moving (an
/// archive is noted again as it is read on) when the game is stopped in
/// order, and not be a suspect for it (seconds): a minute, and the file
/// read at 2 MiB a second (a chart is parsed without a move). Longer, it is
/// taken to hang.
fn read_time(size: i64) -> u64 {
    60 + size.max(0) as u64 / (2 << 20)
}

/// The files a scan is reading, noted in the cache folder as each is begun
/// (`scan-reading-<n>`, one a thread), so that a file that ends the game
/// when it is read, or is never read to its end, does not do so at every
/// start. A file found noted (the scan did not end: the game was closed, or
/// stopped) is a suspect, kept in `scan-suspects.json`: the next scan reads
/// it on its own before the others, and when it is found noted again, the
/// scan after that files it as not readable by this version. A note the
/// game left as it was stopped in order ([`stopping`]) does not count.
struct Marks {
    file: PathBuf,
    suspects: HashMap<String, Suspect>,
    slots: Vec<Mutex<Slot>>,
}

impl Marks {
    /// As many as `parallel_for` has threads.
    const SLOTS: usize = 8;

    /// The notes beside `songs_file`, what a scan before left noted taken
    /// into the suspects (with `forget`, the suspects before are not).
    fn open(songs_file: &Path, forget: bool) -> Marks {
        let file = songs_file.with_file_name("scan-suspects.json");
        let mut suspects: HashMap<String, Suspect> = match forget {
            true => HashMap::new(),
            false => (vfs::read(&file).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default(),
        };
        // (when, if it was this version that stopped; none after the scan
        // died: the stop came after)
        let died = vfs::is_file(&died_file(songs_file));
        let stopped: Option<Option<At>> = (vfs::read(&stop_file(songs_file)).ok())
            .filter(|_| !died)
            .map(|b| serde_json::from_slice(&b).ok());
        if died {
            let _ = vfs::remove_file(&died_file(songs_file));
        }
        // a stop of an earlier run: this one has not stopped since it began
        // (a stop of this one stays until it goes on: this scan may be in the
        // background)
        if stopped.is_some_and(|s| s.is_none_or(|a| a.run != At::now().run)) {
            let _ = vfs::remove_file(&stop_file(songs_file));
        }
        let slots: Vec<PathBuf> = (0..Self::SLOTS)
            .map(|k| songs_file.with_file_name(format!("scan-reading-{k}")))
            .collect();
        // stopped in order, the game could not tell being ended from a crash:
        // only a file it had not read on for too long as it stopped counts
        let crashed = |(Mark(_, stamp), begun): &(Mark, Option<At>)| match stopped {
            None => true,
            Some(stop) => stop.zip(*begun).is_some_and(|(t, b)| {
                let hung = |d: u64| d >= read_time(stamp.1) * 1000;
                t.run == b.run && t.ms.checked_sub(b.ms).is_some_and(hung)
            }),
        };
        let noted: Vec<(Mark, Option<At>)> = slots.iter().filter_map(|p| Slot::left(p)).collect();
        let left: Vec<Mark> = (noted.iter().filter(|n| crashed(n)))
            .map(|(m, _)| m.clone())
            .collect();
        if !noted.is_empty() || forget {
            for Mark(path, stamp) in left {
                match suspects.get_mut(&path) {
                    Some(s) if s.stamp == stamp => s.skip = true,
                    _ => {
                        suspects.insert(path, Suspect { stamp, skip: false });
                    }
                }
            }
            // (before anything is read: the notes are written over)
            if suspects.is_empty() {
                let _ = vfs::remove_file(&file);
            } else {
                save_json(&file, &suspects, "scan suspects");
            }
            for p in &slots {
                let _ = vfs::remove_file(p);
            }
        }
        let slot = |path| Slot {
            path,
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
        };
        Marks {
            file,
            suspects,
            slots: slots.into_iter().map(|p| Mutex::new(slot(p))).collect(),
        }
    }

    fn suspect(&self, m: &Mark) -> Option<&Suspect> {
        self.suspects.get(&m.0).filter(|s| s.stamp == m.1)
    }

    /// `m` is not to be read: it was found noted twice.
    fn skips(&self, m: &Mark) -> bool {
        self.suspect(m).is_some_and(|s| s.skip)
    }

    /// `m` noted as being read, until the slot is given back.
    fn note(&self, m: &Mark) -> std::sync::MutexGuard<'_, Slot> {
        let free = self.slots.iter().find_map(|s| s.try_lock().ok());
        // (more threads than slots: the first one's turn is waited for)
        let mut slot =
            free.unwrap_or_else(|| self.slots[0].lock().unwrap_or_else(|e| e.into_inner()));
        slot.put(&Slot::line(m, At::now()));
        slot
    }

    fn clear(&self) {
        for s in &self.slots {
            s.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }

    /// `work(i)` for `i` in `0..n`, the file `mark(i)` noted meanwhile: the
    /// suspects one at a time first (one found noted after that was read
    /// alone), then the others, several at once.
    fn each(&self, n: usize, mark: impl Fn(usize) -> Mark + Sync, work: impl Fn(usize) + Sync) {
        let alone = |i: &usize| self.suspect(&mark(*i)).is_some_and(|s| !s.skip);
        let (first, rest): (Vec<usize>, Vec<usize>) = (0..n).partition(alone);
        let noted = |i: usize| {
            let m = mark(i);
            let slot = self.note(&m);
            arcfs::watched(Slot::moving(&slot.path, &m, At::now()), || work(i));
        };
        for i in first {
            noted(i);
        }
        self.clear();
        crate::platform::parallel_for(rest.len(), |k| noted(rest[k]));
        self.clear();
    }

    /// The scan ended: nothing is noted, and the suspects are settled
    /// (read, or filed as not readable).
    fn finish(self) {
        for s in self.slots {
            let s = s.into_inner().unwrap_or_else(|e| e.into_inner());
            #[cfg(not(target_arch = "wasm32"))]
            if s.file.is_none() {
                continue;
            }
            let path = s.path.clone();
            drop(s);
            let _ = vfs::remove_file(&path);
        }
        if !self.suspects.is_empty() {
            let _ = vfs::remove_file(&self.file);
        }
    }
}

/// What the scan finds below a folder.
#[derive(Default)]
struct Found {
    charts: Vec<PathBuf>,
    /// Folders that hold a sound file.
    voiced: HashSet<PathBuf>,
    /// Archives (read as folders: `platform::arcfs`).
    archives: Vec<PathBuf>,
    /// Folders that cannot be read (a drive not mounted, no permission).
    unreadable: Vec<PathBuf>,
}

/// Chart files and archives under `dir` into `out`.
fn walk(dir: &Path, depth: usize, skip: &HashSet<PathBuf>, out: &mut Found) {
    if depth > 16 {
        return;
    }
    let Ok(entries) = vfs::read_dir(dir) else {
        out.unreadable.push(dir.to_path_buf());
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
        } else if vibebms_core::is_chart(&e.path) {
            out.charts.push(e.path);
        } else if bmseeker::unpack::is_archive(&e.path) {
            out.archives.push(e.path);
        } else if (e.path.extension().and_then(|x| x.to_str()))
            .is_some_and(|x| vfs::AUDIO_EXTS.iter().any(|a| x.eq_ignore_ascii_case(a)))
        {
            out.voiced.insert(dir.to_path_buf());
        }
    }
}

/// Chart files under `roots` (used by the web build to prefetch them; the
/// archives that changed are read in when the scan asks: `ScanProgress::unread`).
pub fn find_charts(roots: &[PathBuf], skip: &HashSet<PathBuf>) -> Vec<PathBuf> {
    let mut found = Found::default();
    for r in roots {
        walk(r, 0, skip, &mut found);
    }
    found.charts
}

/// The library folder a chart (or an archive) below `root` is listed under:
/// `root`'s sub-folder it is in, else `root`.
fn folder_of(root: &Path, f: &Path) -> String {
    let first = f
        .strip_prefix(root)
        .ok()
        .and_then(|r| r.components().next())
        .map(|c| root.join(c.as_os_str()));
    match first {
        Some(p) if p != f => p.to_string_lossy().into_owned(),
        _ => root.to_string_lossy().into_owned(),
    }
}

/// The archives found that changed since the scan before: listed, and
/// their charts read (several at once natively). Their indexes are updated;
/// the charts' bytes by path. Archives that cannot be read now go into
/// `unreadable` (web: not in memory yet, also into `unread`); one `marks`
/// has as not to be read is indexed as one that failed, and read again by
/// another version or when the scan forgets (`progress.forget`).
fn read_archives(
    archives: &[(PathBuf, (i64, i64))],
    progress: &ScanProgress,
    marks: &Marks,
    unreadable: &mut Vec<PathBuf>,
) -> (HashMap<PathBuf, Vec<u8>>, bool) {
    let mark = |i: usize| Mark::of(&archives[i].0, archives[i].1);
    let mut indexed = false;
    for (i, (a, stamp)) in archives.iter().enumerate() {
        if marks.skips(&mark(i)) {
            log::warn!("scan: {}: {STOPPED}", a.display());
            arcfs::set_index(a, arcfs::Index::left_out(*stamp, STOPPED.into(), VERSION));
            indexed = true;
        }
    }
    let current = |ix: &arcfs::Index, stamp: (i64, i64)| {
        let left_out = ix.error == STOPPED && (progress.forget || ix.left_out_by != VERSION);
        ix.stamp == stamp && !left_out
    };
    let changed: Vec<usize> = (0..archives.len())
        .filter(|&i| {
            let (a, stamp) = &archives[i];
            !arcfs::index_of(a).is_some_and(|ix| current(&ix, *stamp))
        })
        .collect();
    let read: Mutex<Vec<(usize, std::io::Result<arcfs::Unpacked>)>> = Mutex::default();
    let read_one = |k: usize| {
        let i = changed[k];
        let a = &archives[i].0;
        *progress.current.lock().unwrap() = a.display().to_string();
        let r = arcfs::read_charts(a, None);
        read.lock().unwrap().push((i, r));
    };
    marks.each(changed.len(), |k| mark(changed[k]), read_one);
    let mut read = read.into_inner().unwrap();
    read.sort_by_key(|(i, _)| *i);
    let mut bytes = HashMap::new();
    for (i, r) in read {
        let (a, stamp) = &archives[i];
        match r {
            Ok(u) => {
                let files = u.listing.unwrap_or_default();
                log::info!(
                    "scan: {} ({} files, {} charts)",
                    a.display(),
                    files.len(),
                    u.files.len()
                );
                arcfs::set_index(a, arcfs::Index::new(*stamp, &u.format, files));
                for (rel, b) in u.files {
                    bytes.insert(arcfs::join(a, &rel), b);
                }
                indexed = true;
            }
            // not an archive, or its charts larger than is read: until it
            // changes
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::OutOfMemory
                ) =>
            {
                log::warn!("scan: {}: {e}", a.display());
                arcfs::set_index(a, arcfs::Index::failed(*stamp, e.to_string()));
                indexed = true;
            }
            Err(e) => {
                log::debug!("scan: {} not read: {e}", a.display());
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    progress.unread.lock().unwrap().push(a.clone());
                }
                unreadable.push(arcfs::join(a, Path::new("")));
            }
        }
    }
    (bytes, indexed)
}

/// Notes by kind and the densities of a chart (see `Song`).
///
/// Kinds: normal notes and long notes off the scratch lanes, the same on
/// them, mines. Densities (notes per second): the average over the
/// seconds that are not much quieter than the chart's mean, the busiest
/// second, and the busiest stretch of up to 5 s from the point where the
/// notes left could still fill the gauge from empty. A second's count
/// includes the long notes held through it.
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
    if n_notes == 0 {
        return (kinds, [0.0; 3]);
    }

    // (a second each, to the chart's end: `vibebms_core::limits`)
    let second = |t: i64| t.max(0) / SEC;
    let buckets_len = second(last) + 2;
    // each note from its second to the one it ends in: where the count
    // goes up and where it comes down again, then summed
    let mut buckets = vec![0i64; buckets_len as usize + 1];
    for n in &playable {
        let from = second(n.time);
        let to = second(n.end_time()).min(buckets_len - 1).max(from);
        buckets[from as usize] += 1;
        buckets[to as usize + 1] -= 1;
    }
    buckets.truncate(buckets_len as usize);
    let mut held = 0;
    for b in &mut buckets {
        held += *b;
        *b = held;
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
        judge: Some(m.judge_rank_number()),
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
        bare: false,
        added: 0,
    }
}

/// The game's version, which a chart it could not read is filed under.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a file `Marks` has as not to be read is left out.
const STOPPED: &str = "the game stopped twice while it was read: left out";

/// Scan `roots` and update the song cache. Runs on a background thread.
pub fn scan(
    songs_file: PathBuf,
    roots: Vec<PathBuf>,
    skip: HashSet<PathBuf>,
    progress: Arc<ScanProgress>,
) {
    #[cfg(target_arch = "wasm32")]
    scanning(Some(&songs_file));
    let mut known_records = load_songs(&songs_file);
    // charts read before the library kept their judge rank and note
    // kinds, or the types of their long notes, are read again (once)
    let known: HashMap<String, (i64, i64)> = known_records
        .iter()
        .filter(|r| match &r.skipped_by {
            Some(v) => v == VERSION && !progress.forget,
            None => r.song.judge.is_some() && (!r.song.has_ln || r.song.ln_profile.is_some()),
        })
        .map(|r| (r.song.path.clone(), (r.mtime, r.size)))
        .collect();
    let marks = Marks::open(&songs_file, progress.forget);
    let index_file = songs_file.with_file_name("archives.json");
    arcfs::load_index(&index_file);
    let mut files: Vec<(PathBuf, String)> = Vec::new();
    // (each with the root it is below)
    let mut archives: Vec<(PathBuf, &Path)> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    let mut voiced: HashSet<PathBuf> = HashSet::new();
    for root in &roots {
        *progress.current.lock().unwrap() = root.display().to_string();
        let mut found = Found::default();
        walk(root, 0, &skip, &mut found);
        for f in found.charts {
            let folder = folder_of(root, &f);
            files.push((f, folder));
        }
        archives.extend(found.archives.into_iter().map(|a| (a, root.as_path())));
        unreadable.extend(found.unreadable);
        voiced.extend(found.voiced);
        progress.found.store(files.len(), Ordering::Relaxed);
    }
    // archives: those that changed read (their charts in `pre`), every
    // one's charts from its index; a chart in one has the archive's stamp
    let stamped: Vec<(PathBuf, (i64, i64))> = archives
        .iter()
        .map(|(a, _)| (a.clone(), vfs::stamp(a)))
        .collect();
    let (mut pre, indexed) = read_archives(&stamped, &progress, &marks, &mut unreadable);
    let mut archive_stamp: HashMap<PathBuf, (i64, i64)> = HashMap::new();
    for ((a, root), (_, stamp)) in archives.iter().zip(&stamped) {
        let Some(ix) = arcfs::index_of(a).filter(|ix| ix.stamp == *stamp) else {
            continue;
        };
        for rel in ix.charts() {
            let p = arcfs::join(a, rel);
            archive_stamp.insert(p.clone(), *stamp);
            // (an archive right in the root is a folder of its own)
            let folder = folder_of(root, &p);
            files.push((p, folder));
        }
    }
    progress.found.store(files.len(), Ordering::Relaxed);
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
    // (each file's stamp: on a slow drive, several at once)
    let stamps: Vec<(i64, i64)> = {
        let out: Vec<Mutex<(i64, i64)>> = files.iter().map(|_| Mutex::default()).collect();
        crate::platform::parallel_for(files.len(), |i| {
            let f = &files[i].0;
            let stamp = archive_stamp
                .get(f)
                .copied()
                .unwrap_or_else(|| vfs::stamp(f));
            *out[i].lock().unwrap() = stamp;
        });
        out.into_iter().map(|m| m.into_inner().unwrap()).collect()
    };
    let todo: Vec<(PathBuf, String, (i64, i64))> = files
        .into_iter()
        .zip(stamps)
        .filter_map(|((f, folder), stamp)| {
            let key = f.to_string_lossy().into_owned();
            (known.get(&key) != Some(&stamp)).then_some((f, folder, stamp))
        })
        .collect();
    progress.to_parse.store(todo.len(), Ordering::Relaxed);
    // charts in archives that did not change but are read again (records
    // of an older version): each archive read once for them
    // (each with its stamp, which its charts have)
    let mut again: HashMap<PathBuf, ((i64, i64), HashSet<PathBuf>)> = HashMap::new();
    for (f, _, stamp) in &todo {
        if let Some((a, rel)) = arcfs::split(f).filter(|_| !pre.contains_key(f)) {
            again
                .entry(a)
                .or_insert((*stamp, HashSet::new()))
                .1
                .insert(rel);
        }
    }
    let mut again: Vec<_> = again.into_iter().collect();
    again.sort_by(|a, b| a.0.cmp(&b.0));
    let reread: Mutex<Vec<(PathBuf, Vec<u8>)>> = Mutex::default();
    let read_again = |i: usize| {
        let (a, (_, want)) = &again[i];
        match arcfs::read_charts(a, Some(want)) {
            Ok(u) => reread.lock().unwrap().extend(
                u.files
                    .into_iter()
                    .map(|(rel, b)| (arcfs::join(a, &rel), b)),
            ),
            Err(e) => log::debug!("scan: {} not read: {e}", a.display()),
        }
    };
    marks.each(
        again.len(),
        |i| Mark::of(&again[i].0, again[i].1 .0),
        read_again,
    );
    pre.extend(reread.into_inner().unwrap());
    // (taken as each is parsed)
    let pre: Vec<Mutex<Option<Vec<u8>>>> = todo
        .iter()
        .map(|(f, _, _)| Mutex::new(pre.remove(f)))
        .collect();

    let results: Mutex<Vec<SongRecord>> = Mutex::new(Vec::new());
    let mark = |i: usize| Mark::of(&todo[i].0, todo[i].2);
    let parse_one = |i: usize| {
        let (f, folder, stamp) = &todo[i];
        let opts = vibebms_core::bms::ParseOptions::default();
        let packed = arcfs::split(f).is_some();
        // (a file that cannot be read now is tried again next time)
        let bytes = || match pre[i].lock().unwrap().take() {
            Some(b) => Ok(b),
            None if packed => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "not read from its archive",
            )),
            None => vfs::read(f),
        };
        let parsed = if marks.skips(&mark(i)) {
            log::warn!("scan: {}: {STOPPED}", f.display());
            Ok(Err(STOPPED.into()))
        } else if !packed && stamp.1 > vibebms_core::limits::MAX_FILE as i64 {
            // (not read into memory to be refused)
            Ok(Err("larger than a chart may be".into()))
        } else {
            bytes().map(|b| {
                // a parser bug on one file skips that file
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    vibebms_core::load_chart_bytes(f, &b, &opts).map_err(|e| e.to_string())
                }))
                .unwrap_or_else(|_| Err("the parser panicked".into()))
            })
        };
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
                log::debug!("not read {}: {e}", f.display());
                progress.unread.lock().unwrap().push(f.clone());
                None
            }
        };
        if let Some(r) = record {
            results.lock().unwrap().push(r);
        }
        progress.parsed.fetch_add(1, Ordering::Relaxed);
    };
    marks.each(todo.len(), mark, parse_one);
    let mut new = results.into_inner().unwrap();
    // whether each chart has a sound beside it, as its folder is now (of
    // a folder that cannot be read now, as it was)
    let bare =
        |p: &Path| arcfs::split(p).is_none() && p.parent().is_some_and(|d| !voiced.contains(d));
    for r in &mut new {
        r.song.bare = bare(Path::new(&r.song.path));
    }
    let mut moved = 0;
    for r in &mut known_records {
        let p = Path::new(&r.song.path);
        if !unreadable.iter().any(|d| p.starts_with(d)) {
            let b = bare(p);
            moved += usize::from(r.song.bare != b);
            r.song.bare = b;
        }
    }
    // the archives' indexes: those found (and those in folders that cannot
    // be read now, or not below these roots) kept
    let found_archives: HashSet<&Path> = archives.iter().map(|(a, _)| a.as_path()).collect();
    let keep_archive = |a: &Path| {
        found_archives.contains(a)
            || !roots.iter().any(|r| a.starts_with(r))
            || unreadable
                .iter()
                .any(|d| a.starts_with(d) || arcfs::join(a, Path::new("")) == *d)
    };
    let gone = arcfs::indexed().iter().any(|a| !keep_archive(a));
    if indexed || gone || (!archives.is_empty() && !vfs::is_file(&index_file)) {
        arcfs::save_index(&index_file, &keep_archive);
    }
    // (records read again count too: their file may be gone)
    let removed = known_records
        .iter()
        .filter(|r| !seen.contains(&r.song.path))
        .count();
    let changed = !new.is_empty() || removed > 0 || moved > 0;
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
    marks.finish();
    #[cfg(target_arch = "wasm32")]
    scanning(None);
    progress.changed.store(changed, Ordering::Release);
    progress.done.store(true, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::store::tests::{key, open};
    use crate::library::Library;
    use vibebms_core::chart::{Chart, ChartMeta, LnType, Note, NoteKind};
    use vibebms_core::judge::Judge;
    use vibebms_core::score::{ClearType, Score};
    use vibebms_core::PlayMode;

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

    /// A chart past an hour has its kinds and its densities as any other.
    #[test]
    fn chart_info_past_an_hour() {
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
        let (kinds, density) = chart_info(&c);
        assert_eq!(kinds, [1, 1, 1, 1, 1]);
        // buckets [0, 1, 1, 2, 1, 0, .., 1, 0] over 3602 s
        assert_eq!(density[0], 6.0 / 3602.0);
        assert_eq!(density[1], 2.0);
    }

    /// Kinds by scratch lane (any player's), mines; negative times in
    /// second 0; no notes nothing.
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
        let empty = info_chart(
            PlayMode::Beat7K,
            None,
            vec![info_note(1, 1_000_000, NoteKind::Mine { damage: 1.0 })],
        );
        assert_eq!(chart_info(&empty), ([0, 0, 0, 0, 1], [0.0; 3]));
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
        let mut lib = open(&cache);
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
        let b = lib.record_play(&key(&s[0].sha256, 0), &sc, ClearType::Hard, 1500);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 1));
        let worse = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        let b = lib.record_play(&key(&s[0].sha256, 0), &worse, ClearType::Failed, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 2));
        // an assisted play: lamp and counts only, never the best score
        let mut great = Score {
            total_notes: 2,
            passed: 2,
            ..Default::default()
        };
        great.add(Judge::PGreat, 1);
        great.add(Judge::PGreat, 1);
        let b = lib.record_assisted(&key(&s[0].sha256, 0), &great, ClearType::AssistEasy, 0);
        assert_eq!((b.ex_score, b.clear, b.play_count), (3, ClearType::Hard, 3));
        let b = lib.record_assisted(&key("other", 0), &great, ClearType::LightAssistEasy, 0);
        assert_eq!((b.ex_score, b.min_bp, b.max_combo), (0, u32::MAX, 0));
        assert_eq!((b.clear, b.clear_count), (ClearType::LightAssistEasy, 1));
        // persisted
        let lib2 = open(&cache);
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
        assert_eq!(open(&cache).songs().len(), 1);
        // and go once the folder is there without them
        std::fs::create_dir_all(dir.join("pack")).unwrap();
        let p4 = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.join("pack")],
            [cache.clone()].into_iter().collect(),
            p4.clone(),
        );
        assert_eq!(open(&cache).songs().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&moved).ok();
    }

    /// A chart that cannot be read when the scan comes to it (on the web:
    /// its bytes not in memory) gets no record, not one as a skipped
    /// chart: it is listed as unread and read by the next scan.
    #[cfg(unix)]
    #[test]
    fn a_chart_not_read_yet_is_read_by_the_next_scan() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        // (a chart whose file cannot be read: a link to nothing yet)
        let chart = songs.join("a.bme");
        let target = dir.join("a.bme.data");
        std::os::unix::fs::symlink(&target, &chart).unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(
                cache.join("songs.json"),
                vec![songs.clone()],
                [cache.clone()].into_iter().collect(),
                p.clone(),
            );
            p
        };
        let p = run();
        assert_eq!(*p.unread.lock().unwrap(), vec![chart.clone()]);
        assert!(!p.changed.load(Ordering::Relaxed));
        assert!(load_songs(&cache.join("songs.json")).is_empty());
        std::fs::write(&target, "#TITLE A\n#BPM 150\n#00111:01\n").unwrap();
        let p = run();
        assert!(p.unread.lock().unwrap().is_empty());
        assert!(p.changed.load(Ordering::Relaxed));
        let s = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "A");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A record of the song cache that does not read (a number written as
    /// `null`) loses its own song, not every song.
    #[test]
    fn an_unreadable_song_record_loses_one_song() {
        let dir = std::env::temp_dir().join(format!("vibebms-songs-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("songs.json");
        let records = r#"[{"path": "a.bms", "title": "A", "total": 300.0, "mtime": 1, "size": 1},
            {"path": "b.bms", "title": "B", "total": null, "mtime": 1, "size": 1},
            {"path": "c.bms", "title": "C", "mtime": 1, "size": 1}]"#;
        std::fs::write(&file, records).unwrap();
        let titles: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
        assert_eq!(titles, ["A", "C"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A chart a scan was reading when the game stopped is read on its own
    /// by the next scan; one still being read when the game stops then is
    /// filed as not readable, and read again only when it changes.
    #[test]
    fn a_chart_that_stops_the_scan_twice_is_left_out() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        let file = cache.join("songs.json");
        let chart = |title: &str| format!("#TITLE {title}\n#BPM 150\n#00111:01\n");
        std::fs::write(songs.join("a.bme"), chart("A")).unwrap();
        let b = songs.join("b.bme");
        std::fs::write(&b, chart("B")).unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(
                file.clone(),
                vec![songs.clone()],
                [cache.clone()].into_iter().collect(),
                p.clone(),
            );
            p.to_parse.load(Ordering::Relaxed)
        };
        let titles = || {
            let mut t: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
            t.sort();
            t
        };
        // the game stops while a scan reads b: its note stays behind
        let stops_on_b = || {
            let marks = Marks::open(&file, false);
            drop(marks.note(&Mark::of(&b, vfs::stamp(&b))));
        };
        let left = || {
            let names = std::fs::read_dir(&cache).unwrap().flatten();
            let scan_files = names.filter(|e| e.file_name().to_string_lossy().starts_with("scan-"));
            scan_files.count()
        };
        stops_on_b();
        assert_eq!(left(), 1);
        // once: it is read again (the game was only closed), nothing is kept
        assert_eq!(run(), 2);
        assert_eq!(titles(), ["A", "B"]);
        assert_eq!(left(), 0);
        // twice, the second time read on its own: left out, not read again
        std::fs::write(&b, chart("B again")).unwrap();
        stops_on_b();
        stops_on_b();
        assert_eq!(run(), 1);
        assert_eq!(titles(), ["A"]);
        assert_eq!((run(), left()), (0, 0));
        // until it changes
        std::fs::write(&b, chart("B once more")).unwrap();
        assert_eq!(run(), 1);
        assert_eq!(titles(), ["A", "B once more"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The same for an archive: indexed as one that failed.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_archive_that_stops_the_scan_twice_is_left_out() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-marks-a-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        let file = cache.join("songs.json");
        let tar = dir.join("pack.tar");
        let chart: &[u8] = b"#TITLE T\n#BPM 150\n#00111:01\n";
        std::fs::write(&tar, bmseeker::testkit::tar(&[("t.bms", chart)])).unwrap();
        std::fs::write(dir.join("a.bme"), "#TITLE A\n#00111:01\n").unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            let skip = [cache.clone()].into_iter().collect();
            scan(file.clone(), vec![dir.clone()], skip, p);
            let mut t: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
            t.sort();
            t
        };
        let stops = || {
            let marks = Marks::open(&file, false);
            drop(marks.note(&Mark::of(&tar, vfs::stamp(&tar))));
        };
        stops();
        assert_eq!(run(), ["A", "T"]);
        // (as a changed archive: one not changed is not read at all)
        let two = [("t.bms", chart), ("u.bms", chart)];
        std::fs::write(&tar, bmseeker::testkit::tar(&two)).unwrap();
        stops();
        stops();
        assert_eq!(run(), ["A"]);
        assert_eq!(
            crate::platform::arcfs::index_of(&tar).unwrap().error,
            STOPPED
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A file in work when the game was stopped in order is no suspect,
    /// unless it was in work far longer than its size takes; a rescan the
    /// player asks for, or another version, reads what was left out again.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn quits_are_no_strikes_and_a_rescan_forgets() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-quits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        let file = cache.join("songs.json");
        std::fs::write(songs.join("a.bme"), "#TITLE A\n#00111:01\n").unwrap();
        let b = songs.join("b.bme");
        std::fs::write(&b, "#TITLE B\n#00111:01\n").unwrap();
        let tar = songs.join("pack.tar");
        let chart: &[u8] = b"#TITLE T\n#00111:01\n";
        std::fs::write(&tar, bmseeker::testkit::tar(&[("t.bms", chart)])).unwrap();
        let run = |forget: bool| {
            let p = Arc::new(ScanProgress {
                forget,
                ..Default::default()
            });
            scan(file.clone(), vec![songs.clone()], [cache.clone()].into(), p);
            let mut t: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
            t.sort();
            t
        };
        // what an earlier run of the game was reading of `f` as it ended,
        // begun `ago` seconds before it stopped in order, or not stopped
        let stops = |f: &Path, ago: i64, in_order: bool| {
            drop(Marks::open(&file, false));
            let run = At::now().run.wrapping_add(1);
            let begun = At { run, ms: 7_200_000 };
            let line = serde_json::to_vec(&(Mark::of(f, vfs::stamp(f)), begun)).unwrap();
            std::fs::write(cache.join("scan-reading-0"), line).unwrap();
            if in_order {
                let ms = begun.ms.saturating_add_signed(ago * 1000);
                let stop = serde_json::to_vec(&At { run, ms }).unwrap();
                std::fs::write(stop_file(&file), stop).unwrap();
            }
        };
        assert_eq!(run(false), ["A", "B", "T"]);
        std::fs::write(&b, "#TITLE B2\n#00111:01\n").unwrap();
        // two quits while b is read, or b begun after them: no strike
        stops(&b, 5, true);
        stops(&b, -1, true);
        assert_eq!(run(false), ["A", "B2", "T"]);
        assert!(!vfs::is_file(&stop_file(&file)));
        // in work for an hour at each quit: it hung, twice
        std::fs::write(&b, "#TITLE B three\n#00111:01\n").unwrap();
        stops(&b, 3600, true);
        stops(&b, 3600, true);
        assert_eq!(run(false), ["A", "T"]);
        // a stop the game went on after was no quit: the archive (as a
        // changed one) stopped it twice
        let two = [("t.bms", chart), ("u.bms", chart)];
        std::fs::write(&tar, bmseeker::testkit::tar(&two)).unwrap();
        stops(&tar, 5, true);
        going_on(&file);
        stops(&tar, 5, false);
        assert_eq!(run(false), ["A"]);
        assert_eq!(run(false), ["A"]);
        // a rescan the player asks for reads both again
        assert_eq!(run(true), ["A", "B three", "T", "T"]);
        // and a version of the game other than the one that left it out
        crate::platform::arcfs::set_index(
            &tar,
            crate::platform::arcfs::Index::left_out(vfs::stamp(&tar), STOPPED.into(), "0.0.1"),
        );
        let ix = || crate::platform::arcfs::index_of(&tar).unwrap();
        assert_eq!(ix().left_out_by, "0.0.1");
        run(false);
        assert!(ix().error.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Sent to the background, the game goes on scanning, and a scan may
    /// start there; the system may end it at any time: no strike, however
    /// often. Back in the foreground, a crash counts again.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ended_in_the_background_is_no_strike() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-bg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        let file = cache.join("songs.json");
        std::fs::write(songs.join("a.bme"), "#TITLE A\n#00111:01\n").unwrap();
        let b = songs.join("b.bme");
        std::fs::write(&b, "#TITLE B\n#00111:01\n").unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(file.clone(), vec![songs.clone()], [cache.clone()].into(), p);
            let mut t: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
            t.sort();
            t
        };
        // a scan that begins b, and the game ends; then the next start's
        // scan takes in what it left
        let ended_reading_b = || {
            let marks = Marks::open(&file, false);
            drop(marks.note(&Mark::of(&b, vfs::stamp(&b))));
            drop(Marks::open(&file, false));
        };
        assert_eq!(run(), ["A", "B"]);
        std::fs::write(&b, "#TITLE B2\n#00111:01\n").unwrap();
        for _ in 0..2 {
            stopping(&file);
            // (a scan started in the background, which ends)
            drop(Marks::open(&file, false));
            ended_reading_b();
        }
        assert_eq!(run(), ["A", "B2"]);
        // in the foreground again: the stop is over
        going_on(&file);
        std::fs::write(&b, "#TITLE B three\n#00111:01\n").unwrap();
        ended_reading_b();
        ended_reading_b();
        assert_eq!(run(), ["A"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The scan's task died (a panic, on the web): leaving the game after
    /// it, in order, does not excuse what it was reading.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_scan_that_died_is_a_strike_however_the_game_stops() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-died-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let songs = dir.join("pack");
        std::fs::create_dir_all(&songs).unwrap();
        let cache = dir.join("cache");
        let file = cache.join("songs.json");
        std::fs::write(songs.join("a.bme"), "#TITLE A\n#00111:01\n").unwrap();
        let b = songs.join("b.bme");
        std::fs::write(&b, "#TITLE B\n#00111:01\n").unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(file.clone(), vec![songs.clone()], [cache.clone()].into(), p);
            let mut t: Vec<String> = cached_songs(&file).into_iter().map(|s| s.title).collect();
            t.sort();
            t
        };
        assert_eq!(run(), ["A", "B"]);
        std::fs::write(&b, "#TITLE B2\n#00111:01\n").unwrap();
        // the task dies reading b, and the page is left in order (before
        // it died too)
        let died_reading_b = || {
            stopping(&file);
            let marks = Marks::open(&file, false);
            drop(marks.note(&Mark::of(&b, vfs::stamp(&b))));
            died(&file);
            stopping(&file);
            assert!(!vfs::is_file(&stop_file(&file)));
        };
        died_reading_b();
        died_reading_b();
        assert_eq!(run(), ["A"]);
        // the next stop counts again
        stopping(&file);
        assert!(vfs::is_file(&stop_file(&file)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An archive read on is noted again as it moves, at most once a
    /// second: a stop in order soon after its last move is no strike, however
    /// long ago it was begun; one long after it is.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_slow_read_that_moves_is_no_strike() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-moves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        let file = cache.join("songs.json");
        let slot = cache.join("scan-reading-0");
        let m = Mark("pack.7z".into(), (1, 1));
        let run = At::now().run;
        let begun = At { run, ms: 0 };
        while At::now().ms < 1000 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // stopped in order `after` ms after it last moved
        let stopped = |after: u64| -> bool {
            std::fs::write(&slot, Slot::line(&m, begun)).unwrap();
            let moved = Slot::moving(&slot, &m, begun);
            moved();
            let (_, at) = Slot::left(&slot).unwrap();
            let at = at.unwrap();
            assert!(at.ms >= 1000);
            // (at most once a second)
            std::fs::write(&slot, Slot::line(&m, at)).unwrap();
            moved();
            assert_eq!(Slot::left(&slot).unwrap().1.unwrap().ms, at.ms);
            let stop = At {
                run,
                ms: at.ms + after,
            };
            std::fs::write(stop_file(&file), serde_json::to_vec(&stop).unwrap()).unwrap();
            let marks = Marks::open(&file, false);
            marks.suspect(&m).is_some()
        };
        assert!(!stopped(5_000));
        assert!(stopped(3_600_000));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A chart file larger than a chart may be is not read into memory.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_chart_file_too_large_is_not_read() {
        let dir = std::env::temp_dir().join(format!("vibebms-lib-large-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        std::fs::write(dir.join("a.bme"), "#TITLE A\n#00111:01\n").unwrap();
        // (its notes, then nothing, to past the limit)
        let large = dir.join("large.bme");
        std::fs::write(&large, "#TITLE L\n#00111:01\n").unwrap();
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&large)
            .unwrap();
        f.set_len(vibebms_core::limits::MAX_FILE as u64 + 1)
            .unwrap();
        drop(f);
        let p = Arc::new(ScanProgress::default());
        let skip = [cache.clone()].into_iter().collect();
        scan(cache.join("songs.json"), vec![dir.clone()], skip, p);
        let titles: Vec<String> = (cached_songs(&cache.join("songs.json")).into_iter())
            .map(|s| s.title)
            .collect();
        assert_eq!(titles, ["A"]);
        std::fs::remove_dir_all(&dir).ok();
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
        assert_eq!(open(&cache).songs().len(), 1);
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![songs],
            [cache.clone()].into_iter().collect(),
            p.clone(),
        );
        assert!(p.changed.load(Ordering::Relaxed));
        assert!(open(&cache).songs().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A small WAV (16-bit mono, 22050 Hz).
    #[cfg(not(target_arch = "wasm32"))]
    fn wav(frames: u32) -> Vec<u8> {
        let data = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&22050u32.to_le_bytes());
        b.extend_from_slice(&44100u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        b.extend((0..frames).flat_map(|i| ((i as i16).wrapping_mul(300) as u16).to_le_bytes()));
        b
    }

    /// A solid 7z (LZMA2) of `files`.
    #[cfg(not(target_arch = "wasm32"))]
    fn seven(files: &[(&str, &[u8])]) -> Vec<u8> {
        use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};
        let mut w = ArchiveWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        let entries = files
            .iter()
            .map(|(n, _)| ArchiveEntry::new_file(n))
            .collect();
        let readers = files.iter().map(|(_, b)| SourceReader::new(*b)).collect();
        w.push_archive_entries(entries, readers).unwrap();
        w.finish().unwrap().into_inner()
    }

    /// Charts in a .tar.gz and a .tar: scanned as folders, played through
    /// the file system; the .tar.gz decompressed once for its song, not
    /// once for each of its files.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn charts_in_tar_archives() {
        use crate::platform::arcfs;
        let dir = std::env::temp_dir().join(format!("vibebms-lib-tar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        let chart = |title: &str| {
            format!("#TITLE {title}\n#BPM 150\n#WAV01 kick.wav\n#00111:01\n#00112:0101\n")
        };
        let kick = wav(2205);
        let a_chart = chart("A");
        let files: [(&str, &[u8]); 4] = [
            ("Song/a.bme", a_chart.as_bytes()),
            ("Song/kick.wav", &kick),
            ("Song/bga/stage.png", b"png"),
            ("readme.txt", b"hi"),
        ];
        let tgz = dir.join("Pack.tar.gz");
        std::fs::write(&tgz, bmseeker::testkit::tar_gz(&files)).unwrap();
        let tar = dir.join("plain.tar");
        let b = chart("B");
        std::fs::write(&tar, bmseeker::testkit::tar(&[("b.bms", b.as_bytes())])).unwrap();
        let p = Arc::new(ScanProgress::default());
        scan(
            cache.join("songs.json"),
            vec![dir.clone()],
            [cache.clone()].into_iter().collect(),
            p,
        );
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        let mut titles: Vec<(&str, &str)> = songs
            .iter()
            .map(|s| (s.title.as_str(), s.path.as_str()))
            .collect();
        titles.sort();
        let a = arcfs::join(&tgz, Path::new("Song/a.bme"));
        let b_path = arcfs::join(&tar, Path::new("b.bms"));
        assert_eq!(
            titles,
            [("A", a.to_str().unwrap()), ("B", b_path.to_str().unwrap())]
        );
        assert_eq!(songs.iter().find(|s| s.title == "A").unwrap().notes, 3);
        assert_eq!(arcfs::index_of(&tgz).unwrap().format, "tar.gz");
        assert_eq!(arcfs::index_of(&tar).unwrap().format, "tar");
        assert_eq!(arcfs::shown_name("Pack.tar.gz!"), "Pack.tar.gz");

        // its files as the loaders see them; a file of its song read: the
        // song unpacked, its other files then read from memory
        let song = a.parent().unwrap();
        assert!(vfs::is_dir(song) && vfs::is_file(&a));
        let opened = arcfs::times_opened(&tgz);
        let k = vfs::DirIndex::new(song)
            .find("KICK.ogg", vfs::AUDIO_EXTS)
            .unwrap();
        assert_eq!(vfs::read(&k).unwrap(), kick);
        assert!(arcfs::is_ready(&tgz, Path::new("Song/a.bme")));
        assert_eq!(vfs::read(&song.join("bga/stage.png")).unwrap(), b"png");
        assert_eq!(vfs::read(&a).unwrap(), chart("A").into_bytes());
        assert_eq!(arcfs::times_opened(&tgz), opened + 1);
        // a play: the song already unpacked, kept
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        assert_eq!(vfs::read(&k).unwrap(), kick);
        assert_eq!(arcfs::times_opened(&tgz), opened + 1);
        assert_eq!(vfs::read(&b_path).unwrap(), b.as_bytes());
        vfs::keep_song(None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Charts in a zip, a solid 7z and a RAR: scanned as folders (an
    /// unchanged archive not opened again, a changed one read again), read
    /// and played through the file system as files on a disk are.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn charts_in_archives() {
        use crate::platform::arcfs;
        let dir = std::env::temp_dir().join(format!("vibebms-lib-arc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let packs = dir.join("packs");
        std::fs::create_dir_all(&packs).unwrap();
        let cache = dir.join("cache");
        let chart = |title: &str| {
            format!("#TITLE {title}\n#BPM 150\n#WAV01 kick.wav\n#00111:01\n#00112:0101\n")
        };
        let kick = wav(2205);
        let zip_of = |extra: bool| {
            let mut files: Vec<(&str, Vec<u8>)> = vec![
                ("Song/a.bme", chart("A").into_bytes()),
                ("Song/kick.wav", kick.clone()),
                ("Song/bga/stage.png", b"png".to_vec()),
                // a song in a folder of the song's (its own folder)
                ("Song/Another/b.bms", chart("B").into_bytes()),
                ("readme.txt", b"hi".to_vec()),
            ];
            if extra {
                files.push(("New/d.bme", chart("D").into_bytes()));
            }
            bmseeker::testkit::zip(&files)
        };
        let zip = packs.join("pack.zip");
        std::fs::write(&zip, zip_of(false)).unwrap();
        let c = chart("C");
        let seven_bytes = seven(&[
            ("notes/readme.txt", b"read me"),
            ("Other/c.bme", c.as_bytes()),
            ("Other/kick.wav", &kick),
            ("Other/big.bin", &[7u8; 50_000]),
        ]);
        let sz = dir.join("solid.7z");
        std::fs::write(&sz, seven_bytes).unwrap();
        // (a chart with no notes: listed, no song)
        std::fs::write(dir.join("song.rar"), bmseeker::testkit::SONG_RAR).unwrap();
        let run = || {
            let p = Arc::new(ScanProgress::default());
            scan(
                cache.join("songs.json"),
                vec![dir.clone()],
                [cache.clone()].into_iter().collect(),
                p.clone(),
            );
            p
        };
        let p = run();
        assert!(p.changed.load(Ordering::Relaxed));
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        let mut titles: Vec<(&str, &str)> = songs
            .iter()
            .map(|s| (s.title.as_str(), s.path.as_str()))
            .collect();
        titles.sort();
        let a = arcfs::join(&zip, Path::new("Song/a.bme"));
        let c_path = arcfs::join(&sz, Path::new("Other/c.bme"));
        assert_eq!(
            titles,
            [
                ("A", a.to_str().unwrap()),
                (
                    "B",
                    arcfs::join(&zip, Path::new("Song/Another/b.bms"))
                        .to_str()
                        .unwrap()
                ),
                ("C", c_path.to_str().unwrap()),
            ]
        );
        let song_a = songs.iter().find(|s| s.title == "A").unwrap();
        assert_eq!(song_a.notes, 3);
        assert_eq!(song_a.folder, packs.to_string_lossy());
        let song_c = songs.iter().find(|s| s.title == "C").unwrap();
        assert_eq!(
            song_c.folder,
            arcfs::join(&sz, Path::new("")).to_string_lossy()
        );
        assert_eq!(arcfs::shown_name("solid.7z!"), "solid.7z");
        assert!(arcfs::index_of(&dir.join("song.rar")).is_some_and(|ix| ix.files.len() == 2));

        // the folders and files in the archive, as the loaders see them
        let song = a.parent().unwrap();
        assert!(vfs::is_dir(song) && vfs::is_dir(&arcfs::join(&zip, Path::new(""))));
        assert!(vfs::is_file(&a) && !vfs::is_dir(&a));
        assert!(!vfs::is_file(&song.join("none.wav")));
        let mut names: Vec<String> = vfs::read_dir(song)
            .unwrap()
            .into_iter()
            .map(|e| {
                format!(
                    "{}{}",
                    e.path.file_name().unwrap().to_string_lossy(),
                    if e.is_dir { "/" } else { "" }
                )
            })
            .collect();
        names.sort();
        assert_eq!(names, ["Another/", "a.bme", "bga/", "kick.wav"]);
        assert_eq!(vfs::read(&a).unwrap(), chart("A").into_bytes());
        assert_eq!(vfs::stamp(&song.join("kick.wav")).1, kick.len() as i64);
        // a keysound found (`#WAV01 kick.wav`) and decoded as in a play
        let index = vfs::DirIndex::new(song);
        let k = index.find("KICK.ogg", vfs::AUDIO_EXTS).unwrap();
        assert_eq!(k, song.join("kick.wav"));
        let bytes = vfs::read(&k).unwrap();
        assert!(vibebms_audio::decode_bytes(bytes, "wav", 44100).is_ok());
        // nothing is written into an archive
        assert!(vfs::write(&song.join("x.txt"), b"x").is_err());
        assert!(!dir.join("packs/pack.zip!").exists());

        // a play: its song unpacked once, kept until another is chosen
        let opened = arcfs::times_opened(&zip);
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        assert!(arcfs::is_ready(&zip, Path::new("Song/bga/stage.png")));
        assert_eq!(vfs::read(&song.join("bga/stage.png")).unwrap(), b"png");
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        vfs::keep_song(Some(&a));
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        assert_eq!(vfs::read(&k).unwrap(), kick);
        // (read for the sound bank: not copied out of the song in memory)
        assert!(Arc::ptr_eq(
            &vfs::read_shared(&k).unwrap(),
            &vfs::read_shared(&k).unwrap()
        ));
        assert_eq!(arcfs::times_opened(&zip), opened + 1);
        vfs::keep_song(Some(&c_path));
        assert!(!arcfs::is_ready(&zip, Path::new("Song/a.bme")));

        // a 7z's file: its song unpacked (read from the start), then read
        // from memory
        let other = c_path.parent().unwrap();
        let opened = arcfs::times_opened(&sz);
        // (a file of no song: read alone, the rest not decoded)
        let notes = arcfs::join(&sz, Path::new("notes/readme.txt"));
        assert_eq!(vfs::read(&notes).unwrap(), b"read me");
        assert_eq!(vfs::read(&notes).unwrap(), b"read me");
        assert_eq!(arcfs::times_opened(&sz), opened + 1);
        let opened = opened + 1;
        assert_eq!(vfs::read(&other.join("kick.wav")).unwrap(), kick);
        assert_eq!(
            vfs::read(&other.join("big.bin")).unwrap(),
            vec![7u8; 50_000]
        );
        assert_eq!(vfs::read(&c_path).unwrap(), c.as_bytes());
        assert_eq!(arcfs::times_opened(&sz), opened + 1);
        // a large song goes into a folder: a video's file is there
        arcfs::set_memory_limit(Some(1000));
        vfs::prepare_songs(std::slice::from_ref(&a)).unwrap();
        arcfs::set_memory_limit(None);
        assert!(arcfs::in_folder(&zip, Path::new("Song/kick.wav")));
        assert_eq!(vfs::read(&k).unwrap(), kick);
        let on_disk = vfs::native_path(&k).unwrap();
        // (in the first cache's `unpacked/` of the run)
        assert!(on_disk.is_file() && arcfs::split(&on_disk).is_none());
        assert!(on_disk.parent().unwrap().ends_with("Song"));
        assert_eq!(std::fs::read(&on_disk).unwrap(), kick);
        vfs::keep_song(None);

        // unchanged: no archive opened, nothing read
        let opens = || [&zip, &sz, &dir.join("song.rar")].map(|a| arcfs::times_opened(a));
        let before = opens();
        let p = run();
        assert_eq!(opens(), before);
        assert_eq!(p.to_parse.load(Ordering::Relaxed), 0);
        assert!(!p.changed.load(Ordering::Relaxed));
        // (nor from the index kept on disk)
        let kept = std::fs::read_to_string(cache.join("archives.json")).unwrap();
        assert!(kept.contains("Song/kick.wav"), "{kept}");

        // the zip changed: read again (the 7z not)
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&zip, zip_of(true)).unwrap();
        let before = opens();
        let p = run();
        assert!(p.changed.load(Ordering::Relaxed));
        assert_eq!(opens(), [before[0] + 1, before[1], before[2]]);
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(songs.len(), 4);
        assert!(songs.iter().any(|s| s.title == "D"
            && s.path == arcfs::join(&zip, Path::new("New/d.bme")).to_string_lossy()));

        // the zip unpacked beside it: its charts listed once, as files
        let copy = packs.join("pack/Song");
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("a.bme"), chart("A")).unwrap();
        run();
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        let a_paths: Vec<&str> = songs
            .iter()
            .filter(|s| s.title == "A")
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(a_paths, [copy.join("a.bme").to_str().unwrap()]);
        std::fs::remove_dir_all(packs.join("pack")).unwrap();

        // the archive gone: its charts go
        std::fs::remove_file(&sz).unwrap();
        run();
        let songs = Library::open(&cache, &cache, |_, _| None).songs();
        assert_eq!(songs.len(), 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
