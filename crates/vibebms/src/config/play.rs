//! The play options: rules, view and the second player's own in VS.

use serde::{Deserialize, Serialize};
use vibebms_core::gauge::GaugeType;
use vibebms_core::play::RuleOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Side {
    /// Scratch on the left, playfield on the left.
    #[default]
    P1,
    /// Scratch on the right, playfield on the right.
    P2,
}

/// The battles chosen in song select that are not play options: they
/// last until the game closes (never saved).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Versus {
    #[default]
    Off,
    /// G-BATTLE against your MY BEST replay of the chart.
    GhostBest,
    /// G-BATTLE against your latest replay of the chart.
    GhostLast,
    /// Two players, one per side (VS).
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HiSpeedMode {
    /// The hi-speed applies to the main BPM; notes speed up and slow down
    /// with the chart's BPM changes and stop at its stops.
    #[default]
    Floating,
    /// Notes keep the main-BPM speed through BPM changes and stops.
    Constant,
}

vibebms_core::choices! {
    /// How the hi-speed is set.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum HsType {
        /// A multiplier; HS-FIX keeps the green number at its BPM.
        #[default]
        Classic => "CLASSIC",
        /// IIDX's NORMAL hi-speed: levels 1 to 20, each a green number of
        /// the whole lane (`NORMAL_GREENS`), at the HS-FIX BPM.
        Normal => "NORMAL",
        /// IIDX's FLOATING hi-speed: the green number is kept, through the
        /// lane cover and, while playing, through the chart's BPM changes.
        Floating => "FLOATING",
        /// LR2's: a multiplier; with HS-FIX the speed it gives at 150 BPM
        /// (the cover leaves it as it is).
        Lr2 => "LR2",
    }
}

/// The green numbers of NORMAL hi-speed's levels 1 to 20 (the whole lane,
/// no cover), as bmz-player documents IIDX's (`docs/hs.md`).
pub const NORMAL_GREENS: [u32; 20] = [
    1200, 1000, 800, 700, 650, 600, 550, 500, 480, 460, 440, 420, 400, 380, 360, 340, 320, 300,
    280, 260,
];

vibebms_core::choices! {
    /// Which BPM a fixed green number is kept at (beatoraja HS-FIX order).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum HsFix {
        /// The hi-speed is the setting.
        #[default]
        Off => "OFF",
        Start => "START BPM",
        Max => "MAX BPM",
        Main => "MAIN BPM",
        Min => "MIN BPM",
    }
}

vibebms_core::choices! {
    /// Target score: beatoraja's targets (a share of the maximum in 27ths,
    /// MAX, the next rank above the best) and the personal best, in
    /// beatoraja's default target list order (skins index it), then BEST.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
    pub enum Target {
        RateAMinus => "RANK A-",
        #[serde(alias = "RankA")]
        RateA => "RANK A",
        RateAPlus => "RANK A+",
        RateAAMinus => "RANK AA-",
        #[serde(alias = "RankAA")]
        RateAA => "RANK AA",
        RateAAPlus => "RANK AA+",
        RateAAAMinus => "RANK AAA-",
        #[serde(alias = "RankAAA")]
        RateAAA => "RANK AAA",
        RateAAAPlus => "RANK AAA+",
        #[serde(alias = "MaxMinus")]
        RateMaxMinus => "RANK MAX-",
        Max => "MAX",
        /// The next DJ level step (in 27ths) above the best score.
        RankNext => "NEXT RANK",
        #[default]
        Best => "BEST",
    }
}

