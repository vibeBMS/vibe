//! Songs the in-app browser downloaded: `bmseeker::import` on the player's
//! files, and what it tells the user.

use crate::platform::Vfs;
use bmseeker::fs::file_name;
pub use bmseeker::import::{unpack_into, Imported};
use std::path::{Path, PathBuf};

/// What came of a download: what it is, and the charts that waited for it
/// placed.
pub type Outcome = Result<(Imported, Vec<PathBuf>), String>;

/// Bring `file` into the library below `<root>/bms`, whose charts are
/// `songs` (the player's records: a song held is known by its charts'
/// hashes, whatever its folder is called).
pub fn import(file: &Path, root: &Path, songs: &[crate::library::Song]) -> Outcome {
    let req = bmseeker::install::Request::default();
    let lib = &mut crate::placement::library(root, songs);
    bmseeker::import::import(&Vfs, file, lib, &req)
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
pub fn report(ctx: &mut crate::ctx::Ctx, r: Outcome) -> Report {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Song;
    use sha2::{Digest, Sha256};

    /// A song the library holds comes again in an archive named and
    /// folded otherwise: it is the library's song, known by its chart's
    /// hash, and gets no second folder.
    #[test]
    fn a_song_held_is_known_by_its_charts() {
        let root = std::env::temp_dir().join(format!("vibebms-import-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let chart = "#TITLE Held\n#ARTIST alpha\n#WAV01 01.wav\n";
        let held = root.join("bms/My Folder");
        std::fs::create_dir_all(&held).unwrap();
        std::fs::write(held.join("held.bms"), chart).unwrap();
        std::fs::write(held.join("01.ogg"), "x").unwrap();
        let songs = [Song {
            path: held.join("held.bms").to_string_lossy().into_owned(),
            sha256: Sha256::digest(chart)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            title: "Held".into(),
            artist: "alpha".into(),
            ..Default::default()
        }];
        let file = root.join("downloads/Held v2.zip");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let pack = bmseeker::testkit::zip(&[
            ("Held v2/held.bms", chart),
            ("Held v2/01.ogg", "x"),
            ("Held v2/02.ogg", "y"),
        ]);
        std::fs::write(&file, pack).unwrap();
        let (got, _) = import(&file, &root, &songs).unwrap();
        assert!(matches!(&got, Imported::Owned(d) if *d == held), "{got:?}");
        // what its folder lacked is brought in; no other folder
        assert!(held.join("02.ogg").is_file());
        let folders: Vec<_> = std::fs::read_dir(root.join("bms"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(folders.len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
