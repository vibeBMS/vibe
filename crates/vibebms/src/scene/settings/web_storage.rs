//! Web: the songs kept in the browser, and how much room they take.

use super::{Act, Armed, Confirm, Row};
use crate::app::Ctx;
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

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
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
                        .any(|((sha, _), b)| *sha == s.sha256 && b.last_played >= cut)
            })
        })
        .map(|(i, _)| i)
        .collect()
}

pub(super) fn rows(ctx: &Ctx, armed: &Armed<Confirm>, v: &mut Vec<Row>) {
    let st = crate::platform::opfs::status();
    if !st.available {
        v.push(Row::note(
            "Songs stay until the page closes (this browser keeps no files)",
            "",
        ));
        return;
    }
    let room = if st.quota > 0.0 {
        format!("{} of {}", bytes(st.usage), bytes(st.quota))
    } else {
        bytes(st.usage)
    };
    let saving = if st.pending > 0 {
        format!(" · saving {}", st.pending)
    } else {
        String::new()
    };
    v.push(Row::new(
        format!("Songs kept in this browser: {room}{saving}"),
        if st.persisted {
            "kept for good"
        } else {
            "ask to keep ↵"
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
            "press again to delete".into()
        } else {
            value
        }
    };
    let stale = stale_folders(ctx, &folders);
    if !stale.is_empty() {
        let total: u64 = stale.iter().map(|&i| folders[i].bytes).sum();
        v.push(Row::new(
            format!(
                "Delete songs not played for {STALE_DAYS} days ({} folder{}, {})",
                stale.len(),
                plural(stale.len()),
                bytes(total as f64)
            ),
            delete(STALE, "delete ×".into()),
            Act::DeleteKept(STALE),
        ));
    }
    if !folders.is_empty() {
        let total: u64 = folders.iter().map(|f| f.bytes).sum();
        v.push(Row::new(
            format!(
                "Delete every song kept here ({} folder{}, {})",
                folders.len(),
                plural(folders.len()),
                bytes(total as f64)
            ),
            delete(ALL, "delete ×".into()),
            Act::DeleteKept(ALL),
        ));
    }
    for (i, k) in folders.iter().enumerate() {
        let name = k
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        v.push(Row::new(
            format!("    {name}  ({} files)", k.files),
            delete(i, format!("{}  delete ×", bytes(k.bytes as f64))),
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
        ctx.toast("Deleted every song kept here".to_string());
    } else if i == STALE {
        let stale = stale_folders(ctx, &folders);
        let total: u64 = stale.iter().map(|&k| folders[k].bytes).sum();
        for &k in &stale {
            vfs::remove_dir(&folders[k].dir);
        }
        ctx.toast(format!(
            "Deleted {} folder{} ({})",
            stale.len(),
            plural(stale.len()),
            bytes(total as f64)
        ));
    } else if let Some(k) = folders.get(i) {
        vfs::remove_dir(&k.dir);
        let name = k.dir.file_name().map(|n| n.to_string_lossy().into_owned());
        ctx.toast(format!(
            "Deleted {} ({})",
            name.unwrap_or_default(),
            bytes(k.bytes as f64)
        ));
    }
}