impl Target {
    /// The target EX score for a chart with `max_ex` and personal best `best`.
    pub fn score(self, max_ex: u32, best: u32) -> u32 {
        let at = |n: u64| (max_ex as u64 * n).div_ceil(27) as u32;
        match self {
            Target::Best => best,
            Target::Max => max_ex,
            // the first step from 15/27 up that the best has not reached
            Target::RankNext => (15..27).map(at).find(|t| best < *t).unwrap_or(max_ex),
            t => at(17 + Target::ALL.iter().position(|x| *x == t).unwrap_or(0) as u64),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayConfig {
    /// What makes the chart and judges it (read through the play options:
    /// `p.gauge` is `p.rules.gauge`).
    #[serde(flatten)]
    pub rules: RuleOptions,
    /// G-BATTLE / VS for this session (not saved, not in replays).
    #[serde(skip)]
    pub versus: Versus,
    pub side: Side,
    pub autoplay: bool,
    pub hispeed_mode: HiSpeedMode,
    /// Scroll speed: at 1.0 the lane shows one measure (4 beats) of the
    /// chart's main BPM. The time a note is visible (the green number)
    /// follows from it, the BPM and the lane cover.
    pub hispeed: f32,
    /// Keep the green number instead (IIDX floating hi-speed, beatoraja /
    /// LR2 HS-FIX): the hi-speed follows each chart's BPM of this kind and
    /// the lane cover so that notes stay visible for `fixed_duration_ms`.
    pub hs_fix: HsFix,
    pub fixed_duration_ms: f32,
    /// How the hi-speed is set, NORMAL's level (1 ..= 20), and how long a
    /// change made while playing takes to reach the notes (ms; 0: at once).
    pub hs_type: HsType,
    pub hs_level: u8,
    pub hs_anim_ms: u32,
    pub sudden: f32,
    pub hidden: f32,
    pub lift: f32,
    pub lane_cover: bool,
    /// Positive = you hit late, judgement is shifted accordingly.
    pub judge_offset_ms: i32,
    /// Shift the note display (ms).
    pub visual_offset_ms: i32,
    pub bga: bool,
    pub show_fast_slow: bool,
    /// JUDGE AREA: the judge windows drawn above the judge line.
    pub judge_area: bool,
    /// MARK NOTE: a note hit early keeps falling to the line, marked.
    pub mark_notes: bool,
    /// BPM GUIDE: BPM changes and stops shown on the lanes (on a chart with
    /// BPM changes an assist: LIGHT ASSIST EASY, the score is not saved).
    pub bpm_guide: bool,
    /// Move the judge timing toward where you hit (beatoraja notes display
    /// timing auto adjust).
    pub timing_auto_adjust: bool,
    /// The score the pacemaker and the result compare against.
    pub target: Target,
    /// The second player's own options in VS.
    pub p2: SideOptions,
}

/// The options each player of VS has of their own (the first player's
/// are the play options themselves).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SideOptions {
    pub gauge: GaugeType,
    pub hispeed: f32,
    pub hs_fix: HsFix,
    pub fixed_duration_ms: f32,
    pub sudden: f32,
    pub hidden: f32,
    pub lift: f32,
    pub lane_cover: bool,
    pub auto_scratch: bool,
    pub judge_offset_ms: i32,
    /// NORMAL hi-speed's level (the type is the first player's).
    #[serde(skip_serializing_if = "default_level")]
    pub hs_level: u8,
}

const DEFAULT_LEVEL: u8 = 10;

fn default_level(l: &u8) -> bool {
    *l == DEFAULT_LEVEL
}

impl Default for SideOptions {
    fn default() -> Self {
        SideOptions::of(&PlayConfig {
            p2: SideOptions::blank(),
            ..PlayConfig::default()
        })
    }
}

impl SideOptions {
    /// (Only to make the default play options.)
    fn blank() -> SideOptions {
        SideOptions {
            gauge: GaugeType::Normal,
            hispeed: 2.5,
            hs_fix: HsFix::Off,
            fixed_duration_ms: 600.0,
            sudden: 0.0,
            hidden: 0.0,
            lift: 0.0,
            lane_cover: false,
            auto_scratch: false,
            judge_offset_ms: 0,
            hs_level: DEFAULT_LEVEL,
        }
    }

    pub fn of(p: &PlayConfig) -> SideOptions {
        SideOptions {
            gauge: p.gauge,
            hispeed: p.hispeed,
            hs_fix: p.hs_fix,
            fixed_duration_ms: p.fixed_duration_ms,
            sudden: p.sudden,
            hidden: p.hidden,
            lift: p.lift,
            lane_cover: p.lane_cover,
            auto_scratch: p.auto_scratch,
            judge_offset_ms: p.judge_offset_ms,
            hs_level: p.hs_level,
        }
    }

    fn apply(&self, p: &mut PlayConfig) {
        p.gauge = self.gauge;
        p.hispeed = self.hispeed;
        p.hs_fix = self.hs_fix;
        p.fixed_duration_ms = self.fixed_duration_ms;
        p.sudden = self.sudden;
        p.hidden = self.hidden;
        p.lift = self.lift;
        p.lane_cover = self.lane_cover;
        p.auto_scratch = self.auto_scratch;
        p.judge_offset_ms = self.judge_offset_ms;
        p.hs_level = self.hs_level;
    }
}

impl PlayConfig {
    /// The options a player side plays with: the second player's in VS
    /// are these with `p2`'s own.
    pub fn for_side(&self, side: usize) -> PlayConfig {
        let mut p = self.clone();
        if side == 1 {
            self.p2.apply(&mut p);
        }
        p
    }

    /// Keep what changed in `p`, the options of `side` (as `for_side`).
    pub fn set_side(&mut self, side: usize, p: &PlayConfig) {
        if side == 1 {
            self.p2 = SideOptions::of(p);
        } else {
            let p2 = std::mem::replace(&mut self.p2, SideOptions::blank());
            *self = p.clone();
            self.p2 = p2;
        }
    }
}

/// Written field by field in the order they had before the rules moved to
/// `RuleOptions` (a flattened derive writes the rules first): saved
/// configurations and replays keep their bytes. A new field goes here too.
impl Serialize for PlayConfig {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let (p, r) = (self, &self.rules);
        let mut st = s.serialize_struct("PlayConfig", 39)?;
        macro_rules! fields {
            ($($o:ident.$f:ident),* $(,)?) => {
                $(st.serialize_field(stringify!($f), &$o.$f)?;)*
            };
        }
        fields!(
            r.gauge,
            r.gauge_auto_shift,
            r.gauge_preset,
            r.judge_preset,
            r.judge_algorithm,
            r.random_1p,
            r.random_2p,
            r.dp_option,
            r.battle,
            p.side,
            r.ln_mode,
            r.auto_scratch,
            r.legacy_note,
            r.no_mine,
            p.autoplay,
            p.hispeed_mode,
            p.hispeed,
            p.hs_fix,
            p.fixed_duration_ms,
            p.sudden,
            p.hidden,
            p.lift,
            p.lane_cover,
            p.judge_offset_ms,
            p.visual_offset_ms,
            p.bga,
            p.show_fast_slow,
            r.expand_judge,
            r.judge_rates,
            p.judge_area,
            p.mark_notes,
            p.bpm_guide,
            p.timing_auto_adjust,
            r.hran_threshold_bpm,
            p.target,
            p.p2,
        );
        // newer fields only when set: files of before are written again
        // as they were
        let d = PlayConfig::default();
        macro_rules! set_fields {
            ($($f:ident),* $(,)?) => {
                $(match p.$f == d.$f {
                    true => st.skip_field(stringify!($f))?,
                    false => st.serialize_field(stringify!($f), &p.$f)?,
                })*
            };
        }
        set_fields!(hs_type, hs_level, hs_anim_ms);
        st.end()
    }
}

impl std::ops::Deref for PlayConfig {
    type Target = RuleOptions;

