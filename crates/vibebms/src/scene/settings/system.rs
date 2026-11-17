use super::{mb, Act, Row};
use crate::app::Ctx;
use crate::scene::option_panel::onoff;
use crate::update::{Status, Version, SUPPORTED, VERSION};

fn update_now(c: &mut Ctx, _: usize) {
    if let Some(msg) = c.updater.install() {
        c.toast(msg);
    }
}

/// The version, and on the desktop the self-update.
pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let mut v = vec![Row::note("Version", format!("vibeBMS {VERSION}"))];
    if !SUPPORTED {
        return v;
    }
    match Version::current() {
        Some(cur) if cur.is_nightly() => {}
        Some(cur) if cur.dev => {
            v.push(Row::note(
                "A local build: only the nightly builds update themselves",
                "",
            ));
            return v;
        }
        _ => return v,
    }
    // installed by a package manager, or in a folder it cannot write to:
    // its updates come from elsewhere
    if !crate::update::self_updating() {
        return v;
    }
    v.push(Row::adjust(
        "Check for updates (on start)",
        onoff(ctx.config.check_updates),
        |c, _| {
            c.config.check_updates ^= true;
            if matches!(c.updater.status, Status::Idle | Status::CheckFailed(_)) {
                c.updater.check(c.config.check_updates);
            }
        },
    ));
    match &ctx.updater.status {
        Status::Idle => {}
        Status::Checking => v.push(Row::note("Checking for updates…", "")),
        Status::UpToDate => v.push(Row::note("vibeBMS is up to date", "")),
        Status::CheckFailed(e) => {
            v.push(Row::note(format!("Could not check for updates: {e}"), ""))
        }
        Status::Available(o) | Status::Failed(o, _) => {
            let failed = matches!(ctx.updater.status, Status::Failed(..));
            v.push(Row::new(
                format!("Update available: {} ({})", o.version, mb(o.size)),
                if failed {
                    "try again ↵"
                } else {
                    "Update now ↵"
                },
                Act::Run(update_now, 0),
            ));
            if let Status::Failed(_, e) = &ctx.updater.status {
                v.push(Row::note(format!("    The update failed: {e}"), ""));
            }
        }
        Status::Downloading(o, p) => v.push(Row::note(
            format!("Downloading {}… {}%", o.version, p.percent()),
            "",
        )),
        Status::Installed(ver) => v.push(Row::new(
            format!("Updated to {ver}"),
            "Restart now ↵",
            Act::Restart,
        )),
    }
    v
}
