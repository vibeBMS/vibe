use super::row::onoff;
use super::row::{mb, Act, Row, Rows};
use super::{library, Confirm, Sub, TextField};
use crate::app::Ctx;
use crate::i18n::Language;
use crate::ui::Armed;
use crate::update::{Status, Version, SUPPORTED, VERSION};

/// The setting as shown: each language in itself, AUTO with the one it
/// picked.
fn language_name(l: Language) -> String {
    match l {
        Language::Auto => format!("AUTO ({})", l.resolve().native()),
        l => l.resolve().native().to_string(),
    }
}

fn update_now(c: &mut Ctx, _: usize) {
    if let Some(msg) = c.net.updater.install() {
        c.toast(msg);
    }
}

/// The player's name, the version and on the desktop the self-update,
/// the data folder.
pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    b.header(tr!("LANGUAGE"));
    b.push(
        Row::adjust(
            "language",
            tr!("Language"),
            language_name(ctx.config.language),
            |c, d| {
                let l = crate::scene::option_panel::cycle(&Language::ALL, c.config.language, d);
                c.config.language = l;
                crate::i18n::apply(l);
            },
        )
        .desc(tr!(
            "The language of vibeBMS's own text (not the charts' or the skins')."
        ))
        .keys("language locale translation english korean japanese chinese 언어 言語 语言 語言"),
    );
    b.header(tr!("PROFILE"));
    b.push(
        Row::new(
            "player_name",
            tr!("Player name"),
            ctx.config.player_name.clone(),
            Act::Text(TextField::PlayerName),
        )
        .desc(tr!("Shown on results and skins."))
        .keys("profile nickname"),
    );
    profiles(b);
    b.header(tr!("UPDATES"));
    updates(b);
    b.header(tr!("DATA"));
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
                if using { tr!("in use") } else { "" },
                Sub::Profile(p.id.clone()),
            )
            .desc(match using {
                true => tr!("The profile in use: its records, replays, settings and IR accounts."),
                false => tr!("Another profile: switch to it, rename or delete it."),
            })
            .keys("profile player user switch account"),
        );
    }
    b.push(
        Row::new(
            "profile.new",
            tr!("New profile"),
            tr!("name it ↵"),
            Act::Text(TextField::NewProfile),
        )
        .desc(tr!("Starts with these settings (not the IR accounts) and no records; the library is shared."))
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
    b.header(tr!("PROFILE"));
    if using {
        b.push(Row::note(
            "profile.using",
            tr!("This profile is in use"),
            "",
        ));
    } else {
        b.push(
            Row::new(
                "profile.use",
                tr!("Switch to this profile"),
                tr!("switch ↵"),
                Act::Run(use_profile, i),
            )
            .desc(tr!(
                "Back to song select with its settings, records and IR accounts."
            ))
            .keys("profile change load"),
        );
    }
    b.push(
        Row::new(
            "profile.name",
            tr!("Name"),
            reg.profiles[i].name.clone(),
            Act::Text(TextField::ProfileName(i)),
        )
        .desc(tr!(
            "The profile's name (its folder keeps the name it was made with)."
        ))
        .keys("profile rename"),
    );
    let dir = crate::paths::profile_dir(&ctx.paths.root, id);
    let shown = dir
        .strip_prefix(&ctx.paths.root)
        .ok()
        .filter(|r| !r.as_os_str().is_empty())
        .map_or_else(
            || tr!("the data folder").to_string(),
            |r| r.display().to_string(),
        );
    b.push(Row::note("profile.folder", tr!("Its files"), shown));
    if id.is_empty() {
        b.push(Row::note(
            "profile.default",
            tr!("The default profile: its files are the data folder's, it is not deleted"),
            "",
        ));
    } else if using {
        b.push(Row::note(
            "profile.delete_using",
            tr!("To delete this profile, switch to another first"),
            "",
        ));
    } else {
        b.push(
            Row::new(
                "profile.delete",
                tr!("Delete this profile"),
                if b.armed.is(&Confirm::DeleteProfile(id.to_string())) {
                    tr!("press again to delete")
                } else {
                    tr!("delete ×")
                },
                Act::ProfileDelete(i),
            )
            .desc(tr!(
                "Its records, replays, settings and IR keys are deleted; the library stays."
            ))
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
        Ok(_) => ctx.toast(tr!(
            "Profile {name} made: choose it to switch to it",
            name = name
        )),
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
        Ok(()) => ctx.toast(tr!("Profile {name} deleted", name = p.name)),
        Err(e) => ctx.toast(e),
    }
}

fn updates(v: &mut Rows) {
    let ctx = v.ctx;
    v.push(
        Row::note("info.version", tr!("Version"), format!("vibeBMS {VERSION}")).keys("about build"),
    );
    if !SUPPORTED {
        return;
    }
    match Version::current() {
        Some(cur) if cur.is_nightly() => {}
        Some(cur) if cur.dev => {
            v.push(Row::note(
                "update.local",
                tr!("A local build: only the nightly builds update themselves"),
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
            tr!("Check for updates on start"),
            onoff(ctx.config.check_updates),
            |c, _| {
                c.config.check_updates ^= true;
                if matches!(c.net.updater.status, Status::Idle | Status::CheckFailed(_)) {
                    c.net.updater.check(c.config.check_updates);
                }
            },
        )
        .desc(tr!("Nightly builds only.")),
    );
    match &ctx.net.updater.status {
        Status::Idle => {}
        Status::Checking => v.push(Row::note("update.state", tr!("Checking for updates…"), "")),
        Status::UpToDate => v.push(Row::note("update.state", tr!("vibeBMS is up to date"), "")),
        Status::CheckFailed(e) => v.push(Row::note(
            "update.state",
            tr!("Could not check for updates: {e}", e = e),
            "",
        )),
        Status::Available(o) | Status::Failed(o, _) => {
            let failed = matches!(ctx.net.updater.status, Status::Failed(..));
            v.push(Row::new(
                "update.now",
                tr!(
                    "Update available: {version} ({size})",
                    version = o.version,
                    size = mb(o.size)
                ),
                if failed {
                    tr!("try again ↵")
                } else {
                    tr!("Update now ↵")
                },
                Act::Run(update_now, 0),
            ));
            if let Status::Failed(_, e) = &ctx.net.updater.status {
                v.push(Row::note(
                    "update.state",
                    tr!("The update failed: {e}", e = e),
                    "",
                ));
            }
        }
        Status::Downloading(o) => v.push(Row::note(
            "update.state",
            tr!(
                "Downloading {version}… {percent}%",
                version = o.version,
                percent = ctx.net.updater.percent()
            ),
            "",
        )),
        Status::Installed(ver) => v.push(Row::new(
            "update.restart",
            tr!("Updated to {ver}", ver = ver),
            tr!("Restart now ↵"),
            Act::Restart,
        )),
    }
}
