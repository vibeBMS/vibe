use super::{percent, Act, Row, Screen};
use crate::app::Ctx;
use crate::config::PresentMode;
use crate::scene::option_panel::{cycle, onoff};
use crate::scene::play_skin::{configured_path, PERSPECTIVE};
use crate::scene::skinned;
use crate::scene::skinselect::TYPES;
use crate::skin::{SkinType, OFFSET_FIELDS};

/// The row opening `s`, when its skin is chosen.
pub(super) fn open_row(ctx: &Ctx, s: Screen) -> Option<Row> {
    let (kind, label) = match s {
        Screen::KeyConfig => (SkinType::KeyConfig, "    Open the key config screen"),
        Screen::SkinSelect => (SkinType::SkinSelect, "    Open the skin select screen"),
        Screen::SoundSets => (SkinType::SkinSelect, "    Choose in the skin select screen"),
        Screen::TouchLayout => return None,
    };
    skinned::configured_skin(ctx, kind)?;
    Some(Row::new(label, "↵", Act::Open(s)))
}

/// The `config.skins` value of type `t`'s skin and its path.
fn chosen(ctx: &Ctx, t: SkinType) -> Option<(&str, std::path::PathBuf)> {
    let value = ctx.config.skins.get(t.key()?)?;
    Some((value, configured_path(ctx, value)?))
}

/// Step custom option `i` of the skin chosen for `t`.
pub(super) fn step_skin_option(ctx: &mut Ctx, t: SkinType, i: usize, d: i32) {
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let value = value.to_string();
    if let Some(o) = ctx.skin_custom_options(&path).get(i) {
        skinned::step_option(ctx, &value, o, d);
    }
}

/// Step field `field` of adjustable offset `i` of the skin chosen for `t`.
pub(super) fn step_skin_offset(ctx: &mut Ctx, t: SkinType, i: usize, field: usize, d: i32) {
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let value = value.to_string();
    if let Some(o) = ctx.skin_custom_offsets(&path).get(i) {
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
        c.toast(format!("Put more {key} skins in {}", dir.display()));
    }
    c.config.skins.insert(key.to_string(), next);
}

/// Display name of a configured skin (its header name, else the file stem).
fn skin_label(ctx: &Ctx, value: &str) -> String {
    if value == PERSPECTIVE {
        return "TOUCH PERSPECTIVE".into();
    }
    let Some(p) = configured_path(ctx, value) else {
        return "DEFAULT".into();
    };
    crate::skin::header(&p)
        .map(|h| h.1)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

/// The rows of the skin chosen for `t`: which one, the screen it draws,
/// its own options and its position adjustments (beatoraja offsets).
fn skin_rows(ctx: &Ctx, t: SkinType, v: &mut Vec<Row>) {
    let c = &ctx.config;
    let Some(key) = t.key() else {
        return;
    };
    let cur = match c.skins.get(key).map(String::as_str).unwrap_or("") {
        "" => "DEFAULT".into(),
        p => skin_label(ctx, p),
    };
    let label = if t.mode().is_some() {
        format!("Play skin ({key})")
    } else if t == SkinType::SoundSet {
        "Sound set".into()
    } else {
        format!(
            "{}{} skin",
            &key[..1],
            key[1..].to_lowercase().replace('_', " ")
        )
    };
    v.push(Row::new(label, cur, Act::CycleSkin(t)));
    let screen = match t {
        SkinType::KeyConfig => Some(Screen::KeyConfig),
        SkinType::SkinSelect => Some(Screen::SkinSelect),
        SkinType::SoundSet => Some(Screen::SoundSets),
        _ => None,
    };
    v.extend(screen.and_then(|s| open_row(ctx, s)));
    let Some((value, path)) = chosen(ctx, t) else {
        return;
    };
    let picked = c.skin_options.get(value);
    for (i, o) in ctx.skin_custom_options(&path).iter().enumerate() {
        let k = o.chosen(picked.and_then(|m| m.get(&o.name)).map(String::as_str));
        v.push(Row::new(
            format!("    {}", o.name),
            o.labels.get(k).cloned().unwrap_or_default(),
            Act::SkinOption(t, i),
        ));
    }
    let set = c.skin_offsets.get(value);
    for (i, o) in ctx.skin_custom_offsets(&path).iter().enumerate() {
        let cur = set
            .and_then(|m| m.get(&o.name))
            .copied()
            .unwrap_or_default();
        for (f, _) in o.fields.iter().enumerate().filter(|(_, on)| **on) {
            v.push(Row::new(
                format!("    {} {}", o.name, OFFSET_FIELDS[f]),
                cur[f].to_string(),
                Act::SkinOffset(t, i, f),
            ));
        }
    }
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let d = &ctx.config.display;
    let mut v = vec![
        Row::adjust("Fullscreen (F11)", onoff(d.fullscreen), |c, _| {
            c.config.display.fullscreen ^= true;
            c.want_fullscreen = Some(c.config.display.fullscreen);
        }),
        Row::adjust(
            "Present mode",
            format!("{:?}", d.present_mode).to_uppercase(),
            |c, d| {
                use PresentMode::*;
                let m = &mut c.config.display.present_mode;
                *m = cycle(&[LowLatency, VSync, Immediate], *m, d);
                c.want_present_mode = true;
            },
        ),
        Row::adjust("Max FPS (0 = unlimited)", d.max_fps.to_string(), |c, d| {
            let steps = [0u32, 60, 75, 120, 144, 165, 240, 360, 480, 1000];
            c.config.display.max_fps = cycle(&steps, c.config.display.max_fps, d)
        }),
        Row::adjust("UI scale", percent(d.ui_scale), |c, d| {
            let s = &mut c.config.display.ui_scale;
            *s = (*s + d as f32 * 0.05).clamp(0.5, 3.0)
        }),
        Row::adjust("Show FPS", onoff(d.show_fps), |c, _| {
            c.config.display.show_fps ^= true
        }),
        Row::note("GPU", ctx.gpu_name.clone()),
    ];
    // built-in default skin / skins in <root>/skins
    for t in TYPES {
        skin_rows(ctx, t, &mut v);
    }
    v
}
