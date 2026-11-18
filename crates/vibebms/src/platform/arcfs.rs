//! Songs kept in archives: a `.zip`, `.7z`, `.rar`, `.lzh` (`.lha`) or
//! `.tar` (`.tar.gz`, `.tgz`) in the library is read as a folder, without
//! unpacking it. A file in one is named by the archive's path, a `!`, and
//! its path in the archive: `bms/pack.zip!/Song/chart.bme` (a folder named
//! so on a disk is taken for the archive). [`super::vfs`] reads such
//! paths through here, so the chart, sound and image loaders read them as
//! any other file.
//!
//! - The scan lists an archive and reads its charts once per change of
//!   its size and time ([`unpack`]); the list of its files (the index) is
//!   kept in `cache/archives.json`, which is what the folder's files are
//!   looked up in (no archive is opened to list a folder).
//! - Playing a chart in one unpacks its song: the folder holding the chart
//!   in the archive, its sub-folders too ([`prepare`]), into memory, or
//!   natively into `cache/unpacked/` when it is large ([`MEMORY`]). One
//!   song is kept so ([`retain`] lets it go when another is chosen).
//! - A single file read when its song is not unpacked: a zip's own (zips
//!   are read where a file is, nothing else decoded); in a 7z, RAR, LZH or
//!   tar, which are read from the start (a solid one, or a gzip-compressed
//!   tar, cannot be read otherwise), its song is unpacked, else the file
//!   alone (decoding stops there).
//!
//! The formats are BMSeeker's (`bmseeker::unpack`, which decodes the 7z,
//! RAR, LZH and tar here: on the web too); a zip's files are read with the
//! `zip` crate it uses, as names are decoded there (Shift_JIS, EUC-KR).
//! Nothing is ever written into an archive.

use super::vfs::Entry;
use bmseeker::fs::ReadSeek;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// What follows an archive's name in a path to a file in it.
pub const MARK: char = '!';

/// Archive file names, by extension (`tar.gz`: a name's last two).
const EXTS: &[&str] = &["zip", "7z", "rar", "lzh", "lha", "tar", "tgz", "tar.gz"];

/// A song this large or smaller is unpacked into memory; natively a larger
/// one goes into a folder (`cache/unpacked/`). On the web, which has no
/// folder for it, a song up to [`WEB_MAX`] is unpacked into memory.
pub const MEMORY: u64 = if cfg!(any(target_os = "android", target_os = "ios")) {
    192 << 20
} else {
    512 << 20
};

/// [`MEMORY`], or a test's.
#[cfg(not(target_arch = "wasm32"))]
fn memory_limit() -> u64 {
    #[cfg(test)]
    if let Some(n) = *TEST_MEMORY.lock().unwrap_or_else(|e| e.into_inner()) {
        return n;
    }
    MEMORY
}

#[cfg(test)]
static TEST_MEMORY: Mutex<Option<u64>> = Mutex::new(None);

/// Songs larger than `n` bytes are unpacked into a folder (tests).
#[cfg(test)]
pub fn set_memory_limit(n: Option<u64>) {
    *TEST_MEMORY.lock().unwrap_or_else(|e| e.into_inner()) = n;
}

/// Whether the file `rel` of `archive` is unpacked into a folder (tests).
#[cfg(all(test, not(target_arch = "wasm32")))]
pub fn in_folder(archive: &Path, rel: &Path) -> bool {
    mounted(archive, rel).is_some_and(|m| matches!(m.files, Files::Folder(_)))
}

/// The largest song unpacked in a web page.
pub const WEB_MAX: u64 = 1 << 30;

/// The largest chart file read by the scan (larger ones are no chart).
const CHART_MAX: u64 = 32 << 20;

/// Single files kept after they were read: their bytes in all, and the
/// largest kept.
const RECENT_BYTES: usize = 16 << 20;
const RECENT_ONE: usize = 4 << 20;

/// Whether `p` names an archive (by its extension).
pub fn is_archive(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| archive_stem(&n.to_string_lossy()).is_some())
}

/// The file name `name` without its archive extension, if it has one, in
/// any case (`Song.TAR.GZ`: `Song`).
pub fn archive_stem(name: &str) -> Option<&str> {
    EXTS.iter().find_map(|e| {
        let at = name.len().checked_sub(e.len() + 1)?;
        let (stem, ext) = (name.get(..at)?, name.get(at..)?);
        (ext.starts_with('.') && ext[1..].eq_ignore_ascii_case(e) && !stem.is_empty())
            .then_some(stem)
    })
}

