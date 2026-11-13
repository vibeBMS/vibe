//! Shared pieces of the skinned (non-play) scenes: which skin to use, loading
//! it, and skin values common to several screens.

use crate::app::Ctx;
use crate::gfx::Gfx;
use crate::skin::render::LoadedSkin;
use crate::skin::SkinType;
use std::path::PathBuf;
use vibebms_core::chart::ChartMeta;
use vibebms_core::score::DjLevel;

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
/// default. `static_option` answers options at load time (LR2 `#IF`). A
/// course result accepts a result skin.
pub fn load(
    ctx: &Ctx,
    gfx: &mut Gfx,
    kind: SkinType,
    static_option: &dyn Fn(i32) -> Option<bool>,
) -> Option<LoadedSkin> {
    let accept =
        |k: SkinType| k == kind || (kind == SkinType::CourseResult && k == SkinType::Result);
    let default = default_skin(kind, ctx.height > ctx.width);
    let value = kind
        .config_key()
        .and_then(|k| ctx.config.skins.get(k))
        .cloned()
        .unwrap_or_default();
    for path in configured_skin(ctx, kind).into_iter().chain(default) {
        let opts = ctx.skin_user_options(&value, &path);
        match crate::skin::load_file(
            &path,
            &ctx.paths.skins,
            &opts,
            static_option,
            &crate::skin::image_size,
        ) {
            Ok(skin) if accept(skin.kind) => return Some(LoadedSkin::load(skin, gfx)),
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

/// Chart options shared by select / result (LR2 / beatoraja ids).
pub fn chart_option(m: &ChartMeta, mode: vibebms_core::PlayMode, id: i32) -> Option<bool> {
    use vibebms_core::PlayMode;
    Some(match id {
        150..=155 => m.difficulty == id - 150,
        160 => mode == PlayMode::Beat7K,
        161 => mode == PlayMode::Beat5K,
        162 => mode == PlayMode::Beat14K,
        163 => mode == PlayMode::Beat10K,
        164 => mode == PlayMode::Popn9K,
        170 => !m.has_bga,
        171 => m.has_bga,
        172 => !m.has_long,
        173 => m.has_long,
        176 => (m.max_bpm - m.min_bpm).abs() < 0.01,
        177 => (m.max_bpm - m.min_bpm).abs() >= 0.01,
        178 => !m.has_random,
        179 => m.has_random,
        180..=184 => {
            matches!(m.judge_rank, vibebms_core::chart::JudgeRank::Rank(r) if r == id - 180)
        }
        190 => m.stagefile.is_empty(),
        191 => !m.stagefile.is_empty(),
        192 => m.banner.is_empty(),
        193 => !m.banner.is_empty(),
        194 => m.backbmp.is_empty(),
        195 => !m.backbmp.is_empty(),
        _ => return None,
    })
}

/// Numbers every screen has: the local date and time (LR2 21-26: year,
/// month, day, hour, minute, second).
pub fn clock_number(id: i32) -> Option<i64> {
    use chrono::{Datelike, Timelike};
    if !(21..=26).contains(&id) {
        return None;
    }
    let t = chrono::Local::now();
    Some(match id {
        21 => t.year() as i64,
        22 => t.month() as i64,
        23 => t.day() as i64,
        24 => t.hour() as i64,
        25 => t.minute() as i64,
        _ => t.second() as i64,
    })
}

/// LR2 lamp options 100-105 (exactly one holds: lamps LR2 lacks count as
/// the nearest one) and beatoraja's exact 1100-1104.
pub fn lamp_option(c: vibebms_core::score::ClearType, id: i32) -> Option<bool> {
    use vibebms_core::score::ClearType as C;
    Some(match id {
        100 => c == C::NoPlay,
        101 => c == C::Failed,
        102 => matches!(c, C::AssistEasy | C::LightAssistEasy | C::Easy),
        103 => c == C::Normal,
        104 => matches!(c, C::Hard | C::ExHard),
        105 => matches!(c, C::FullCombo | C::Perfect | C::Max),
        1100 => c == C::AssistEasy,
        1101 => c == C::LightAssistEasy,
        1102 => c == C::ExHard,
        1103 => c == C::Perfect,
        1104 => c == C::Max,
        _ => return None,
    })
}

/// Index of a DJ level in LR2 / beatoraja rank options (0 = AAA ... 7 = F).
pub fn rank_index(l: DjLevel) -> i32 {
    7 - l as i32
}

/// Elapsed ms since `t0` (negative before it).
pub fn ms_since(t0: crate::platform::Instant, now: crate::platform::Instant) -> i64 {
    if now >= t0 {
        now.duration_since(t0).as_millis() as i64
    } else {
        -(t0.duration_since(now).as_millis() as i64)
    }
}
