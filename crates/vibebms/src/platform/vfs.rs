//! File system facade. Natively this is `std::fs`; on the web it is an
//! in-memory tree: files picked by the user (read lazily via [`prefetch`]),
//! plus small written files persisted in `localStorage`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;

    pub fn read(p: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(p)
    }
    pub fn write(p: &Path, data: &[u8]) -> io::Result<()> {
        // (an unset path: nothing to write, and no `.tmp~` left behind)
        if p.as_os_str().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "no path"));
        }
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // `a.json` and `a.toml` must not share one
        let mut tmp = p.as_os_str().to_owned();
        tmp.push(".tmp~");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, p)
    }
    pub fn remove_file(p: &Path) -> io::Result<()> {
        std::fs::remove_file(p)
    }
    pub fn create_dir_all(p: &Path) -> io::Result<()> {
        std::fs::create_dir_all(p)
    }
    /// Move a file; false when `to` exists or the move fails.
    pub fn rename(from: &Path, to: &Path) -> bool {
        !to.exists() && std::fs::rename(from, to).is_ok()
    }
    pub fn read_dir(p: &Path) -> io::Result<Vec<Entry>> {
        Ok(std::fs::read_dir(p)?
            .flatten()
            .map(|e| {
                let path = e.path();
                let is_dir = e
                    .file_type()
                    .map(|t| t.is_dir() || (t.is_symlink() && path.is_dir()))
                    .unwrap_or(false);
                Entry { path, is_dir }
            })
            .collect())
    }
    pub fn is_file(p: &Path) -> bool {
        p.is_file()
    }
    pub fn is_dir(p: &Path) -> bool {
        p.is_dir()
    }
    /// (modification time in seconds, size)
    pub fn stamp(p: &Path) -> (i64, i64) {
        std::fs::metadata(p)
            .map(|m| {
                let mt = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(crate::platform::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                (mt, m.len() as i64)
            })
            .unwrap_or((0, 0))
    }
    pub fn writable(dir: &Path) -> bool {
        let probe = dir.join(".vibebms-write-test");
        let ok = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }
    pub fn unloaded_under(_dir: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
    pub fn release_charts() {}

    pub fn take_added() -> bool {
        false
    }
    /// Remove the folder `dir` and everything below it.
    pub fn remove_dir(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::ops::Bound;
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;

    enum Data {
        Bytes(Rc<Vec<u8>>),
        File(web_sys::File),
    }

    struct Node {
        data: Data,
        size: i64,
        mtime: i64,
    }

    #[derive(Default)]
    struct WebFs {
        files: BTreeMap<PathBuf, Node>,
        added: bool,
        /// Charts kept in memory after their stored copy was written (the
        /// scan reads them): the stored copy, swapped in by
        /// [`release_charts`] once the library has them.
        stored_charts: BTreeMap<PathBuf, web_sys::File>,
    }

    thread_local! {
        static FS: RefCell<WebFs> = RefCell::new(WebFs::default());
    }

    const LS_PREFIX: &str = "vibebms:";

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    fn not_found(p: &Path) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, p.display().to_string())
    }

    pub fn read(p: &Path) -> io::Result<Vec<u8>> {
        let r = FS.with(|fs| {
            fs.borrow().files.get(p).map(|n| match &n.data {
                Data::Bytes(b) => Ok(b.as_ref().clone()),
                Data::File(_) => Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("{} not prefetched", p.display()),
                )),
            })
        });
        if let Some(r) = r {
            return r;
        }
        // persisted small files
        let key = format!("{LS_PREFIX}{}", p.display());
        if let Some(s) = storage().and_then(|s| s.get_item(&key).ok().flatten()) {
            return Ok(s.into_bytes());
        }
        Err(not_found(p))
    }

    pub fn write(p: &Path, data: &[u8]) -> io::Result<()> {
        FS.with(|fs| {
            fs.borrow_mut().files.insert(
                p.to_path_buf(),
                Node {
                    data: Data::Bytes(Rc::new(data.to_vec())),
                    size: data.len() as i64,
                    mtime: 0,
                },
            );
        });
        if data.len() < (1 << 20) {
            if let (Some(s), Ok(text)) = (storage(), std::str::from_utf8(data)) {
                let _ = s.set_item(&format!("{LS_PREFIX}{}", p.display()), text);
            }
        }
        Ok(())
    }

    pub fn remove_file(p: &Path) -> io::Result<()> {
        if FS.with(|fs| fs.borrow_mut().files.remove(p)).is_some() {
            super::super::opfs::remove(p);
        }
        if let Some(s) = storage() {
            let _ = s.remove_item(&format!("{LS_PREFIX}{}", p.display()));
        }
        Ok(())
    }

    pub fn create_dir_all(_p: &Path) -> io::Result<()> {
        Ok(())
    }

    /// The files at and below `p`: paths order by component, so they are
    /// one run of the map.
    fn below<'a>(
        files: &'a BTreeMap<PathBuf, Node>,
        p: &'a Path,
    ) -> impl Iterator<Item = (&'a PathBuf, &'a Node)> {
        files
            .range::<Path, _>((Bound::Included(p), Bound::Unbounded))
            .take_while(move |(k, _)| k.starts_with(p))
    }

    pub fn read_dir(p: &Path) -> io::Result<Vec<Entry>> {
        FS.with(|fs| {
            let fs = fs.borrow();
            let mut out: Vec<Entry> = Vec::new();
            let mut any = false;
            for (k, _) in below(&fs.files, p) {
                any = true;
                let Ok(rest) = k.strip_prefix(p) else {
                    continue;
                };
                let mut comps = rest.components();
                let Some(first) = comps.next() else { continue };
                // a folder's files are one run too
                if out.last().is_some_and(|e| e.path.ends_with(first)) {
                    continue;
                }
                out.push(Entry {
                    path: p.join(first.as_os_str()),
                    is_dir: comps.next().is_some(),
                });
            }
            if !any {
                return Err(not_found(p));
            }
            Ok(out)
        })
    }

    pub fn is_file(p: &Path) -> bool {
        FS.with(|fs| fs.borrow().files.contains_key(p))
            || storage()
                .and_then(|s| {
                    s.get_item(&format!("{LS_PREFIX}{}", p.display()))
                        .ok()
                        .flatten()
                })
                .is_some()
    }

    pub fn is_dir(p: &Path) -> bool {
        FS.with(|fs| below(&fs.borrow().files, p).any(|(k, _)| k != p))
    }

    pub fn stamp(p: &Path) -> (i64, i64) {
        FS.with(|fs| {
            fs.borrow()
                .files
                .get(p)
                .map(|n| (n.mtime, n.size))
                .unwrap_or((0, 0))
        })
    }

    pub fn writable(_dir: &Path) -> bool {
        true
    }

    pub fn unloaded_under(dir: &Path) -> Vec<PathBuf> {
        FS.with(|fs| {
            below(&fs.borrow().files, dir)
                .filter(|(_, n)| matches!(n.data, Data::File(_)))
                .map(|(k, _)| k.clone())
                .collect()
        })
    }

    pub fn take_added() -> bool {
        FS.with(|fs| std::mem::take(&mut fs.borrow_mut().added))
    }

    /// Have the library rescan (files were moved).
    pub fn mark_added() {
        FS.with(|fs| fs.borrow_mut().added = true);
    }

    /// Move a mounted file; false when `to` exists or `from` does not.
    pub fn rename(from: &Path, to: &Path) -> bool {
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            if fs.files.contains_key(to) {
                return false;
            }
            match fs.files.remove(from) {
                Some(n) => {
                    keep(to, &n.data);
                    super::super::opfs::remove(from);
                    fs.files.insert(to.to_path_buf(), n);
                    true
                }
                None => false,
            }
        })
    }

    /// Write `p`'s data to the browser's storage (if it is kept there);
    /// once written, `p` is read from there (its bytes leave memory; a
    /// chart's stay).
    fn keep(p: &Path, data: &Data) {
        let to = p.to_path_buf();
        let swap = move |file: web_sys::File| {
            FS.with(|fs| {
                let mut fs = fs.borrow_mut();
                let fs = &mut *fs;
                if let Some(n) = fs.files.get_mut(&to) {
                    if n.size == file.size() as i64 {
                        // its time as the stored file will have it
                        n.mtime = (file.last_modified() / 1000.0) as i64;
                        // a chart's bytes stay until the library has read
                        // it (the scan may be about to: a file swapped out
                        // here is not read until read in again)
                        if matches!(n.data, Data::Bytes(_)) && crate::library::is_chart(&to) {
                            fs.stored_charts.insert(to.clone(), file);
                        } else {
                            n.data = Data::File(file);
                        }
                    }
                }
            })
        };
        match data {
            Data::Bytes(b) => super::super::opfs::save(p, b, swap),
            Data::File(f) => super::super::opfs::save_file(p, f, swap),
        }
    }

    /// The charts kept in memory for the scan read from their stored copy
    /// from now on (their bytes leave memory): once the library has them.
    pub fn release_charts() {
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            let fs = &mut *fs;
            for (p, file) in std::mem::take(&mut fs.stored_charts) {
                if let Some(n) = fs.files.get_mut(&p) {
                    if matches!(n.data, Data::Bytes(_)) && n.size == file.size() as i64 {
                        n.data = Data::File(file);
                    }
                }
            }
        })
    }

    /// A file kept in the browser's storage, mounted (read when needed).
    pub fn mount(p: PathBuf, file: web_sys::File) {
        FS.with(|fs| {
            let size = file.size() as i64;
            let mtime = (file.last_modified() / 1000.0) as i64;
            fs.borrow_mut().files.insert(
                p,
                Node {
                    data: Data::File(file),
                    size,
                    mtime,
                },
            );
        });
    }

    /// Forget the files below `dir` read from the browser's storage (it
    /// changed there: they are mounted again).
    pub fn unmount(dir: &Path) {
        FS.with(|fs| {
            fs.borrow_mut()
                .files
                .retain(|k, n| !(k.starts_with(dir) && matches!(n.data, Data::File(_))))
        });
    }

    /// Remove the folder `dir` and everything below it (here and in the
    /// browser's storage); the library is read again.
    pub fn remove_dir(dir: &Path) {
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            fs.files.retain(|k, _| !k.starts_with(dir));
            fs.added = true;
        });
        super::super::opfs::remove(dir);
    }

    /// A folder right below the library: its size and when it was last
    /// written.
    pub struct Kept {
        pub dir: PathBuf,
        pub bytes: u64,
        pub files: usize,
        /// The newest of its files' times (s; 0: not known).
        pub newest: i64,
    }

    /// The folders right below `dir`, the largest first.
    pub fn folder_sizes(dir: &Path) -> Vec<Kept> {
        FS.with(|fs| {
            let mut by: BTreeMap<PathBuf, Kept> = BTreeMap::new();
            for (k, n) in fs.borrow().files.iter() {
                let Ok(rest) = k.strip_prefix(dir) else {
                    continue;
                };
                let mut c = rest.components();
                let (Some(first), Some(_)) = (c.next(), c.next()) else {
                    continue;
                };
                let d = dir.join(first.as_os_str());
                let e = by.entry(d.clone()).or_insert(Kept {
                    dir: d,
                    bytes: 0,
                    files: 0,
                    newest: 0,
                });
                e.bytes += n.size.max(0) as u64;
                e.files += 1;
                e.newest = e.newest.max(n.mtime);
            }
            let mut out: Vec<Kept> = by.into_values().collect();
            out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.dir.cmp(&b.dir)));
            out
        })
    }

    /// Mount files chosen with `<input type=file webkitdirectory>` below `<root>/bms/`.
    /// Mount picked files under `<root>/bms/` (by their path in a picked
    /// folder, else under `<root>/bms/<sub>/`). Zip files are skipped:
    /// see `add_zip`.
    pub fn add_files(root: &Path, files: &web_sys::FileList, sub: &str) {
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            for i in 0..files.length() {
                let Some(f) = files.get(i) else { continue };
                if f.name().to_lowercase().ends_with(".zip") {
                    continue;
                }
                let rel = js_sys::Reflect::get(&f, &JsValue::from_str("webkitRelativePath"))
                    .ok()
                    .and_then(|v| v.as_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| format!("{sub}/{}", f.name()));
                let path = root.join("bms").join(rel);
                let size = f.size() as i64;
                let mtime = (f.last_modified() / 1000.0) as i64;
                let data = Data::File(f);
                keep(&path, &data);
                fs.files.insert(path, Node { data, size, mtime });
            }
            fs.added = true;
        });
    }

    /// Unpack an archive (a song pack: .zip, .7z, .rar, .lzh) under
    /// `<root>/bms/<archive name>/`. Returns the files added.
    pub fn add_zip(root: &Path, name: &str, bytes: Vec<u8>) -> usize {
        add_archive_at(&root.join("bms"), name, bytes)
    }

    /// Unpack an archive under `<dir>/<archive name>/` (entry names in
    /// Shift_JIS / EUC-KR decoded; archives in one holding no chart
    /// unpacked too).
    pub fn add_archive_at(dir: &Path, name: &str, bytes: Vec<u8>) -> usize {
        use bmseeker::unpack::{unpack, Blob, BUDGET};
        let stem = Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "archive".into());
        let base = dir.join(stem);
        match unpack(Blob::Bytes(bytes), &crate::placement::Vfs, &base, BUDGET) {
            Ok(m) => m.len(),
            Err(e) => {
                log::warn!("{name}: {e}");
                0
            }
        }
    }

    /// A file held in memory only (song files: not saved with the
    /// settings, as `write` does).
    pub fn put_bytes(p: &Path, data: Vec<u8>) {
        let data = Data::Bytes(Rc::new(data));
        keep(p, &data);
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            let size = match &data {
                Data::Bytes(b) => b.len() as i64,
                Data::File(f) => f.size() as i64,
            };
            fs.files.insert(
                p.to_path_buf(),
                Node {
                    data,
                    size,
                    mtime: 0,
                },
            );
            fs.added = true;
        });
    }

    /// A skin's file: kept in the browser's storage, and its bytes stay in
    /// memory (skins are read at once, Lua's `require` too, with no time to
    /// fetch a file back from the storage).
    pub fn store_resident(p: &Path, data: Vec<u8>) {
        let data = Rc::new(data);
        super::super::opfs::save(p, &data, |_| {});
        FS.with(|fs| {
            fs.borrow_mut().files.insert(
                p.to_path_buf(),
                Node {
                    size: data.len() as i64,
                    data: Data::Bytes(data),
                    mtime: 0,
                },
            );
        });
    }

    /// A file of the player's own (a replay): held in memory and kept in
    /// the browser's storage, the library not read again for it.
    pub fn store(p: &Path, data: Vec<u8>) {
        let data = Data::Bytes(Rc::new(data));
        keep(p, &data);
        FS.with(|fs| {
            let size = match &data {
                Data::Bytes(b) => b.len() as i64,
                Data::File(f) => f.size() as i64,
            };
            fs.borrow_mut().files.insert(
                p.to_path_buf(),
                Node {
                    data,
                    size,
                    mtime: 0,
                },
            );
        });
    }

    /// Read the given lazily-mounted files into memory.
    pub fn prefetch(paths: Vec<PathBuf>, progress: Rc<std::cell::Cell<usize>>) {
        wasm_bindgen_futures::spawn_local(async move {
            for p in paths {
                let file = FS.with(|fs| match fs.borrow().files.get(&p) {
                    Some(Node {
                        data: Data::File(f),
                        ..
                    }) => Some(f.clone()),
                    _ => None,
                });
                if let Some(f) = file {
                    if let Ok(buf) = wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await {
                        let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                        FS.with(|fs| {
                            if let Some(n) = fs.borrow_mut().files.get_mut(&p) {
                                n.data = Data::Bytes(Rc::new(bytes));
                            }
                        });
                    }
                }
                progress.set(progress.get() + 1);
            }
        });
    }
}

