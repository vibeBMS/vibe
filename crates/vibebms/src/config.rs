//! Persistent configuration (portable `config/config.toml`).
//!
//! Every field has a default so older/partial files keep working, and every
//! option is editable from the in-game settings screen.

use serde::{Deserialize, Serialize};
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::LnType;
use vibebms_ir::IrConfig;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Side {
    /// Scratch on the left, playfield on the left.
    #[default]
    P1,
    /// Scratch on the right, playfield on the right.
    P2,
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

/// Which BPM a fixed green number is kept at (beatoraja HS-FIX order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HsFix {
    /// The hi-speed is the setting.
    #[default]
    Off,
    Start,
    Max,
    Main,
    Min,
}

impl HsFix {
    pub const ALL: [HsFix; 5] = [
        HsFix::Off,
        HsFix::Start,
        HsFix::Max,
        HsFix::Main,
        HsFix::Min,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HsFix::Off => "OFF",
            HsFix::Start => "START BPM",
            HsFix::Max => "MAX BPM",
            HsFix::Main => "MAIN BPM",
            HsFix::Min => "MIN BPM",
        }
    }
}

/// A chart's BPMs, for hi-speed and green numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bpms {
    pub start: f64,
    pub min: f64,
    pub max: f64,
    pub main: f64,
}

impl Bpms {
    /// One BPM for all (a chart without changes, or a reference).
    pub fn flat(bpm: f64) -> Bpms {
        Bpms {
            start: bpm,
            min: bpm,
            max: bpm,
            main: bpm,
        }
    }

    pub fn of_meta(m: &vibebms_core::ChartMeta) -> Bpms {
        Bpms {
            start: m.initial_bpm,
            min: m.min_bpm,
            max: m.max_bpm,
            main: m.main_bpm,
        }
    }

    pub fn of_song(s: &crate::library::Song) -> Bpms {
        Bpms {
            start: if s.start_bpm > 0.0 {
                s.start_bpm
            } else {
                s.main_bpm
            },
            min: s.min_bpm,
            max: s.max_bpm,
            main: s.main_bpm,
        }
    }
}

fn valid(bpm: f64) -> f64 {
    if bpm > 0.0 && bpm.is_finite() {
        bpm
    } else {
        130.0
    }
}

/// Target score: beatoraja's targets (a share of the maximum in 27ths, MAX,
/// the next rank above the best) and the personal best.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Target {
    RateAMinus,
    #[serde(alias = "RankA")]
    RateA,
    RateAPlus,
    RateAAMinus,
    #[serde(alias = "RankAA")]
    RateAA,
    RateAAPlus,
    RateAAAMinus,
    #[serde(alias = "RankAAA")]
    RateAAA,
    RateAAAPlus,
    #[serde(alias = "MaxMinus")]
    RateMaxMinus,
    Max,
    /// The next DJ level step (in 27ths) above the best score.
    RankNext,
    #[default]
    Best,
}

impl Target {
    /// beatoraja's default target list order (skins index it), then BEST.
    pub const ALL: [Target; 13] = [
        Target::RateAMinus,
        Target::RateA,
        Target::RateAPlus,
        Target::RateAAMinus,
        Target::RateAA,
        Target::RateAAPlus,
        Target::RateAAAMinus,
        Target::RateAAA,
        Target::RateAAAPlus,
        Target::RateMaxMinus,
        Target::Max,
        Target::RankNext,
        Target::Best,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Target::RateAMinus => "RANK A-",
            Target::RateA => "RANK A",
            Target::RateAPlus => "RANK A+",
            Target::RateAAMinus => "RANK AA-",
            Target::RateAA => "RANK AA",
            Target::RateAAPlus => "RANK AA+",
            Target::RateAAAMinus => "RANK AAA-",
            Target::RateAAA => "RANK AAA",
            Target::RateAAAPlus => "RANK AAA+",
            Target::RateMaxMinus => "RANK MAX-",
            Target::Max => "MAX",
            Target::RankNext => "NEXT RANK",
            Target::Best => "BEST",
        }
    }

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlayConfig {
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
    pub side: Side,
    pub ln_mode: LnType,
    pub auto_scratch: bool,
    pub legacy_note: bool,
    pub no_mine: bool,
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
    /// EXPAND JUDGE (beatoraja custom judge): the judge windows scaled by
    /// `judge_rates` (PG, GR, GD in percent). Wider than 100 % is an assist
    /// (ASSIST EASY, the score is not saved).
    pub expand_judge: bool,
    pub judge_rates: [i64; 3],
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
    /// H-RANDOM / ALL-SCR jack threshold in BPM (16th notes).
    pub hran_threshold_bpm: u32,
    /// The score the pacemaker and the result compare against.
    pub target: Target,
}

