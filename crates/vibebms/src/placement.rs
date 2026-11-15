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
}

pub fn chart_info(path: &Path) -> Option<ChartInfo> {
    bmseeker::placement::chart_info(&Vfs, path)
}

pub fn locate(chart: &Path, library: &Path) -> Option<Located> {
    bmseeker::placement::locate(&Vfs, chart, library)
}
