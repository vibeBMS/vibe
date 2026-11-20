//! SKINS: the skin of each screen (a sub-page each for its options and
//! offsets), and GET SKINS, the catalog (a sub-page each skin).

use super::row::{mb, Act, Row, Rows};
use super::{Armed, Confirm, Screen, Sub};
use crate::app::Ctx;
use crate::platform::IS_WEB;
use crate::scene::play_skin::{configured_path, PERSPECTIVE};
use crate::scene::skinned;
use crate::scene::skinselect::TYPES;
use crate::skin::{SkinType, OFFSET_FIELDS};
use crate::skinstore::{SkinEntry, State};
use std::path::{Path, PathBuf};

/// The row opening `s`, when its skin is chosen.
pub(super) fn open_row(ctx: &Ctx, s: Screen) -> Option<Row> {
    let (kind, id, label, desc) = match s {
        Screen::KeyConfig => (
            SkinType::KeyConfig,
            "open.key_config",
            tr!("Open the key config screen"),
            tr!("The key config skin's screen, with a picture of the controller."),
        ),
        Screen::SkinSelect => (
            SkinType::SkinSelect,
            "open.skin_select",
            tr!("Open the skin select screen"),
            tr!("The skin select skin's screen: every skin with a preview."),
        ),
        Screen::SoundSets => (
            SkinType::SkinSelect,
            "open.sound_sets",
            tr!("Choose in the skin select screen"),
            tr!("The sound sets, in the skin select skin's screen."),
        ),
        Screen::TouchLayout => return None,
    };
    skinned::configured_skin(ctx, kind)?;
    Some(Row::new(id, label, tr!("open"), Act::Open(s)).desc(desc))
}

/// The `config.skins` value of type `t`'s skin and its path.
fn chosen(ctx: &Ctx, t: SkinType) -> Option<(&str, PathBuf)> {
    let value = ctx.config.skins.get(t.key()?)?;
    Some((value, configured_path(ctx, value)?))
}

/// Step custom option `i` of the skin chosen for `t`.
pub(super) fn step_skin_option(ctx: &mut Ctx, t: SkinType, i: usize, d: i32) {
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let value = value.to_string();
    if let Some(o) = ctx.skins.custom_options(&path, &ctx.paths.skins).get(i) {
        skinned::step_option(ctx, &value, o, d);
    }
}

/// Step field `field` of adjustable offset `i` of the skin chosen for `t`.
pub(super) fn step_skin_offset(ctx: &mut Ctx, t: SkinType, i: usize, field: usize, d: i32) {
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let value = value.to_string();
    if let Some(o) = ctx.skins.custom_offsets(&path, &ctx.paths.skins).get(i) {
        skinned::step_offset(ctx, &value, &o.name, field, d);
    }
}

/// Choose the next (`d`) skin for `t`: the built-in default, then the
/// installed skins of its type.
pub(super) fn cycle_skin(c: &mut Ctx, t: SkinType, d: i32) {
    let Some(key) = t.key() else {
        return;
    };
    let opts = skinned::skin_choices(c, t);
    let cur = c.config.skins.get(key).cloned().unwrap_or_default();
    let i = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
    let next = opts[(i + d).rem_euclid(opts.len() as i32) as usize].clone();
    if opts.len() == 1 {
        let dir = if t == SkinType::SoundSet {
            c.paths.root.join("sounds")
        } else {
            c.paths.skins.clone()
        };
        c.toast(tr!(
            "Put more {key} skins in {dir}",
            key = key,
            dir = dir.display()
        ));
    }
    c.config.skins.insert(key.to_string(), next);
}