/// `p` as the archive it is in and its path there, if it is in one
/// (`a/pack.zip!/Song/x.bme`: `a/pack.zip`, `Song/x.bme`; `a/pack.zip!`:
/// the archive's top).
pub fn split(p: &Path) -> Option<(PathBuf, PathBuf)> {
    if !p.as_os_str().as_encoded_bytes().contains(&(MARK as u8)) {
        return None;
    }
    let mut archive = PathBuf::new();
    let mut comps = p.components();
    while let Some(c) = comps.next() {
        if let Component::Normal(n) = c {
            if let Some(name) = n.to_str().and_then(|s| s.strip_suffix(MARK)) {
                if is_archive(Path::new(name)) {
                    archive.push(name);
                    let mut rel = PathBuf::new();
                    for c in comps {
                        match c {
                            Component::Normal(n) => rel.push(n),
                            Component::ParentDir => {
                                rel.pop();
                            }
                            _ => {}
                        }
                    }
                    return Some((archive, rel));
                }
            }
        }
        archive.push(c);
    }
    None
}

/// The path of `rel` in the archive `archive`.
pub fn join(archive: &Path, rel: &Path) -> PathBuf {
    let mut s = archive.as_os_str().to_owned();
    s.push(MARK.to_string());
    let p = PathBuf::from(s);
    if rel.as_os_str().is_empty() {
        p
    } else {
        p.join(rel)
    }
}

/// A folder's name as shown: an archive's without its mark (`pack.zip!`:
/// `pack.zip`).
pub fn shown_name(name: &str) -> &str {
    match name.strip_suffix(MARK) {
        Some(n) if is_archive(Path::new(n)) => n,
        _ => name,
    }
}

/// An archive's files, as listed when it was last read.
#[derive(Debug, Default)]
pub struct Index {
    /// The archive's (modification time, size) then.
    pub stamp: (i64, i64),
    /// Its format's extension ("zip", "7z", "rar", "lzh", "tar",
    /// "tar.gz"); empty: it could not be read.
    pub format: String,
    /// Why it could not be read.
    pub error: String,
    /// Its files and their sizes.
    pub files: BTreeMap<PathBuf, u64>,
    /// The folders holding a chart.
    chart_dirs: HashSet<PathBuf>,
}

impl Index {
    pub fn new(stamp: (i64, i64), format: &str, files: BTreeMap<PathBuf, u64>) -> Index {
        let chart_dirs = files
            .keys()
            .filter(|p| crate::library::is_chart(p))
            .map(|p| p.parent().map(Path::to_path_buf).unwrap_or_default())
            .collect();
        Index {
            stamp,
            format: format.into(),
            error: String::new(),
            files,
            chart_dirs,
        }
    }

    /// An archive that could not be read (it is tried again once it
    /// changes).
    pub fn failed(stamp: (i64, i64), error: String) -> Index {
        Index {
            stamp,
            error,
            ..Index::default()
        }
    }

    /// Its charts.
    pub fn charts(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys().filter(|p| crate::library::is_chart(p))
    }

    /// The files at and below `dir`.
    fn below<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = (&'a PathBuf, &'a u64)> {
        use std::ops::Bound;
        self.files
            .range::<Path, _>((Bound::Included(dir), Bound::Unbounded))
            .take_while(move |(k, _)| k.starts_with(dir))
    }

    fn is_dir(&self, rel: &Path) -> bool {
        self.below(rel).any(|(k, _)| k != rel)
    }

    /// The files and folders right in `dir`.
    fn children(&self, dir: &Path) -> Vec<(PathBuf, bool)> {
        let mut out: Vec<(PathBuf, bool)> = Vec::new();
        for (k, _) in self.below(dir) {
            let Ok(rest) = k.strip_prefix(dir) else {
                continue;
            };
            let mut c = rest.components();
            let Some(first) = c.next() else { continue };
            let name = PathBuf::from(first.as_os_str());
            // (a folder's files are one run)
            if out.last().is_some_and(|(n, _)| *n == name) {
                continue;
            }
            out.push((name, c.next().is_some()));
        }
        out
    }

    /// The song `rel` is of: the nearest folder above it holding a chart.
    fn song_of(&self, rel: &Path) -> Option<PathBuf> {
        rel.ancestors()
            .skip(1)
            .find(|d| self.chart_dirs.contains(*d))
            .map(Path::to_path_buf)
    }
}

/// `cache/archives.json`: an archive's index by its path.
#[derive(Serialize, Deserialize)]
struct Stored {
    mtime: i64,
    size: i64,
    #[serde(default)]
    format: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    error: String,
    /// (path with `/`, size)
    #[serde(default)]
    files: Vec<(String, u64)>,
}

