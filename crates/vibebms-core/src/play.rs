//! Play setup: the options that make a play's chart and its rules, and the
//! chart and judge they make (the same for a play, a replay watched again
//! and a replay checked without the app).

use crate::chart::{Chart, LnProfile, LnType};
use crate::course::{CourseKind, Join};
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
    /// A course's stage (NONSTOP: the course): its kind, which decides
    /// the LR2 set's gauges (`GrooveGauge::new_in`). Not a player option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course: Option<CourseKind>,
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
            course: None,
        }
    }
}

impl RuleOptions {
    /// The rules of a course of `kind` played on these options. Under the
    /// LR2 gauge set a class course forces LR2's settings (S-OLR2b 5.1):
    /// EASY and G-ATTACK play GROOVE (its class gauges, as HARD plays
    /// EX-CLASS), and no lane option, BATTLE, DP FLIP, assist or note
    /// option; an expert or NONSTOP course forces nothing (4.2). Under the
    /// other sets every course plays the set's dan gauges, as vibeBMS's
    /// table courses do. No course has GAUGE AUTO SHIFT.
    pub fn for_course(&self, kind: CourseKind) -> RuleOptions {
        let lr2 = self.gauge_preset == GaugePreset::Lr2;
        let mut r = RuleOptions {
            course: Some(kind),
            gauge_auto_shift: GaugeAutoShift::None,
            ..self.clone()
        };
        match (lr2, kind) {
            (true, CourseKind::Class) => {
                r.gauge = self.gauge.to_lr2_class();
                r.random_1p = LaneOption::Off;
                r.random_2p = LaneOption::Off;
                r.dp_option = DoubleOption::Off;
                r.battle = false;
                r.auto_scratch = false;
                r.legacy_note = false;
                r.no_mine = false;
                r.expand_judge = false;
            }
            (true, _) => {}
            (false, _) => r.gauge = self.gauge.to_class(),
        }
        r
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
    let chart = crate::load_chart_bytes(path, bytes, &opts).map_err(|e| e.to_string())?;
    Ok(arrange(chart, rules, seed, course))
}

/// A NONSTOP course's chart: its stages (path, bytes) joined with `joins`
/// (`nonstop::join`), each stage's `#RANDOM` rolled on from `seed`, then
/// arranged by `rules` as one chart.
pub fn build_nonstop_chart(
    stages: &[(&Path, &[u8])],
    joins: &[Join],
    rules: &RuleOptions,
    seed: i64,
) -> Result<(PlayChart, crate::nonstop::Nonstop), String> {
    let mut shared = crate::bms::Shared::new(seed);
    let mut read = Vec::new();
    for (path, bytes) in stages {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext.eq_ignore_ascii_case("bmson") {
            return Err("a NONSTOP course takes BMS charts only".into());
        }
        let stage = crate::bms::parse_stage(bytes, ext, &mut shared)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        read.push(stage);
    }
    let (chart, ns) = crate::nonstop::join(read, joins).map_err(|e| e.to_string())?;
    Ok((arrange(chart, rules, seed, None), ns))
}

/// `chart` arranged by `rules` (`course`: as `build_chart`'s).
fn arrange(mut chart: Chart, rules: &RuleOptions, seed: i64, course: Option<bool>) -> PlayChart {
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
    PlayChart {
        chart,
        facts,
        ln_profile,
    }
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
    let gauge = GrooveGauge::new_in(
        rules.gauge_preset,
        fam,
        rules.gauge,
        rules.gauge_auto_shift,
        total,
        notes,
        rules.course,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::CourseTotals;
    use crate::judge::Judge;

    fn lr2() -> RuleOptions {
        RuleOptions {
            judge_preset: JudgePreset::Lr2,
            gauge_preset: GaugePreset::Lr2,
            ..Default::default()
        }
    }

    #[test]
    fn class_courses_force_lr2_settings() {
        let mine = RuleOptions {
            random_1p: LaneOption::SRandom,
            random_2p: LaneOption::Mirror,
            dp_option: DoubleOption::Flip,
            battle: true,
            auto_scratch: true,
            legacy_note: true,
            no_mine: true,
            expand_judge: true,
            gauge_auto_shift: GaugeAutoShift::BestClear,
            ..lr2()
        };
        for (gauge, played) in [
            (GaugeType::Easy, GaugeType::Class),
            (GaugeType::Normal, GaugeType::Class),
            (GaugeType::GAttack, GaugeType::Class),
            (GaugeType::Hard, GaugeType::ExClass),
            (GaugeType::Hazard, GaugeType::Hazard),
            (GaugeType::PAttack, GaugeType::PAttack),
        ] {
            let r = RuleOptions { gauge, ..mine.clone() }.for_course(CourseKind::Class);
            assert_eq!(r.gauge, played, "{gauge:?}");
            assert_eq!((r.random_1p, r.random_2p), (LaneOption::Off, LaneOption::Off));
            assert_eq!(r.dp_option, DoubleOption::Off);
            assert!(!(r.battle || r.auto_scratch || r.legacy_note || r.no_mine || r.expand_judge));
            assert_eq!(r.gauge_auto_shift, GaugeAutoShift::None);
            assert_eq!(r.course, Some(CourseKind::Class));
        }
        // expert and NONSTOP force nothing (but GAUGE AUTO SHIFT)
        let r = RuleOptions {
            gauge: GaugeType::GAttack,
            ..mine.clone()
        }
        .for_course(CourseKind::Expert);
        assert_eq!((r.gauge, r.random_1p, r.battle), (GaugeType::GAttack, LaneOption::SRandom, true));
        // another set: its dan gauges, the options as they are
        let b = RuleOptions {
            gauge: GaugeType::Hard,
            random_1p: LaneOption::Random,
            ..Default::default()
        }
        .for_course(CourseKind::Class);
        assert_eq!((b.gauge, b.random_1p), (GaugeType::ExClass, LaneOption::Random));
    }

    /// Two stages of an expert course: the second starts from the first's
    /// exact gauge, the course combo and totals run on (S-OLR2b 4.2).
    #[test]
    fn expert_courses_carry_the_gauge_combo_and_totals() {
        let rules = lr2().for_course(CourseKind::Expert);
        let mut totals = CourseTotals::default();
        let mut gauge: Option<Vec<f32>> = None;
        let (mut notes, mut pg, mut empty) = (0, 0, 0);
        for k in 0..2 {
            let text = crate::bot::chart_text(7, 20 + k);
            let p = build_chart(Path::new("c.bme"), text.as_bytes(), &rules, 9, None).unwrap();
            let (mut e, _) = build_engine(&p.chart, &rules, None, false);
            assert_eq!(e.gauge.value(), 100.0);
            if let Some(v) = &gauge {
                e.gauge.set_values(v);
                assert_eq!(e.gauge.values(), *v);
            }
            let inputs = crate::bot::inputs(&p.chart, None, 3 + k);
            crate::bot::drive(&mut e, &inputs, p.chart.length() + 2_000_000);
            let mines = e
                .notes()
                .iter()
                .filter(|n| n.is_mine() && n.state.is_some())
                .count() as u32;
            let first_break = e.ghost.iter().position(|j| !e.rules().combo[*j as usize]);
            totals.add(&e.score, mines, e.combo(), first_break.map(|i| i as u32));
            notes += e.score.total_notes;
            pg += e.score.count(Judge::PGreat);
            empty += e.score.count(Judge::Miss) - mines;
            gauge = Some(e.gauge.values());
        }
        let s = &totals.score;
        assert_eq!((s.total_notes, s.passed), (notes, notes));
        assert_eq!((s.count(Judge::PGreat), s.count(Judge::Miss)), (pg, empty));
        assert!(s.max_combo >= totals.combo);
        // the course's lamp from the gauge it ended with
        let v = gauge.unwrap()[GaugeType::Normal as usize];
        let lamp = totals.lamp(true, v, crate::score::ClearType::Normal);
        assert_eq!(lamp == crate::score::ClearType::Failed, v <= 2.0);
    }
}