/// Display name of a configured skin (its header name, else the file stem).
fn skin_label(ctx: &Ctx, value: &str) -> String {
    if value == PERSPECTIVE {
        return "TOUCH PERSPECTIVE".into();
    }
    if value == crate::sysound::RANDOM_SET {
        return "RANDOM".into();
    }
    let Some(p) = configured_path(ctx, value) else {
        return tr!("DEFAULT").into();
    };
    crate::skin::header(&p, &ctx.paths.skins)
        .map(|h| h.1)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

/// What type `t`'s screen is called.
pub(super) fn screen_name(t: SkinType) -> String {
    let Some(key) = t.key() else {
        return String::new();
    };
    match t {
        _ if t.mode().is_some() => tr!("Play ({key})", key = key),
        SkinType::Select => tr!("Select").into(),
        SkinType::Decide => tr!("Decide").into(),
        SkinType::Result => tr!("Result").into(),
        SkinType::CourseResult => tr!("Course result").into(),
        SkinType::KeyConfig => tr!("Key config").into(),
        SkinType::SkinSelect => tr!("Skin select").into(),
        SkinType::SoundSet => tr!("Sound set").into(),
        SkinType::Battle7 => tr!("7KEYS battle").into(),
        SkinType::Battle5 => tr!("5KEYS battle").into(),
        _ => format!("{}{}", &key[..1], key[1..].to_lowercase().replace('_', " ")),
    }
}

/// The row choosing type `t`'s skin: ←→ choose, Enter its sub-page.
fn choice_row(ctx: &Ctx, t: SkinType) -> Row {
    let cur = match t
        .key()
        .and_then(|k| ctx.config.skins.get(k))
        .map(String::as_str)
    {
        None | Some("") => tr!("DEFAULT").into(),
        Some(p) => skin_label(ctx, p),
    };
    Row::new("skins.choice", screen_name(t), cur, Act::CycleSkin(t))
        .desc(tr!(
            "The skin this screen uses. ←→ choose; Enter: its options."
        ))
        .opens(Sub::Skin(t))
}

/// The skins chosen other than the built-in ones.
fn custom(ctx: &Ctx) -> usize {
    TYPES
        .iter()
        .filter_map(|t| t.key().and_then(|k| ctx.config.skins.get(k)))
        .filter(|v| !v.is_empty())
        .count()
}

/// The catalog skins with an update waiting (that can be downloaded now).
pub(super) fn updates(ctx: &Ctx) -> usize {
    let store = &ctx.skins.store;
    (store.catalog.skins.iter())
        .filter(|e| {
            offered(ctx, e)
                && matches!(
                    store.state(&ctx.paths.skins, e),
                    State::Installed { update: true }
                )
        })
        .count()
}

/// SKINS' chips on the top level.
pub(super) fn chips(ctx: &Ctx) -> Vec<String> {
    let mut v = vec![tr!("{n} custom", n = custom(ctx))];
    match updates(ctx) {
        0 => {}
        1 => v.push(tr!("1 update").into()),
        n => v.push(tr!("{n} updates", n = n)),
    }
    v
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    b.header(tr!("SCREENS"));
    for t in TYPES {
        b.push(choice_row(ctx, t));
    }
    b.extend(open_row(ctx, Screen::SkinSelect));
    b.header(tr!("GET SKINS"));
    let n = ctx
        .skins
        .store
        .catalog
        .skins
        .iter()
        .filter(|e| shown(ctx, e))
        .count();
    let value = match updates(ctx) {
        0 => tr!("{n} skins", n = n),
        u => tr!("{n} skins · {u} to update", n = n, u = u),
    };
    b.push(
        Row::sub("catalog", tr!("Get skins"), value, Sub::Catalog)
            .desc(tr!("Skins from the catalog: install, update, delete."))
            .keys("download install catalog"),
    );
}

/// SKINS › one screen: the skin, the screen it draws, its own options and
/// its position adjustments (beatoraja offsets).
pub(super) fn skin(b: &mut Rows, t: SkinType) {
    let ctx = b.ctx;
    let c = &ctx.config;
    b.header(tr!("SKIN"));
    let mut choice = choice_row(ctx, t);
    choice.open = None;
    b.push(choice.desc(tr!(
        "The skin this screen uses: the default, then those installed."
    )));
    let screen = match t {
        SkinType::KeyConfig => Some(Screen::KeyConfig),
        SkinType::SkinSelect => Some(Screen::SkinSelect),
        SkinType::SoundSet => Some(Screen::SoundSets),
        _ => None,
    };
    b.extend(screen.and_then(|s| open_row(ctx, s)));
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let options = ctx.skins.custom_options(&path, &ctx.paths.skins);
    let offsets = ctx.skins.custom_offsets(&path, &ctx.paths.skins);
    if options.is_empty() && offsets.is_empty() {
        b.push(Row::note("skin.none", tr!("This skin has no options"), ""));
        return;
    }
    if !options.is_empty() {
        b.header(tr!("OPTIONS"));
    }
    let picked = c.skin_options.get(value);
    for (i, o) in options.iter().enumerate() {
        let k = o.chosen(picked.and_then(|m| m.get(&o.name)).map(String::as_str));
        b.push(
            Row::new(
                "skin.option",
                o.name.clone(),
                o.labels.get(k).cloned().unwrap_or_default(),
                Act::SkinOption(t, i),
            )
            .desc(tr!("One of the skin's own options.")),
        );
    }
    if !offsets.is_empty() {
        b.header(tr!("OFFSETS"));
    }
    let set = c.skin_offsets.get(value);
    for (i, o) in offsets.iter().enumerate() {
        let cur = set
            .and_then(|m| m.get(&o.name))
            .copied()
            .unwrap_or_default();
        for (f, _) in o.fields.iter().enumerate().filter(|(_, on)| **on) {
            b.push(
                Row::new(
                    "skin.offset",
                    format!("{} {}", o.name, OFFSET_FIELDS[f]),
                    cur[f].to_string(),
                    Act::SkinOffset(t, i, f),
                )
                .desc(tr!("Moves or sizes a part of the skin.")),
            );
        }
    }
}

/// Third-party sources are on, or `e` is the author's own: it can be
/// downloaded.
fn offered(ctx: &Ctx, e: &SkinEntry) -> bool {
    e.third_party_refusal(ctx.net.hosts.config.third_party)
        .is_none()
}

/// The browser leaves out what it cannot install (the largest skins), and
/// third parties' copies are left out while their sources are off (unless
/// installed: they can be deleted).
pub(super) fn shown(ctx: &Ctx, e: &SkinEntry) -> bool {
    (!IS_WEB || e.web_refusal().is_none())
        && (offered(ctx, e)
            || matches!(
                ctx.skins.store.state(&ctx.paths.skins, e),
                State::Installed { .. }
            ))
}

/// What can be done with catalog skin `e` now.
fn state_value(ctx: &Ctx, armed: &Armed<Confirm>, e: &SkinEntry) -> String {
    let size = if e.size > 0 {
        format!(" ({})", mb(e.size))
    } else {
        String::new()
    };
    match ctx.skins.store.state(&ctx.paths.skins, e) {
        State::Busy(p) => format!("{}  {}", p.label(), tr!("cancel ×")),
        State::Installed { update: true } if !offered(ctx, e) => tr!("installed ✓").into(),
        State::Installed { update: true } => tr!("update ↓{size}", size = size),
        State::Installed { .. } if armed.is(&Confirm::Reinstall(e.id.clone())) => {
            tr!("press again to reinstall").into()
        }
        State::Installed { .. } => tr!("installed ✓").into(),
        State::Absent if IS_WEB && e.web_refusal().is_some() => {
            tr!("desktop / mobile app only").into()
        }
        State::Absent if !offered(ctx, e) => tr!("third-party source: turn on in LIBRARY").into(),
        State::Absent => tr!("install ↓{size}", size = size),
    }
}

/// "LR2 · SELECT, PLAY 7K · 1280x720".
fn kind_line(e: &SkinEntry) -> String {
    let kind = if e.kind == "lr2" {
        "LR2"
    } else if e.lua {
        "beatoraja Lua"
    } else {
        "beatoraja"
    };
    let mut info = format!("{kind} · {}", e.scenes_label());
    if !e.resolution.is_empty() {
        info.push_str(&format!(" · {}", e.resolution));
    }
    info
}

/// SKINS › GET SKINS: one row per catalog skin.
pub(super) fn catalog(b: &mut Rows) {
    let ctx = b.ctx;
    let store = &ctx.skins.store;
    b.header(tr!("CATALOG"));
    b.push(
        Row::new(
            "catalog.update",
            tr!("Skin catalog"),
            if store.is_fetching() {
                tr!("reading…")
            } else {
                tr!("update ↻")
            },
            Act::Run(|c, _| c.skins.store.refresh(true), 0),
        )
        .desc(tr!(
            "Read the list again. Skins are installed into {dir}.",
            dir = ctx.paths.skins.display()
        )),
    );
    if IS_WEB {
        b.push(Row::note(
            "catalog.web",
            tr!("Skins over 400 MB are in the desktop and mobile apps"),
            "",
        ));
    }
    let hidden = (store.catalog.skins.iter())
        .filter(|e| !shown(ctx, e) && (!IS_WEB || e.web_refusal().is_none()))
        .count();
    if hidden > 0 {
        b.push(
            Row::note(
                "catalog.third_party",
                tr!("{n} more from third-party sources", n = hidden),
                "",
            )
            .desc(tr!(
                "Other people's copies, not their authors' own downloads: they are listed with Third-party sources on (SETTINGS › LIBRARY)."
            )),
        );
    }
    b.header(tr!("SKINS"));
    for e in store.catalog.skins.iter().filter(|e| shown(ctx, e)) {
        let version = if e.version.is_empty() {
            String::new()
        } else {
            format!(" {}", e.version)
        };
        b.push(
            Row::sub(
                "catalog.skin",
                format!("{}{version}", e.name),
                state_value(ctx, b.armed, e),
                Sub::CatalogSkin(e.id.clone()),
            )
            .desc(tr!(
                "by {author} · {kind}",
                author = e.author,
                kind = kind_line(e)
            )),
        );
    }
}

/// The catalog's skin with id `id`, and its index.
pub(super) fn entry<'a>(ctx: &'a Ctx, id: &str) -> Option<(usize, &'a SkinEntry)> {
    (ctx.skins.store.catalog.skins.iter())
        .enumerate()
        .find(|(_, e)| e.id == id)
}