fn rel_text(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// A song unpacked: the files below `folder` in `archive`.
struct Mount {
    archive: PathBuf,
    stamp: (i64, i64),
    folder: PathBuf,
    files: Files,
    /// Natively, files in memory written out for a program that reads a
    /// file (a video for ffmpeg).
    #[cfg(not(target_arch = "wasm32"))]
    spill: Mutex<Option<PathBuf>>,
}

enum Files {
    Memory(HashMap<PathBuf, Arc<Vec<u8>>>),
    #[cfg(not(target_arch = "wasm32"))]
    Folder(PathBuf),
}

impl Mount {
    fn covers(&self, archive: &Path, rel: &Path) -> bool {
        self.archive == archive && rel.starts_with(&self.folder)
    }

    fn read(&self, rel: &Path) -> Option<io::Result<Vec<u8>>> {
        match &self.files {
            Files::Memory(m) => m.get(rel).map(|b| Ok(b.to_vec())),
            #[cfg(not(target_arch = "wasm32"))]
            Files::Folder(d) => Some(std::fs::read(d.join(rel))),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Mount {
    fn drop(&mut self) {
        let mut dirs: Vec<PathBuf> = self
            .spill
            .get_mut()
            .ok()
            .and_then(Option::take)
            .into_iter()
            .collect();
        if let Files::Folder(d) = &self.files {
            dirs.push(d.clone());
        }
        if !dirs.is_empty() {
            // (a song's files: not on the main thread)
            super::spawn("unpacked files", move || {
                for d in dirs {
                    let _ = std::fs::remove_dir_all(d);
                }
            });
        }
    }
}

/// A file read alone: its path, its archive's stamp, its bytes.
type Recent = (PathBuf, (i64, i64), Arc<Vec<u8>>);

#[derive(Default)]
struct State {
    index: HashMap<PathBuf, Arc<Index>>,
    /// Index files read.
    loaded: HashSet<PathBuf>,
    /// Where large songs are unpacked (`cache/unpacked`).
    work: Option<PathBuf>,
    mounts: Vec<Arc<Mount>>,
    /// Single files read lately: (path, archive's stamp, bytes).
    recent: VecDeque<Recent>,
    /// How many times each archive was opened (tests).
    opened: HashMap<PathBuf, usize>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);
/// Held while a song is unpacked (one at a time).
static MOUNTING: Mutex<()> = Mutex::new(());

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read the index kept in `file` (once a run); large songs are unpacked
/// beside it (`unpacked/`, emptied now: what a run before left there).
pub fn load_index(file: &Path) {
    let mut st = state();
    if !st.loaded.insert(file.to_path_buf()) {
        return;
    }
    if let Some(dir) = file.parent() {
        let work = dir.join("unpacked");
        #[cfg(not(target_arch = "wasm32"))]
        if st.work.is_none() {
            let _ = std::fs::remove_dir_all(&work);
        }
        st.work.get_or_insert(work);
    }
    let stored: HashMap<String, Stored> = super::vfs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for (a, s) in stored {
        let stamp = (s.mtime, s.size);
        let ix = if s.format.is_empty() {
            Index::failed(stamp, s.error)
        } else {
            let files = (s.files.into_iter())
                .filter_map(|(p, n)| Some((bmseeker_safe_rel(&p)?, n)))
                .collect();
            Index::new(stamp, &s.format, files)
        };
        st.index
            .entry(PathBuf::from(a))
            .or_insert_with(|| Arc::new(ix));
    }
}

/// Write the index of the archives `keep` keeps to `file` (the others are
/// forgotten).
pub fn save_index(file: &Path, keep: &dyn Fn(&Path) -> bool) {
    let stored: BTreeMap<String, Stored> = {
        let mut st = state();
        st.index.retain(|a, _| keep(a));
        st.index
            .iter()
            .map(|(a, ix)| {
                let s = Stored {
                    mtime: ix.stamp.0,
                    size: ix.stamp.1,
                    format: ix.format.clone(),
                    error: ix.error.clone(),
                    files: ix.files.iter().map(|(p, n)| (rel_text(p), *n)).collect(),
                };
                (a.to_string_lossy().into_owned(), s)
            })
            .collect()
    };
    match serde_json::to_vec(&stored) {
        Ok(j) => {
            if let Err(e) = super::vfs::write(file, &j) {
                log::error!("saving the archives' index: {e}");
            }
        }
        Err(e) => log::error!("saving the archives' index: {e}"),
    }
}

/// The index of `archive`, as last read.
pub fn index_of(archive: &Path) -> Option<Arc<Index>> {
    state().index.get(archive).cloned()
}

/// The archives with an index.
pub fn indexed() -> Vec<PathBuf> {
    state().index.keys().cloned().collect()
}

pub fn set_index(archive: &Path, ix: Index) {
    state().index.insert(archive.to_path_buf(), Arc::new(ix));
}

/// How many times `archive` was opened (tests).
pub fn times_opened(archive: &Path) -> usize {
    state().opened.get(archive).copied().unwrap_or(0)
}

/// `name` from an archive as BMSeeker makes it a path (no way out of its
/// folder).
fn bmseeker_safe_rel(name: &str) -> Option<PathBuf> {
    let rel: PathBuf = name
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != "." && *c != "..")
        .filter(|c| !c.contains(':'))
        .collect();
    (!rel.as_os_str().is_empty()).then_some(rel)
}

fn open(archive: &Path) -> io::Result<Box<dyn ReadSeek + Send>> {
    *state().opened.entry(archive.to_path_buf()).or_default() += 1;
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(Box::new(io::BufReader::new(std::fs::File::open(archive)?)))
    }
    // (in memory once read in: `vfs::Prefetch`)
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Box::new(io::Cursor::new(super::vfs::read(archive)?)))
    }
}

