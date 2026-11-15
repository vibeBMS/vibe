//! Songs the in-app browser downloaded: `bmseeker::import` on the player's
//! files, and what it tells the user.

use crate::placement::Vfs;
pub use bmseeker::import::*;
use std::path::{Path, PathBuf};

pub fn import(file: &Path, library: &Path) -> Result<Imported, String> {
    bmseeker::import::import(&Vfs, file, library)
}

pub fn place_waiting(downloads: &Path, library: &Path) -> Vec<PathBuf> {
    bmseeker::import::place_waiting(&Vfs, downloads, library)
}

/// What a finished import means for the browser.
pub struct Report {
    /// The library changed.
    pub rescan: bool,
    /// A song arrived (time for the separate chart's page).
    pub song_arrived: bool,
    /// A chart that could belong to several songs, and their folders.
    pub ask: Option<(PathBuf, Vec<(PathBuf, String)>)>,
}

/// Tell the user how an import went; charts waiting for this song go into
/// its folder.
pub fn report(ctx: &mut crate::app::Ctx, r: Result<Imported, String>) -> Report {
    let mut out = Report {
        rescan: true,
        song_arrived: false,
        ask: None,
    };
    match r {
        Err(e) => {
            ctx.toast(format!("Could not unpack the download: {e}"));
            out.rescan = false;
        }
        Ok(Imported::Unpacked(dir, n)) => {
            ctx.toast(format!("Added {} ({n} files)", file_name(&dir)));
            let root = &ctx.paths.root;
            for p in place_waiting(&root.join("downloads"), &root.join("bms")) {
                ctx.toast(format!("{} → its song", file_name(&p)));
            }
            out.song_arrived = true;
        }
        Ok(Imported::Placed(p)) => {
            let dir = p.parent().map(file_name).unwrap_or_default();
            ctx.toast(format!("{} → {dir}", file_name(&p)));
        }
        Ok(Imported::Waiting(p)) => {
            ctx.toast(format!("{}: download its song too", file_name(&p)));
            out.rescan = false;
        }
        Ok(Imported::Ask(chart, folders)) => {
            out.rescan = false;
            out.ask = Some((chart, folders));
        }
        Ok(Imported::Other(p)) => {
            ctx.toast(format!(
                "{} is not a song (kept in downloads)",
                file_name(&p)
            ));
            out.rescan = false;
        }
    }
    out
}
