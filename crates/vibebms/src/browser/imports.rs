//! Downloads of the in-app browser being brought into the library.

use crate::ctx::Ctx;
use crate::library::Song;
use crate::net::import;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub type ImportResult = import::Outcome;

/// Downloads being brought into the library, on threads.
pub struct Imports {
    tx: Sender<ImportResult>,
    rx: Receiver<ImportResult>,
}

impl Default for Imports {
    fn default() -> Imports {
        let (tx, rx) = channel();
        Imports { tx, rx }
    }
}

impl Imports {
    /// Bring `file` into the library below `<root>/bms`, whose charts are
    /// `songs` (the bmseeker library of them is made on the import's
    /// thread: it copies every chart's record).
    pub fn start(&self, file: PathBuf, root: PathBuf, songs: Arc<Vec<Song>>) {
        let tx = self.tx.clone();
        crate::platform::spawn("import", move || {
            let _ = tx.send(import::import(&file, &root, &songs));
        });
    }

    /// The imports that are done: the player is told, the library scanned
    /// when it changed; `song_arrived` when a song came in (time for the
    /// separate chart's page).
    pub fn poll(&self, ctx: &mut Ctx, mut song_arrived: impl FnMut(&mut Ctx)) {
        for r in self.rx.try_iter() {
            let done = import::report(r);
            for s in done.said {
                ctx.toast(s);
            }
            if done.song_arrived {
                song_arrived(ctx);
            }
            let mut rescan = done.rescan;
            if let Some((chart, folders)) = done.ask {
                rescan |= ask_folder(ctx, &chart, &folders);
            }
            if rescan {
                ctx.ensure_scanned("bms");
                ctx.start_scan();
            }
        }
    }
}

/// A chart several songs could be for: the player picks the song's folder.
/// True when the chart was put there.
#[cfg(desktop)]
fn ask_folder(ctx: &mut Ctx, chart: &Path, folders: &[(PathBuf, String)]) -> bool {
    let name = bmseeker::fs::file_name(chart);
    let mut dialog =
        rfd::FileDialog::new().set_title(tr!("Which song folder is {name} for?", name = name));
    if let Some((d, _)) = folders.first() {
        dialog = dialog.set_directory(d);
    }
    let Some(dir) = dialog.pick_folder() else {
        return false;
    };
    let placed = crate::net::placement::bring(chart, &dir).is_some();
    if placed {
        ctx.toast(format!("{name} → {}", bmseeker::fs::file_name(&dir)));
    }
    placed
}

/// Phones and tablets have no folder dialog: the chart waits in downloads.
#[cfg(not(desktop))]
fn ask_folder(ctx: &mut Ctx, chart: &Path, _folders: &[(PathBuf, String)]) -> bool {
    ctx.toast(tr!(
        "{name}: several songs could be it; it stays in downloads",
        name = bmseeker::fs::file_name(chart)
    ));
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::import::Imported;
    use std::sync::atomic::Ordering;

    fn toasts(ctx: &Ctx) -> Vec<String> {
        ctx.toasts.iter().map(|(t, _)| t.clone()).collect()
    }

    /// What an import says is shown, in order, before the rest of it is
    /// acted on (the song's page, the scan).
    #[test]
    fn what_an_import_says_comes_first() {
        let root = std::env::temp_dir().join(format!("vibebms-imports-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let done = || -> ImportResult {
            let song = root.join("bms/Song");
            Ok((
                Imported::Unpacked(song, 3),
                vec![root.join("downloads/b.bme")],
            ))
        };
        let said = import::report(done()).said;
        assert_eq!(said.len(), 2);
        let imports = Imports::default();
        imports.tx.send(done()).unwrap();
        let mut shown = None;
        imports.poll(&mut ctx, |ctx| shown = Some(toasts(ctx)));
        assert_eq!(shown.as_ref(), Some(&said));
        assert_eq!(toasts(&ctx), said);
        let scan = ctx.lib.scan.clone().expect("the library scanned");
        while !scan.done.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