/// SKINS › GET SKINS › a skin: install or update it, its page, its
/// terms, delete it.
pub(super) fn catalog_skin(b: &mut Rows, id: &str) {
    let ctx = b.ctx;
    let Some((i, e)) = entry(ctx, id) else {
        return;
    };
    b.header(kind_line(e));
    b.push(
        Row::new(
            "catalog.get",
            tr!("{name} by {author}", name = e.name, author = e.author),
            state_value(ctx, b.armed, e),
            Act::SkinGet(i),
        )
        .desc(tr!(
            "Install it, update it, or (while it downloads) cancel."
        )),
    );
    b.push(
        Row::new(
            "catalog.page",
            tr!("Its page"),
            tr!("open ↗"),
            Act::Run(
                |c, i| {
                    if let Some(e) = c.skins.store.catalog.skins.get(i) {
                        crate::platform::open_url(&e.homepage);
                    }
                },
                i,
            ),
        )
        .desc(tr!("The skin's own page, in the browser.")),
    );
    if e.third_party() {
        b.push(
            Row::note("catalog.source", tr!("Third-party source"), "").desc(tr!(
                "Someone else's copy, not the author's own download: it is downloaded only with Third-party sources on (SETTINGS › LIBRARY)."
            )),
        );
    }
    // the author's terms, on a line of their own (they can be long)
    if !e.license.is_empty() {
        b.push(Row::note("catalog.terms", tr!("Terms"), "").desc(e.license.clone()));
    }
    if matches!(
        ctx.skins.store.state(&ctx.paths.skins, e),
        State::Installed { .. }
    ) {
        b.push(
            Row::new(
                "catalog.delete",
                tr!("Delete it"),
                if b.armed.is(&Confirm::DeleteSkin(e.id.clone())) {
                    tr!("press again to delete")
                } else {
                    tr!("delete ×")
                },
                Act::SkinDelete(i),
            )
            .desc(format!("skins/{}", e.folder())),
        );
    }
}