/// Where the files wanted go.
pub enum Dest {
    Memory,
    #[cfg(not(target_arch = "wasm32"))]
    Folder(PathBuf),
}

/// What [`unpack`] read.
#[derive(Default)]
pub struct Unpacked {
    pub format: String,
    /// Every file in the archive and its size (when asked for).
    pub listing: Option<BTreeMap<PathBuf, u64>>,
    /// The files wanted, in memory ([`Dest::Memory`]).
    pub files: Vec<(PathBuf, Vec<u8>)>,
}

/// The first bytes of an archive its format is told by.
const HEAD: usize = 4096;

/// Said by [`Capture`] once it has what it wants (the rest is not decoded).
const ENOUGH: &str = "vibebms: the files wanted are read";

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, "the files wanted are too large")
}

/// Read the archive `archive`: the files `want` takes into `dest` (at
/// most `cap` bytes of them), and with `list` the list of all its files.
/// `count`: how many files are wanted, if known (a 7z, RAR, LZH or tar is
/// not decoded past the last of them; not with `list`). An error of kind
/// `WouldBlock` (web): the archive is not in memory yet.
pub fn unpack(
    archive: &Path,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    count: Option<usize>,
    dest: &Dest,
    cap: u64,
) -> io::Result<Unpacked> {
    let mut r = open(archive)?;
    // (a tar's first header; a gzip's compressed)
    let mut head = Vec::with_capacity(HEAD);
    (&mut r).take(HEAD as u64).read_to_end(&mut head)?;
    r.seek(io::SeekFrom::Start(0))?;
    let format = bmseeker::unpack::sniff(&head)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an archive"))?;
    if format == bmseeker::unpack::Format::Zip {
        return unzip(r, want, list, dest, cap);
    }
    let cap = Capture {
        want,
        dest,
        count: count.filter(|_| !list),
        cap,
        reader: Mutex::new(Some(r)),
        out: Mutex::default(),
    };
    let r = bmseeker::unpack::unpack(
        bmseeker::unpack::Blob::File(archive.to_path_buf()),
        &cap,
        Path::new(""),
        u64::MAX,
    );
    let out = cap.out.into_inner().unwrap_or_else(|e| e.into_inner());
    match r {
        Ok(_) => {}
        Err(e) if e.contains(ENOUGH) && !list => {}
        Err(_) if out.too_large => return Err(too_large()),
        Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
    Ok(Unpacked {
        format: format.ext().into(),
        listing: list.then_some(out.listing),
        files: out.files,
    })
}

/// A zip: its list from its central directory, the files wanted read
/// where they are (nothing else decoded).
fn unzip(
    r: Box<dyn ReadSeek + Send>,
    want: &(dyn Fn(&Path) -> bool + Sync),
    list: bool,
    dest: &Dest,
    cap: u64,
) -> io::Result<Unpacked> {
    let bad = |e: zip::result::ZipError| io::Error::new(io::ErrorKind::InvalidData, e);
    let mut z = zip::ZipArchive::new(r).map_err(bad)?;
    let mut listing = BTreeMap::new();
    let mut wanted = Vec::new();
    for i in 0..z.len() {
        let e = z.by_index_data(i).map_err(bad)?;
        // (as BMSeeker unpacks a zip: no folders, no links)
        if e.is_dir() || e.is_symlink() {
            continue;
        }
        let Some(rel) = bmseeker_safe_rel(&super::vfs::decode_name(e.name_raw())) else {
            continue;
        };
        if want(&rel) {
            wanted.push((i, rel.clone()));
        }
        listing.insert(rel, e.size());
    }
    let mut files = Vec::new();
    let mut left = cap;
    for (i, rel) in wanted {
        let e = z.by_index(i).map_err(bad)?;
        if e.size() > left {
            return Err(too_large());
        }
        let mut e = e.take(left + 1);
        match dest {
            Dest::Memory => {
                let mut b = Vec::new();
                e.read_to_end(&mut b)?;
                left = left.checked_sub(b.len() as u64).ok_or_else(too_large)?;
                files.push((rel, b));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Dest::Folder(d) => {
                let p = d.join(&rel);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut f = io::BufWriter::new(std::fs::File::create(&p)?);
                let n = io::copy(&mut e, &mut f)?;
                f.flush()?;
                left = left.checked_sub(n).ok_or_else(too_large)?;
            }
        }
    }
    Ok(Unpacked {
        format: "zip".into(),
        listing: list.then_some(listing),
        files,
    })
}

/// BMSeeker's unpacking into this: the files wanted kept, the others only
/// counted.
struct Capture<'a> {
    want: &'a (dyn Fn(&Path) -> bool + Sync),
    dest: &'a Dest,
    count: Option<usize>,
    cap: u64,
    /// The archive, read once.
    reader: Mutex<Option<Box<dyn ReadSeek + Send>>>,
    out: Mutex<CaptureOut>,
}

#[derive(Default)]
struct CaptureOut {
    listing: BTreeMap<PathBuf, u64>,
    files: Vec<(PathBuf, Vec<u8>)>,
    /// Files wanted taken.
    taken: usize,
    kept: u64,
    too_large: bool,
}

enum Keep {
    Skip,
    Memory(Vec<u8>),
    #[cfg(not(target_arch = "wasm32"))]
    File(io::BufWriter<std::fs::File>),
}

/// A file being unpacked.
struct Member<'a> {
    cap: &'a Capture<'a>,
    rel: PathBuf,
    keep: Keep,
    n: u64,
    /// What it may keep.
    left: u64,
}

