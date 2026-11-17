//! Play setup: the options that make a play's chart and its rules, and the
//! chart and judge they make (the same for a play, a replay watched again
//! and a replay checked without the app).

use crate::chart::{Chart, LnProfile, LnType};
use crate::engine::{Engine, EngineConfig, JudgeAlgorithm};
use crate::gauge::{GaugeAutoShift, GaugePreset, GaugeType, GrooveGauge};
use crate::judge::{judge_rules, JudgePreset};
use crate::lane_option::{self, DoubleOption, LaneOption};
use crate::mode::PlayMode;
use crate::score::AssistFacts;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The play options that decide the chart and how it is judged (the rest
/// of the player's options only change what is shown). A replay keeps
/// them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RuleOptions {
    pub gauge: GaugeType,
    pub gauge_auto_shift: GaugeAutoShift,
    pub gauge_preset: GaugePreset,
    pub judge_preset: JudgePreset,
    pub judge_algorithm: JudgeAlgorithm,
    pub random_1p: LaneOption,
    pub random_2p: LaneOption,
    pub dp_option: DoubleOption,
    /// Play SP charts on both sides (BATTLE).
    pub battle: bool,
    pub ln_mode: LnType,
    pub auto_scratch: bool,
    pub legacy_note: bool,
    pub no_mine: bool,
    /// EXPAND JUDGE (beatoraja custom judge): the judge windows scaled by
    /// `judge_rates` (PG, GR, GD in percent). Wider than 100 % is an assist
    /// (ASSIST EASY, the score is not saved).
    pub expand_judge: bool,
    pub judge_rates: [i64; 3],
    /// H-RANDOM / ALL-SCR jack threshold in BPM (16th notes).
    pub hran_threshold_bpm: u32,
}

impl Default for RuleOptions {
    fn default() -> Self {
        RuleOptions {
            gauge: GaugeType::default(),
            gauge_auto_shift: GaugeAutoShift::default(),
            gauge_preset: GaugePreset::default(),
            judge_preset: JudgePreset::default(),
            judge_algorithm: JudgeAlgorithm::default(),
            random_1p: LaneOption::default(),
            random_2p: LaneOption::default(),
            dp_option: DoubleOption::default(),
            battle: false,
            ln_mode: LnType::default(),
            auto_scratch: false,
            legacy_note: false,
            no_mine: false,
            expand_judge: false,
            judge_rates: [400, 400, 100],
            hran_threshold_bpm: 120,
        }
    }
}

impl AsRef<RuleOptions> for RuleOptions {
    fn as_ref(&self) -> &RuleOptions {
        self
    }
}

/// The chart a play is judged on.
pub struct PlayChart {
    pub chart: Chart,
    /// What the options did to it (for the assist level).
    pub facts: AssistFacts,
    /// Its long notes as written (before the options).
    pub ln_profile: LnProfile,
}

/// The chart read with `seed` (its `#RANDOM`) and arranged by `rules`.
/// `course`: in a course, whether it allows MIRROR (no other lane option).
pub fn build_chart(
    path: &Path,
    bytes: &[u8],
    rules: &RuleOptions,
    seed: i64,
    course: Option<bool>,
) -> Result<PlayChart, String> {
    let opts = crate::bms::ParseOptions {
        random_seed: seed,
        random_selections: None,
    };
    let mut chart = crate::load_chart_bytes(path, bytes, &opts).map_err(|e| e.to_string())?;
    let ln_profile = chart.ln_profile();
    let mut facts = AssistFacts::default();
    if rules.battle && !chart.mode().is_double() {
        lane_option::battle(&mut chart);
        facts.battle = true;
    }
    let thr = 60_000_000 / (rules.hran_threshold_bpm.max(1) as i64 * 4);
    let allowed = |o: LaneOption| match course {
        Some(mirror) if !(mirror && o == LaneOption::Mirror) => LaneOption::Off,
        _ => o,
    };
    lane_option::apply(&mut chart, 0, allowed(rules.random_1p), seed, thr);
    facts.lane.push(allowed(rules.random_1p));
    if chart.mode().is_double() {
        let random_2p = allowed(rules.random_2p);
        lane_option::apply(&mut chart, 1, random_2p, seed.wrapping_add(1), thr);
        facts.lane.push(random_2p);
        if rules.dp_option == DoubleOption::Flip {
            lane_option::flip(&mut chart);
        }
    }
    if rules.legacy_note {
        facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(&mut chart, None);
    }
    if rules.no_mine {
        let n = chart.notes.len();
        lane_option::remove_mines_of(&mut chart, None);
        facts.no_mine = chart.notes.len() != n;
    }
    Ok(PlayChart {
        chart,
        facts,
        ln_profile,
    })
}

/// G-BATTLE's chart: the single play chart read with the opponent's seed
/// and arranged by its lane option, on both sides (as LR2 lends a ghost's
/// random); each side's LEGACY NOTE and NO MINE apply to its own lanes.
/// What the player's options did (the player's side is a single play).
pub fn build_battle_chart(
    path: &Path,
    bytes: &[u8],
    mine: &RuleOptions,
    theirs: &RuleOptions,
    seed: i64,
) -> Result<PlayChart, String> {
    let single = RuleOptions {
        random_1p: theirs.random_1p,
        battle: false,
        legacy_note: false,
        no_mine: false,
        ..mine.clone()
    };
    let mut p = build_chart(path, bytes, &single, seed, None)?;
    let chart = &mut p.chart;
    if chart.mode().is_double() || chart.mode() == PlayMode::Popn9K {
        return Err("G-BATTLE needs a 5 or 7 key chart".into());
    }
    lane_option::battle(chart);
    if mine.legacy_note {
        p.facts.legacy_note = chart.meta.has_long;
        lane_option::legacy_note_of(chart, Some(0));
    }
    for (player, no_mine) in [(0, mine.no_mine), (1, theirs.no_mine)] {
        if no_mine {
            let n = chart.notes.len();
            lane_option::remove_mines_of(chart, Some(player));
            p.facts.no_mine |= player == 0 && chart.notes.len() != n;
        }
    }
    Ok(p)
}

/// The judge of `player`'s lanes (`None`: all) of `chart` under `rules`
/// (`autoplay`: every lane played automatically), and the TOTAL its gauge
/// uses.
pub fn build_engine(
    chart: &Chart,
    rules: &RuleOptions,
    player: Option<usize>,
    autoplay: bool,
) -> (Engine, f64) {
    let mode = chart.mode();
    let fam = mode.family();
    let rates = rules.expand_judge.then_some(rules.judge_rates);
    let judge = judge_rules(rules.judge_preset, fam, chart.meta.judge_rank, rates);
    let total = chart.total_of(rules.ln_mode, rules.gauge_preset, player);
    let notes = chart.total_judgements_of(rules.ln_mode, player);
    let gauge = GrooveGauge::new(
        rules.gauge_preset,
        fam,
        rules.gauge,
        rules.gauge_auto_shift,
        total,
        notes,
    );
    let cfg = EngineConfig {
        rules: judge,
        default_ln: rules.ln_mode,
        algorithm: rules.judge_algorithm,
        auto_lanes: (0..mode.lane_count())
            .map(|l| autoplay || (rules.auto_scratch && mode.is_scratch(l)))
            .collect(),
        scratch_release_ends_bss: true,
        player,
    };
    (Engine::new(chart, cfg, gauge), total)
}
