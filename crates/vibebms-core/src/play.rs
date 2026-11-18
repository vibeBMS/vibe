//! Play setup: the options that make a play's chart and its rules, and the
//! chart and judge they make (the same for a play, a replay watched again
//! and a replay checked without the app).

use crate::chart::{Chart, LnPref, LnProfile, LnType, NoteKind};
use crate::course::{CourseKind, Join};
use crate::engine::{Engine, EngineConfig, JudgeAlgorithm};
use crate::gauge::{GaugeAutoShift, GaugePreset, GaugeType, GrooveGauge};
use crate::judge::{judge_rules, JudgePreset};
use crate::lane_option::{
    self, DoubleOption, LaneOption, LaneRules, LnAdd, MineAdd, SRandomInterval,
};
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
    /// The LN mode: the type of the long notes the chart leaves untyped,
    /// and with `ln_force` of every long note (LN MODE FORCE).
    pub ln_mode: LnType,
    pub ln_force: bool,
    pub auto_scratch: bool,
    pub legacy_note: bool,
    pub no_mine: bool,
    /// Chart modifiers (assists where they take effect): SCROLL REMOVE,
    /// MINE ADD, LONGNOTE ADD.
    pub scroll_remove: bool,
    pub mine_add: MineAdd,
    pub ln_add: LnAdd,
    /// EXPAND JUDGE (beatoraja custom judge): the judge windows scaled by
    /// `judge_rates` (PG, GR, GD in percent). Wider than 100 % is an assist
    /// (ASSIST EASY, the score is not saved).
    pub expand_judge: bool,
    pub judge_rates: [i64; 3],
    /// H-RANDOM / ALL-SCR jack threshold in BPM (16th notes).
    pub hran_threshold_bpm: u32,
    /// How S-RANDOM avoids jacks.
    pub s_random_interval: SRandomInterval,
    /// TRAINER's lane order (`lane_option::trainer_order`).
    pub trainer_order: String,
    /// FREQ (LR2's): the whole chart played `freq` semitones higher
    /// (`FREQ_MIN..=FREQ_MAX`), faster by 2^(freq / 12), its sounds
    /// resampled to match; the judge windows stay in real time.
    pub freq: i32,
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
            ln_force: false,
            auto_scratch: false,
            legacy_note: false,
            no_mine: false,
            scroll_remove: false,
            mine_add: MineAdd::Off,
            ln_add: LnAdd::Off,
            expand_judge: false,
            judge_rates: [400, 400, 100],
            hran_threshold_bpm: 120,
            s_random_interval: SRandomInterval::default(),
            trainer_order: lane_option::plain_order(7),
            freq: 0,
            course: None,
        }
    }
}

/// FREQ's range in semitones.
pub const FREQ_MIN: i32 = -12;
pub const FREQ_MAX: i32 = 12;

/// The speed FREQ `semitones` plays at (1.0: as written).
pub fn freq_rate(semitones: i32) -> f64 {
    2f64.powf(semitones.clamp(FREQ_MIN, FREQ_MAX) as f64 / 12.0)
}

/// FREQ as shown: "OFF", "+3 (119%)", "-12 (50%)".
pub fn freq_label(semitones: i32) -> String {
    match semitones {
        0 => "OFF".into(),
        s => format!("{s:+} ({:.0}%)", freq_rate(s) * 100.0),
    }
}

impl RuleOptions {
    /// The LN mode (AUTO or FORCE).
    pub fn ln_pref(&self) -> LnPref {
        LnPref {
            ty: self.ln_mode,
            force: self.ln_force,
        }
    }

    pub fn set_ln_pref(&mut self, p: LnPref) {
        (self.ln_mode, self.ln_force) = (p.ty, p.force);
    }

    /// The speed the chart plays at (FREQ).
    pub fn freq_rate(&self) -> f64 {
        freq_rate(self.freq)
    }

