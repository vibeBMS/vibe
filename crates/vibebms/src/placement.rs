//! Which song's folder a chart of its own belongs in: `bmseeker::placement`
//! on the player's files (`platform::vfs`).

pub use bmseeker::placement::*;
use std::path::Path;

/// The player's files, for `bmseeker`.
pub struct Vfs;

impl bmseeker::fs::Fs for Vfs {
    fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
        crate::platform::vfs::read(p)
    }
    fn read_dir(&self, p: &Path) -> std::io::Result<Vec<bmseeker::fs::Entry>> {
        Ok(crate::platform::vfs::read_dir(p)?
            .into_iter()
            .map(|e| bmseeker::fs::Entry {
                path: e.path,
                is_dir: e.is_dir,
            })
            .collect())
    }
    fn rename(&self, from: &Path, to: &Path) -> bool {
        crate::platform::vfs::rename(from, to)
    }
    fn stamp(&self, p: &Path) -> Option<(i64, u64)> {
        use crate::platform::vfs;
        vfs::is_file(p).then(|| {
            let (t, n) = vfs::stamp(p);
            (t, n as u64)
        })
    }
    fn remove(&self, p: &Path) -> bool {
        crate::platform::vfs::remove_file(p).is_ok()
    }
}

pub fn chart_info(path: &Path) -> Option<ChartInfo> {
    bmseeker::placement::chart_info(&Vfs, path)
}

pub fn locate(chart: &Path, library: &Path) -> Option<Located> {
    bmseeker::placement::locate(&Vfs, chart, library)
}

/// A chart that came on its own (the files beside it are not its).
pub fn locate_loose(chart: &Path, library: &Path) -> Option<Located> {
    bmseeker::placement::locate_loose(&Vfs, chart, library)
}

/// Move `file` into `dir` (smart overwrite: `bmseeker::placement::bring`).
pub fn bring(file: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    bmseeker::placement::bring(&Vfs, file, dir)
}
