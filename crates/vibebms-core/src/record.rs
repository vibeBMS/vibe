//! A finished play as facts: the chart, the rules its judge really had,
//! the devices that played it and what came of it. Made once when the play
//! ends; whatever is sent or shown of the play later (IR, history, the
//! result's rules line) reads these, never the options as they are now.

use crate::chart::{Chart, JudgeRank, LnProfile, LnType};
use crate::engine::{Engine, JudgeAlgorithm};
use crate::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
use crate::judge::{judge_rules, JudgePreset};
use crate::lane_option::DoubleOption;
use crate::mode::PlayMode;
use crate::play::RuleOptions;
use crate::score::{AssistFacts, ClearType, Score};
use serde::{Deserialize, Serialize};

/// The record format.
pub const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayRecord {
    pub v: u32,
    /// The play's own id (32 hex digits): unique across installs, the same
    /// on every copy or submission of the play. Given as it is kept (the
    /// app's IR history), empty before.
    pub play_id: String,
    pub chart: ChartFacts,
    pub rules: RuleFacts,
    pub device: DeviceFacts,
    pub flags: PlayFlags,
    pub result: PlayResult,
    pub client: ClientId,
    /// When the play ended (Unix ms): taken once, as it ended; every date
    /// sent or kept of the play is this one.
    pub ended_ms: i64,
    /// Real time from chart time 0 to the end (ms).
    pub wall_ms: u64,
    pub course: Option<CourseStage>,
}

impl PlayRecord {
    /// The LN mode the play is filed under (`LnProfile::score_ln`).
    pub fn ln_filed(&self) -> LnType {
        self.chart.ln.score_ln(self.rules.ln_mode)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChartFacts {
    pub md5: String,
    pub sha256: String,
    /// The chart's own key mode (a battle's: the single play chart's).
    pub mode: PlayMode,
    pub title: String,
    pub artist: String,
    pub level: String,
    pub difficulty: i32,
    pub judge_rank: JudgeRank,
    /// The TOTAL the gauge used.
    pub total: f64,
    /// Its long notes as written.
    pub ln: LnProfile,
    /// `#RANDOM` / `#SWITCH`, and the branches this play took.
    pub has_random: bool,
    pub random_selections: Vec<u32>,
    pub length_us: i64,
}

impl ChartFacts {
    /// `chart` as played in its own key mode `mode`; `ln` its long notes
    /// before the options, `total` the gauge's TOTAL.
    pub fn new(chart: &Chart, mode: PlayMode, ln: LnProfile, total: f64) -> ChartFacts {
        let m = &chart.meta;
        ChartFacts {
            md5: m.md5.clone(),
            sha256: m.sha256.clone(),
            mode,
            title: m.title.clone(),
            artist: m.artist.clone(),
            level: m.level.clone(),
            difficulty: m.difficulty,
            judge_rank: m.judge_rank,
            total,
            ln,
            has_random: m.has_random,
            random_selections: m.random_selections.clone(),
            length_us: chart.length(),
        }
    }
}

/// The rules the judge had: the options it was built with (a course's
/// forced ones included), checked against the judge itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleFacts {
    pub judge: JudgePreset,
    /// The windows were not the preset's own for the chart: EXPAND JUDGE's
    /// rates (PG, GR, GD %).
    pub judge_rates: Option<[i64; 3]>,
    pub algorithm: JudgeAlgorithm,
    pub gauge_set: GaugePreset,
    /// The gauge played from.
    pub gauge: GaugeType,
    pub gas: GaugeAutoShift,
    /// The LN mode of long notes the chart leaves untyped.
    pub ln_mode: LnType,
    /// A back-spin scratch played as CN or HCN: there a key release can
    /// end it (vibeBMS's rule for keyboards and touch).
    pub charge_bss: bool,
    pub dp_flip: bool,
    /// The seed of `#RANDOM` and the lane options (vibeBMS's own scheme:
    /// 2P draws from seed + 1).
    pub seed: i64,
    /// What the options did (the lane options per side among them).
    pub assist: AssistFacts,
}

impl RuleFacts {
    /// The rules of `engine`, built from `rules` (`play::build_engine`) on
    /// `chart`.
    pub fn new(
        rules: &RuleOptions,
        engine: &Engine,
        chart: &ChartFacts,
        assist: AssistFacts,
        seed: i64,
    ) -> RuleFacts {
        let mode = chart.mode;
        let own = judge_rules(rules.judge_preset, mode.family(), chart.judge_rank, None);
        let charge_bss = engine.notes().iter().any(|n| {
            mode.is_scratch(n.lane as usize)
                && matches!(n.ln_type(), Some(LnType::Cn | LnType::Hcn))
        });
        RuleFacts {
            judge: rules.judge_preset,
            judge_rates: (*engine.rules() != own).then_some(rules.judge_rates),
            algorithm: rules.judge_algorithm,
            gauge_set: rules.gauge_preset,
            gauge: rules.gauge,
            gas: rules.gauge_auto_shift,
            ln_mode: rules.ln_mode,
            charge_bss,
            dp_flip: mode.is_double() && rules.dp_option == DoubleOption::Flip,
            seed,
            assist,
        }
    }
}

/// Where a key that reached the judge came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSource {
    Keyboard,
    Gamepad,
    Touch,
}