/// Hi-speed limits and steps (as in LR2 / beatoraja).
pub const HISPEED_MIN: f32 = 0.25;
pub const HISPEED_MAX: f32 = 20.0;
pub const HISPEED_STEP: f32 = 0.25;
/// The BPM a duration from an old configuration is read at.
const LEGACY_BPM: f64 = 150.0;

impl PlayConfig {
    /// Share of the lane notes are visible in (below the lane cover, above
    /// the LIFT).
    pub fn visible(&self, with_cover: bool) -> f32 {
        let cover = if with_cover && (self.lane_cover || self.sudden > 0.0) {
            self.sudden
        } else {
            0.0
        };
        (1.0 - cover - self.lift).max(0.05)
    }

    /// The BPM the green number is shown (and with HS-FIX kept) at.
    pub fn basis(&self, b: &Bpms) -> f64 {
        valid(match self.hs_fix {
            HsFix::Off | HsFix::Main => b.main,
            HsFix::Start => b.start,
            HsFix::Max => b.max,
            HsFix::Min => b.min,
        })
    }

    /// The hi-speed a chart plays at: the setting, or with HS-FIX what
    /// keeps the green number.
    pub fn hispeed_at(&self, b: &Bpms) -> f32 {
        if self.hs_fix == HsFix::Off {
            return self.hispeed;
        }
        (240_000.0 * self.visible(true) as f64
            / (self.basis(b) * self.fixed_duration_ms.max(1.0) as f64)) as f32
    }

    /// How long (ms) a note is visible at `bpm` with the chart's hi-speed:
    /// the lane holds 4 / hi-speed beats.
    pub fn duration_at(&self, b: &Bpms, bpm: f64, with_cover: bool) -> f64 {
        240_000.0 * self.visible(with_cover) as f64
            / (valid(bpm) * self.hispeed_at(b).max(0.01) as f64)
    }

    /// The duration at the basis BPM (the one the green number is about).
    pub fn duration_ms(&self, b: &Bpms) -> f64 {
        self.duration_at(b, self.basis(b), true)
    }

    pub fn duration_ms_with(&self, b: &Bpms, with_cover: bool) -> f64 {
        self.duration_at(b, self.basis(b), with_cover)
    }

    /// The green number (beatoraja: the duration x 0.6).
    pub fn green(&self, b: &Bpms) -> i64 {
        (self.duration_ms(b) * 0.6).round() as i64
    }

    /// Change the hi-speed by `d` for a chart; with HS-FIX this sets the new
    /// green number.
    pub fn add_hispeed(&mut self, d: f32, b: &Bpms) {
        let hs =
            (((self.hispeed_at(b) + d) * 100.0).round() / 100.0).clamp(HISPEED_MIN, HISPEED_MAX);
        self.hispeed = hs;
        if self.hs_fix != HsFix::Off {
            self.fixed_duration_ms =
                (240_000.0 * self.visible(true) as f64 / (self.basis(b) * hs as f64)) as f32;
        }
    }

    /// Change the green number (the duration at the basis BPM) by `d` ms:
    /// beatoraja's duration keys. With HS-FIX this is the kept duration,
    /// otherwise the hi-speed that gives it.
    pub fn add_duration(&mut self, d: f64, b: &Bpms) {
        let vis = self.visible(true) as f64;
        let min = 240_000.0 * vis / (self.basis(b) * HISPEED_MAX as f64);
        let max = 240_000.0 * vis / (self.basis(b) * HISPEED_MIN as f64);
        let dur = (self.duration_ms(b) + d).clamp(min, max).max(1.0);
        if self.hs_fix != HsFix::Off {
            self.fixed_duration_ms = dur as f32;
        }
        self.hispeed = (240_000.0 * vis / (self.basis(b) * dur)) as f32;
    }

    /// Change HS-FIX, keeping the speed the chart `b` plays at.
    pub fn set_hs_fix(&mut self, fix: HsFix, b: &Bpms) {
        let hs = self.hispeed_at(b);
        if fix == HsFix::Off {
            if self.hs_fix != HsFix::Off {
                self.hispeed = (hs / HISPEED_STEP).round().max(1.0) * HISPEED_STEP;
            }
            self.hs_fix = fix;
        } else {
            self.hs_fix = fix;
            self.fixed_duration_ms =
                (240_000.0 * self.visible(true) as f64 / (self.basis(b) * hs as f64)) as f32;
        }
    }
}

