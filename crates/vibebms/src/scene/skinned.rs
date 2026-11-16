//! Shared pieces of the skinned (non-play) scenes: which skin to use, loading
//! it, and skin values common to several screens.

use crate::app::Ctx;
use crate::config::HiSpeedMode;
use crate::gfx::Gfx;
use crate::skin::render::LoadedSkin;
use crate::skin::SkinType;
use std::path::PathBuf;
use vibebms_core::chart::ChartMeta;
use vibebms_core::gauge::{GaugeAutoShift, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::score::DjLevel;
use vibebms_core::LnType;

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
    let default = default_skin(kind, ctx.height > ctx.width);
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
        180..=184 => judge_option(judge_rank_number(m), id),
        190 => m.stagefile.is_empty(),
        191 => !m.stagefile.is_empty(),
        192 => m.banner.is_empty(),
        193 => !m.banner.is_empty(),
        194 => m.backbmp.is_empty(),
        195 => !m.backbmp.is_empty(),
        _ => return None,
    })
}

/// beatoraja's table texts (1001 name, 1002 level, 1003 level then
/// name) of a chart: the first difficulty table that lists it, as
/// beatoraja finds one for a chart not chosen from a table.
pub fn table_text(ctx: &Ctx, md5: &str, sha256: &str, id: i32) -> Option<String> {
    if !(1001..=1003).contains(&id) {
        return None;
    }
    let same = |a: &str, b: &str| !a.is_empty() && a.eq_ignore_ascii_case(b);
    let (name, level) = ctx.tables.iter().find_map(|t| {
        let e = t
            .entries
            .iter()
            .find(|e| same(&e.md5, md5) || same(&e.sha256, sha256))?;
        Some((t.header.name.clone(), t.label(&e.level)))
    })?;
    Some(match id {
        1001 => name,
        1002 => level,
        _ => format!("{level}{name}"),
    })
}

/// beatoraja's chart numbers from the library: notes by kind (350-354),
/// densities (360-365), TOTAL (368) and judge rank (400).
pub fn song_number(s: &crate::library::Song, id: i32) -> Option<i64> {
    let known = s.note_kinds.iter().any(|n| *n > 0);
    let split = |v: f64, after: bool| {
        if after {
            ((v * 100.0) as i64) % 100
        } else {
            v as i64
        }
    };
    Some(match id {
        350..=354 if known => s.note_kinds[(id - 350) as usize],
        400 => s.judge?,
        360 | 361 if known => split(s.density[1], id == 361),
        362 | 363 if known => split(s.density[2], id == 363),
        364 | 365 if known => split(s.density[0], id == 365),
        368 => s.total as i64,
        _ => return None,
    })
}

/// beatoraja's judge rank options (180 very hard .. 184 very easy) of a
/// judge rank number: `#RANK`, or a percentage in steps of 25 from 10.
pub fn judge_option(judge: i64, id: i32) -> bool {
    let k = (id - 180) as i64;
    judge == k || (judge >= 10 && ((judge - 10) / 25).min(4) == k)
}

/// beatoraja's judge rank number (400): `#RANK`, else the percentage.
pub fn judge_rank_number(m: &ChartMeta) -> i64 {
    use vibebms_core::chart::JudgeRank;
    match m.judge_rank {
        JudgeRank::Rank(r) => r as i64,
        JudgeRank::DefExRank(v) | JudgeRank::Bmson(v) => v.round() as i64,
    }
}

/// beatoraja option indices.
fn random_index(o: LaneOption) -> i64 {
    match o {
        LaneOption::Off => 0,
        LaneOption::Mirror => 1,
        LaneOption::Random => 2,
        LaneOption::RRandom => 3,
        LaneOption::SRandom => 4,
        LaneOption::Spiral => 5,
        LaneOption::HRandom => 6,
        LaneOption::AllScratch => 7,
        LaneOption::RandomEx => 8,
        LaneOption::SRandomEx => 9,
    }
}

