//! File system facade. Natively this is `std::fs`; on the web it is an
//! in-memory tree: files picked by the user (read lazily via [`prefetch`]),
//! plus small written files persisted in `localStorage`.

use std::io;
use std::path::{Path, PathBuf};

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
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("tmp~");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, p)
    }
    pub fn remove_file(p: &Path) -> io::Result<()> {
        std::fs::remove_file(p)
    }
    pub fn create_dir_all(p: &Path) -> io::Result<()> {
        std::fs::create_dir_all(p)
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
    pub fn take_added() -> bool {
        false
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
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
        FS.with(|fs| fs.borrow_mut().files.remove(p));
        if let Some(s) = storage() {
            let _ = s.remove_item(&format!("{LS_PREFIX}{}", p.display()));
        }
        Ok(())
    }

    pub fn create_dir_all(_p: &Path) -> io::Result<()> {
        Ok(())
    }

    pub fn read_dir(p: &Path) -> io::Result<Vec<Entry>> {
        FS.with(|fs| {
            let fs = fs.borrow();
            let mut out: Vec<Entry> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for k in fs.files.keys() {
                if let Ok(rest) = k.strip_prefix(p) {
                    let mut comps = rest.components();
                    let Some(first) = comps.next() else { continue };
                    let child = p.join(first.as_os_str());
                    let is_dir = comps.next().is_some();
                    if seen.insert(child.clone()) {
                        out.push(Entry {
                            path: child,
                            is_dir,
                        });
                    }
                }
            }
            if out.is_empty() && !fs.files.keys().any(|k| k.starts_with(p)) {
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
        FS.with(|fs| fs.borrow().files.keys().any(|k| k != p && k.starts_with(p)))
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
            fs.borrow()
                .files
                .iter()
                .filter(|(k, n)| k.starts_with(dir) && matches!(n.data, Data::File(_)))
                .map(|(k, _)| k.clone())
                .collect()
        })
    }

    pub fn take_added() -> bool {
        FS.with(|fs| std::mem::take(&mut fs.borrow_mut().added))
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
                fs.files.insert(
                    path,
                    Node {
                        data: Data::File(f),
                        size,
                        mtime,
                    },
                );
            }
            fs.added = true;
        });
    }

    /// Unpack a zip (a song pack) under `<root>/bms/<zip name>/`; entry
    /// names in Shift_JIS / EUC-KR are decoded. Returns the files added.
    pub fn add_zip(root: &Path, name: &str, bytes: Vec<u8>) -> usize {
        use std::io::Read;
        let mut z = match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
            Ok(z) => z,
            Err(e) => {
                log::warn!("{name}: {e}");
                return 0;
            }
        };
        let stem = Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "zip".into());
        let base = root.join("bms").join(stem);
        let mut n = 0;
        // what a pack may unpack to (a zip can claim, or expand to, anything)
        let mut budget: u64 = 2 << 30;
        FS.with(|fs| {
            let mut fs = fs.borrow_mut();
            for i in 0..z.len() {
                let Ok(mut f) = z.by_index(i) else { continue };
                if f.is_dir() {
                    continue;
                }
                let entry = crate::platform::vfs::decode_name(f.name_raw());
                // no way out of the pack's folder
                let rel: PathBuf = entry
                    .split(['/', '\\'])
                    .filter(|c| !c.is_empty() && *c != "." && *c != "..")
                    .collect();
                if rel.as_os_str().is_empty() {
                    continue;
                }
                let mut buf = Vec::with_capacity(f.size().min(budget) as usize);
                if (&mut f).take(budget + 1).read_to_end(&mut buf).is_err() {
                    continue;
                }
                if buf.len() as u64 > budget {
                    log::warn!(
                        "{name}: larger than {} MB unpacked; stopped",
                        (2u64 << 30) >> 20
                    );
                    break;
                }
                budget -= buf.len() as u64;
                let size = buf.len() as i64;
                fs.files.insert(
                    base.join(rel),
                    Node {
                        data: Data::Bytes(Rc::new(buf)),
                        size,
                        mtime: 0,
                    },
                );
                n += 1;
            }
            fs.added = true;
        });
        n
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
            super::dxa::read(p, &|q| imp::read(q).ok()).ok_or(e)
        }),
    }
}

pub fn read_to_string(p: &Path) -> io::Result<String> {
    String::from_utf8(read(p)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
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
/// `a.wav` that ship as `a.ogg`.
pub fn find_by_stem(dir: &Path, name: &str, exts: &[&str]) -> Option<PathBuf> {
    let p = inside(dir, name)?;
    let name = name.replace('\\', "/");
    if is_file(&p) {
        return Some(p);
    }
    let rel = Path::new(&name);
    let parent = dir.join(rel.parent().unwrap_or(Path::new("")));
    let stem = rel.file_stem()?.to_string_lossy().to_lowercase();
    let mut best: Option<(usize, PathBuf)> = None;
    for e in read_dir(&parent).ok()? {
        if e.is_dir {
            continue;
        }
        let Some(fs) = e
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
        else {
            continue;
        };
        if fs != stem {
            continue;
        }
        let ext = e
            .path
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if let Some(rank) = exts.iter().position(|x| *x == ext) {
            if best.as_ref().map(|b| rank < b.0).unwrap_or(true) {
                best = Some((rank, e.path));
            }
        }
    }
    best.map(|b| b.1)
}

pub const AUDIO_EXTS: &[&str] = &["wav", "ogg", "mp3", "flac"];
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif"];

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
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Kick.OGG"), b"x").unwrap();
        assert_eq!(
            find_by_stem(&dir, "kick.wav", AUDIO_EXTS),
            Some(dir.join("Kick.OGG"))
        );
        assert_eq!(find_by_stem(&dir, "../kick.wav", AUDIO_EXTS), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