    /// The rules of a course of `kind` played on these options. Under the
    /// LR2 gauge set a class course forces LR2's settings (S-OLR2b 5.1):
    /// EASY and G-ATTACK play GROOVE (its class gauges, as HARD plays
    /// EX-CLASS), and no lane option, BATTLE, DP FLIP, assist or note
    /// option; an expert or NONSTOP course forces nothing (4.2). Under the
    /// other sets every course plays the set's dan gauges, as vibeBMS's
    /// table courses do. No course has GAUGE AUTO SHIFT or FREQ (LR2
    /// switches FREQ off in class and NONSTOP courses: S-OLR2b 3.8, 5.1).
    pub fn for_course(&self, kind: CourseKind) -> RuleOptions {
        let lr2 = self.gauge_preset == GaugePreset::Lr2;
        let mut r = RuleOptions {
            course: Some(kind),
            gauge_auto_shift: GaugeAutoShift::None,
            freq: 0,
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
                r.scroll_remove = false;
                r.mine_add = MineAdd::Off;
                r.ln_add = LnAdd::Off;
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
    // (its scroll goes by time: nothing for SCROLL REMOVE to take off)
    let rules = RuleOptions {
        scroll_remove: false,
        ..rules.clone()
    };
    Ok((arrange(chart, &rules, seed, None), ns))
}

/// `chart` at FREQ's speed (so the lane options' intervals are real
/// time), arranged by `rules` (`course`: as `build_chart`'s), then its
/// modifiers; with LN MODE FORCE its long notes (LONGNOTE ADD's too)
/// have no type of their own (they play as the LN mode).
fn arrange(mut chart: Chart, rules: &RuleOptions, seed: i64, course: Option<bool>) -> PlayChart {
    let ln_profile = chart.ln_profile();
    let rate = rules.freq_rate();
    if rate != 1.0 {
        chart = crate::practice::retime(chart, 0, rate);
    }
    let mut facts = AssistFacts::default();
    if rules.battle && !chart.mode().is_double() {
        lane_option::battle(&mut chart);
        facts.battle = true;
    }
    let lanes = LaneRules {
        hran_threshold: 60_000_000 / (rules.hran_threshold_bpm.max(1) as i64 * 4),
        s_random: rules.s_random_interval,
        trainer: rules.trainer_order.clone(),
    };
    let allowed = |o: LaneOption| match course {
        Some(mirror) if !(mirror && o == LaneOption::Mirror) => LaneOption::Off,
        _ => o,
    };
    let took = lane_option::apply_with(&mut chart, 0, allowed(rules.random_1p), seed, &lanes);
    facts.lane.push(took);
    if chart.mode().is_double() {
        let random_2p = allowed(rules.random_2p);
        let took = lane_option::apply_with(&mut chart, 1, random_2p, seed.wrapping_add(1), &lanes);
        facts.lane.push(took);
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
    modify(&mut chart, rules, seed, &mut facts);
    if rules.ln_force {
        for n in &mut chart.notes {
            if let NoteKind::Long { ln_type, .. } = &mut n.kind {
                *ln_type = None;
            }
        }
    }
    PlayChart {
        chart,
        facts,
        ln_profile,
    }
}

/// The chart modifiers: SCROLL REMOVE, MINE ADD, LONGNOTE ADD (what took
/// effect into `facts`).
fn modify(chart: &mut Chart, rules: &RuleOptions, seed: i64, facts: &mut AssistFacts) {
    if rules.scroll_remove {
        facts.scroll_remove = lane_option::remove_scroll(chart);
    }
    facts.ln_add = lane_option::add_long_notes(chart, rules.ln_add);
    facts.mine_add = lane_option::add_mines(chart, rules.mine_add, seed);
}

/// G-BATTLE's chart: the single play chart read with the opponent's seed
/// and arranged by its lane option, on both sides (as LR2 lends a ghost's
/// random), at its FREQ; each side's LEGACY NOTE and NO MINE apply to its
/// own lanes.
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
        trainer_order: theirs.trainer_order.clone(),
        freq: theirs.freq,
        battle: false,
        legacy_note: false,
        no_mine: false,
        // (each side's own would make two charts: none)
        mine_add: MineAdd::Off,
        ln_add: LnAdd::Off,
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
    let total = chart.total_of(rules.ln_mode, rules.gauge_preset, player);
    let notes = chart.total_judgements_of(rules.ln_mode, player);
    let engine = build_engine_with(chart, rules, player, autoplay, total, notes);
    (engine, total)
}

/// As `build_engine`, its gauge moved by `total` over `notes` judgements
/// (a practice section's: the whole chart's).
pub fn build_engine_with(
    chart: &Chart,
    rules: &RuleOptions,
    player: Option<usize>,
    autoplay: bool,
    total: f64,
    notes: usize,
) -> Engine {
    let mode = chart.mode();
    let fam = mode.family();
    let rates = rules.expand_judge.then_some(rules.judge_rates);
    let judge = judge_rules(rules.judge_preset, fam, chart.meta.judge_rank, rates);
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
    Engine::new(chart, cfg, gauge)
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
            scroll_remove: true,
            mine_add: MineAdd::Blank,
            ln_add: LnAdd::Hcn,
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
            let r = RuleOptions {
                gauge,
                ..mine.clone()
            }
            .for_course(CourseKind::Class);
            assert_eq!(r.gauge, played, "{gauge:?}");
            assert_eq!(
                (r.random_1p, r.random_2p),
                (LaneOption::Off, LaneOption::Off)
            );
            assert_eq!(r.dp_option, DoubleOption::Off);
            assert!(!(r.battle || r.auto_scratch || r.legacy_note || r.no_mine || r.expand_judge));
            assert!(!r.scroll_remove && r.mine_add == MineAdd::Off && r.ln_add == LnAdd::Off);
            assert_eq!(r.gauge_auto_shift, GaugeAutoShift::None);
            assert_eq!(r.course, Some(CourseKind::Class));
        }
        // expert and NONSTOP force nothing (but GAUGE AUTO SHIFT)
        let r = RuleOptions {
            gauge: GaugeType::GAttack,
            ..mine.clone()
        }
        .for_course(CourseKind::Expert);
        assert_eq!(
            (r.gauge, r.random_1p, r.battle),
            (GaugeType::GAttack, LaneOption::SRandom, true)
        );
        // another set: its dan gauges, the options as they are
        let b = RuleOptions {
            gauge: GaugeType::Hard,
            random_1p: LaneOption::Random,
            ..Default::default()
        }
        .for_course(CourseKind::Class);
        assert_eq!(
            (b.gauge, b.random_1p),
            (GaugeType::ExClass, LaneOption::Random)
        );
    }

