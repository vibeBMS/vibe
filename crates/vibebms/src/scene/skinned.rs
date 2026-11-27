//! Shared pieces of the skinned (non-play) scenes: which skin to use and
//! loading it; its options and offsets.

use crate::ctx::Ctx;
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

/// What a screen's skin is made from: the skin chosen for it, its custom
/// options and offsets, and the orientation (the built-in default has a
/// portrait version). A screen that stays under others (song select)
/// loads its skin again when this changed meanwhile.
pub fn skin_key(ctx: &Ctx, kind: SkinType) -> String {
    let value = kind
        .config_key()
        .and_then(|k| ctx.config.skins.get(k))
        .map_or("", String::as_str);
    format!(
        "{value}\n{:?}\n{:?}\n{}",
        ctx.config.skin_options.get(value),
        ctx.config.skin_offsets.get(value),
        ctx.portrait()
    )
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

/// How long a choice of skin rests before it is loaded ([`settle`]):
/// stepping through the skins stays quick.
const TRIAL_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

/// Choose `value` (one of [`skin_choices`]) as the skin of `kind`. A skin
/// file is loaded once the choice has rested ([`settle`]); one that fails
/// to load is chosen all the same, and said (its screen shows the
/// built-in skin). A skin that takes the game down as it loads then is
/// set aside at the next start (`skin::guard`).
pub fn choose(ctx: &mut Ctx, kind: SkinType, value: String) {
    let Some(key) = kind.key() else {
        return;
    };
    ctx.skins.trial = Some((kind, value.clone(), crate::platform::Instant::now()));
    ctx.config.skins.insert(key.to_string(), value);
}

/// Load the skin last chosen once the choice has rested (at `now`; a
/// sound set is none to load), and save the config.
pub fn settle(ctx: &mut Ctx, now: crate::platform::Instant) {
    match &ctx.skins.trial {
        Some((_, _, at)) if now.saturating_duration_since(*at) >= TRIAL_AFTER => {}
        _ => return,
    }
    let Some((kind, value, _)) = ctx.skins.trial.take() else {
        return;
    };
    let chosen = kind.key().and_then(|k| ctx.config.skins.get(k)) == Some(&value);
    let path = (super::play_skin::configured_path(ctx, &value))
        .filter(|_| chosen && kind != SkinType::SoundSet);
    if let Some(path) = path {
        let loaded = crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &ctx.skin_user_settings(&value, &path),
            &|_| None,
            Some(&super::skinselect::PreviewState),
            &crate::skin::image_size,
        );
        if let Err(e) = loaded {
            log::warn!("skin: {e}");
            ctx.toast(tr!(
                "This skin cannot be loaded: the built-in one is shown instead"
            ));
        }
    }
    ctx.save_config();
}

/// The start: skin loads are noted from here on (`skin::guard`), and the
/// skins the last run was loading when it stopped are set aside.
pub fn recover(ctx: &mut Ctx) {
    let left = crate::skin::guard::arm(ctx.paths.cache.join("skins-loading"));
    set_aside(ctx, &left);
}

