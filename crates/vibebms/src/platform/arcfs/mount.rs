//! Songs unpacked to be played: the folder holding a chart in an
//! archive, into memory or (large) into `cache/unpacked/`; and files read
//! from them, or alone.

use super::index::index;
use super::unpack::{unpack, Dest};
use super::{join, split, state};
use crate::platform::vfs;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A song this large or smaller is unpacked into memory; natively a larger
/// one goes into a folder (`cache/unpacked/`). A phone's is small: the
/// system ends an app over its share of memory, and a song's sounds are
/// in memory once more, decoded. On the web, which has no folder for it,
/// a song up to [`WEB_MAX`] is unpacked into memory.
#[cfg(not(target_arch = "wasm32"))]
const MEMORY: u64 = if cfg!(mobile) { 32 << 20 } else { 512 << 20 };

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
#[cfg(target_arch = "wasm32")]
const WEB_MAX: u64 = 1 << 30;

/// Single files kept after they were read: their bytes in all, and the
/// largest kept.
const RECENT_BYTES: usize = 16 << 20;
const RECENT_ONE: usize = 4 << 20;

/// A song unpacked: the files below `folder` in `archive`.
pub(super) struct Mount {
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

    fn read(&self, rel: &Path) -> Option<io::Result<Arc<Vec<u8>>>> {
        match &self.files {
            Files::Memory(m) => m.get(rel).map(|b| Ok(b.clone())),
            #[cfg(not(target_arch = "wasm32"))]
            Files::Folder(d) => Some(std::fs::read(d.join(rel)).map(Arc::new)),
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
            crate::platform::spawn("unpacked files", move || {
                for d in dirs {
                    let _ = std::fs::remove_dir_all(d);
                }
            });
        }
    }
}

/// A file read alone: its path, its archive's stamp, its bytes.
pub(super) type Recent = (PathBuf, (i64, i64), Arc<Vec<u8>>);

/// Held while a song is unpacked (one at a time).
static MOUNTING: Mutex<()> = Mutex::new(());

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

fn not_found(archive: &Path, rel: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        join(archive, rel).display().to_string(),
    )
}

/// Read the file `rel` of `archive` (its bytes shared with its unpacked
/// song, or with the files read lately).
pub fn read(archive: &Path, rel: &Path) -> io::Result<Arc<Vec<u8>>> {
    if let Some(r) = mounted(archive, rel).and_then(|m| m.read(rel)) {
        return r;
    }
    let p = join(archive, rel);
    let now = vfs::stamp(archive);
    let recent = (state().recent.iter())
        .find(|(q, s, _)| *q == p && *s == now)
        .map(|(_, _, b)| b.clone());
    if let Some(b) = recent {
        return Ok(b);
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
            prepare_folders(&[(archive.to_path_buf(), song)])?;
            if let Some(r) = mounted(archive, rel).and_then(|m| m.read(rel)) {
                return r;
            }
        }
    }
    let want = |r: &Path| r == rel;
    let cap = size.max(1) << 1;
    let u = unpack(archive, &want, false, Some(1), &Dest::Memory, cap)?;
    let b = (u.files.into_iter().next())
        .map(|(_, b)| Arc::new(b))
        .ok_or_else(|| not_found(archive, rel))?;
    if b.len() <= RECENT_ONE {
        let mut st = state();
        st.recent.push_front((p, now, b.clone()));
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
    prepare_folders(&want)
}

/// Unpack the songs `want` (archive, folder in it), as [`prepare`].
fn prepare_folders(want: &[(PathBuf, PathBuf)]) -> io::Result<()> {
    let _one = MOUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let stamps: Vec<(i64, i64)> = want.iter().map(|(a, _)| vfs::stamp(a)).collect();
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
        let dir = work_dir("");
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
        vfs::release(archive);
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

/// A new folder in `cache/unpacked/` (named for this run, `kind` and a
/// count).
#[cfg(not(target_arch = "wasm32"))]
fn work_dir(kind: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let work =
        (state().work.clone()).unwrap_or_else(|| std::env::temp_dir().join("vibebms-unpacked"));
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    work.join(format!("{}-{kind}{n}", std::process::id()))
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
            let dir = spill.get_or_insert_with(|| work_dir("spill-")).clone();
            let to = dir.join(&rel);
            if !to.is_file() {
                std::fs::create_dir_all(to.parent()?).ok()?;
                std::fs::write(&to, bytes.as_slice()).ok()?;
            }
            Some(to)
        }
    }
}