    /// FREQ retimes the whole chart: every time ÷ the rate, the BPMs ×
    /// it, the lanes as at 0; the judge windows stay in real time;
    /// courses play at 0.
    #[test]
    fn freq_plays_the_chart_faster_and_higher() {
        let text = crate::bot::chart_text(7, 11);
        let at = |freq: i32, random_1p: LaneOption| {
            let rules = RuleOptions {
                freq,
                random_1p,
                ..Default::default()
            };
            let p = build_chart(Path::new("c.bme"), text.as_bytes(), &rules, 5, None).unwrap();
            let (e, _) = build_engine(&p.chart, &rules, None, false);
            (p.chart, e.windows().note.clone())
        };
        let (plain, windows) = at(0, LaneOption::Random);
        for freq in [-12, -1, 3, 12] {
            let k = freq_rate(freq);
            let (c, w) = at(freq, LaneOption::Random);
            let want: Vec<(u8, i64)> = (plain.notes.iter())
                .map(|n| (n.lane, (n.time as f64 / k).round() as i64))
                .collect();
            let got: Vec<(u8, i64)> = c.notes.iter().map(|n| (n.lane, n.time)).collect();
            assert_eq!(got, want, "FREQ {freq}");
            assert!((c.meta.initial_bpm - plain.meta.initial_bpm * k).abs() < 1e-9);
            assert_eq!(w, windows);
        }
        assert!((freq_rate(12) - 2.0).abs() < 1e-12);
        assert!((freq_rate(-12) - 0.5).abs() < 1e-12);
        assert_eq!(freq_rate(40), freq_rate(12));
        assert_eq!(
            [freq_label(0), freq_label(3), freq_label(-12)],
            ["OFF", "+3 (119%)", "-12 (50%)"]
        );
        for kind in [CourseKind::Class, CourseKind::Expert, CourseKind::Nonstop] {
            for rules in [lr2(), RuleOptions::default()] {
                let r = RuleOptions { freq: 5, ..rules }.for_course(kind);
                assert_eq!(r.freq, 0, "{kind:?}");
            }
        }
    }

    /// TRAINER plays its order on a chart of as many keys, an L-ASSIST
    /// play; on another it does nothing (OFF, no assist).
    #[test]
    fn the_trainer_is_a_light_assist_where_it_fits() {
        let rules = RuleOptions {
            random_1p: LaneOption::Trainer,
            trainer_order: "7654321".into(),
            ..Default::default()
        };
        let mirror = RuleOptions {
            random_1p: LaneOption::Mirror,
            ..Default::default()
        };
        let seven = crate::bot::chart_text(7, 11);
        let build = |r: &RuleOptions, text: &str| {
            build_chart(Path::new("c.bme"), text.as_bytes(), r, 5, None).unwrap()
        };
        let (t, m) = (build(&rules, &seven), build(&mirror, &seven));
        let lanes = |c: &Chart| c.notes.iter().map(|n| (n.time, n.lane)).collect::<Vec<_>>();
        assert_eq!(lanes(&t.chart), lanes(&m.chart));
        assert_eq!(t.facts.lane, [LaneOption::Trainer]);
        assert_eq!(t.facts.level(), crate::score::Assist::Light);
        let five = crate::bot::chart_text(5, 11);
        let f = build(&rules, &five);
        assert_eq!(f.facts.lane, [LaneOption::Off]);
        assert_eq!(f.facts.level(), crate::score::Assist::None);
        // a class course plays no lane option
        let c = build_chart(Path::new("c.bme"), seven.as_bytes(), &rules, 5, Some(false)).unwrap();
        assert_eq!(c.facts.lane, [LaneOption::Off]);
    }

