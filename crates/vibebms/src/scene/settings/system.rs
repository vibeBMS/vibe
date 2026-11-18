use super::row::{mb, Act, Row, Rows};
use super::{library, Confirm, Sub, TextField};
use crate::app::Ctx;
use crate::scene::option_panel::onoff;
use crate::ui::Armed;
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
    profiles(b);
    b.header("UPDATES");
    updates(b);
    b.header("DATA");
    b.push(library::data_folder(ctx));
    #[cfg(desktop)]
    super::streaming::rows(b);
}

/// The profiles: each opens its page (switch, rename, delete); a new one.
fn profiles(b: &mut Rows) {
    let ctx = b.ctx;
    let reg = &ctx.profiles.registry;
    for p in &reg.profiles {
        let using = p.id == ctx.paths.profile;
        b.push(
            Row::sub(
                "profile",
                p.name.clone(),
                if using { "in use" } else { "" },
                Sub::Profile(p.id.clone()),
            )
            .desc(match using {
                true => "The profile in use: its records, replays, settings and IR accounts.",
                false => "Another profile: switch to it, rename or delete it.",
            })
            .keys("profile player user switch account"),
        );
    }
    b.push(
        Row::new(
            "profile.new",
            "New profile",
            "name it ↵",
            Act::Text(TextField::NewProfile),
        )
        .desc("Starts with these settings (not the IR accounts) and no records; the library is shared.")
        .keys("profile add create player"),
    );
}

/// Profile `id`'s page.
pub(super) fn profile(b: &mut Rows, id: &str) {
    let ctx = b.ctx;
    let reg = &ctx.profiles.registry;
    let Some(i) = reg.profiles.iter().position(|p| p.id == id) else {
        return;
    };
    let using = id == ctx.paths.profile;
    b.header("PROFILE");
    if using {
        b.push(Row::note("profile.using", "This profile is in use", ""));
    } else {
        b.push(
            Row::new(
                "profile.use",
                "Switch to this profile",
                "switch ↵",
                Act::Run(use_profile, i),
            )
            .desc("Back to song select with its settings, records and IR accounts.")
            .keys("profile change load"),
        );
    }
    b.push(
        Row::new(
            "profile.name",
            "Name",
            reg.profiles[i].name.clone(),
            Act::Text(TextField::ProfileName(i)),
        )
        .desc("The profile's name (its folder keeps the name it was made with).")
        .keys("profile rename"),
    );
    let dir = crate::paths::profile_dir(&ctx.paths.root, id);
    let shown = dir
        .strip_prefix(&ctx.paths.root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map_or_else(
            || "the data folder".to_string(),
            |r| r.display().to_string(),
        );
    b.push(Row::note("profile.folder", "Its files", shown));
    if id.is_empty() {
        b.push(Row::note(
            "profile.default",
            "The default profile: its files are the data folder's, it is not deleted",
            "",
        ));
    } else if using {
        b.push(Row::note(
            "profile.delete_using",
            "To delete this profile, switch to another first",
            "",
        ));
    } else {
        b.push(
            Row::new(
                "profile.delete",
                "Delete this profile",
                if b.armed.is(&Confirm::DeleteProfile(id.to_string())) {
                    "press again to delete"
                } else {
                    "delete ×"
                },
                Act::ProfileDelete(i),
            )
            .desc("Its records, replays, settings and IR keys are deleted; the library stays.")
            .keys("profile remove"),
        );
    }
}

fn use_profile(c: &mut Ctx, i: usize) {
    if let Some(p) = c.profiles.registry.profiles.get(i) {
        c.profiles.switch_to = Some(p.id.clone());
    }
}

pub(super) fn profile_name(ctx: &Ctx, i: usize) -> String {
    (ctx.profiles.registry.profiles.get(i)).map_or_else(String::new, |p| p.name.clone())
}

pub(super) fn new_profile(ctx: &mut Ctx, name: &str) {
    if name.is_empty() {
        return;
    }
    match ctx.create_profile(name) {
        Ok(_) => ctx.toast(format!("Profile {name} made: choose it to switch to it")),
        Err(e) => ctx.toast(e),
    }
}

pub(super) fn rename_profile(ctx: &mut Ctx, i: usize, name: &str) {
    let Some(id) = ctx.profiles.registry.profiles.get(i).map(|p| p.id.clone()) else {
        return;
    };
    if let Err(e) = ctx.rename_profile(&id, name) {
        ctx.toast(e);
    }
}

pub(super) fn delete_profile(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    let Some(p) = ctx.profiles.registry.profiles.get(i).cloned() else {
        return;
    };
    if !armed.confirm(Confirm::DeleteProfile(p.id.clone())) {
        return;
    }
    match ctx.delete_profile(&p.id) {
        Ok(()) => ctx.toast(format!("Profile {} deleted", p.name)),
        Err(e) => ctx.toast(e),
    }
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
