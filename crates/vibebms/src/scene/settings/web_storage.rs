//! Web: the songs kept in the browser, and how much room they take.

use super::row::{Act, Row, Rows};
use super::{Armed, Confirm};
use crate::ctx::Ctx;
use crate::platform::vfs::{self, Kept};

/// Days a kept song folder may go unplayed (and unchanged) before the
/// LIBRARY page offers to delete it.
const STALE_DAYS: i64 = 30;
/// `Act::DeleteKept` of every kept folder, and of those not played for
/// [`STALE_DAYS`].
const ALL: usize = usize::MAX;
const STALE: usize = usize::MAX - 1;

/// `n` bytes for people.
fn bytes(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0} MB", n / 1e6)
    } else {
        format!("{:.0} KB", n / 1e3)
    }
}

/// The kept folders (indexes into `folders`) written and played last more
/// than [`STALE_DAYS`] ago (a song never played counts from when it came).
fn stale_folders(ctx: &Ctx, folders: &[Kept]) -> Vec<usize> {
    let cut = crate::platform::unix_now() as i64 - STALE_DAYS * 86_400;
    folders
        .iter()
        .enumerate()
        .filter(|(_, k)| k.newest > 0 && k.newest < cut)
        .filter(|(_, k)| {
            !ctx.lib.songs.iter().any(|s| {
                std::path::Path::new(&s.path).starts_with(&k.dir)
                    && ctx
                        .lib
                        .scores
                        .iter()
                        .any(|(k, b)| k.id == s.sha256 && b.last_played >= cut)
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// "1 folder", "3 folders".
fn folders_n(n: usize) -> String {
    if n == 1 {
        tr!("1 folder").into()
    } else {
        tr!("{n} folders", n = n)
    }
}

/// How much room the kept songs take, of how much.
pub(super) fn summary() -> String {
    let st = crate::platform::opfs::status();
    if !st.available {
        return tr!("until the page closes").into();
    }
    if st.quota > 0.0 {
        tr!(
            "{used} of {quota}",
            used = bytes(st.usage),
            quota = bytes(st.quota)
        )
    } else {
        bytes(st.usage)
    }
}

/// LIBRARY › STORAGE.
pub(super) fn rows(b: &mut Rows) {
    let (ctx, armed) = (b.ctx, b.armed);
    b.header(tr!("SONGS KEPT IN THIS BROWSER"));
    // BMSeeker's web page, beside this one: what it adds shows here at once
    b.push(Row::new(
        "storage.bmseeker",
        tr!("Manage the library with BMSeeker (difficulty tables, downloads)"),
        tr!("open ↗"),
        Act::Run(
            |_, _| {
                if let Some(origin) = web_sys::window().and_then(|w| w.location().origin().ok()) {
                    crate::platform::open_url(&format!("{origin}/BMSeeker/"));
                }
            },
            0,
        ),
    ));
    let st = crate::platform::opfs::status();
    if !st.available {
        b.push(Row::note(
            "storage.none",
            tr!("Songs stay until the page closes (this browser keeps no files)"),
            "",
        ));
        return;
    }
    let room = summary();
    let saving = if st.pending > 0 {
        tr!(" · saving {n}", n = st.pending)
    } else {
        String::new()
    };
    b.push(Row::new(
        "storage.persist",
        tr!(
            "Songs kept in this browser: {room}{saving}",
            room = room,
            saving = saving
        ),
        if st.persisted {
            tr!("kept for good")
        } else {
            tr!("ask to keep ↵")
        },
        if st.persisted {
            Act::None
        } else {
            Act::Run(|_, _| crate::platform::opfs::request_persist(), 0)
        },
    ));
    let folders = vfs::folder_sizes(&ctx.paths.root.join("bms"));
    let delete = |i: usize, value: String| {
        if armed.is(&Confirm::DeleteKept(i)) {
            tr!("press again to delete").into()
        } else {
            value
        }
    };
    let stale = stale_folders(ctx, &folders);
    if !stale.is_empty() {
        let total: u64 = stale.iter().map(|&i| folders[i].bytes).sum();
        b.push(Row::new(
            "storage.delete_stale",
            tr!(
                "Delete songs not played for {days} days ({folders}, {size})",
                days = STALE_DAYS,
                folders = folders_n(stale.len()),
                size = bytes(total as f64)
            ),
            delete(STALE, tr!("delete ×").into()),
            Act::DeleteKept(STALE),
        ));
    }
    if !folders.is_empty() {
        let total: u64 = folders.iter().map(|f| f.bytes).sum();
        b.push(Row::new(
            "storage.delete_all",
            tr!(
                "Delete every song kept here ({folders}, {size})",
                folders = folders_n(folders.len()),
                size = bytes(total as f64)
            ),
            delete(ALL, tr!("delete ×").into()),
            Act::DeleteKept(ALL),
        ));
    }
    for (i, k) in folders.iter().enumerate() {
        let name = k
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        b.push(Row::new(
            "storage.folder",
            tr!("{name}  ({n} files)", name = name, n = k.files),
            delete(i, tr!("{size}  delete ×", size = bytes(k.bytes as f64))),
            Act::DeleteKept(i),
        ));
    }
}

/// Delete kept song folder `i` (or [`ALL`], [`STALE`]), on a second press.
pub(super) fn delete(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    if !armed.confirm(Confirm::DeleteKept(i)) {
        return;
    }
    let bms = ctx.paths.root.join("bms");
    let folders = vfs::folder_sizes(&bms);
    if i == ALL {
        vfs::remove_dir(&bms);
        ctx.toast(tr!("Deleted every song kept here").to_string());
    } else if i == STALE {
        let stale = stale_folders(ctx, &folders);
        let total: u64 = stale.iter().map(|&k| folders[k].bytes).sum();
        for &k in &stale {
            vfs::remove_dir(&folders[k].dir);
        }
        ctx.toast(tr!(
            "Deleted {folders} ({size})",
            folders = folders_n(stale.len()),
            size = bytes(total as f64)
        ));
    } else if let Some(k) = folders.get(i) {
        vfs::remove_dir(&k.dir);
        let name = k.dir.file_name().map(|n| n.to_string_lossy().into_owned());
        ctx.toast(tr!(
            "Deleted {name} ({size})",
            name = name.unwrap_or_default(),
            size = bytes(k.bytes as f64)
        ));
    }
}
