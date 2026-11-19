//! Songs the in-app browser downloaded: `bmseeker::import` on the player's
//! files, and what it tells the user.

use crate::placement::Vfs;
use bmseeker::fs::file_name;
pub use bmseeker::import::{unpack_into, Imported};
use bmseeker::library::Library;
use std::path::{Path, PathBuf};

/// What came of a download: what it is, and the charts that waited for it
/// placed.
pub type Outcome = Result<(Imported, Vec<PathBuf>), String>;

pub fn import(file: &Path, library: &Path) -> Outcome {
    let req = bmseeker::install::Request::default();
    bmseeker::import::import(&Vfs, file, &mut Library::new(library), &req)
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

/// Tell the user how an import went, and of the charts that waited for it.
pub fn report(ctx: &mut crate::app::Ctx, r: Outcome) -> Report {
    let mut out = Report {
        rescan: true,
        song_arrived: false,
        ask: None,
    };
    let (r, placed) = match r {
        Ok((i, placed)) => (Ok(i), placed),
        Err(e) => (Err(e), Vec::new()),
    };
    match r {
        Err(e) => {
            ctx.toast(tr!("Could not unpack the download: {e}", e = e));
            out.rescan = false;
        }
        Ok(Imported::Unpacked(dir, n)) => {
            ctx.toast(tr!(
                "Added {name} ({n} files)",
                name = file_name(&dir),
                n = n
            ));
            out.song_arrived = true;
        }
        Ok(Imported::Owned(dir)) => {
            ctx.toast(tr!("{name}: already installed", name = file_name(&dir)));
        }
        Ok(Imported::Placed(p)) => {
            let dir = p.parent().map(file_name).unwrap_or_default();
            ctx.toast(format!("{} → {dir}", file_name(&p)));
        }
        Ok(Imported::Waiting(p)) => {
            ctx.toast(tr!("{name}: download its song too", name = file_name(&p)));
            out.rescan = false;
        }
        Ok(Imported::Ask(chart, folders)) => {
            out.rescan = false;
            out.ask = Some((chart, folders));
        }
        Ok(Imported::Other(p)) => {
            ctx.toast(tr!(
                "{name} is not a song (kept in downloads)",
                name = file_name(&p)
            ));
            out.rescan = false;
        }
        Ok(other) => {
            log::warn!("import: {other:?}");
            out.rescan = false;
        }
    }
    for p in &placed {
        ctx.toast(tr!("{name} → its song", name = file_name(p)));
    }
    out.rescan |= !placed.is_empty();
    out
}