impl Default for PlayConfig {
    fn default() -> Self {
        PlayConfig {
            gauge: GaugeType::Normal,
            gauge_auto_shift: GaugeAutoShift::None,
            gauge_preset: GaugePreset::Beatoraja,
            judge_preset: JudgePreset::Beatoraja,
            judge_algorithm: JudgeAlgorithm::Combo,
            random_1p: LaneOption::Off,
            random_2p: LaneOption::Off,
            dp_option: DoubleOption::Off,
            battle: false,
            side: Side::P1,
            ln_mode: LnType::Ln,
            auto_scratch: false,
            legacy_note: false,
            no_mine: false,
            autoplay: false,
            hispeed_mode: HiSpeedMode::Floating,
            hispeed: 2.5,
            hs_fix: HsFix::Off,
            fixed_duration_ms: 600.0,
            sudden: 0.0,
            hidden: 0.0,
            lift: 0.0,
            lane_cover: false,
            judge_offset_ms: 0,
            visual_offset_ms: 0,
            bga: true,
            show_fast_slow: true,
            expand_judge: false,
            judge_rates: [400, 400, 100],
            judge_area: false,
            mark_notes: false,
            bpm_guide: false,
            timing_auto_adjust: false,
            hran_threshold_bpm: 120,
            target: Target::Best,
        }
    }
}

/// One lane binding: up to a few keyboard keys and gamepad buttons.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LaneKeys {
    pub keys: Vec<KeyCode>,
    /// gilrs raw button codes, prefixed with the pad index: `pad * 1_000_000 + code`.
    pub pad: Vec<u64>,
}

impl LaneKeys {
    fn k(keys: &[KeyCode]) -> Self {
        LaneKeys {
            keys: keys.to_vec(),
            pad: Vec::new(),
        }
    }
}

/// Bindings for one player side: 9 key slots (7K uses 0..7, PMS uses 0..9)
/// plus two scratch directions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SideKeys {
    pub keys: Vec<LaneKeys>,
    pub scratch_up: LaneKeys,
    pub scratch_down: LaneKeys,
    /// gilrs axis codes used as turntable (`pad * 1_000_000 + code`).
    pub scratch_axis: Vec<u64>,
}

