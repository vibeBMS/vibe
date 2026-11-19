//! Persistent configuration (portable `config/config.toml`).
//!
//! Every field has a default so older/partial files keep working, and every
//! option is editable from the in-game settings screen.

mod hispeed;
mod ir;
mod keys;
mod migrate;
mod play;
mod practice;
mod select;
mod streaming;
mod touch;

pub use hispeed::{Bpms, HISPEED_MAX, HISPEED_MIN, HISPEED_STEP};
pub use ir::{provider_name, Beside, IrAccount, IrSettings, OldToken, PlayerKey, SubmitPolicy};
pub use keys::{InputTuning, KeyConfig, LaneKeys, PadId, PadSides, SideKeys};
pub use play::{
    BgaMode, BgaSize, Ghost, HiSpeedMode, HsFix, HsType, PlayConfig, Side, SideOptions, Target,
    Versus, NORMAL_GREENS,
};
pub use practice::PracticeConfig;
pub use select::{
    DifficultyFilter, KeyFilter, RandomSelect, ReplayPick, SelectConfig, SortMode, WATCH_SPEED_MAX,
    WATCH_SPEED_MIN,
};
pub use streaming::{DiscordConfig, ObsConfig, ObsScenes};
pub use touch::{
    PerspectiveConfig, TouchButton, TouchConfig, TouchLayout, TouchMode, TouchSide, TouchStyle,
    LENGTH_RANGE, OLD_TOUCH_HEIGHTS, PERSPECTIVE_LAYOUT, PERSPECTIVE_MAX, TOUCH_HEIGHT_RANGE,
};

use migrate::migrate;
use serde::{Deserialize, Serialize};

/// How frames reach the screen (`gfx::device::choose_present_mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PresentMode {
    /// "Smooth": vsync (FIFO) with two frames queued, each drawn for when
    /// it will be shown: even motion at the display's rate, about a frame
    /// more from a key to the screen.
    #[default]
    VSync,
    /// "Low latency": the newest frame at each vblank, without tearing
    /// (mailbox); where the GPU has none (Apple's Metal, some drivers),
    /// the same as Smooth.
    LowLatency,
    /// "Unlimited": as many frames as the GPU makes, shown at once
    /// (immediate, tearing), else mailbox, else Smooth.
    Immediate,
}

impl PresentMode {
    pub fn label(self) -> &'static str {
        match self {
            PresentMode::VSync => "SMOOTH (VSYNC, BUFFERED)",
            PresentMode::LowLatency => "LOW LATENCY",
            PresentMode::Immediate => "UNLIMITED (TEARING)",
        }
    }
}

/// The graphics API the GPU is driven through (`gfx::device`; desktop,
/// Android). AUTO lets wgpu choose among the ones the platform has
/// (Vulkan, Metal, DX12, then OpenGL), as before the setting existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GraphicsBackend {
    #[default]
    Auto,
    Vulkan,
    Metal,
    Dx12,
    OpenGl,
}

impl GraphicsBackend {
    /// The ones that can be chosen, in the order AUTO prefers them.
    pub const APIS: [GraphicsBackend; 4] = [
        GraphicsBackend::Vulkan,
        GraphicsBackend::Metal,
        GraphicsBackend::Dx12,
        GraphicsBackend::OpenGl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            GraphicsBackend::Auto => "AUTO",
            GraphicsBackend::Vulkan => "VULKAN",
            GraphicsBackend::Metal => "METAL",
            GraphicsBackend::Dx12 => "DX12",
            GraphicsBackend::OpenGl => "OPENGL",
        }
    }

    pub fn is_auto(&self) -> bool {
        *self == GraphicsBackend::Auto
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DisplayConfig {
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    /// Read at start: a change applies at the next launch. One that cannot
    /// start is replaced by the one that did.
    #[serde(skip_serializing_if = "GraphicsBackend::is_auto")]
    pub backend: GraphicsBackend,
    /// 0 = unlimited.
    pub max_fps: u32,
    pub ui_scale: f32,
    pub show_fps: bool,
    /// The latency overlay (`app::diag`); written only when on.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub show_latency: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            fullscreen: false,
            width: 1280,
            height: 720,
            present_mode: PresentMode::VSync,
            backend: GraphicsBackend::Auto,
            max_fps: 0,
            ui_scale: 1.0,
            show_fps: false,
            show_latency: false,
        }
    }
}

/// The loudness normalised sounds play at by default (LUFS).
pub const NORMALIZE_TARGET: f32 = -12.0;