pub use imp::*;

/// Files compiled into the executable (the default skins), addressed as
/// `<builtin>/...` — a name that cannot exist on any real file system.
pub const BUILTIN: &str = "<builtin>";
static BUILTIN_FILES: include_dir::Dir =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets/builtin");

fn builtin(p: &Path) -> Option<&'static [u8]> {
    let rel = p.strip_prefix(BUILTIN).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    BUILTIN_FILES.get_file(rel.as_str()).map(|f| f.contents())
}

/// A file name from an archive: UTF-8, else Shift_JIS / EUC-KR as BMS
/// packs use.
pub fn decode_name(raw: &[u8]) -> String {
    vibebms_core::encoding::decode_text(raw)
}

/// Read a file (built-in files first; a folder may be packed as
/// `folder.dxa`, as LR2 skins do).
pub fn read(p: &Path) -> io::Result<Vec<u8>> {
    match builtin(p) {
        Some(b) => Ok(b.to_vec()),
        None => imp::read(p).or_else(|e| {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e);
            }
            crate::formats::dxa::read(p, &|q| imp::read(q).ok()).ok_or(e)
        }),
    }
}

/// The first `n` bytes of a file (all of it when shorter); natively the
/// rest is not read.
pub fn read_head(p: &Path, n: usize) -> io::Result<Vec<u8>> {
    if let Some(b) = builtin(p) {
        return Ok(b[..n.min(b.len())].to_vec());
    }
    #[cfg(not(target_arch = "wasm32"))]
    match std::fs::File::open(p) {
        Ok(f) => {
            let mut b = Vec::new();
            io::Read::read_to_end(&mut io::Read::take(f, n as u64), &mut b)?;
            return Ok(b);
        }
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        // perhaps in a `.dxa`
        Err(_) => {}
    }
    let mut b = read(p)?;
    b.truncate(n);
    Ok(b)
}

