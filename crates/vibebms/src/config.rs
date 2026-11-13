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
    /// Green number fixed at the chart's main BPM (beatoraja "main BPM" floating).
    #[default]
    Floating,
    /// Notes move at constant speed regardless of BPM changes/stops.
    Constant,
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
    /// Visible duration of a note (ms) from lane top (below SUDDEN+) to the judge line.
    pub duration_ms: u32,
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
    /// Custom judge window rate (PG, GR, GD) in percent; `None` = default.
    pub custom_judge: Option<[i64; 3]>,
    /// H-RANDOM / ALL-SCR jack threshold in BPM (16th notes).
    pub hran_threshold_bpm: u32,
    pub target_rate: f32,
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
            duration_ms: 600,
            sudden: 0.0,
            hidden: 0.0,
            lift: 0.0,
            lane_cover: false,
            judge_offset_ms: 0,
            visual_offset_ms: 0,
            bga: true,
            show_fast_slow: true,
            custom_judge: None,
            hran_threshold_bpm: 120,
            target_rate: 0.0,
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
    /// Fraction of the screen height used by the controls.
    pub height: f32,
    /// Opacity of the controls.
    pub opacity: f32,
    /// Widest a key gets, as a fraction of the controls' height (wide
    /// landscape screens would otherwise spread the keys far apart).
    pub key_width: f32,
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
            height: 0.32,
            opacity: 0.85,
            key_width: 0.5,
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
            Ok(s) => match toml::from_str(&s) {
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
        let p: Config = toml::from_str("[play]\nduration_ms = 400\n").unwrap();
        assert_eq!(p.play.duration_ms, 400);
        assert_eq!(p.keys, KeyConfig::default());
    }
}