    fn deref(&self) -> &RuleOptions {
        &self.rules
    }
}

impl std::ops::DerefMut for PlayConfig {
    fn deref_mut(&mut self) -> &mut RuleOptions {
        &mut self.rules
    }
}

impl AsRef<RuleOptions> for PlayConfig {
    fn as_ref(&self) -> &RuleOptions {
        &self.rules
    }
}

impl Default for PlayConfig {
    fn default() -> Self {
        PlayConfig {
            rules: RuleOptions::default(),
            versus: Versus::Off,
            side: Side::P1,
            autoplay: false,
            hispeed_mode: HiSpeedMode::Floating,
            hispeed: 2.5,
            hs_fix: HsFix::Off,
            fixed_duration_ms: 600.0,
            hs_type: HsType::Classic,
            hs_level: DEFAULT_LEVEL,
            hs_anim_ms: 150,
            sudden: 0.0,
            hidden: 0.0,
            lift: 0.0,
            lane_cover: false,
            judge_offset_ms: 0,
            visual_offset_ms: 0,
            bga: true,
            show_fast_slow: true,
            judge_area: false,
            mark_notes: false,
            bpm_guide: false,
            timing_auto_adjust: false,
            target: Target::Best,
            p2: SideOptions::blank(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn targets_as_beatoraja() {
        // 1000 notes: max 2000
        assert_eq!(Target::RateA.score(2000, 0), 1334); // ceil(2000 * 18 / 27)
        assert_eq!(Target::RateAMinus.score(2000, 0), 1260);
        assert_eq!(Target::RateMaxMinus.score(2000, 0), 1926);
        assert_eq!(Target::RankNext.score(2000, 1334), 1408); // A -> A+
        assert_eq!(Target::RankNext.score(2000, 1999), 2000);
        assert_eq!(Target::Best.score(2000, 1500), 1500);
        // older names still load
        let c: Config = toml::from_str("[play]\ntarget = \"RankAA\"\n").unwrap();
        assert_eq!(c.play.target, Target::RateAA);
    }
}