impl Write for Member<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.n += buf.len() as u64;
        let over = || {
            self.cap
                .out
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .too_large = true;
            too_large()
        };
        match &mut self.keep {
            Keep::Skip => {}
            Keep::Memory(v) => {
                if self.n > self.left {
                    return Err(over());
                }
                v.extend_from_slice(buf);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => {
                if self.n > self.left {
                    return Err(over());
                }
                f.write_all(buf)?;
            }
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        match &mut self.keep {
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(f) => f.flush(),
            _ => Ok(()),
        }
    }
}

impl Drop for Member<'_> {
    fn drop(&mut self) {
        let mut out = self.cap.out.lock().unwrap_or_else(|e| e.into_inner());
        out.listing.insert(self.rel.clone(), self.n);
        match std::mem::replace(&mut self.keep, Keep::Skip) {
            Keep::Skip => return,
            Keep::Memory(v) => out.files.push((self.rel.clone(), v)),
            #[cfg(not(target_arch = "wasm32"))]
            Keep::File(_) => {}
        }
        out.taken += 1;
        out.kept += self.n;
    }
}

impl bmseeker::fs::Fs for Capture<'_> {
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            p.display().to_string(),
        ))
    }
    fn read_dir(&self, _: &Path) -> io::Result<Vec<bmseeker::fs::Entry>> {
        Ok(Vec::new())
    }
    fn rename(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    fn stamp(&self, _: &Path) -> Option<(i64, u64)> {
        None
    }
    /// A file that could not be unpacked in full: it goes.
    fn remove(&self, p: &Path) -> io::Result<()> {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = out.files.iter().position(|(r, _)| r == p) {
            let (_, b) = out.files.remove(i);
            out.taken -= 1;
            out.kept -= b.len() as u64;
        }
        out.listing.remove(p);
        #[cfg(not(target_arch = "wasm32"))]
        if let Dest::Folder(d) = self.dest {
            let _ = std::fs::remove_file(d.join(p));
        }
        Ok(())
    }
}

impl bmseeker::fs::Store for Capture<'_> {
    fn create(&self, p: &Path, _mtime: Option<i64>) -> io::Result<Box<dyn Write + '_>> {
        let (left, enough) = {
            let out = self.out.lock().unwrap_or_else(|e| e.into_inner());
            (
                self.cap.saturating_sub(out.kept),
                self.count.is_some_and(|n| out.taken >= n),
            )
        };
        if enough {
            return Err(io::Error::other(ENOUGH));
        }
        let keep = match (self.want)(p) {
            false => Keep::Skip,
            true => match self.dest {
                Dest::Memory => Keep::Memory(Vec::new()),
                #[cfg(not(target_arch = "wasm32"))]
                Dest::Folder(d) => {
                    let to = d.join(p);
                    if let Some(parent) = to.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    Keep::File(io::BufWriter::new(std::fs::File::create(to)?))
                }
            },
        };
        Ok(Box::new(Member {
            cap: self,
            rel: p.to_path_buf(),
            keep,
            n: 0,
            left,
        }))
    }
    fn remove_dir(&self, _: &Path) {}
    /// The archive (once); an archive in it is not read.
    fn open(&self, p: &Path) -> io::Result<Box<dyn ReadSeek + Send>> {
        let r = self.reader.lock().unwrap_or_else(|e| e.into_inner()).take();
        r.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, p.display().to_string()))
    }
}

/// The current (modification time, size) of `archive`.
fn stamp_now(archive: &Path) -> (i64, i64) {
    super::vfs::stamp(archive)
}

/// `archive`'s index: as the scan left it; a zip's read now when there is
/// none or it changed (its list is quick to read). With `slow`, a 7z's,
/// RAR's, LZH's or tar's too (decoded in full: not for the main thread).
fn index(archive: &Path, slow: bool) -> Option<Arc<Index>> {
    let now = stamp_now(archive);
    let have = index_of(archive);
    if let Some(ix) = &have {
        if ix.stamp == now || (ix.format != "zip" && !slow) {
            return have;
        }
    }
    if now == (0, 0) && !super::vfs::is_file(archive) {
        return None;
    }
    let is_zip = {
        let mut head = [0u8; 4];
        open_head(archive, &mut head) && head.starts_with(b"PK")
    };
    if !is_zip && !slow {
        return have;
    }
    match unpack(archive, &|_| false, true, None, &Dest::Memory, 0) {
        Ok(u) => {
            let ix = Index::new(now, &u.format, u.listing.unwrap_or_default());
            set_index(archive, ix);
            index_of(archive)
        }
        Err(e) => {
            log::debug!("{}: {e}", archive.display());
            have
        }
    }
}

