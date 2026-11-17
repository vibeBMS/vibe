//! The chart a play is judged on and its judges.

use crate::config::PlayConfig;
use std::path::Path;
use vibebms_core::chart::Chart;
use vibebms_core::engine::{Engine, EngineConfig};
use vibebms_core::gauge::GrooveGauge;
use vibebms_core::judge::judge_rules;
use vibebms_core::lane_option::{self, DoubleOption, LaneOption};
use vibebms_core::score::AssistFacts;
use vibebms_core::{LnType, PlayMode};

/// The chart a play is judged on: read with `seed` (its `#RANDOM`) and
/// arranged by the play's options, what they did (for the assist level)
/// and its long notes as written. `course`: in a course, whether it
/// allows MIRROR (no other lane option).
pub(crate) fn load_chart(
    path: &Path,
    bytes: &[u8],
    play: &PlayConfig,
    seed: i64,
    course: Option<bool>,
) -> Result<(Chart, AssistFacts, vibebms_core::LnProfile), String> {
    let mut chart = vibebms_core::load_chart_bytes(
        path,
        bytes,
        &vibebms_core::bms::ParseOptions {
            random_seed: Some(seed),
            random_selections: None,
        },
    )
    .map_err(|e| e.to_string())?;
    let ln_profile = chart.ln_profile();
    // chart transforms (options)
    let mut facts = AssistFacts::default();
    if play.battle && !chart.mode().is_double() {
        lane_option::battle(&mut chart);
        facts.battle = true;
    }
    let thr = 60_000_000 / (play.hran_threshold_bpm.max(1) as i64 * 4);
    let allowed = |o: LaneOption| match course {
        Some(mirror) if !(mirror && o == LaneOption::Mirror) => LaneOption::Off,
        _ => o,
    };
    lane_option::apply(&mut chart, 0, allowed(play.random_1p), seed, thr);
    facts.lane.push(allowed(play.random_1p));
    if chart.mode().is_double() {
        lane_option::apply(
            &mut chart,
            1,
            allowed(play.random_2p),
            seed.wrapping_add(1),
            thr,
        );
        facts.lane.push(allowed(play.random_2p));
        if play.dp_option == DoubleOption::Flip {
            lane_option::flip(&mut chart);
        }
    }
    if play.legacy_note {
        facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(&mut chart, None);
    }
    if play.no_mine {
        let n = chart.notes.len();
        lane_option::remove_mines_of(&mut chart, None);
        facts.no_mine = chart.notes.len() != n;
    }
    Ok((chart, facts, ln_profile))
}

/// G-BATTLE's chart: the single play chart read with the opponent's seed
/// and arranged by its lane option, on both sides (as LR2 lends a ghost's
/// random); each side's LEGACY NOTE and NO MINE apply to its own lanes.
/// What the player's options did (the player's side is a single play).
pub(crate) fn load_battle_chart(
    path: &Path,
    bytes: &[u8],
    mine: &PlayConfig,
    theirs: &PlayConfig,
    seed: i64,
) -> Result<(Chart, AssistFacts, vibebms_core::LnProfile), String> {
    let p = PlayConfig {
        random_1p: theirs.random_1p,
        battle: false,
        legacy_note: false,
        no_mine: false,
        ..mine.clone()
    };
    let (mut chart, mut facts, ln_profile) = load_chart(path, bytes, &p, seed, None)?;
    if chart.mode().is_double() || chart.mode() == PlayMode::Popn9K {
        return Err("G-BATTLE needs a 5 or 7 key chart".into());
    }
    lane_option::battle(&mut chart);
    if mine.legacy_note {
        facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(&mut chart, Some(0));
    }
    for (player, no_mine) in [(0, mine.no_mine), (1, theirs.no_mine)] {
        if no_mine {
            let n = chart.notes.len();
            lane_option::remove_mines_of(&mut chart, Some(player));
            facts.no_mine |= player == 0 && chart.notes.len() != n;
        }
    }
    Ok((chart, facts, ln_profile))
}

/// What judges a side: the play options' rules, as a course may change
/// them.
#[derive(Clone, Debug)]
pub(crate) struct Rules {
    pub judge: vibebms_core::judge::JudgePreset,
    pub judge_rates: Option<[i64; 3]>,
    pub gauge_preset: vibebms_core::gauge::GaugePreset,
    pub gauge: vibebms_core::gauge::GaugeType,
    pub gas: vibebms_core::gauge::GaugeAutoShift,
    pub ln: LnType,
    pub algorithm: vibebms_core::engine::JudgeAlgorithm,
    pub auto_scratch: bool,
    pub autoplay: bool,
}

impl Rules {
    pub fn of(p: &PlayConfig) -> Rules {
        Rules {
            judge: p.judge_preset,
            judge_rates: p.expand_judge.then_some(p.judge_rates),
            gauge_preset: p.gauge_preset,
            gauge: p.gauge,
            gas: p.gauge_auto_shift,
            ln: p.ln_mode,
            algorithm: p.judge_algorithm,
            auto_scratch: p.auto_scratch,
            autoplay: false,
        }
    }

    /// The lanes played automatically (autoplay, AUTO SCRATCH).
    pub fn auto_lanes(&self, mode: PlayMode) -> Vec<bool> {
        (0..mode.lane_count())
            .map(|l| self.autoplay || (self.auto_scratch && mode.is_scratch(l)))
            .collect()
    }
}

/// The judge of `player`'s lanes (`None`: all) of `chart` under `r`, and
/// the TOTAL its gauge uses.
pub(crate) fn make_engine(chart: &Chart, r: &Rules, player: Option<usize>) -> (Engine, f64) {
    let mode = chart.mode();
    let fam = mode.family();
    let rules = judge_rules(r.judge, fam, chart.meta.judge_rank, r.judge_rates);
    let total = chart.total_of(r.ln, r.gauge_preset, player);
    let notes = chart.total_judgements_of(r.ln, player);
    let gauge = GrooveGauge::new(r.gauge_preset, fam, r.gauge, r.gas, total, notes);
    let cfg = EngineConfig {
        rules,
        default_ln: r.ln,
        algorithm: r.algorithm,
        auto_lanes: r.auto_lanes(mode),
        scratch_release_ends_bss: true,
        player,
    };
    (Engine::new(chart, cfg, gauge), total)
}