/// Install catalog skin `i`: an installed one again on a second press, a
/// downloading one is cancelled.
pub(super) fn get(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    let Some(e) = ctx.skins.store.catalog.skins.get(i).cloned() else {
        return;
    };
    match ctx.skins.store.state(&ctx.paths.skins, &e) {
        State::Busy(_) => {
            ctx.skins.store.cancel(&e.id);
            return;
        }
        State::Installed { update: false } if !armed.confirm(Confirm::Reinstall(e.id)) => return,
        _ => armed.clear(),
    }
    let r = ctx.skins.store.install(&ctx.paths, &ctx.net.hosts, i);
    ctx.toast(r.unwrap_or_else(|e| e));
}

/// Delete catalog skin `i`, on a second press; the skins chosen from its
/// folders go back to the default.
pub(super) fn delete(ctx: &mut Ctx, armed: &mut Armed<Confirm>, i: usize) {
    let Some(e) = ctx.skins.store.catalog.skins.get(i).cloned() else {
        return;
    };
    if !armed.confirm(Confirm::DeleteSkin(e.id.clone())) {
        return;
    }
    match ctx.skins.store.delete(&ctx.paths, i) {
        Ok(folders) => {
            let skins = &ctx.paths.skins;
            let rel = skins
                .strip_prefix(&ctx.paths.root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| skins.clone());
            let gone: Vec<PathBuf> = folders
                .iter()
                .flat_map(|f| [rel.join(f), skins.join(f)])
                .collect();
            for v in ctx.config.skins.values_mut() {
                if gone.iter().any(|g| Path::new(v.as_str()).starts_with(g)) {
                    v.clear();
                }
            }
            ctx.skins.files_changed();
            ctx.toast(tr!("Deleted {name}", name = e.name));
        }
        Err(m) => ctx.toast(m),
    }
}