/// The first bytes of `archive` into `head`.
fn open_head(archive: &Path, head: &mut [u8]) -> bool {
    super::vfs::read_head(archive, head.len())
        .map(|b| {
            head[..b.len()].copy_from_slice(&b);
            b.len() == head.len()
        })
        .unwrap_or(false)
}

fn mounted(archive: &Path, rel: &Path) -> Option<Arc<Mount>> {
    state()
        .mounts
        .iter()
        .find(|m| m.covers(archive, rel))
        .cloned()
}

/// Whether the file `rel` of `archive` is in memory or unpacked now.
pub fn is_ready(archive: &Path, rel: &Path) -> bool {
    mounted(archive, rel).is_some()
}

pub fn is_file(archive: &Path, rel: &Path) -> bool {
    index(archive, false).is_some_and(|ix| ix.files.contains_key(rel))
}

pub fn is_dir(archive: &Path, rel: &Path) -> bool {
    match index(archive, false) {
        Some(ix) if rel.as_os_str().is_empty() => ix.error.is_empty(),
        Some(ix) => ix.is_dir(rel),
        None => false,
    }
}

pub fn read_dir(archive: &Path, rel: &Path) -> io::Result<Vec<Entry>> {
    let ix = index(archive, false)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, archive.display().to_string()))?;
    if !rel.as_os_str().is_empty() && !ix.is_dir(rel) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            join(archive, rel).display().to_string(),
        ));
    }
    let base = join(archive, rel);
    Ok(ix
        .children(rel)
        .into_iter()
        .map(|(name, is_dir)| Entry {
            path: base.join(name),
            is_dir,
        })
        .collect())
}

/// (the archive's modification time, the file's size)
pub fn stamp(archive: &Path, rel: &Path) -> (i64, i64) {
    match index(archive, false) {
        Some(ix) => match ix.files.get(rel) {
            Some(n) => (ix.stamp.0, *n as i64),
            None => (0, 0),
        },
        None => (0, 0),
    }
}

fn not_found(archive: &Path, rel: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        join(archive, rel).display().to_string(),
    )
}

/// Read the file `rel` of `archive`.
pub fn read(archive: &Path, rel: &Path) -> io::Result<Vec<u8>> {
    if let Some(r) = mounted(archive, rel).and_then(|m| m.read(rel)) {
        return r;
    }
    let p = join(archive, rel);
    let now = stamp_now(archive);
    let recent = (state().recent.iter())
        .find(|(q, s, _)| *q == p && *s == now)
        .map(|(_, _, b)| b.clone());
    if let Some(b) = recent {
        return Ok(b.to_vec());
    }
    let ix = index(archive, true).ok_or_else(|| not_found(archive, rel))?;
    let Some(&size) = ix.files.get(rel) else {
        return Err(not_found(archive, rel));
    };
    // a 7z's, RAR's, LZH's or tar's file: its song unpacked (it is read
    // from the archive's start all the same: a .tar.gz is decompressed
    // once for the song, not once a file)
    if ix.format != "zip" {
        if let Some(song) = ix.song_of(rel) {
            mount_one(archive, &song)?;
            if let Some(r) = mounted(archive, rel).and_then(|m| m.read(rel)) {
                return r;
            }
        }
    }
    let want = |r: &Path| r == rel;
    let cap = size.max(1) << 1;
    let u = unpack(archive, &want, false, Some(1), &Dest::Memory, cap)?;
    let b = (u.files.into_iter().next())
        .map(|(_, b)| b)
        .ok_or_else(|| not_found(archive, rel))?;
    if b.len() <= RECENT_ONE {
        let mut st = state();
        st.recent.push_front((p, now, Arc::new(b.clone())));
        while st.recent.iter().map(|(_, _, b)| b.len()).sum::<usize>() > RECENT_BYTES {
            st.recent.pop_back();
        }
    }
    Ok(b)
}

/// The song (folder) `chart` is of in its archive.
fn song_folder(chart: &Path) -> Option<(PathBuf, PathBuf)> {
    let (a, rel) = split(chart)?;
    Some((a, rel.parent().map(Path::to_path_buf).unwrap_or_default()))
}

