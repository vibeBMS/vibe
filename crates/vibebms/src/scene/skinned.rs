//! Shared pieces of the skinned (non-play) scenes: which skin to use and
//! loading it; its options and offsets.

use crate::app::Ctx;
use crate::gfx::Gfx;
use crate::skin::render::LoadedSkin;
use crate::skin::SkinType;
use std::path::PathBuf;

/// The built-in default skin of a screen.
pub fn default_skin(kind: SkinType, portrait: bool) -> Option<PathBuf> {
    let name = match kind {
        SkinType::Select => "select",
        SkinType::Result | SkinType::CourseResult => "result",
        _ => return None,
    };
    let dir = PathBuf::from(crate::platform::vfs::BUILTIN).join("skins/default");
    let p = dir.join(format!("{name}_portrait.json"));
    if portrait && crate::platform::vfs::read(&p).is_ok() {
        return Some(p);
    }
    Some(dir.join(format!("{name}.json")))
}

/// The configured skin for a screen (`None` = the built-in default).
pub fn configured_skin(ctx: &Ctx, kind: SkinType) -> Option<PathBuf> {
    let v = ctx.config.skins.get(kind.config_key()?)?;
    super::play_skin::configured_path(ctx, v)
}

/// Load the skin for a screen: the configured one, else the built-in
/// default. `static_option` answers options at load time (LR2 `#IF`),
/// `state` the values Lua skins read while they are built. A
/// course result accepts a result skin.
pub fn load(
    ctx: &Ctx,
    gfx: &mut Gfx,
    kind: SkinType,
    static_option: &dyn Fn(i32) -> Option<bool>,
    state: &dyn crate::skin::SkinState,
) -> Option<LoadedSkin> {
    let accept =
        |k: SkinType| k == kind || (kind == SkinType::CourseResult && k == SkinType::Result);
    let default = default_skin(kind, ctx.view.height > ctx.view.width);
    let value = kind
        .config_key()
        .and_then(|k| ctx.config.skins.get(k))
        .cloned()
        .unwrap_or_default();
    for path in configured_skin(ctx, kind).into_iter().chain(default) {
        match crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &ctx.skin_user_settings(&value, &path),
            static_option,
            Some(state),
            &crate::skin::image_size,
        ) {
            Ok(skin) if accept(skin.kind) => {
                let flow = [
                    SkinType::Select,
                    SkinType::Decide,
                    SkinType::Result,
                    SkinType::CourseResult,
                ];
                if flow.contains(&kind) {
                    crate::skin::lr2::keep_images(&skin);
                }
                return Some(LoadedSkin::load(skin, gfx));
            }
            Ok(skin) => log::warn!(
                "{}: not a {:?} skin ({:?})",
                path.display(),
                kind,
                skin.kind
            ),
            Err(e) => log::warn!("skin: {e}"),
        }
    }
    None
}

/// What can be chosen for a skin type, as `config.skins` values: the
/// built-in default (empty), then the installed skins (sound sets for
/// SOUND_SET). A course result can be a result skin.
pub fn skin_choices(ctx: &Ctx, kind: SkinType) -> Vec<String> {
    let rel = |p: PathBuf| {
        p.strip_prefix(&ctx.paths.root)
            .map(|r| r.to_path_buf())
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    let mut out = vec![String::new()];
    if kind == SkinType::SoundSet {
        out.extend(
            crate::sysound::find_sets(&ctx.paths.root)
                .into_iter()
                .map(|s| rel(s.path)),
        );
        return out;
    }
    // play skins: the built-in perspective one too
    if kind.mode().is_some() && !kind.is_battle() {
        out.push(super::play_skin::PERSPECTIVE.into());
    }
    // (a battle slot takes double play skins too)
    let fits = |t: SkinType| {
        t == kind
            || (kind == SkinType::CourseResult && t == SkinType::Result)
            || (kind.is_battle() && t.battle_pair() == Some(kind))
    };
    for (p, t, _) in crate::skin::lr2::find_skins(&ctx.paths.skins) {
        if fits(t) {
            out.push(rel(p));
        }
    }
    out
}

/// The skin file a choice of a type stands for (the built-in default for
/// an empty one, where there is one).
pub fn choice_path(ctx: &Ctx, kind: SkinType, value: &str) -> Option<PathBuf> {
    if let Some(p) = super::play_skin::configured_path(ctx, value) {
        return Some(p);
    }
    match kind.mode() {
        Some(m) if kind.is_battle() => Some(super::play_skin::battle_skin(m, ctx.touch_enabled())),
        Some(m) => Some(super::play_skin::default_skin(
            m,
            ctx.config.play.side == crate::config::Side::P2,
            ctx.portrait(),
            ctx.touch_enabled(),
        )),
        None => default_skin(kind, ctx.portrait()),
    }
}

/// (name, author) of a choice of a type.
pub fn choice_name(ctx: &Ctx, kind: SkinType, value: &str) -> (String, String) {
    let Some(p) = super::play_skin::configured_path(ctx, value) else {
        return ("DEFAULT".into(), "vibeBMS".into());
    };
    if kind == SkinType::SoundSet {
        if let Some(s) = crate::sysound::set_info(&p) {
            return (s.name, s.author);
        }
    }
    let name = crate::skin::header(&p)
        .map(|h| h.1)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    (name, String::new())
}

/// Step custom option `o` of the skin chosen as `value` by `d` choices.
pub fn step_option(ctx: &mut Ctx, value: &str, o: &crate::skin::CustomOption, d: i32) {
    if o.labels.is_empty() {
        return;
    }
    let cur = o.chosen(
        ctx.config
            .skin_options
            .get(value)
            .and_then(|m| m.get(&o.name))
            .map(String::as_str),
    );
    let n = o.labels.len() as i32;
    let next = (cur as i32 + d).rem_euclid(n) as usize;
    ctx.config
        .skin_options
        .entry(value.to_string())
        .or_default()
        .insert(o.name.clone(), o.labels[next].clone());
}

/// Step field `field` (x, y, w, h, angle, alpha) of offset `name` of the
/// skin chosen as `value` by `d` steps.
pub fn step_offset(ctx: &mut Ctx, value: &str, name: &str, field: usize, d: i32) {
    let v = ctx
        .config
        .skin_offsets
        .entry(value.to_string())
        .or_default()
        .entry(name.to_string())
        .or_default();
    // alpha in 0..255 steps of 5, angle in degrees, the rest in pixels
    let step = if field == 5 { 5 } else { 1 };
    v[field] = (v[field] + d * step).clamp(-2000, 2000);
    if field == 5 {
        v[5] = v[5].clamp(-255, 255);
    }
}

#[cfg(test)]
mod golden;