pub fn read_to_string(p: &Path) -> io::Result<String> {
    String::from_utf8(read(p)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// `p` as it is on disk when only the letter case differs (skins made
/// on Windows name `Play\\x.png` for `play/x.png`); else `p`. A `*` in
/// the file name is kept for globbing, its folder is resolved.
pub fn resolve_case(p: &Path) -> PathBuf {
    let wild = p
        .file_name()
        .is_some_and(|n| n.to_string_lossy().contains('*'));
    if !wild && (is_file(p) || is_dir(p)) || builtin(p).is_some() {
        return p.to_path_buf();
    }
    let (dir, last) = match (wild, p.parent(), p.file_name()) {
        (true, Some(d), Some(n)) => (d, Some(n)),
        _ => (p, None),
    };
    let mut out = PathBuf::new();
    for c in dir.components() {
        let next = out.join(c);
        if matches!(c, std::path::Component::Normal(_)) && !is_dir(&next) && !is_file(&next) {
            let want = c.as_os_str().to_string_lossy().to_lowercase();
            let found = read_dir(if out.as_os_str().is_empty() {
                Path::new(".")
            } else {
                &out
            })
            .ok()
            .and_then(|es| {
                es.into_iter().find(|e| {
                    e.path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().to_lowercase() == want)
                })
            });
            match found {
                Some(e) => out.push(e.path.file_name().unwrap_or_default()),
                None => return p.to_path_buf(),
            }
        } else {
            out = next;
        }
    }
    match last {
        Some(n) => out.join(n),
        None => out,
    }
}

/// `p` with `.` and `..` resolved (without looking at the file system).
pub fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// `name` (a file a chart or skin refers to) under `dir`; `None` for an
/// absolute path, a drive or network path, or one that climbs out with `..`.
pub fn inside(dir: &Path, name: &str) -> Option<PathBuf> {
    use std::path::Component;
    let name = name.replace('\\', "/");
    let rel = Path::new(&name);
    let ok = !name.starts_with('/')
        && rel
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    ok.then(|| dir.join(rel))
}

/// Find a file in `dir` whose stem matches `name`'s stem (case-insensitive),
/// preferring extensions in `exts` order. Used for BMS-style references like
/// `a.wav` that ship as `a.ogg`. For many lookups in one folder, use [`DirIndex`].
pub fn find_by_stem(dir: &Path, name: &str, exts: &[&str]) -> Option<PathBuf> {
    DirIndex::new(dir).find(name, exts)
}

/// [`find_by_stem`] for many names under one folder: each (sub-)folder the
/// names lead into is listed once. Shared by the threads decoding a chart.
pub struct DirIndex {
    dir: PathBuf,
    /// by folder, relative to `dir`
    folders: Mutex<HashMap<PathBuf, ByStem>>,
}

impl DirIndex {
    pub fn new(dir: &Path) -> DirIndex {
        DirIndex {
            dir: dir.to_path_buf(),
            folders: Mutex::default(),
        }
    }

    pub fn find(&self, name: &str, exts: &[&str]) -> Option<PathBuf> {
        let p = inside(&self.dir, name)?;
        if is_file(&p) {
            return Some(p);
        }
        let rel = p.strip_prefix(&self.dir).ok()?;
        let stem = lower(rel.file_stem()?);
        let sub = rel.parent().unwrap_or(Path::new(""));
        let mut folders = self.folders.lock().unwrap_or_else(|e| e.into_inner());
        if !folders.contains_key(sub) {
            folders.insert(sub.to_path_buf(), by_stem(&self.dir.join(sub)));
        }
        folders[sub]
            .iter()
            .filter(|(s, _)| *s == stem)
            .filter_map(|(_, p)| {
                let ext = p.extension().map(lower).unwrap_or_default();
                Some((exts.iter().position(|x| *x == ext)?, p))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, p)| p.clone())
    }
}

/// A folder's files with their lower-cased stems.
type ByStem = Vec<(String, PathBuf)>;

fn by_stem(dir: &Path) -> ByStem {
    read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| Some((lower(e.path.file_stem()?), e.path)))
        .collect()
}