fn is_true(b: &bool) -> bool {
    *b
}

fn is_normal_target(t: &f32) -> bool {
    *t == NORMALIZE_TARGET
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
    /// The song select plays a chart's preview (its own, else one made of
    /// its sounds): once, then the select BGM again, or over and over.
    pub preview: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preview_once: bool,
    /// Silent while the app is not the one in use (`app::activity`).
    #[serde(skip_serializing_if = "is_true")]
    pub mute_inactive: bool,
    /// The chart's sounds and the song preview played at a common loudness,
    /// `normalize_target` LUFS (`loudness`).
    #[serde(skip_serializing_if = "is_true")]
    pub normalize: bool,
    #[serde(skip_serializing_if = "is_normal_target")]
    pub normalize_target: f32,
    /// The sound set's music (select, decide, `.loop` files) too.
    #[serde(skip_serializing_if = "is_true")]
    pub normalize_system: bool,
    /// Windows: the device in WASAPI exclusive mode (else shared).
    #[cfg(windows)]
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exclusive: bool,
}

impl AudioSettings {
    /// The mixer's settings from these, with `max_voices` voices.
    pub fn mixer_config(&self, max_voices: usize) -> vibebms_audio::AudioConfig {
        vibebms_audio::AudioConfig {
            buffer_frames: self.buffer_frames,
            sample_rate: self.sample_rate,
            master: self.master,
            key_volume: self.key,
            bgm_volume: self.bgm,
            system_volume: self.system,
            max_voices,
            #[cfg(windows)]
            exclusive: self.exclusive,
        }
    }
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
            preview_once: false,
            mute_inactive: true,
            normalize: true,
            normalize_target: NORMALIZE_TARGET,
            normalize_system: true,
            #[cfg(windows)]
            exclusive: false,
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

/// Whether third parties' mirrors are on by default: a build made with
/// `VIBEBMS_THIRD_PARTY=1` (the web build's CI, from a repository
/// variable) turns them on for those who have not chosen.
const THIRD_PARTY_DEFAULT: bool = match option_env!("VIBEBMS_THIRD_PARTY") {
    Some(v) => matches!(v.as_bytes(), b"1" | b"true"),
    None => false,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub player_name: String,
    /// The UI's language (app-wide; AUTO: the system's).
    #[serde(skip_serializing_if = "crate::i18n::Language::is_auto")]
    pub language: crate::i18n::Language,
    /// Extra BMS folders (absolute or relative to the executable directory).
    pub bms_dirs: Vec<String>,
    /// Scan the executable directory itself.
    pub scan_root: bool,
    pub play: PlayConfig,
    /// Song select's order and filters (written once changed).
    #[serde(skip_serializing_if = "SelectConfig::is_default")]
    pub select: SelectConfig,
    pub keys: KeyConfig,
    pub touch: TouchConfig,
    /// The touch perspective play skin.
    pub perspective: PerspectiveConfig,
    pub display: DisplayConfig,
    pub audio: AudioSettings,
    pub tables: Vec<TableConfig>,
    /// Indexes of mirrored event entries (shown as folders; see `archive`).
    pub archives: Vec<String>,
    /// Downloads may use third parties' copies: other people's mirrors of
    /// dead sites and uploaders, the community archive (bmseeker's
    /// `third_party`).
    pub third_party_mirrors: bool,
    /// The player has read what turning on third parties' copies means
    /// and said OK (app-wide; written once set): turning them on in the
    /// settings asks first until then.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub third_party_acknowledged: bool,
    /// A song downloaded into the library gets a folder named after its
    /// charts (`[artist] title`, as BeMusicSeeker names one), not after
    /// the song's title (bmseeker's `folder_naming`).
    pub name_song_folders: bool,
    /// Internet ranking: accounts and when plays are sent.
    pub ir: IrSettings,
    /// Look for a newer nightly build on start (desktop CI builds; see
    /// `update`).
    pub check_updates: bool,
    /// Play skin per key mode ("7KEYS" -> path of an .lr2skin, relative to the data folder).
    pub skins: std::collections::BTreeMap<String, String>,
    /// Skin custom options: skin (as in `skins`) -> option name -> chosen label.
    pub skin_options:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// Skin offsets: skin -> offset name -> x, y, w, h, angle, alpha.
    pub skin_offsets:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, [i32; 6]>>,
    /// Practice mode's settings (written once changed).
    #[serde(skip_serializing_if = "PracticeConfig::is_default")]
    pub practice: PracticeConfig,
    /// RANDOM MIX's settings (written once changed).
    #[serde(skip_serializing_if = "crate::courses::RandomMix::is_default")]
    pub random_mix: crate::courses::RandomMix,
    /// Discord Rich Presence (desktop; written once changed).
    #[serde(skip_serializing_if = "DiscordConfig::is_default")]
    pub discord: DiscordConfig,
    /// OBS WebSocket (desktop; written once changed).
    #[serde(skip_serializing_if = "ObsConfig::is_default")]
    pub obs: ObsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            player_name: "PLAYER".into(),
            language: Default::default(),
            bms_dirs: Vec::new(),
            scan_root: true,
            play: PlayConfig::default(),
            select: SelectConfig::default(),
            keys: KeyConfig::default(),
            touch: TouchConfig::default(),
            perspective: PerspectiveConfig::default(),
            display: DisplayConfig::default(),
            audio: AudioSettings::default(),
            tables: bmseeker::table::DEFAULT_TABLES
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
            third_party_mirrors: THIRD_PARTY_DEFAULT,
            third_party_acknowledged: false,
            name_song_folders: false,
            ir: IrSettings::default(),
            check_updates: true,
            skins: Default::default(),
            skin_options: Default::default(),
            skin_offsets: Default::default(),
            practice: Default::default(),
            random_mix: Default::default(),
            discord: Default::default(),
            obs: Default::default(),
        }
    }
}

