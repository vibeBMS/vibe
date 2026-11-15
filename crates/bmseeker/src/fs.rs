//! The files the library tools read and move: a disk ([`Disk`]) or whatever
//! a player keeps its library in.

use std::io;
use std::path::{Path, PathBuf};

/// A directory entry.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

pub trait Fs: Sync {
    fn read(&self, p: &Path) -> io::Result<Vec<u8>>;
    fn read_dir(&self, p: &Path) -> io::Result<Vec<Entry>>;
    /// Move a file; false when `to` exists or the move fails.
    fn rename(&self, from: &Path, to: &Path) -> bool;
    /// A file's (modification time in seconds, size); None: no such file.
    fn stamp(&self, p: &Path) -> Option<(i64, u64)>;
    /// Delete a file; false when it could not be.
    fn remove(&self, p: &Path) -> bool;
}

/// `std::fs`.
pub struct Disk;

impl Fs for Disk {
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(p)
    }

    fn read_dir(&self, p: &Path) -> io::Result<Vec<Entry>> {
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

    fn rename(&self, from: &Path, to: &Path) -> bool {
        !to.exists() && std::fs::rename(from, to).is_ok()
    }

    fn stamp(&self, p: &Path) -> Option<(i64, u64)> {
        let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
        let t = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);
        Some((t, m.len()))
    }

    fn remove(&self, p: &Path) -> bool {
        std::fs::remove_file(p).is_ok()
    }
}