/// What the play was played with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceFacts {
    /// Presses that reached the judge, by source.
    pub keyboard: u32,
    pub gamepad: u32,
    pub touch: u32,
    /// The touch controls were on screen.
    pub touch_ui: bool,
    pub platform: Platform,
}

impl DeviceFacts {
    pub fn count(&mut self, src: InputSource) {
        let n = match src {
            InputSource::Keyboard => &mut self.keyboard,
            InputSource::Gamepad => &mut self.gamepad,
            InputSource::Touch => &mut self.touch,
        };
        *n += 1;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    #[default]
    Other,
    Windows,
    MacOs,
    Linux,
    Web,
    Android,
    Ios,
}

impl Platform {
    /// The platform this build runs on.
    pub const fn current() -> Platform {
        if cfg!(target_arch = "wasm32") {
            Platform::Web
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "android") {
            Platform::Android
        } else if cfg!(target_os = "ios") {
            Platform::Ios
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

/// How the play was played (BATTLE is an assist: `RuleFacts::assist`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayFlags {
    pub autoplay: bool,
    /// A replay watched.
    pub replay: bool,
    /// VS: two players here.
    pub versus: bool,
    /// G-BATTLE: against a replay.
    pub ghost_battle: bool,
    /// Quit by the player.
    pub aborted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayResult {
    pub score: Score,
    pub clear: ClearType,
    /// The gauge at the end (GAUGE AUTO SHIFT: the one it shifted to).
    pub gauge: GaugeType,
    pub gauge_value: f32,
    /// Every gauge's value at the end (`GaugeType` order).
    pub gauge_values: Vec<f32>,
    /// The gauge every 500 ms, at most `GAUGE_LOG_MAX` samples.
    pub gauge_log: Vec<f32>,
    /// Chart time of the last judgement (ms).
    pub play_ms: u64,
}

/// The longest gauge log a record keeps.
pub const GAUGE_LOG_MAX: usize = 1000;

/// `log` cut to at most `GAUGE_LOG_MAX` samples, evenly, the last kept.
pub fn thin_gauge_log(log: &[f32]) -> Vec<f32> {
    let step = log.len().div_ceil(GAUGE_LOG_MAX).max(1);
    let mut v: Vec<f32> = log.iter().step_by(step).copied().collect();
    if step > 1 && (log.len() - 1) % step != 0 {
        v.pop();
        v.extend(log.last());
    }
    v
}

/// Which program played it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientId {
    pub name: String,
    pub version: String,
}

/// A course's stage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseStage {
    pub name: String,
    pub index: u32,
    pub of: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{build_chart, build_engine};
    use std::path::Path;

    /// A bot's play of a made-up chart under `rules`: its record's rules.
    fn played(keys: usize, rules: &RuleOptions) -> RuleFacts {
        let text = crate::bot::chart_text(keys, 11);
        let p = build_chart(Path::new("c.bme"), text.as_bytes(), rules, 5, None).unwrap();
        let (mut e, total) = build_engine(&p.chart, rules, None, false);
        crate::bot::drive(
            &mut e,
            &crate::bot::inputs(&p.chart, None, 3),
            p.chart.length(),
        );
        let chart = ChartFacts::new(&p.chart, p.chart.mode(), p.ln_profile, total);
        RuleFacts::new(rules, &e, &chart, p.facts, 5)
    }

    #[test]
    fn rule_facts_are_the_judges() {
        let base = RuleOptions::default();
        let r = played(7, &base);
        assert_eq!(
            (r.judge, r.gauge_set),
            (JudgePreset::Beatoraja, GaugePreset::Beatoraja)
        );
        assert_eq!((r.judge_rates, r.charge_bss, r.seed), (None, false, 5));
        // EXPAND JUDGE at 100 % judges as the preset: not custom
        let same = RuleOptions {
            expand_judge: true,
            judge_rates: [100; 3],
            ..base.clone()
        };
        assert_eq!(played(7, &same).judge_rates, None);
        let narrow = RuleOptions {
            expand_judge: true,
            judge_rates: [50, 80, 100],
            ..base.clone()
        };
        assert_eq!(played(7, &narrow).judge_rates, Some([50, 80, 100]));
        // the bot's charts have scratch long notes of no type
        let cn = RuleOptions {
            ln_mode: LnType::Cn,
            ..base
        };
        assert!(played(7, &cn).charge_bss);
    }

    #[test]
    fn a_record_reads_back() {
        let mut rec = PlayRecord {
            v: VERSION,
            rules: played(14, &RuleOptions::default()),
            ended_ms: 1_767_225_600_123,
            ..Default::default()
        };
        rec.device.count(InputSource::Gamepad);
        rec.result.score.max_combo = 3;
        let json = serde_json::to_string(&rec).unwrap();
        assert_eq!(serde_json::from_str::<PlayRecord>(&json).unwrap(), rec);
    }

    #[test]
    fn gauge_logs_are_thinned() {
        let log: Vec<f32> = (0..2501).map(|i| i as f32).collect();
        let t = thin_gauge_log(&log);
        assert!(t.len() <= GAUGE_LOG_MAX);
        assert_eq!((t[0], t[1], *t.last().unwrap()), (0.0, 3.0, 2500.0));
        assert_eq!(thin_gauge_log(&log[..10]), &log[..10]);
    }
}