impl Config {
    pub fn load(path: &std::path::Path) -> Config {
        let Ok(s) = crate::platform::vfs::read_to_string(path) else {
            return Config::default();
        };
        let parsed = toml::from_str::<toml::Value>(&s).map(|mut v| {
            migrate(&mut v);
            v
        });
        let e = match parsed {
            Ok(v) => match v.clone().try_into::<Config>() {
                Ok(c) => return c,
                // a value of the wrong type (a setting that changed type):
                // keep everything else
                Err(e) => {
                    log::error!("config: {e}; the settings that cannot be read are reset");
                    backup(path);
                    return Config::lenient(&v);
                }
            },
            Err(e) => e,
        };
        log::error!("config parse error ({e}); backing up and using defaults");
        backup(path);
        Config::default()
    }

    /// The settings of `v` that can be read, the defaults for the others:
    /// each value is taken if the whole still reads (a table's values one
    /// by one when it does not).
    pub fn lenient(v: &toml::Value) -> Config {
        fn reads(v: &toml::Value) -> bool {
            v.clone().try_into::<Config>().is_ok()
        }
        fn table_at<'a>(root: &'a mut toml::Value, path: &[String]) -> Option<&'a mut toml::Table> {
            let mut cur = root;
            for p in path {
                cur = cur.as_table_mut()?.get_mut(p)?;
            }
            cur.as_table_mut()
        }
        // `user`'s values into the table at `path`
        fn merge(root: &mut toml::Value, path: &mut Vec<String>, user: &toml::Value) {
            let Some(t) = user.as_table() else {
                return;
            };
            for (k, uv) in t {
                let Some(table) = table_at(root, path) else {
                    return;
                };
                let old = table.insert(k.clone(), uv.clone());
                if reads(root) {
                    continue;
                }
                let Some(table) = table_at(root, path) else {
                    return;
                };
                match old {
                    Some(o) => {
                        let both = o.is_table() && uv.is_table();
                        table.insert(k.clone(), o);
                        if both {
                            path.push(k.clone());
                            merge(root, path, uv);
                            path.pop();
                        }
                    }
                    None => {
                        table.remove(k);
                    }
                }
            }
        }
        let mut root = toml::Value::try_from(Config::default())
            .unwrap_or(toml::Value::Table(Default::default()));
        merge(&mut root, &mut Vec::new(), v);
        root.try_into().unwrap_or_default()
    }

    /// The file's text. Through a `toml::Value` so that an `f32` is written
    /// as the `f64` it widens to (`0.800000011920929`), as the files
    /// written so far have it (toml 1 writes `0.8`).
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(&toml::Value::try_from(self)?)
    }

    pub fn save(&self, path: &std::path::Path) {
        match self.to_toml() {
            Ok(s) => {
                if let Err(e) = crate::platform::vfs::write(path, s.as_bytes()) {
                    log::error!("saving config: {e}");
                }
            }
            Err(e) => log::error!("config serialize error: {e}"),
        }
    }
}

