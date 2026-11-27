//! Which song's folder a chart of its own belongs in: `bmseeker::placement`
//! on the player's files (`platform::Vfs`).

use crate::platform::Vfs;
pub use bmseeker::placement::{is_chart, same_artist, same_name, same_title, Located};
use bmseeker::placement::{ChartInfo, Rules};
use std::path::Path;

pub fn chart_info(path: &Path) -> Option<ChartInfo> {
    bmseeker::placement::chart_info(&Vfs, path)
}

/// The library below `<root>/bms` as bmseeker sees it, its charts as the
/// player's library has them (their hashes, titles and artists: a web
/// page cannot read its library's charts when it likes).
/// Charts in archives are left out: nothing is written into an archive, so
/// their songs' folders take no file.
pub fn library(root: &Path, songs: &[crate::library::Song]) -> bmseeker::library::Library {
    let owned = songs
        .iter()
        .filter(|s| !s.in_archive())
        .map(|s| bmseeker::library::Owned {
            path: s.path.clone().into(),
            md5: s.md5.clone(),
            sha256: s.sha256.clone(),
            title: s.title.clone(),
            artist: s.artist.clone(),
        });
    bmseeker::library::Library::from_catalog(&root.join("bms"), owned)
}

/// Where in `lib` a chart that came on its own belongs (the files beside
/// it are not its).
pub fn locate_loose(chart: &Path, lib: &bmseeker::library::Library) -> Option<Located> {
    bmseeker::placement::locate(&Vfs, chart, lib, false, Rules::default()).map(|(l, _)| l)
}

/// Move `file` into `dir` (smart overwrite: `bmseeker::placement::bring`).
pub fn bring(file: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    bmseeker::placement::bring(&Vfs, file, dir)
        .inspect_err(|e| log::warn!("{} into {}: {e}", file.display(), dir.display()))
        .ok()
        .flatten()
}
