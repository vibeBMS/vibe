//! Songs kept in archives: a `.zip`, `.7z`, `.rar`, `.lzh` (`.lha`) or
//! `.tar` (`.tar.gz`, `.tgz`) in the library is read as a folder, without
//! unpacking it. A file in one is named by the archive's path, a `!`, and
//! its path in the archive: `bms/pack.zip!/Song/chart.bme` (a folder named
//! so on a disk is taken for the archive). [`super::vfs`] reads such
//! paths through here, so the chart, sound and image loaders read them as
//! any other file.
//!
//! - The scan lists an archive and reads its charts once per change of
//!   its size and time ([`read_charts`]); the list of its files (the index) is
//!   kept in `cache/archives.json`, which is what the folder's files are
//!   looked up in (no archive is opened to list a folder).
//! - Playing a chart in one unpacks its song: the folder holding the chart
//!   in the archive, its sub-folders too ([`prepare`]), into memory, or
//!   natively into `cache/unpacked/` when it is large ([`mount::MEMORY`]).
//!   One song is kept so ([`retain`] lets it go when another is chosen).
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

mod index;
mod mount;
mod unpack;

pub use index::{
    index_of, indexed, is_dir, is_file, load_index, read_dir, save_index, set_index, stamp, Index,
};
#[cfg(all(test, not(target_arch = "wasm32")))]
pub use mount::in_folder;
#[cfg(not(target_arch = "wasm32"))]
pub use mount::native_path;
#[cfg(test)]
pub use mount::set_memory_limit;
pub use mount::{is_ready, prepare, read, retain};
pub use unpack::{read_charts, times_opened, Unpacked};

use mount::{Mount, Recent};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

/// What follows an archive's name in a path to a file in it.
pub const MARK: char = '!';

/// Archive file names, by extension (`tar.gz`: a name's last two).
const EXTS: &[&str] = &["zip", "7z", "rar", "lzh", "lha", "tar", "tgz", "tar.gz"];

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

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
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
}