    /// The chart modifiers are assists where they take effect: LONGNOTE
    /// ADD as LEGACY NOTE, MINE ADD as NO MINE, SCROLL REMOVE as BPM GUIDE.
    #[test]
    fn chart_modifiers_are_assists_where_they_change_the_chart() {
        use crate::score::Assist;
        let text = crate::bot::chart_text(7, 11);
        let with = |f: fn(&mut RuleOptions)| {
            let mut r = RuleOptions::default();
            f(&mut r);
            build_chart(Path::new("c.bme"), text.as_bytes(), &r, 5, None).unwrap()
        };
        let plain = with(|_| {});
        assert_eq!(plain.facts.level(), Assist::None);
        // the bot's chart has no scroll changes: SCROLL REMOVE does nothing
        let sr = with(|r| r.scroll_remove = true);
        assert!(!sr.facts.scroll_remove);
        assert_eq!(sr.facts.level(), Assist::None);
        let mines = with(|r| r.mine_add = MineAdd::Near);
        assert!(mines.facts.mine_add && mines.chart.meta.has_mine);
        assert_eq!(mines.facts.level(), Assist::Light);
        let lns = with(|r| r.ln_add = LnAdd::Cn);
        assert!(lns.facts.ln_add);
        assert_eq!(lns.facts.level(), Assist::Full);
        // the added long notes are CN, judged at both ends
        let rules = RuleOptions {
            ln_add: LnAdd::Cn,
            ..Default::default()
        };
        let (e, _) = build_engine(&lns.chart, &rules, None, false);
        assert!(e.score.total_notes as usize > plain.chart.note_count());
        // FORCE turns them too
        let forced = with(|r| {
            r.ln_add = LnAdd::Cn;
            r.ln_force = true;
        });
        assert!(forced.chart.notes.iter().all(|n| !matches!(
            n.kind,
            NoteKind::Long {
                ln_type: Some(_),
                ..
            }
        )));
        // a scroll gimmick: taken off, the notes at their beats
        let sc = "#BPM 120\n#SCROLL01 0.5\n#SPEED01 3\n#001SC:01\n#001SP:01\n#00211:01\n";
        let r = RuleOptions {
            scroll_remove: true,
            ..Default::default()
        };
        let p = build_chart(Path::new("c.bme"), sc.as_bytes(), &r, 1, None).unwrap();
        assert!(p.facts.scroll_remove);
        assert_eq!(p.facts.level(), Assist::Light);
        assert!(!p.chart.timing.has_scroll_changes());
        assert!(p.chart.notes.iter().all(|n| n.pos == n.beat));
    }

    /// FORCE plays every long note as the LN mode, whatever its type; the
    /// long notes as written stay the chart's (`ln_profile`).
    #[test]
    fn ln_mode_force_overrides_the_charts_types() {
        // a CN typed by #LNMODE 2 on key 1 and a note on key 2
        let text = "#BPM 120\n#LNMODE 2\n#WAV01 a.wav\n#00151:0101\n#00112:01\n";
        let at = |ln_mode: LnType, ln_force: bool| {
            let rules = RuleOptions {
                ln_mode,
                ln_force,
                ..Default::default()
            };
            let p = build_chart(Path::new("c.bme"), text.as_bytes(), &rules, 1, None).unwrap();
            let (e, _) = build_engine(&p.chart, &rules, None, false);
            (p, e.score.total_notes, rules)
        };
        // AUTO: the chart's CN, its end judged (3 judgements)
        let (p, n, _) = at(LnType::Hcn, false);
        assert_eq!(n, 3);
        assert_eq!(p.ln_profile.typed, [false, true, false]);
        // FORCE LN: an LN, judged once; filed under LN
        let (p, n, rules) = at(LnType::Ln, true);
        assert_eq!(n, 2);
        assert!(p.chart.notes.iter().all(|n| !matches!(
            n.kind,
            NoteKind::Long {
                ln_type: Some(_),
                ..
            }
        )));
        assert_eq!(p.ln_profile.typed, [false, true, false]);
        assert_eq!(p.ln_profile.score_ln(rules.ln_pref()), LnType::Ln);
        // FORCE HCN: filed under HCN, the AUTO plays under CN
        let (p, n, rules) = at(LnType::Hcn, true);
        assert_eq!(n, 3);
        assert_eq!(p.ln_profile.score_ln(rules.ln_pref()), LnType::Hcn);
        assert_eq!(p.ln_profile.score_ln(LnType::Hcn), LnType::Cn);
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