impl Default for SideKeys {
    fn default() -> Self {
        SideKeys {
            keys: vec![LaneKeys::default(); 9],
            scratch_up: LaneKeys::default(),
            scratch_down: LaneKeys::default(),
            scratch_axis: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KeyConfig {
    pub p1: SideKeys,
    pub p2: SideKeys,
    /// pop'n 9 button layout.
    pub pms: Vec<LaneKeys>,
    pub start: LaneKeys,
    pub select: LaneKeys,
    /// Any gamepad axis movement counts as scratch when no axis is bound.
    pub any_axis_is_scratch: bool,
}

impl Default for KeyConfig {
    fn default() -> Self {
        use KeyCode::*;
        let side = |keys: [KeyCode; 7], up: KeyCode, down: KeyCode| {
            let mut s = SideKeys::default();
            for (i, k) in keys.iter().enumerate() {
                s.keys[i] = LaneKeys::k(&[*k]);
            }
            s.scratch_up = LaneKeys::k(&[up]);
            s.scratch_down = LaneKeys::k(&[down]);
            s
        };
        KeyConfig {
            p1: side(
                [KeyZ, KeyS, KeyX, KeyD, KeyC, KeyF, KeyV],
                ShiftLeft,
                ControlLeft,
            ),
            p2: side(
                [KeyM, KeyK, Comma, KeyL, Period, Semicolon, Slash],
                ShiftRight,
                ControlRight,
            ),
            pms: [KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL]
                .iter()
                .map(|k| LaneKeys::k(&[*k]))
                .collect(),
            start: LaneKeys::k(&[KeyQ]),
            select: LaneKeys::k(&[KeyW]),
            any_axis_is_scratch: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TouchMode {
    /// On for phones/tablets, and on desktop as soon as a touch is seen.
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TouchStyle {
    /// A row of equally sized keys (keyboard-like).
    Keyboard,
    /// Turntable + 7 buttons in two rows like an IIDX controller.
    #[default]
    Iidx,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TouchConfig {
    pub mode: TouchMode,
    pub style: TouchStyle,
    /// Fraction of the screen height used by the controls, in landscape
    /// (the default skin's touch layout fills the rest on a 4:3 tablet) and
    /// in portrait.
    pub landscape_height: f32,
    pub portrait_height: f32,
    /// Opacity of the controls.
    pub opacity: f32,
    /// The screen's diagonal in inches, for controls at their real size
    /// (0: guessed from the kind of device).
    pub screen_inches: f32,
    /// Play double charts in landscape with two control sets.
    pub allow_dp: bool,
    /// Scratch sensitivity: degrees of rotation needed to register a direction change.
    pub scratch_threshold_deg: f32,
}

impl Default for TouchConfig {
    fn default() -> Self {
        TouchConfig {
            mode: TouchMode::Auto,
            style: TouchStyle::Iidx,
            landscape_height: 0.45,
            portrait_height: 0.32,
            opacity: 0.85,
            screen_inches: 0.0,
            allow_dp: true,
            scratch_threshold_deg: 6.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PresentMode {
    /// Lowest latency available (Mailbox > Immediate > VSync).
    #[default]
    LowLatency,
    VSync,
    Immediate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayConfig {
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    /// 0 = unlimited.
    pub max_fps: u32,
    pub ui_scale: f32,
    pub show_fps: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            fullscreen: false,
            width: 1280,
            height: 720,
            present_mode: PresentMode::LowLatency,
            max_fps: 0,
            ui_scale: 1.0,
            show_fps: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSettings {
    pub buffer_frames: u32,
    pub sample_rate: u32,
    pub master: f32,
    pub key: f32,
    pub bgm: f32,
    pub system: f32,
    pub preview: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings {
            buffer_frames: 256,
            sample_rate: 0,
            master: 0.8,
            key: 1.0,
            bgm: 1.0,
            system: 0.7,
            preview: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TableConfig {
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        TableConfig {
            name: String::new(),
            url: String::new(),
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub player_name: String,
    /// Extra BMS folders (absolute or relative to the executable directory).
    pub bms_dirs: Vec<String>,
    /// Scan the executable directory itself.
    pub scan_root: bool,
    pub play: PlayConfig,
    pub keys: KeyConfig,
    pub touch: TouchConfig,
    pub display: DisplayConfig,
    pub audio: AudioSettings,
    pub tables: Vec<TableConfig>,
    /// Indexes of mirrored event entries (shown as folders; see `archive`).
    pub archives: Vec<String>,
    /// Internet ranking server (an older `ir` list is ignored).
    pub ir_server: IrConfig,
    /// Play skin per key mode ("7KEYS" -> path of an .lr2skin, relative to the data folder).
    pub skins: std::collections::BTreeMap<String, String>,
    /// Skin custom options: skin (as in `skins`) -> option name -> chosen label.
    pub skin_options:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Skin offsets: skin -> offset name -> x, y, w, h, angle, alpha.
    pub skin_offsets:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, [i32; 6]>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            player_name: "PLAYER".into(),
            bms_dirs: Vec::new(),
            scan_root: true,
            play: PlayConfig::default(),
            keys: KeyConfig::default(),
            touch: TouchConfig::default(),
            display: DisplayConfig::default(),
            audio: AudioSettings::default(),
            tables: vibebms_tables::DEFAULT_TABLES
                .iter()
                .map(|(n, u)| TableConfig {
                    name: n.to_string(),
                    url: u.to_string(),
                    enabled: true,
                })
                .collect(),
            archives: crate::archive::DEFAULT_INDEXES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            ir_server: IrConfig::default(),
            skins: Default::default(),
            skin_options: Default::default(),
            skin_offsets: Default::default(),
        }
    }
}

impl Config {
    pub fn load(path: &std::path::Path) -> Config {
        match crate::platform::vfs::read_to_string(path) {
            Ok(s) => match toml::from_str::<toml::Value>(&s).and_then(|mut v| {
                migrate(&mut v);
                v.try_into::<Config>()
            }) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("config parse error ({e}); backing up and using defaults");
                    if let Ok(b) = crate::platform::vfs::read(path) {
                        let _ = crate::platform::vfs::write(&path.with_extension("toml.bak"), &b);
                    }
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self, path: &std::path::Path) {
        match toml::to_string_pretty(self) {
            Ok(s) => {
                if let Err(e) = crate::platform::vfs::write(path, s.as_bytes()) {
                    log::error!("saving config: {e}");
                }
            }
            Err(e) => log::error!("config serialize error: {e}"),
        }
    }
}

/// Older configurations stored the duration (ms) instead of the hi-speed:
/// keep the speed it gave at 150 BPM.
fn migrate(v: &mut toml::Value) {
    let Some(play) = v.get_mut("play").and_then(|p| p.as_table_mut()) else {
        return;
    };
    // custom_judge = [pg, gr, gd] became expand_judge + judge_rates
    if let Some(rates) = play.remove("custom_judge") {
        if rates.as_array().is_some_and(|a| a.len() == 3) {
            play.insert("expand_judge".into(), toml::Value::Boolean(true));
            play.insert("judge_rates".into(), rates);
        }
    }
    if play.contains_key("hispeed") {
        return;
    }
    let Some(ms) = play.remove("duration_ms").and_then(|d| d.as_integer()) else {
        return;
    };
    let f = |k: &str| play.get(k).and_then(|x| x.as_float()).unwrap_or(0.0);
    let cover = if play
        .get("lane_cover")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
        || f("sudden") > 0.0
    {
        f("sudden")
    } else {
        0.0
    };
    let visible = (1.0 - cover - f("lift")).max(0.05);
    let hs = 240_000.0 * visible / (LEGACY_BPM * ms.max(1) as f64);
    let hs = ((hs / HISPEED_STEP as f64).round() * HISPEED_STEP as f64)
        .clamp(HISPEED_MIN as f64, HISPEED_MAX as f64);
    play.insert("hispeed".into(), toml::Value::Float(hs));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_toml() {
        let c = Config::default();
        let s = toml::to_string_pretty(&c).unwrap();
        let d: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, d);
        // partial files fill in defaults
        let mut v: toml::Value = toml::from_str("[play]\nduration_ms = 400\n").unwrap();
        migrate(&mut v);
        let p: Config = v.try_into().unwrap();
        // 400 ms at 150 BPM is hi-speed 4.0
        assert_eq!(p.play.hispeed, 4.0);
        assert!((p.play.duration_ms(&Bpms::flat(150.0)) - 400.0).abs() < 0.5);
        assert_eq!(p.keys, KeyConfig::default());
    }

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

    #[test]
    fn custom_judge_becomes_expand_judge() {
        let mut v: toml::Value =
            toml::from_str("[play]\ncustom_judge = [200, 150, 100]\n").unwrap();
        migrate(&mut v);
        let c: Config = v.try_into().unwrap();
        assert!(c.play.expand_judge);
        assert_eq!(c.play.judge_rates, [200, 150, 100]);
    }

    #[test]
    fn fixed_green_number() {
        let mut p = PlayConfig {
            hispeed: 2.0,
            ..Default::default()
        };
        let at150 = Bpms::flat(150.0);
        // 240000 / (150 x 2) = 800 ms at 150 BPM
        p.set_hs_fix(HsFix::Main, &at150);
        assert!((p.fixed_duration_ms - 800.0).abs() < 0.5);
        // a 300 BPM chart plays at hi-speed 1.0, same green number
        let at300 = Bpms::flat(300.0);
        assert!((p.hispeed_at(&at300) - 1.0).abs() < 1e-4);
        assert_eq!(p.green(&at300), p.green(&at150));
        // SUDDEN+ keeps the green number: the hi-speed drops
        p.lane_cover = true;
        p.sudden = 0.5;
        assert_eq!(p.green(&at150), 480);
        assert!((p.hispeed_at(&at150) - 1.0).abs() < 1e-4);
        // hi-speed +0.25 at 150 BPM sets a new green number
        p.add_hispeed(0.25, &at150);
        assert!((p.hispeed_at(&at150) - 1.25).abs() < 1e-3);
        assert!((p.duration_ms(&at150) - 640.0).abs() < 0.5);
        // MAX BPM: a 100..200 chart keeps the green number at 200
        let b = Bpms {
            start: 100.0,
            min: 100.0,
            max: 200.0,
            main: 100.0,
        };
        p.hs_fix = HsFix::Max;
        assert!((p.hispeed_at(&b) - 1.25 * 150.0 / 200.0).abs() < 1e-3);
        // back to OFF keeps the speed, on the 0.25 grid
        p.set_hs_fix(HsFix::Off, &at150);
        assert_eq!(p.hispeed, 1.25);
    }
}
