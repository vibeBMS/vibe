//! Persistent configuration (portable `config/config.toml`).
//!
//! Every field has a default so older/partial files keep working, and every
//! option is editable from the in-game settings screen.

mod hispeed;
mod ir;
mod keys;
mod migrate;
mod play;
mod touch;

pub use hispeed::{Bpms, HISPEED_MAX, HISPEED_MIN, HISPEED_STEP};
pub use ir::{provider_name, Beside, IrAccount, IrSettings, OldToken, PlayerKey, SubmitPolicy};
pub use keys::{KeyConfig, LaneKeys, SideKeys};
pub use play::{HiSpeedMode, HsFix, PlayConfig, Side, SideOptions, Target, Versus};
pub use touch::{
    PerspectiveConfig, TouchButton, TouchConfig, TouchLayout, TouchMode, TouchSide, TouchStyle,
    LENGTH_RANGE, PERSPECTIVE_MAX, TOUCH_HEIGHT_RANGE,
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
            present_mode: PresentMode::VSync,
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
    /// Extra BMS folders (absolute or relative to the executable directory).
    pub bms_dirs: Vec<String>,
    /// Scan the executable directory itself.
    pub scan_root: bool,
    pub play: PlayConfig,
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
            name_song_folders: false,
            ir: IrSettings::default(),
            check_updates: true,
            skins: Default::default(),
            skin_options: Default::default(),
            skin_offsets: Default::default(),
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
                    auto_scratch: true,
                    legacy_note: true,
                    no_mine: true,
                    expand_judge: true,
                    judge_rates: [200, 150, 75],
                    hran_threshold_bpm: 150,
                },
                versus: Versus::Off,
                side: Side::P2,
                autoplay: true,
                hispeed_mode: HiSpeedMode::Constant,
                hispeed: 3.75,
                hs_fix: HsFix::Max,
                fixed_duration_ms: 512.5,
                sudden: 0.25,
                hidden: 0.125,
                lift: 0.0625,
                lane_cover: true,
                judge_offset_ms: -12,
                visual_offset_ms: 7,
                bga: false,
                show_fast_slow: false,
                judge_area: true,
                mark_notes: true,
                bpm_guide: true,
                timing_auto_adjust: true,
                target: Target::RateAAPlus,
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
                },
            },
            keys: KeyConfig {
                p1: side(0),
                p2: side(1_000_000),
                pms: (0..9).map(|i| lk(KeyCode::KeyA, 100 + i)).collect(),
                start: lk(KeyCode::Enter, 200),
                select: lk(KeyCode::Space, 201),
                p2_start: lk(KeyCode::Numpad1, 202),
                p2_select: lk(KeyCode::Numpad2, 203),
                any_axis_is_scratch: false,
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
            },
            display: DisplayConfig {
                fullscreen: true,
                width: 1920,
                height: 1080,
                present_mode: PresentMode::VSync,
                max_fps: 144,
                ui_scale: 1.25,
                show_fps: true,
            },
            audio: AudioSettings {
                buffer_frames: 128,
                sample_rate: 48000,
                master: 0.5,
                key: 0.75,
                bgm: 0.25,
                system: 0.125,
                preview: false,
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
        }
    }

    /// A configuration as written now (`tests/golden/config_accounts.toml`,
    /// and the play options as a replay stores them, `play_config.json`)
    /// reads back as it was and is written byte for byte the same.
    /// `GOLDEN_BLESS=1` writes the files anew.
    #[test]
    fn full_config_stays_the_same() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
        let (toml_path, json_path) = (
            dir.join("config_accounts.toml"),
            dir.join("play_config.json"),
        );
        let c = full_config();
        if std::env::var_os("GOLDEN_BLESS").is_some() {
            std::fs::write(&toml_path, toml::to_string_pretty(&c).unwrap()).unwrap();
            std::fs::write(&json_path, serde_json::to_vec(&c.play).unwrap()).unwrap();
        }
        let s = std::fs::read_to_string(&toml_path).unwrap();
        let read: Config = toml::from_str(&s).unwrap();
        assert_eq!(read, c);
        assert_eq!(toml::to_string_pretty(&read).unwrap(), s);
        assert_eq!(Config::load(&toml_path), c);
        let j = std::fs::read(&json_path).unwrap();
        let play: PlayConfig = serde_json::from_slice(&j).unwrap();
        assert_eq!(play, c.play);
        assert_eq!(serde_json::to_vec(&play).unwrap(), j);
    }

    /// The same configuration written before the IR accounts
    /// (`tests/golden/config.toml`, one `[ir_server]`) reads as it was, its
    /// server an account whose token is still to move.
    #[test]
    fn a_config_with_an_ir_server_reads() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/config.toml");
        let mut c = full_config();
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
            let mut again: toml::Value =
                toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
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

    #[test]
    fn perspective_roundtrip() {
        let mut c = Config {
            perspective: PerspectiveConfig {
                strength: 0.4,
                length: 0.8,
                touch_height: 0.25,
            },
            ..Config::default()
        };
        c.skins.insert("7KEYS".into(), "builtin:perspective".into());
        let s = toml::to_string_pretty(&c).unwrap();
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
        let s = toml::to_string_pretty(&c).unwrap();
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