/// Unpack the songs of `charts` (those in an archive: the folders they
/// are in), keeping those already unpacked; every other song unpacked
/// before is let go (first: one song is kept at a time).
pub fn prepare(charts: &[PathBuf]) -> io::Result<()> {
    let mut want: Vec<(PathBuf, PathBuf)> = charts.iter().filter_map(|c| song_folder(c)).collect();
    want.dedup();
    let _one = MOUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let stamps: Vec<(i64, i64)> = want.iter().map(|(a, _)| stamp_now(a)).collect();
    let mut have: Vec<Arc<Mount>> = Vec::new();
    {
        let mut st = state();
        st.mounts.retain(|m| {
            want.iter()
                .zip(&stamps)
                .any(|((a, f), s)| m.archive == *a && m.folder == *f && m.stamp == *s)
        });
        have.extend(st.mounts.iter().cloned());
    }
    for ((a, f), s) in want.iter().zip(&stamps) {
        if have.iter().any(|m| m.archive == *a && m.folder == *f) {
            continue;
        }
        let m = Arc::new(mount(a, f, *s)?);
        state().mounts.push(m.clone());
        have.push(m);
    }
    Ok(())
}

/// Unpack the song `folder` of `archive` in place of those unpacked (a
/// file of it was read).
fn mount_one(archive: &Path, folder: &Path) -> io::Result<()> {
    prepare(&[join(archive, &folder.join("_"))])
}

/// Let go of the songs unpacked unless `chart` is of one of them (another
/// song was chosen).
pub fn retain(chart: Option<&Path>) {
    let keep = chart.and_then(song_folder);
    let gone: Vec<Arc<Mount>> = {
        let mut st = state();
        let (kept, gone) = std::mem::take(&mut st.mounts).into_iter().partition(|m| {
            keep.as_ref()
                .is_some_and(|(a, f)| m.archive == *a && m.folder == *f)
        });
        st.mounts = kept;
        gone
    };
    if !gone.is_empty() {
        log::debug!("archives: {} unpacked song(s) let go", gone.len());
    }
}

/// Unpack the files below `folder` in `archive`.
fn mount(archive: &Path, folder: &Path, stamp: (i64, i64)) -> io::Result<Mount> {
    let ix = index(archive, true).ok_or_else(|| not_found(archive, folder))?;
    let (count, total) = ix
        .below(folder)
        .fold((0usize, 0u64), |(c, t), (_, n)| (c + 1, t + n));
    let want = |r: &Path| r.starts_with(folder);
    log::info!(
        "{}: unpacking {} ({count} files, {} MB)",
        archive.display(),
        if folder.as_os_str().is_empty() {
            "its files".into()
        } else {
            folder.display().to_string()
        },
        total >> 20
    );
    #[cfg(not(target_arch = "wasm32"))]
    let files = if total > memory_limit() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let work = state()
            .work
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("vibebms-unpacked"));
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = work.join(format!("{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let dest = Dest::Folder(dir.clone());
        if let Err(e) = unpack(
            archive,
            &want,
            false,
            Some(count),
            &dest,
            total + (64 << 20),
        ) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Files::Folder(dir)
    } else {
        memory_files(archive, &want, count, total)?
    };
    #[cfg(target_arch = "wasm32")]
    let files = {
        if total > WEB_MAX {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!(
                    "{}: its song is {} MB, more than a web page unpacks",
                    archive.display(),
                    total >> 20
                ),
            ));
        }
        let files = memory_files(archive, &want, count, total);
        // the archive's bytes leave memory (read from storage again when
        // needed)
        super::vfs::release(archive);
        files?
    };
    Ok(Mount {
        archive: archive.to_path_buf(),
        stamp,
        folder: folder.to_path_buf(),
        files,
        #[cfg(not(target_arch = "wasm32"))]
        spill: Mutex::new(None),
    })
}

fn memory_files(
    archive: &Path,
    want: &(dyn Fn(&Path) -> bool + Sync),
    count: usize,
    total: u64,
) -> io::Result<Files> {
    let u = unpack(
        archive,
        want,
        false,
        Some(count),
        &Dest::Memory,
        total + (16 << 20),
    )?;
    Ok(Files::Memory(
        u.files.into_iter().map(|(p, b)| (p, Arc::new(b))).collect(),
    ))
}

/// A file on a disk with the bytes of `p` (in an archive: written out of
/// its unpacked song, which must be), for a program that reads files
/// itself (ffmpeg); `p` itself when it is in no archive.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_path(p: &Path) -> Option<PathBuf> {
    let Some((a, rel)) = split(p) else {
        return Some(p.to_path_buf());
    };
    let m = mounted(&a, &rel)?;
    match &m.files {
        Files::Folder(d) => Some(d.join(&rel)),
        Files::Memory(files) => {
            let bytes = files.get(&rel)?;
            let mut spill = m.spill.lock().unwrap_or_else(|e| e.into_inner());
            let dir = match &*spill {
                Some(d) => d.clone(),
                None => {
                    static N: std::sync::atomic::AtomicUsize =
                        std::sync::atomic::AtomicUsize::new(0);
                    let work = state()
                        .work
                        .clone()
                        .unwrap_or_else(|| std::env::temp_dir().join("vibebms-unpacked"));
                    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let d = work.join(format!("{}-spill-{n}", std::process::id()));
                    *spill = Some(d.clone());
                    d
                }
            };
            let to = dir.join(&rel);
            if !to.is_file() {
                std::fs::create_dir_all(to.parent()?).ok()?;
                std::fs::write(&to, bytes.as_slice()).ok()?;
            }
            Some(to)
        }
    }
}