/// The skin files `skins` are no screen's skin any more (the built-in
/// one is), which the player is told.
fn set_aside(ctx: &mut Ctx, skins: &[PathBuf]) {
    let chosen = |value: &String| {
        super::play_skin::configured_path(ctx, value).is_some_and(|p| skins.contains(&p))
    };
    let aside: std::collections::BTreeSet<String> =
        (ctx.config.skins.values().filter(|v| chosen(v)).cloned()).collect();
    if aside.is_empty() {
        return;
    }
    ctx.config.skins.retain(|_, v| !aside.contains(v));
    ctx.save_config();
    let names: Vec<&str> = aside.iter().map(String::as_str).collect();
    ctx.toast(tr!(
        "The game stopped while a skin was loading. Set aside (SETTINGS › SKINS chooses it again): {skins}",
        skins = names.join(", ")
    ));
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
        let sets = crate::audio::sysound::find_sets(&ctx.paths.root);
        // (one of them drawn each time the song select comes)
        if sets.len() > 1 {
            out.push(crate::audio::sysound::RANDOM_SET.into());
        }
        out.extend(sets.into_iter().map(|s| rel(s.path)));
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
    if let Some(p) = super::play_skin::options_path(ctx, kind, value) {
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
        if let Some(s) = crate::audio::sysound::set_info(&p) {
            return (s.name, s.author);
        }
    }
    let name = crate::skin::header(&p, &ctx.paths.skins)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &std::path::Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    const SKIN: &str = "#INFORMATION,5,S,me\n#IMAGE,a.png\n#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n\
                        #DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n";

    /// A skin is loaded once its choice rests: one that does not load is
    /// said, a sound set is none to load.
    #[test]
    fn a_skin_is_loaded_once_its_choice_rests() {
        let root = std::env::temp_dir().join(format!("vibebms-choose-{}", std::process::id()));
        write(&root.join("skins/good/select.lr2skin"), SKIN);
        image::RgbImage::new(16, 16)
            .save(root.join("skins/good/a.png"))
            .unwrap();
        write(
            &root.join("skins/bad/select.lr2skin"),
            "#INFORMATION,5,S,me\n",
        );
        let mut ctx = Ctx::for_test(root.clone());
        let later = || crate::platform::Instant::now() + TRIAL_AFTER;
        choose(
            &mut ctx,
            SkinType::Select,
            "skins/good/select.lr2skin".into(),
        );
        settle(&mut ctx, later());
        choose(&mut ctx, SkinType::SoundSet, "sounds/none".into());
        settle(&mut ctx, later());
        assert!(ctx.toasts.is_empty());
        assert_eq!(ctx.config.skins["SELECT"], "skins/good/select.lr2skin");
        // the bad one stepped past: not loaded
        choose(
            &mut ctx,
            SkinType::Select,
            "skins/bad/select.lr2skin".into(),
        );
        assert!(ctx.toasts.is_empty());
        settle(&mut ctx, crate::platform::Instant::now());
        assert!(ctx.toasts.is_empty());
        settle(&mut ctx, later());
        assert_eq!(ctx.toasts.len(), 1);
        assert_eq!(ctx.config.skins["SELECT"], "skins/bad/select.lr2skin");
        let saved = crate::config::Config::load(&ctx.paths.config_file());
        assert_eq!(saved.skins["SELECT"], "skins/bad/select.lr2skin");
        choose(&mut ctx, SkinType::Select, String::new());
        settle(&mut ctx, later());
        assert_eq!(
            (ctx.toasts.len(), &ctx.config.skins["SELECT"]),
            (1, &String::new())
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The skins a run stopped in the middle of loading are no screen's
    /// skin at the next start, in the saved config too; the others stay.
    #[test]
    fn skins_the_last_run_stopped_in_are_set_aside() {
        let root = std::env::temp_dir().join(format!("vibebms-aside-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        for (key, value) in [
            ("SELECT", "skins/a/select.lr2skin"),
            ("7KEYS", "skins/a/play.lr2skin"),
            ("RESULT", "skins/a/select.lr2skin"),
        ] {
            ctx.config.skins.insert(key.into(), value.into());
        }
        ctx.save_config();
        set_aside(&mut ctx, &[root.join("skins/other/x.json")]);
        assert_eq!((ctx.config.skins.len(), ctx.toasts.len()), (3, 0));
        set_aside(&mut ctx, &[root.join("skins/a/select.lr2skin")]);
        let saved = crate::config::Config::load(&ctx.paths.config_file());
        std::fs::remove_dir_all(&root).ok();
        let left: Vec<&String> = ctx.config.skins.keys().collect();
        assert_eq!(left, ["7KEYS"]);
        assert_eq!(saved.skins, ctx.config.skins);
        assert!(ctx.toasts[0].0.ends_with(": skins/a/select.lr2skin"));
    }
}