fn lower(s: &std::ffi::OsStr) -> String {
    s.to_string_lossy().to_lowercase()
}

pub const AUDIO_EXTS: &[&str] = &["wav", "ogg", "mp3", "flac"];
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "cim"];

/// Background read of lazily mounted files (web). Natively it completes immediately.
pub struct Prefetch {
    #[cfg(target_arch = "wasm32")]
    done: std::rc::Rc<std::cell::Cell<usize>>,
    pub total: usize,
}

impl Prefetch {
    /// Load every not-yet-read file below `dirs`.
    pub fn dirs(dirs: &[PathBuf]) -> Prefetch {
        let mut v: Vec<PathBuf> = dirs.iter().flat_map(|d| unloaded_under(d)).collect();
        v.sort();
        v.dedup();
        Self::files(v)
    }

    pub fn files(paths: Vec<PathBuf>) -> Prefetch {
        #[cfg(target_arch = "wasm32")]
        {
            let done = std::rc::Rc::new(std::cell::Cell::new(0));
            let total = paths.len();
            if total > 0 {
                imp::prefetch(paths, done.clone());
            }
            Prefetch { done, total }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = paths;
            Prefetch { total: 0 }
        }
    }

    pub fn done(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        {
            self.done.get()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.total
        }
    }

    pub fn finished(&self) -> bool {
        self.done() >= self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_files_stay_in_the_song_folder() {
        let dir = Path::new("/songs/a");
        assert_eq!(inside(dir, "bga\\x.png"), Some(dir.join("bga/x.png")));
        assert_eq!(inside(dir, "./x.wav"), Some(dir.join("x.wav")));
        for bad in [
            "../b/x.wav",
            "/etc/passwd",
            "\\\\host\\share\\a.png",
            "a/../../x",
        ] {
            assert_eq!(inside(dir, bad), None, "{bad}");
        }
        assert_eq!(normalize(Path::new("/a/b/../c/./d")), Path::new("/a/c/d"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sound_fallback_by_stem() {
        let dir = std::env::temp_dir().join(format!("vibebms-stem-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("wav")).unwrap();
        std::fs::create_dir_all(dir.join("tom.wav")).unwrap();
        for f in [
            "Kick.OGG",
            "snare.mp3",
            "snare.ogg",
            "movie.mpg",
            "wav/Hat.ogg",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(
            find_by_stem(&dir, "kick.wav", AUDIO_EXTS),
            Some(dir.join("Kick.OGG"))
        );
        assert_eq!(find_by_stem(&dir, "../kick.wav", AUDIO_EXTS), None);
        let index = DirIndex::new(&dir);
        let found = |name| index.find(name, AUDIO_EXTS);
        assert_eq!(found("kick.wav"), Some(dir.join("Kick.OGG")));
        assert_eq!(found("SNARE.wav"), Some(dir.join("snare.ogg")));
        assert_eq!(found("snare.mp3"), Some(dir.join("snare.mp3")));
        // the file named is taken whatever its extension
        assert_eq!(found("movie.mpg"), Some(dir.join("movie.mpg")));
        assert_eq!(found("movie.wav"), None);
        assert_eq!(found("wav\\hat.wav"), Some(dir.join("wav/Hat.ogg")));
        assert_eq!(found("none/hat.wav"), None);
        assert_eq!(found("tom.ogg"), None);
        assert_eq!(found("../kick.wav"), None);
        assert_eq!(found(""), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn letter_case_differs() {
        let dir = std::env::temp_dir().join(format!("vibebms-case-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("play/Img")).unwrap();
        std::fs::write(dir.join("play/Img/A.png"), b"").unwrap();
        assert_eq!(
            resolve_case(&dir.join("Play/img/a.PNG")),
            dir.join("play/Img/A.png")
        );
        assert_eq!(
            resolve_case(&dir.join("PLAY/IMG/*.png")),
            dir.join("play/Img/*.png")
        );
        assert_eq!(
            resolve_case(&dir.join("play/none.png")),
            dir.join("play/none.png")
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