/// The charts of `archive` and the files the scan needs: `want` (none:
/// all its charts, and its list of files). For the scan.
pub fn read_charts(archive: &Path, want: Option<&HashSet<PathBuf>>) -> io::Result<Unpacked> {
    let take = |r: &Path| crate::library::is_chart(r) && want.is_none_or(|w| w.contains(r));
    let count = want.map(HashSet::len);
    let mut u = unpack(
        archive,
        &take,
        want.is_none(),
        count,
        &Dest::Memory,
        CHART_MAX * 64,
    )?;
    u.files.retain(|(_, b)| b.len() as u64 <= CHART_MAX);
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_in_archives() {
        let p = Path::new("/bms/packs/pack.ZIP!/Song/a.bme");
        assert_eq!(
            split(p),
            Some((
                PathBuf::from("/bms/packs/pack.ZIP"),
                PathBuf::from("Song/a.bme")
            ))
        );
        assert_eq!(
            split(Path::new("/bms/x.7z!")),
            Some((PathBuf::from("/bms/x.7z"), PathBuf::new()))
        );
        assert_eq!(split(Path::new("/bms/hey!/a.bms")), None);
        assert_eq!(split(Path::new("/bms/pack.zip/a.bms")), None);
        assert_eq!(
            join(Path::new("/bms/packs/pack.zip"), Path::new("Song/a.bme")),
            PathBuf::from("/bms/packs/pack.zip!/Song/a.bme")
        );
        assert_eq!(
            join(Path::new("a.rar"), Path::new("")),
            PathBuf::from("a.rar!")
        );
        assert_eq!(shown_name("pack.zip!"), "pack.zip");
        assert_eq!(shown_name("wow!"), "wow!");
        assert!(is_archive(Path::new("x.LZH")) && !is_archive(Path::new("x.gz")));
        assert_eq!(
            split(Path::new("/bms/Pack.TAR.GZ!/Song/a.bms")),
            Some((
                PathBuf::from("/bms/Pack.TAR.GZ"),
                PathBuf::from("Song/a.bms")
            ))
        );
        assert_eq!(shown_name("pack.tgz!"), "pack.tgz");
        assert_eq!(shown_name("x.tar!"), "x.tar");
        assert_eq!(archive_stem("Song.tar.gz"), Some("Song"));
        assert_eq!(archive_stem(".tar.gz"), None);
    }

    /// A file of a RAR read alone: decoding stops once it is read.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn read_up_to_the_files_wanted() {
        let d = std::env::temp_dir().join(format!("vibebms-arcfs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let rar = d.join("song.rar");
        std::fs::write(&rar, bmseeker::testkit::SONG_RAR).unwrap();
        let want = |r: &Path| r == Path::new("song/a.bms");
        let u = unpack(&rar, &want, false, Some(1), &Dest::Memory, 1 << 20).unwrap();
        assert_eq!(
            u.files,
            [(PathBuf::from("song/a.bms"), b"#TITLE A".to_vec())]
        );
        // listed in full
        let u = unpack(&rar, &|_| false, true, None, &Dest::Memory, 0).unwrap();
        assert_eq!(u.format, "rar");
        assert_eq!(
            u.listing.unwrap().into_iter().collect::<Vec<_>>(),
            [
                (PathBuf::from("song/01.ogg"), 3),
                (PathBuf::from("song/a.bms"), 8)
            ]
        );
        // more than may be kept
        let all = |_: &Path| true;
        let e = unpack(&rar, &all, false, None, &Dest::Memory, 4)
            .err()
            .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::OutOfMemory);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn an_index_lists_folders() {
        let files = ["Song/a.bms", "Song/bga/x.png", "Song/s.wav", "readme.txt"]
            .iter()
            .map(|p| (PathBuf::from(p), 1))
            .collect();
        let ix = Index::new((1, 2), "zip", files);
        assert_eq!(
            ix.children(Path::new("")),
            [(PathBuf::from("Song"), true), ("readme.txt".into(), false)]
        );
        assert_eq!(
            ix.children(Path::new("Song")),
            [
                (PathBuf::from("a.bms"), false),
                ("bga".into(), true),
                ("s.wav".into(), false)
            ]
        );
        assert!(ix.is_dir(Path::new("Song/bga")) && !ix.is_dir(Path::new("Song/s.wav")));
        assert_eq!(
            ix.song_of(Path::new("Song/bga/x.png")),
            Some(PathBuf::from("Song"))
        );
        assert_eq!(ix.song_of(Path::new("readme.txt")), None);
        assert_eq!(ix.charts().count(), 1);
    }
}
