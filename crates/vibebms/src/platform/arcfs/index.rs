//! An archive's list of files (its index), kept in `cache/archives.json`:
//! what a folder in it is listed from.

use super::unpack::{unpack, Dest};
use super::{join, state};
use crate::platform::vfs::{self, Entry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    /// The version of the game that left it out (it stopped the game while
    /// it was read: `library`'s scan marks); empty otherwise.
    pub left_out_by: String,
    /// Its files and their sizes.
    pub files: BTreeMap<PathBuf, u64>,
    /// The folders holding a chart.
    chart_dirs: HashSet<PathBuf>,
}

impl Index {
    pub fn new(stamp: (i64, i64), format: &str, files: BTreeMap<PathBuf, u64>) -> Index {
        let chart_dirs = files
            .keys()
            .filter(|p| vibebms_core::is_chart(p))
            .map(|p| p.parent().map(Path::to_path_buf).unwrap_or_default())
            .collect();
        Index {
            stamp,
            format: format.into(),
            error: String::new(),
            left_out_by: String::new(),
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

    /// An archive left out by version `by` of the game.
    pub fn left_out(stamp: (i64, i64), error: String, by: &str) -> Index {
        Index {
            left_out_by: by.into(),
            ..Index::failed(stamp, error)
        }
    }

    /// Its charts.
    pub fn charts(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys().filter(|p| vibebms_core::is_chart(p))
    }

    /// The files at and below `dir`.
    pub(super) fn below<'a>(
        &'a self,
        dir: &'a Path,
    ) -> impl Iterator<Item = (&'a PathBuf, &'a u64)> {
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
    pub(super) fn song_of(&self, rel: &Path) -> Option<PathBuf> {
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    left_out_by: String,
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
    let stored: HashMap<String, Stored> = vfs::read(file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    for (a, s) in stored {
        let stamp = (s.mtime, s.size);
        let ix = if s.format.is_empty() {
            Index::left_out(stamp, s.error, &s.left_out_by)
        } else {
            // a path as it was listed (made safe), else not one: an entry
            // that is not comes back as no other name
            let files = (s.files.into_iter())
                .filter_map(|(p, n)| {
                    let rel = bmseeker::unpack::member_path(p.as_bytes())?;
                    (rel_text(&rel) == p).then_some((rel, n))
                })
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
                    left_out_by: ix.left_out_by.clone(),
                    files: ix.files.iter().map(|(p, n)| (rel_text(p), *n)).collect(),
                };
                (a.to_string_lossy().into_owned(), s)
            })
            .collect()
    };
    match serde_json::to_vec(&stored) {
        Ok(j) => {
            if let Err(e) = vfs::write(file, &j) {
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

/// `archive`'s index: as the scan left it; a zip's read now when there is
/// none or it changed (its list is its directory, read alone). With
/// `slow`, a 7z's, RAR's, LZH's or tar's too (not for the main thread: a
/// 7z, a solid RAR and a .tar.gz are decoded in full, an LZH and a tar
/// read through).
pub(super) fn index(archive: &Path, slow: bool) -> Option<Arc<Index>> {
    let now = vfs::stamp(archive);
    let have = index_of(archive);
    if let Some(ix) = &have {
        if ix.stamp == now || (ix.format != "zip" && !slow) {
            return have;
        }
    }
    if now == (0, 0) && !vfs::is_file(archive) {
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
    vfs::read_head(archive, head.len())
        .map(|b| {
            head[..b.len()].copy_from_slice(&b);
            b.len() == head.len()
        })
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A path in `cache/archives.json` comes back as it was written, or not
    /// at all: one that is no path an archive is listed with is dropped.
    #[test]
    fn a_cached_path_comes_back_as_it_was() {
        let dir = std::env::temp_dir().join(format!("vibebms-index-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("archives.json");
        let archive = dir.join("p.zip");
        let json = serde_json::json!({
            archive.to_string_lossy(): {
                "mtime": 1,
                "size": 2,
                "format": "zip",
                "files": [["\u{feff}x.wav", 4], ["Song/a.bms", 8], ["Song/../b.bms", 8]],
            }
        });
        std::fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
        load_index(&file);
        let ix = index_of(&archive).unwrap();
        let files: Vec<&PathBuf> = ix.files.keys().collect();
        assert_eq!(files, [Path::new("Song/a.bms")]);
        std::fs::remove_dir_all(&dir).ok();
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