/// beatoraja button values (image set indices) of the play settings,
/// shown on every screen.
pub fn config_button(ctx: &Ctx, id: i32) -> Option<i64> {
    let p = &ctx.config.play;
    Some(match id {
        40 => gauge_index(p.gauge),
        42 => random_index(p.random_1p),
        43 => random_index(p.random_2p),
        54 => super::option_panel::dp_index(p),
        // beatoraja HS-FIX: off, start, max, main, min BPM
        55 => p.hs_fix as i64,
        // beatoraja BGA: 0 on, 2 off
        72 => {
            if p.bga {
                0
            } else {
                2
            }
        }
        // beatoraja assist buttons vibeBMS has (0 off, 1 on)
        301 => p.expand_judge as i64,
        302 => (p.hispeed_mode == HiSpeedMode::Constant) as i64,
        303 => p.judge_area as i64,
        304 => p.legacy_note as i64,
        305 => p.mark_notes as i64,
        306 => p.bpm_guide as i64,
        307 => p.no_mine as i64,
        75 => p.timing_auto_adjust as i64,
        330 => p.lane_cover as i64,
        331 => (p.lift > 0.0) as i64,
        332 => (p.hidden > 0.0) as i64,
        340 => p.judge_algorithm as i64,
        351 => p.no_mine as i64,
        352 => (p.hispeed_mode == HiSpeedMode::Constant) as i64,
        353 => p.legacy_note as i64,
        78 => crate::scene::option_panel::index_of(&GaugeAutoShift::ALL, p.gauge_auto_shift),
        // vibeBMS: the options its panels show that beatoraja has not
        1401 => p.auto_scratch as i64,
        1402 => p.lane_cover as i64,
        1403 => crate::scene::option_panel::index_of(&JudgePreset::ALL, p.judge_preset),
        1404 => crate::scene::option_panel::index_of(&GaugeAutoShift::ALL, p.gauge_auto_shift),
        1405 => crate::scene::option_panel::index_of(&crate::config::Target::ALL, p.target),
        // TARGET as LR2 skins for beatoraja draw it (LITONE5): A- .. AAA+,
        // MAX, NEXT (an older list, without MAX-)
        77 => {
            use crate::config::Target;
            match p.target {
                Target::RateMaxMinus | Target::Max => 9,
                Target::RankNext => 10,
                Target::Best => 11,
                t => crate::scene::option_panel::index_of(&Target::ALL, t),
            }
        }
        308 => match p.ln_mode {
            LnType::Ln => 0,
            LnType::Cn => 1,
            LnType::Hcn => 2,
        },
        _ => return None,
    })
}

/// beatoraja's gauge index (button 40).
pub fn gauge_index(g: GaugeType) -> i64 {
    match g {
        GaugeType::AssistEasy => 0,
        GaugeType::Easy => 1,
        GaugeType::Normal => 2,
        GaugeType::Hard | GaugeType::Class => 3,
        GaugeType::ExHard | GaugeType::ExHardClass | GaugeType::ExClass => 4,
        GaugeType::Hazard => 5,
    }
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

/// The player's totals over all charts (beatoraja `PlayerData`; LR2 has
/// 30-37 too): time played 17-19 (h, min, s), plays, clears and fails
/// 30-32, PG / GR / GD / BD / POOR 33-37 (POOR without empty POORs) and
/// notes hit (PG to BD) 333.
pub fn player_number(ctx: &Ctx, id: i32) -> Option<i64> {
    let sum = |f: fn(&crate::library::BestScore) -> u32| -> i64 {
        ctx.scores.values().map(|b| f(b) as i64).sum()
    };
    let totals = ctx.lib.as_ref().map(|l| l.totals());
    let secs = totals.map_or(0, |t| t.play_ms / 1000) as i64;
    Some(match id {
        17 => secs / 3600,
        18 => secs / 60 % 60,
        19 => secs % 60,
        30 => sum(|b| b.play_count),
        31 => sum(|b| b.clear_count),
        32 => sum(|b| b.play_count.saturating_sub(b.clear_count)),
        33..=37 => totals.map_or(0, |t| t.judge((id - 33) as usize)) as i64,
        333 => totals.map_or(0, |t| t.notes()) as i64,
        _ => return None,
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