/// Keep a copy of a configuration that could not be read whole.
fn backup(path: &std::path::Path) {
    if let Ok(b) = crate::platform::vfs::read(path) {
        let _ = crate::platform::vfs::write(&path.with_extension("toml.bak"), &b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibebms_core::engine::JudgeAlgorithm;
    use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
    use vibebms_core::judge::JudgePreset;
    use vibebms_core::lane_option::{DoubleOption, LaneOption};
    use vibebms_core::play::RuleOptions;
    use vibebms_core::LnType;
    use winit::keyboard::KeyCode;

    /// Every setting away from its default.
    fn full_config() -> Config {
        use std::collections::BTreeMap;
        let lk = |k: KeyCode, pad: u64| LaneKeys {
            keys: vec![k, KeyCode::F1],
            pad: vec![pad],
        };
        let side = |base: u64| SideKeys {
            keys: (0..9).map(|i| lk(KeyCode::Digit1, base + i)).collect(),
            scratch_up: lk(KeyCode::ArrowUp, base + 20),
            scratch_down: lk(KeyCode::ArrowDown, base + 21),
            scratch_axis: vec![base + 30, base + 31],
        };
        Config {
            player_name: "TESTER".into(),
            language: Default::default(),
            bms_dirs: vec!["/music/bms".into(), "relative/dir".into()],
            scan_root: false,
            play: PlayConfig {
                rules: RuleOptions {
                    gauge: GaugeType::ExHard,
                    gauge_auto_shift: GaugeAutoShift::BestClear,
                    gauge_preset: GaugePreset::Lr2,
                    judge_preset: JudgePreset::Iidx,
                    judge_algorithm: JudgeAlgorithm::Duration,
                    random_1p: LaneOption::SRandom,
                    random_2p: LaneOption::Mirror,
                    dp_option: DoubleOption::Flip,
                    battle: true,
                    ln_mode: LnType::Hcn,
                    ln_force: false,
                    auto_scratch: true,
                    legacy_note: true,
                    no_mine: true,
                    expand_judge: true,
                    judge_rates: [200, 150, 75],
                    hran_threshold_bpm: 150,
                    // (newer fields at their defaults: written as before)
                    scroll_remove: false,
                    mine_add: Default::default(),
                    ln_add: Default::default(),
                    s_random_interval: Default::default(),
                    trainer_order: "1234567".into(),
                    freq: 0,
                    course: None,
                    key_convert: Default::default(),
                },
                versus: Versus::Off,
                side: Side::P2,
                autoplay: true,
                hispeed_mode: HiSpeedMode::Constant,
                hispeed: 3.75,
                hs_fix: HsFix::Max,
                fixed_duration_ms: 512.5,
                hs_type: HsType::Lr2,
                hs_level: 7,
                hs_anim_ms: 300,
                sudden: 0.25,
                hidden: 0.125,
                lift: 0.0625,
                lane_cover: true,
                judge_offset_ms: -12,
                visual_offset_ms: 7,
                bga: BgaMode::Off,
                show_fast_slow: false,
                // (newer fields at their defaults: written as before)
                bga_size: Default::default(),
                fast_slow_ms: 0,
                guide_se: false,
                result_wait: false,
                good_clears_poor: false,
                miss_keysound: false,
                hold_at_line: false,
                score_graph: true,
                ghost: Default::default(),
                judge_area: true,
                mark_notes: true,
                bpm_guide: true,
                timing_auto_adjust: true,
                target: Target::RateAAPlus,
                target_rival: Some(PlayerKey {
                    provider: vibebms_ir::ProviderId::BmsIr,
                    player: "100005".into(),
                }),
                target_profile: None,
                p2: SideOptions {
                    gauge: GaugeType::Hazard,
                    hispeed: 1.5,
                    hs_fix: HsFix::Start,
                    fixed_duration_ms: 400.0,
                    sudden: 0.5,
                    hidden: 0.25,
                    lift: 0.125,
                    lane_cover: true,
                    auto_scratch: true,
                    judge_offset_ms: 9,
                    hs_level: 12,
                },
            },
            select: SelectConfig {
                sort: SortMode::MissCount,
                key_filter: KeyFilter::K14,
                difficulty_filter: DifficultyFilter::Another,
                random: RandomSelect::NotHard,
                replay: ReplayPick::Slot3,
                replay_speed: 175,
            },
            keys: KeyConfig {
                p1: side(0),
                p2: side(1_000_000),
                pms: (0..9).map(|i| lk(KeyCode::KeyA, 100 + i)).collect(),
                // (newer fields at their defaults: written as before)
                k4: KeyConfig::default().k4,
                k6: KeyConfig::default().k6,
                k8: KeyConfig::default().k8,
                start: lk(KeyCode::Enter, 200),
                select: lk(KeyCode::Space, 201),
                p2_start: lk(KeyCode::Numpad1, 202),
                p2_select: lk(KeyCode::Numpad2, 203),
                any_axis_is_scratch: false,
                tuning: InputTuning {
                    turntable_dead_band: 12,
                    turntable_stop_ms: 150,
                    chatter_ms: 8,
                },
                controllers: Default::default(),
            },
            touch: TouchConfig {
                mode: TouchMode::On,
                style: TouchStyle::Custom,
                landscape_height: 0.5,
                portrait_height: 0.25,
                opacity: 0.5,
                screen_inches: 10.5,
                allow_dp: false,
                scratch_threshold_deg: 4.5,
                start_select_side: TouchSide::Right,
                custom_from: TouchStyle::Keyboard,
                layouts: BTreeMap::from([(
                    "7KEYS landscape".into(),
                    TouchLayout {
                        buttons: vec![
                            TouchButton {
                                lane: 7,
                                x: -0.05,
                                y: 0.0,
                                w: 0.25,
                                h: 1.0,
                                round: true,
                            },
                            TouchButton {
                                lane: 0,
                                x: 0.3,
                                y: 0.55,
                                w: 0.08,
                                h: 0.4,
                                round: false,
                            },
                        ],
                    },
                )]),
            },
            perspective: PerspectiveConfig {
                strength: 0.4,
                length: 0.8,
                touch_height: 0.25,
                layout: PERSPECTIVE_LAYOUT,
            },
            display: DisplayConfig {
                fullscreen: true,
                width: 1920,
                height: 1080,
                present_mode: PresentMode::VSync,
                backend: GraphicsBackend::Auto,
                max_fps: 144,
                ui_scale: 1.25,
                show_fps: true,
                show_latency: false,
            },
            audio: AudioSettings {
                buffer_frames: 128,
                sample_rate: 48000,
                master: 0.5,
                key: 0.75,
                bgm: 0.25,
                system: 0.125,
                preview: false,
                preview_once: true,
                mute_inactive: true,
                normalize: true,
                normalize_target: NORMALIZE_TARGET,
                normalize_system: true,
                #[cfg(windows)]
                exclusive: false,
            },
            tables: vec![
                TableConfig {
                    name: "Mine".into(),
                    url: "https://example.org/table.html".into(),
                    enabled: false,
                },
                TableConfig {
                    name: "Other".into(),
                    url: "https://example.org/other/".into(),
                    enabled: true,
                },
            ],
            archives: vec!["https://example.org/events/index.json".into()],
            third_party_mirrors: !THIRD_PARTY_DEFAULT,
            // (written only once set: `third_party_acknowledgement_is_written_once_set`)
            third_party_acknowledged: false,
            name_song_folders: true,
            ir: IrSettings {
                primary: Some(vibebms_ir::ProviderId::Bokutachi),
                submit: SubmitPolicy::Ask,
                accounts: vec![
                    IrAccount {
                        name: "Test IR".into(),
                        url: "https://ir.example.org/api".into(),
                        submit: false,
                        user: "TESTER".into(),
                        user_id: "7".into(),
                        ..Default::default()
                    },
                    IrAccount {
                        provider: vibebms_ir::ProviderId::Bokutachi,
                        enabled: false,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            check_updates: false,
            skins: BTreeMap::from([
                ("7KEYS".into(), "skins/a/play7.lr2skin".into()),
                ("SOUND_SET".into(), "sounds/x".into()),
            ]),
            skin_options: BTreeMap::from([(
                "skins/a/play7.lr2skin".into(),
                BTreeMap::from([("LANE".into(), "BLACK".into())]),
            )]),
            skin_offsets: BTreeMap::from([(
                "skins/a/play7.lr2skin".into(),
                BTreeMap::from([("JUDGE".into(), [1, -2, 3, 4, 5, 255])]),
            )]),
            // (written only once changed: `practice_settings_roundtrip`)
            practice: Default::default(),
            random_mix: Default::default(),
            discord: Default::default(),
            obs: Default::default(),
        }
    }

    /// A configuration as written now (`tests/golden/config_accounts.toml`,
    /// and the play options as a replay stores them, `play_config.json`)
    /// reads back as it was and is written again with the same keys and
    /// values (`[play]`'s rules come first since they are derived: files
    /// of before had them mixed with the view's). `GOLDEN_BLESS=1` writes
    /// the files anew.
    #[test]
    fn full_config_stays_the_same() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        let (toml_path, json_path) = (
            dir.join("config_accounts.toml"),
            dir.join("play_config.json"),
        );
        let c = full_config();
        if std::env::var_os("GOLDEN_BLESS").is_some() {
            std::fs::write(&toml_path, c.to_toml().unwrap()).unwrap();
            std::fs::write(&json_path, serde_json::to_vec(&c.play).unwrap()).unwrap();
        }
        let s = std::fs::read_to_string(&toml_path).unwrap();
        let read: Config = toml::from_str(&s).unwrap();
        assert_eq!(read, c);
        let toml = |s: &str| s.parse::<toml::Table>().unwrap();
        assert_eq!(toml(&read.to_toml().unwrap()), toml(&s));
        assert_eq!(Config::load(&toml_path), c);
        let j = std::fs::read(&json_path).unwrap();
        let play: PlayConfig = serde_json::from_slice(&j).unwrap();
        assert_eq!(play, c.play);
        let json = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).unwrap();
        assert_eq!(json(&serde_json::to_vec(&play).unwrap()), json(&j));
    }

    /// Every play option away from its default (KEY CONVERT and a course
    /// stage's kind too) comes back from the configuration file and from a
    /// replay.
    #[test]
    fn every_play_option_comes_back() {
        use vibebms_core::lane_option::{LnAdd, MineAdd, SRandomInterval};
        let mut c = full_config();
        let p = &mut c.play;
        p.rules.ln_force = true;
        p.rules.scroll_remove = true;
        p.rules.mine_add = MineAdd::Near;
        p.rules.ln_add = LnAdd::Cn;
        p.rules.s_random_interval = SRandomInterval::Ms40;
        p.rules.trainer_order = "7654321".into();
        p.rules.freq = -3;
        p.rules.course = Some(vibebms_core::course::CourseKind::Class);
        p.rules.key_convert = vibebms_core::convert::KeyConvert::To9;
        p.bga_size = BgaSize::Extend;
        p.fast_slow_ms = 12;
        (p.guide_se, p.result_wait, p.good_clears_poor) = (true, true, true);
        (p.miss_keysound, p.hold_at_line, p.score_graph) = (true, true, false);
        p.ghost = Ghost::TypeB;
        p.target_profile = Some("RIVAL".into());
        // (every option written, each off its default)
        let json = |p: &PlayConfig| serde_json::to_value(p).unwrap();
        let (set, d) = (json(&c.play), json(&PlayConfig::default()));
        for (k, v) in set.as_object().unwrap() {
            assert_ne!(Some(v), d.get(k), "{k}");
        }
        assert!(d.as_object().unwrap().keys().all(|k| set.get(k).is_some()));
        let back: Config = toml::from_str(&c.to_toml().unwrap()).unwrap();
        assert_eq!(back, c);
        let r = crate::replay::Replay {
            version: crate::replay::VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed: 1,
            play: c.play.clone(),
            ex_score: 0,
            clear: Default::default(),
            ghost: Vec::new(),
            ops: Vec::new(),
            course: None,
        };
        let back = crate::replay::Replay::decode(&r.encode().unwrap()).unwrap();
        assert_eq!(back.play, c.play);
    }

    /// The same configuration written before the IR accounts
    /// (`tests/golden/config.toml`, one `[ir_server]`) reads as it was, its
    /// server an account whose token is still to move.
    #[test]
    fn a_config_with_an_ir_server_reads() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/config.toml");
        let mut c = full_config();
        // (written before song select's order and filters, the input
        // tuning and the hi-speed types were kept)
        c.select = SelectConfig::default();
        c.keys.tuning = InputTuning::default();
        let d = PlayConfig::default();
        (c.play.hs_type, c.play.hs_level, c.play.hs_anim_ms) =
            (d.hs_type, d.hs_level, d.hs_anim_ms);
        c.play.p2.hs_level = d.hs_level;
        c.play.target_rival = None;
        c.audio.preview_once = false;
        c.ir = IrSettings {
            accounts: vec![IrAccount {
                name: "Test IR".into(),
                url: "https://ir.example.org/api".into(),
                old_token: OldToken("secret".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(Config::load(&path), c);
    }

    /// The language is written only when chosen (AUTO: the system's).
    #[test]
    fn language_is_written_once_chosen() {
        let mut c = Config::default();
        assert!(!c.to_toml().unwrap().contains("language"));
        c.language = crate::i18n::Language::ZhHant;
        let s = c.to_toml().unwrap();
        assert!(s.contains("language = \"zh-Hant\""), "{s}");
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
    }

    /// The third-party notice's OK is written only once given.
    #[test]
    fn third_party_acknowledgement_is_written_once_set() {
        let mut c = Config::default();
        assert!(!c.to_toml().unwrap().contains("third_party_acknowledged"));
        c.third_party_acknowledged = true;
        let s = c.to_toml().unwrap();
        assert!(s.contains("third_party_acknowledged = true"), "{s}");
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
    }

    /// MUTE WHEN INACTIVE is written only when turned off.
    #[test]
    fn mute_inactive_off_is_kept() {
        let mut c = Config::default();
        assert!(c.audio.mute_inactive);
        assert!(!c.to_toml().unwrap().contains("mute_inactive"));
        c.audio.mute_inactive = false;
        let s = c.to_toml().unwrap();
        assert!(s.contains("mute_inactive = false"));
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
    }

    /// Loudness normalisation is written only when it is not as it comes.
    #[test]
    fn normalizing_is_written_when_changed() {
        let mut c = Config::default();
        assert!(!c.to_toml().unwrap().contains("normalize"));
        (c.audio.normalize, c.audio.normalize_target) = (false, -16.0);
        c.audio.normalize_system = false;
        let s = c.to_toml().unwrap();
        assert!(s.contains("normalize = false") && s.contains("normalize_target = -16.0"));
        let d: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, d);
    }

    #[test]
    fn roundtrip_toml() {
        let c = Config::default();
        let s = c.to_toml().unwrap();
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

    /// The newer rule options are written when set and read back.
    #[test]
    fn newer_rule_options_roundtrip() {
        let mut c = Config::default();
        c.play.freq = -3;
        c.play.ln_force = true;
        c.play.s_random_interval = vibebms_core::lane_option::SRandomInterval::Ms40;
        c.play.trainer_order = "3142576".into();
        c.play.mine_add = vibebms_core::lane_option::MineAdd::Near;
        let s = c.to_toml().unwrap();
        assert!(s.contains("s_random_interval = \"Ms40\""));
        assert!(s.contains("trainer_order = \"3142576\""));
        assert!(s.contains("mine_add = \"Near\""));
        assert!(s.contains("freq = -3") && s.contains("ln_force = true"));
        let d: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, d);
        // at their defaults they are not written
        let d = Config::default().to_toml().unwrap();
        for f in [
            "freq", "ln_force", "s_random", "trainer", "mine_add", "ln_add", "scroll",
        ] {
            assert!(!d.contains(f), "{f}");
        }
    }

    /// Files written before the rules were split from the play options
    /// read the same and are written again with the same settings.
    #[test]
    fn a_config_of_before_reads_whole() {
        for old in [
            include_str!("../../tests/data/config_default.toml"),
            include_str!("../../tests/data/config_changed.toml"),
        ] {
            let mut old: toml::Value = toml::from_str(old).unwrap();
            // their `[ir_server]` is empty: no account
            migrate(&mut old);
            let c: Config = old.clone().try_into().unwrap();
            assert_eq!(c.ir, IrSettings::default());
            let mut again: toml::Value = toml::from_str(&c.to_toml().unwrap()).unwrap();
            again.as_table_mut().unwrap().remove("ir");
            assert_eq!(again, old);
        }
    }

    #[test]
    fn a_setting_of_the_wrong_type_resets_alone() {
        // VS options as a number, an unknown gauge, a key list as a string
        let s = "player_name = \"ME\"\n[play]\nhispeed = 3.5\np2 = 5\nbattle = true\n\
                 [play.target]\nx = 1\n[keys]\np2_start = \"P\"\n[display]\nwidth = 999\n";
        let v: toml::Value = toml::from_str(s).unwrap();
        assert!(v.clone().try_into::<Config>().is_err());
        let c = Config::lenient(&v);
        assert_eq!(c.player_name, "ME");
        assert_eq!(c.play.hispeed, 3.5);
        assert!(c.play.battle);
        assert_eq!(c.play.p2, SideOptions::default());
        assert_eq!(c.play.target, Target::Best);
        assert_eq!(c.keys, KeyConfig::default());
        assert_eq!(c.display.width, 999);
        // second player's options of their own
        let mut p = PlayConfig::default();
        p.p2.hispeed = 4.0;
        p.p2.sudden = 0.3;
        let two = p.for_side(1);
        assert_eq!((two.hispeed, two.sudden), (4.0, 0.3));
        let mut changed = two.clone();
        changed.hispeed = 5.0;
        p.set_side(1, &changed);
        assert_eq!((p.hispeed, p.p2.hispeed), (2.5, 5.0));
        let mut one = p.for_side(0);
        one.lift = 0.2;
        p.set_side(0, &one);
        assert_eq!((p.lift, p.p2.hispeed), (0.2, 5.0));
    }

    /// Practice mode's settings in a section of their own, read back as
    /// written; a file without them has the defaults.
    #[test]
    fn practice_settings_roundtrip() {
        let mut c = Config::default();
        assert!(!c.to_toml().unwrap().contains("[practice]"));
        c.practice = PracticeConfig {
            speed: 75,
            lead_in: 2,
            gauge: GaugeType::Hard,
            gauge_start: Some(60),
            rank: Some(0),
            total: Some(250),
            repeat: true,
            sections: [("ab".repeat(32), [12, 24])].into(),
        };
        let s = c.to_toml().unwrap();
        assert!(s.contains("[practice]") && s.contains("loop = true"));
        assert_eq!(toml::from_str::<Config>(&s).unwrap(), c);
        let none: Config = toml::from_str("player_name = \"ME\"\n").unwrap();
        assert_eq!(none.practice, PracticeConfig::default());
        // a value of the wrong type resets the practice settings, the
        // others stay
        let s = "player_name = \"ME\"\n[practice]\nspeed = \"fast\"\nlead_in = 3\n";
        let c = Config::lenient(&toml::from_str(s).unwrap());
        assert_eq!((c.player_name.as_str(), c.practice.lead_in), ("ME", 1));
    }

    #[test]
    fn perspective_roundtrip() {
        let mut c = Config {
            perspective: PerspectiveConfig {
                strength: 0.4,
                length: 0.8,
                touch_height: 0.25,
                layout: PERSPECTIVE_LAYOUT,
            },
            ..Config::default()
        };
        c.skins.insert("7KEYS".into(), "builtin:perspective".into());
        let s = c.to_toml().unwrap();
        let d: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, d);
        // older files have no [perspective]: the defaults
        let d: Config = toml::from_str("[perspective]\nstrength = 0.0\n").unwrap();
        assert_eq!(d.perspective.strength, 0.0);
        assert_eq!(d.perspective.length, PerspectiveConfig::default().length);
        let d: Config = toml::from_str("").unwrap();
        assert_eq!(d.perspective, PerspectiveConfig::default());
    }

    #[test]
    fn touch_layouts_roundtrip() {
        let mut c = Config::default();
        c.touch.style = TouchStyle::Custom;
        c.touch.custom_from = TouchStyle::Keyboard;
        c.touch.start_select_side = TouchSide::Right;
        c.touch.layouts.insert(
            "7KEYS landscape".into(),
            TouchLayout {
                buttons: vec![
                    TouchButton {
                        lane: 7,
                        x: -0.05,
                        y: 0.0,
                        w: 0.25,
                        h: 1.0,
                        round: true,
                    },
                    TouchButton {
                        lane: 0,
                        x: 0.3,
                        y: 0.55,
                        w: 0.08,
                        h: 0.4,
                        round: false,
                    },
                ],
            },
        );
        let s = c.to_toml().unwrap();
        let d: Config = toml::from_str(&s).unwrap();
        assert_eq!(c, d);
        // a configuration from before custom layouts: its style stays
        let old: Config =
            toml::from_str("[touch]\nmode = \"On\"\nstyle = \"Keyboard\"\nopacity = 0.5\n")
                .unwrap();
        assert_eq!(old.touch.style, TouchStyle::Keyboard);
        assert_eq!(old.touch.custom_from, TouchStyle::Iidx);
        assert!(old.touch.layouts.is_empty());
        assert_eq!(old.touch.start_select_side, TouchSide::Auto);
        let old: Config = toml::from_str("[touch]\nstyle = \"Iidx\"\n").unwrap();
        assert_eq!(old.touch, TouchConfig::default());
        // a button without `round` is square
        let b: TouchButton =
            toml::from_str("lane = 1\nx = 0.1\ny = 0.2\nw = 0.1\nh = 0.5\n").unwrap();
        assert!(!b.round);
    }
}
