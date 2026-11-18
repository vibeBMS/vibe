use super::row::{mb, Act, Row, Rows};
use super::{library, TextField};
use crate::app::Ctx;
use crate::scene::option_panel::onoff;
use crate::update::{Status, Version, SUPPORTED, VERSION};

fn update_now(c: &mut Ctx, _: usize) {
    if let Some(msg) = c.net.updater.install() {
        c.toast(msg);
    }
}

/// The player's name, the version and on the desktop the self-update,
/// the data folder.
pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    b.header("PROFILE");
    b.push(
        Row::new(
            "player_name",
            "Player name",
            ctx.config.player_name.clone(),
            Act::Text(TextField::PlayerName),
        )
        .desc("Shown on results and skins.")
        .keys("profile nickname"),
    );
    b.header("UPDATES");
    updates(b);
    b.header("DATA");
    b.push(library::data_folder(ctx));
    #[cfg(desktop)]
    super::streaming::rows(b);
}

fn updates(v: &mut Rows) {
    let ctx = v.ctx;
    v.push(Row::note("info.version", "Version", format!("vibeBMS {VERSION}")).keys("about build"));
    if !SUPPORTED {
        return;
    }
    match Version::current() {
        Some(cur) if cur.is_nightly() => {}
        Some(cur) if cur.dev => {
            v.push(Row::note(
                "update.local",
                "A local build: only the nightly builds update themselves",
                "",
            ));
            return;
        }
        _ => return,
    }
    // installed by a package manager, or in a folder it cannot write to:
    // its updates come from elsewhere
    if !crate::update::self_updating() {
        return;
    }
    v.push(
        Row::adjust(
            "check_updates",
            "Check for updates on start",
            onoff(ctx.config.check_updates),
            |c, _| {
                c.config.check_updates ^= true;
                if matches!(c.net.updater.status, Status::Idle | Status::CheckFailed(_)) {
                    c.net.updater.check(c.config.check_updates);
                }
            },
        )
        .desc("Nightly builds only."),
    );
    match &ctx.net.updater.status {
        Status::Idle => {}
        Status::Checking => v.push(Row::note("update.state", "Checking for updates…", "")),
        Status::UpToDate => v.push(Row::note("update.state", "vibeBMS is up to date", "")),
        Status::CheckFailed(e) => v.push(Row::note(
            "update.state",
            format!("Could not check for updates: {e}"),
            "",
        )),
        Status::Available(o) | Status::Failed(o, _) => {
            let failed = matches!(ctx.net.updater.status, Status::Failed(..));
            v.push(Row::new(
                "update.now",
                format!("Update available: {} ({})", o.version, mb(o.size)),
                if failed {
                    "try again ↵"
                } else {
                    "Update now ↵"
                },
                Act::Run(update_now, 0),
            ));
            if let Status::Failed(_, e) = &ctx.net.updater.status {
                v.push(Row::note(
                    "update.state",
                    format!("The update failed: {e}"),
                    "",
                ));
            }
        }
        Status::Downloading(o, p) => v.push(Row::note(
            "update.state",
            format!("Downloading {}… {}%", o.version, p.percent()),
            "",
        )),
        Status::Installed(ver) => v.push(Row::new(
            "update.restart",
            format!("Updated to {ver}"),
            "Restart now ↵",
            Act::Restart,
        )),
    }
}
