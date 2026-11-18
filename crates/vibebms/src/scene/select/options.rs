//! The play options of the select screen: its own option list (skin ids
//! 9000 on), the skin's option buttons and panels, and the START / SELECT
//! panels (`option_panel`).

use super::input::PanelKey;
use super::skin::{panel_of_op, PANELS, PANEL_TILES};
use super::status::SIBLINGS;
use super::{SelectScene, REFERENCE_BPMS};
use crate::app::{Ctx, Next};
use crate::config::{AudioSettings, BgaMode, BgaSize, Ghost, PlayConfig};
use crate::config::{
    Bpms, DifficultyFilter, HsFix, HsType, KeyFilter, RandomSelect, ReplayPick, Side, SortMode,
    Versus, HISPEED_STEP,
};
use crate::gfx::Rect;
use crate::input::{Nav, Repeat};
use crate::library::{Favorites, Song};
use crate::platform::{Duration, Instant};
use crate::scene::option_panel::{self, cycle, onoff};
use crate::scene::skin_host::Tap;
use crate::scene::target::{self, Rival};
use crate::sysound::Sound;
use vibebms_audio::Bus;
use vibebms_core::convert::KeyConvert;
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{LaneOption, LnAdd, MineAdd};
use vibebms_core::LnPref;

/// beatoraja: a held panel key repeats after 300 ms, every 50 ms.
pub(super) const PANEL_REPEAT_DELAY: Duration = Duration::from_millis(300);
pub(super) const PANEL_REPEAT_EVERY: Duration = Duration::from_millis(50);

/// The select screen's options (skin texts `9100 + i` name them, `9000 +
/// i` show their values, events `9000 + i` change them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SelectOption {
    Random,
    Gauge,
    AutoShift,
    LnMode,
    HiSpeedDown,
    HiSpeedUp,
    Side,
    Judge,
    Autoplay,
    Settings,
    Battle,
    Sort,
    KeyFilter,
    DifficultyFilter,
    FavoriteChart,
    FavoriteSong,
    RandomSelect,
    Screenshot,
    LaneCover,
    JudgeTiming,
    GreenNumber,
    VisualOffset,
    FastSlow,
    ScreenMode,
    MasterVolume,
    KeyVolume,
    BgmVolume,
    SystemVolume,
    Preview,
    BgaSize,
    Ghost,
    ScoreGraph,
    Sudden,
    Hidden,
    Lift,
    AutoScratch,
    Bga,
    TimingAutoAdjust,
    Freq,
    TrainerOrder,
    ScrollRemove,
    MineAdd,
    LnAdd,
    Replay,
    ReplaySpeed,
    /// The chart's folder in the file manager (F3, desktop).
    ChartFolder,
    /// The chart's web page (U).
    ChartUrl,
    /// SAME FOLDER (B).
    SameFolder,
    /// KEY CONVERT: the chart played in another key mode (not saved).
    KeyConvert,
}

/// Full screen can be switched (desktop).
pub(super) const WINDOWED: bool = !cfg!(any(target_arch = "wasm32", target_os = "android"));

/// A lane cover / lift share in 1 % steps, up to `max`.
fn cover(v: &mut f32, d: i32, max: f32) {
    *v = (*v + d as f32 * 0.01).clamp(0.0, max);
}

/// PLAY OPTION's key option `t` (RANDOM, GAUGE, DP OPTION · BATTLE,
/// HS-FIX, RANDOM 2P, TARGET) moved `d` places, as its key and its tile
/// do.
pub(crate) fn step_key_option(p: &mut PlayConfig, t: usize, d: i32, b: &Bpms, rivals: &[Rival]) {
    match t {
        5 => {
            target::step(p, rivals, d);
        }
        t => {
            option_panel::key(p, 1, [0, 2, 3, 4, 6][t], b, d);
        }
    }
}

/// Key option `t`'s value.
pub(crate) fn key_option_label(p: &PlayConfig, t: usize, rivals: &[Rival]) -> String {
    match t {
        0 => p.random_1p.label().into(),
        1 => p.gauge.label().into(),
        2 => option_panel::BATTLE_LABELS[option_panel::battle_index(p) as usize].into(),
        3 => p.hs_fix.label().into(),
        4 => p.random_2p.label().into(),
        _ => target::label(p, rivals),
    }
}

/// The song preview: ONCE, LOOP, OFF.
fn preview_index(a: &AudioSettings) -> i32 {
    match (a.preview, a.preview_once) {
        (true, true) => 0,
        (true, false) => 1,
        (false, _) => 2,
    }
}

impl SelectOption {
    pub const ALL: [SelectOption; 49] = [
        SelectOption::Random,
        SelectOption::Gauge,
        SelectOption::AutoShift,
        SelectOption::LnMode,
        SelectOption::HiSpeedDown,
        SelectOption::HiSpeedUp,
        SelectOption::Side,
        SelectOption::Judge,
        SelectOption::Autoplay,
        SelectOption::Settings,
        SelectOption::Battle,
        SelectOption::Sort,
        SelectOption::KeyFilter,
        SelectOption::DifficultyFilter,
        SelectOption::FavoriteChart,
        SelectOption::FavoriteSong,
        SelectOption::RandomSelect,
        SelectOption::Screenshot,
        SelectOption::LaneCover,
        SelectOption::JudgeTiming,
        SelectOption::GreenNumber,
        SelectOption::VisualOffset,
        SelectOption::FastSlow,
        SelectOption::ScreenMode,
        SelectOption::MasterVolume,
        SelectOption::KeyVolume,
        SelectOption::BgmVolume,
        SelectOption::SystemVolume,
        SelectOption::Preview,
        SelectOption::BgaSize,
        SelectOption::Ghost,
        SelectOption::ScoreGraph,
        SelectOption::Sudden,
        SelectOption::Hidden,
        SelectOption::Lift,
        SelectOption::AutoScratch,
        SelectOption::Bga,
        SelectOption::TimingAutoAdjust,
        SelectOption::Freq,
        SelectOption::TrainerOrder,
        SelectOption::ScrollRemove,
        SelectOption::MineAdd,
        SelectOption::LnAdd,
        SelectOption::Replay,
        SelectOption::ReplaySpeed,
        SelectOption::ChartFolder,
        SelectOption::ChartUrl,
        SelectOption::SameFolder,
        SelectOption::KeyConvert,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SelectOption::Random => "RANDOM",
            SelectOption::Gauge => "GAUGE",
            SelectOption::AutoShift => "AUTO SHIFT",
            SelectOption::LnMode => "LN MODE",
            SelectOption::HiSpeedDown => "HI-SPEED −",
            SelectOption::HiSpeedUp => "HI-SPEED +",
            SelectOption::Side => "SIDE",
            SelectOption::Judge => "JUDGE",
            SelectOption::Autoplay => "AUTOPLAY",
            SelectOption::Settings => "SETTINGS",
            SelectOption::Battle => "BATTLE",
            SelectOption::Sort => "SORT",
            SelectOption::KeyFilter => "KEY MODE",
            SelectOption::DifficultyFilter => "DIFFICULTY",
            SelectOption::FavoriteChart => "FAV CHART",
            SelectOption::FavoriteSong => "FAV SONG",
            SelectOption::RandomSelect => "RANDOM SELECT",
            SelectOption::Screenshot => "SCREENSHOT",
            SelectOption::LaneCover => "LANE COVER",
            SelectOption::JudgeTiming => "JUDGE TIMING",
            SelectOption::GreenNumber => "GREEN NUMBER",
            SelectOption::VisualOffset => "VISUAL OFFSET",
            SelectOption::FastSlow => "FAST / SLOW",
            SelectOption::ScreenMode => "SCREEN MODE",
            SelectOption::MasterVolume => "MASTER",
            SelectOption::KeyVolume => "KEY SOUND",
            SelectOption::BgmVolume => "BGM",
            SelectOption::SystemVolume => "SYSTEM",
            SelectOption::Preview => "PREVIEW",
            SelectOption::BgaSize => "BGA SIZE",
            SelectOption::Ghost => "GHOST POSITION",
            SelectOption::ScoreGraph => "SCORE GRAPH",
            SelectOption::Sudden => "SUDDEN+",
            SelectOption::Hidden => "HIDDEN+",
            SelectOption::Lift => "LIFT",
            SelectOption::AutoScratch => "AUTO SCRATCH",
            SelectOption::Bga => "BGA",
            SelectOption::TimingAutoAdjust => "JUDGE AUTO ADJUST",
            SelectOption::Freq => "FREQ",
            SelectOption::TrainerOrder => "TRAINER ORDER",
            SelectOption::ScrollRemove => "SCROLL REMOVE",
            SelectOption::MineAdd => "MINE ADD",
            SelectOption::LnAdd => "LONGNOTE ADD",
            SelectOption::Replay => "REPLAY",
            SelectOption::ReplaySpeed => "REPLAY SPEED",
            SelectOption::ChartFolder => "SONG FOLDER",
            SelectOption::ChartUrl => "WEB PAGE",
            SelectOption::SameFolder => "SAME FOLDER",
            SelectOption::KeyConvert => "KEY CONVERT",
        }
    }

    /// Its value; `bpm` is the selected chart's (for its green number),
    /// `song` the selected chart (for the favourites).
    pub fn value(self, ctx: &Ctx, bpm: Option<Bpms>, song: Option<&Song>) -> String {
        let p = &ctx.config.play;
        let sel = &ctx.config.select;
        let ms = |v: i32| format!("{v:+} ms");
        let pct = |v: f32| format!("{:.0}%", v * 100.0);
        let fav = |f: fn(&Favorites, &Song) -> bool| match (song, ctx.lib.db.as_ref()) {
            (Some(s), Some(l)) => onoff(f(l.favorites(), s)),
            _ => "—",
        };
        match self {
            SelectOption::FavoriteChart => fav(Favorites::chart).into(),
            SelectOption::FavoriteSong => fav(Favorites::song).into(),
            SelectOption::Sort => sel.sort.label().into(),
            SelectOption::KeyFilter => sel.key_filter.label().into(),
            SelectOption::DifficultyFilter => sel.difficulty_filter.label().into(),
            SelectOption::RandomSelect => sel.random.label().into(),
            SelectOption::Replay => sel.replay.label().into(),
            SelectOption::ReplaySpeed => format!("{}%", sel.replay_speed),
            SelectOption::Screenshot => "SAVE".into(),
            SelectOption::LaneCover => if p.lane_cover { "SUDDEN+" } else { "OFF" }.into(),
            SelectOption::JudgeTiming => ms(p.judge_offset_ms),
            SelectOption::GreenNumber => p.green(&bpm.unwrap_or(REFERENCE_BPMS)).to_string(),
            SelectOption::VisualOffset => ms(p.visual_offset_ms),
            SelectOption::FastSlow => onoff(p.show_fast_slow).into(),
            SelectOption::ScreenMode if !WINDOWED => "—".into(),
            SelectOption::ScreenMode => match ctx.config.display.fullscreen {
                true => "FULL SCREEN",
                false => "WINDOW",
            }
            .into(),
            SelectOption::MasterVolume => pct(ctx.config.audio.master),
            SelectOption::KeyVolume => pct(ctx.config.audio.key),
            SelectOption::BgmVolume => pct(ctx.config.audio.bgm),
            SelectOption::SystemVolume => pct(ctx.config.audio.system),
            SelectOption::Preview => {
                ["ONCE", "LOOP", "OFF"][preview_index(&ctx.config.audio) as usize].into()
            }
            SelectOption::BgaSize => p.bga_size.label().into(),
            SelectOption::Ghost => p.ghost.label().into(),
            SelectOption::ScoreGraph => onoff(p.score_graph).into(),
            SelectOption::Sudden => pct(p.sudden),
            SelectOption::Hidden => pct(p.hidden),
            SelectOption::Lift => pct(p.lift),
            SelectOption::AutoScratch => onoff(p.auto_scratch).into(),
            SelectOption::Bga => p.bga.label().into(),
            SelectOption::TimingAutoAdjust => onoff(p.timing_auto_adjust).into(),
            SelectOption::Freq => vibebms_core::play::freq_label(p.freq),
            SelectOption::TrainerOrder => p.trainer_order.clone(),
            SelectOption::ScrollRemove => onoff(p.scroll_remove).into(),
            SelectOption::MineAdd => p.mine_add.label().into(),
            SelectOption::LnAdd => p.ln_add.label().into(),
            SelectOption::KeyConvert => p.key_convert.label().into(),
            SelectOption::ChartFolder if !crate::platform::open::CAN_OPEN_FILES => "—".into(),
            SelectOption::ChartFolder | SelectOption::ChartUrl | SelectOption::SameFolder => {
                "OPEN".into()
            }
            SelectOption::Random => p.random_1p.label().into(),
            SelectOption::Gauge => p.gauge.label().into(),
            SelectOption::AutoShift => p.gauge_auto_shift.label().into(),
            SelectOption::LnMode => p.ln_pref().label(),
            SelectOption::HiSpeedDown => match p.hs_type {
                HsType::Normal => format!("NHS {}", p.hs_level),
                _ => format!("{:.2}", p.hispeed_shown(&bpm.unwrap_or(REFERENCE_BPMS))),
            },
            SelectOption::HiSpeedUp => match bpm {
                _ if p.hs_fix != HsFix::Off || p.hs_type == HsType::Floating => {
                    format!("GN {} FIX", p.green(&bpm.unwrap_or(REFERENCE_BPMS)))
                }
                Some(b) => format!("GN {}", p.green(&b)),
                None => format!("{:.2}", p.hispeed),
            },
            SelectOption::Side => format!("{:?}", p.side),
            SelectOption::Judge => p.judge_preset.label().into(),
            SelectOption::Autoplay => onoff(p.autoplay).into(),
            SelectOption::Settings => "OPEN".into(),
            SelectOption::Battle => match option_panel::battle_index(p) {
                2 => "BATTLE".into(),
                3 => "BATTLE AS".into(),
                4 => "G-BEST".into(),
                5 => "G-LAST".into(),
                6 => "VS".into(),
                n @ 7..=10 => format!("G-SLOT{}", n - 6),
                _ => "OFF".into(),
            },
        }
    }
}

/// What VS needs of the controls, said when it is chosen.
fn vs_hint(ctx: &mut Ctx) {
    if crate::scene::play::vs_touch_too_small(ctx) {
        ctx.toast(crate::scene::play::VS_TOUCH_TOO_SMALL.to_string());
        return;
    }
    let pads = ctx.config.keys.p2.keys.iter().any(|k| !k.pad.is_empty());
    if !ctx.touch_enabled() && !pads {
        ctx.toast(
            "VS: 1P and 2P keys on one keyboard; many keyboards miss keys when several are held \
             (rollover)"
                .to_string(),
        );
    }
}

impl SelectScene {
    /// Change option `o`: the next value, the one before for `d` = -1 (a
    /// tap on a value's left half).
    pub(super) fn option(&mut self, ctx: &mut Ctx, o: SelectOption, d: i32) -> Next {
        let bpm = self.bpms(ctx);
        let sel = &mut ctx.config.select;
        // the list: its order, what it shows, the records it is sorted by
        let relist = match o {
            SelectOption::Sort => {
                sel.sort = cycle(&SortMode::ALL, sel.sort, d);
                true
            }
            SelectOption::KeyFilter => {
                sel.key_filter = cycle(&KeyFilter::ALL, sel.key_filter, d);
                true
            }
            SelectOption::DifficultyFilter => {
                sel.difficulty_filter = cycle(&DifficultyFilter::ALL, sel.difficulty_filter, d);
                true
            }
            SelectOption::RandomSelect => {
                sel.random = cycle(&RandomSelect::ALL, sel.random, d);
                false
            }
            SelectOption::Replay => {
                sel.replay = cycle(&ReplayPick::ALL, sel.replay, d);
                false
            }
            SelectOption::ReplaySpeed => {
                sel.step_replay_speed(d);
                false
            }
            _ => matches!(o, SelectOption::Judge | SelectOption::LnMode),
        };
        let p = &mut ctx.config.play;
        match o {
            SelectOption::Random => p.random_1p = cycle(&LaneOption::ALL, p.random_1p, d),
            SelectOption::Gauge => p.gauge = cycle(p.gauge_preset.selectable(), p.gauge, d),
            SelectOption::AutoShift => {
                p.gauge_auto_shift = cycle(&GaugeAutoShift::ALL, p.gauge_auto_shift, d)
            }
            SelectOption::LnMode => {
                let ln = cycle(&LnPref::ALL, p.ln_pref(), d);
                p.set_ln_pref(ln)
            }
            SelectOption::HiSpeedDown => p.add_hispeed(-HISPEED_STEP * d as f32, &bpm),
            SelectOption::HiSpeedUp => p.add_hispeed(HISPEED_STEP * d as f32, &bpm),
            SelectOption::Side => p.side = cycle(&[Side::P1, Side::P2], p.side, d),
            SelectOption::Judge => p.judge_preset = cycle(&JudgePreset::ALL, p.judge_preset, d),
            SelectOption::Autoplay => p.autoplay ^= true,
            SelectOption::Settings => {
                return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx)))
            }
            SelectOption::Battle => {
                // OFF, BATTLE, BATTLE AS, G-BATTLE (MY BEST, LAST, SLOT
                // 1-4), VS
                let all = option_panel::battle_choices(true);
                let row: Vec<i64> = all.iter().copied().filter(|i| *i != 1).collect();
                let label = option_panel::step_battle(p, &row, d);
                // (skins other than the default show no such row)
                ctx.toast(format!("BATTLE: {label}"));
                if ctx.config.play.versus == Versus::Local {
                    vs_hint(ctx);
                }
            }
            SelectOption::Sort
            | SelectOption::KeyFilter
            | SelectOption::DifficultyFilter
            | SelectOption::RandomSelect
            | SelectOption::Replay
            | SelectOption::ReplaySpeed => {
                // (skins without these show nothing of it)
                ctx.toast(format!("{}: {}", o.name(), o.value(ctx, None, None)));
            }
            SelectOption::Screenshot => {
                ctx.view.want_screenshot = true;
                return Next::Stay;
            }
            SelectOption::ChartFolder => {
                self.reveal_chart(ctx);
                return Next::Stay;
            }
            SelectOption::ChartUrl => {
                self.open_chart_url(ctx);
                return Next::Stay;
            }
            SelectOption::SameFolder => {
                self.same_folder(ctx);
                return Next::Stay;
            }
            SelectOption::FavoriteChart | SelectOption::FavoriteSong => {
                self.toggle_favorite(ctx, o == SelectOption::FavoriteSong)
            }
            SelectOption::LaneCover => p.lane_cover ^= true,
            SelectOption::BgaSize => p.bga_size = cycle(&BgaSize::ALL, p.bga_size, d),
            SelectOption::Ghost => p.ghost = cycle(&Ghost::ALL, p.ghost, d),
            SelectOption::ScoreGraph => p.score_graph ^= true,
            SelectOption::Sudden => cover(&mut p.sudden, d, 0.9),
            SelectOption::Hidden => cover(&mut p.hidden, d, 0.9),
            SelectOption::Lift => cover(&mut p.lift, d, 0.6),
            SelectOption::AutoScratch => p.auto_scratch ^= true,
            SelectOption::Bga => p.bga = cycle(&BgaMode::ALL, p.bga, d),
            SelectOption::TimingAutoAdjust => p.timing_auto_adjust ^= true,
            SelectOption::Freq => {
                option_panel::step_freq(p, d);
            }
            // a lane on (or back): the order turned
            SelectOption::TrainerOrder => {
                p.trainer_order = vibebms_core::lane_option::turn_order(&p.trainer_order, d)
            }
            SelectOption::ScrollRemove => p.scroll_remove ^= true,
            SelectOption::MineAdd => p.mine_add = cycle(&MineAdd::ALL, p.mine_add, d),
            SelectOption::LnAdd => p.ln_add = cycle(&LnAdd::ALL, p.ln_add, d),
            SelectOption::KeyConvert => {
                p.key_convert = cycle(&KeyConvert::ALL, p.key_convert, d);
                // (skins other than the default show no such option)
                ctx.toast(format!(
                    "KEY CONVERT: {} (such plays are not saved)",
                    ctx.config.play.key_convert.label()
                ));
            }
            SelectOption::JudgeTiming => {
                p.judge_offset_ms = (p.judge_offset_ms + d).clamp(-500, 500)
            }
            SelectOption::GreenNumber => p.add_duration(d as f64, &bpm),
            SelectOption::VisualOffset => {
                p.visual_offset_ms = (p.visual_offset_ms + d).clamp(-500, 500)
            }
            SelectOption::FastSlow => p.show_fast_slow ^= true,
            SelectOption::ScreenMode => {
                let v = &mut ctx.view;
                if WINDOWED {
                    ctx.config.display.fullscreen ^= true;
                    v.want_fullscreen = Some(ctx.config.display.fullscreen);
                }
            }
            SelectOption::MasterVolume
            | SelectOption::KeyVolume
            | SelectOption::BgmVolume
            | SelectOption::SystemVolume => {
                let a = &mut ctx.config.audio;
                let (v, bus) = match o {
                    SelectOption::MasterVolume => (&mut a.master, None),
                    SelectOption::KeyVolume => (&mut a.key, Some(Bus::Key)),
                    SelectOption::BgmVolume => (&mut a.bgm, Some(Bus::Bgm)),
                    _ => (&mut a.system, Some(Bus::System)),
                };
                *v = (*v + d as f32 * 0.05).clamp(0.0, 1.0);
                match bus {
                    Some(b) => ctx.audio.mixer.set_bus_volume(b, *v),
                    None => ctx.audio.set_master(*v),
                }
            }
            SelectOption::Preview => {
                let a = &mut ctx.config.audio;
                let i = (preview_index(a) + d).rem_euclid(3);
                (a.preview, a.preview_once) = (i < 2, i == 0);
                ctx.audio.mixer.stop(super::PREVIEW_KEY);
                self.moved(ctx);
            }
        }
        if relist {
            self.rebuild(ctx);
        }
        ctx.play_sound(Sound::OptionChange);
        ctx.save_config();
        Next::Stay
    }

    /// Make the selected chart (`song`: its song) a favourite, or one no
    /// more.
    pub(super) fn toggle_favorite(&mut self, ctx: &mut Ctx, song: bool) {
        let Some((_, s)) = self.selected_song(ctx) else {
            return;
        };
        let s = s.clone();
        let Some(lib) = ctx.lib.db.as_mut() else {
            return;
        };
        let on = lib.toggle_favorite(&s, song);
        let what = if song {
            "FAVORITE SONG"
        } else {
            "FAVORITE CHART"
        };
        ctx.toast(format!("{what}: {} ({})", onoff(on), s.title));
        self.rebuild(ctx);
    }

    /// Open / close a skin panel (1..=`PANELS`; 0 closes the open one).
    pub(crate) fn toggle_panel(&mut self, n: usize) {
        let now = Instant::now();
        if self.panel > 0 {
            self.panel_off[self.panel - 1] = Some(now);
        }
        self.panel = if n == self.panel || n == 0 {
            0
        } else {
            n.min(PANELS)
        };
        if self.panel > 0 {
            self.panel_on[self.panel - 1] = Some(now);
        }
        self.focus = 0;
    }

    /// With a panel open (not held): ↑↓ (the turntable) move the focus
    /// between its tiles, → / ← (white / black keys) step the focused value
    /// on / back, Back closes.
    pub(super) fn panel_nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        let tiles = PANEL_TILES[self.panel - 1];
        let n = tiles.len();
        match nav {
            Nav::Up | Nav::Down if n > 0 => {
                let d = if nav == Nav::Up { n - 1 } else { 1 };
                self.focus = (self.focus + d) % n;
                ctx.play_sound(Sound::Scratch);
            }
            Nav::Decide | Nav::Close if n > 0 => {
                let d = if nav == Nav::Decide { 1 } else { -1 };
                return self.skin_event(ctx, tiles[self.focus.min(n - 1)], d);
            }
            Nav::Settings => {
                return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx)))
            }
            _ => self.toggle_panel(0),
        }
        Next::Stay
    }

    /// Does the skin have panels (objects shown only with a panel option)?
    pub(super) fn skin_has_panels(&self) -> bool {
        self.host.skin.as_ref().is_some_and(|s| {
            s.skin
                .objects
                .iter()
                .any(|o| o.dst.ops.iter().any(|op| panel_of_op(*op).is_some()))
        })
    }

    /// A clicked skin object (LR2 / beatoraja button ids, or vibeBMS ids);
    /// `d` -1: the value before (a tap on its left half), else the next.
    pub(super) fn skin_event(&mut self, ctx: &mut Ctx, act: i32, d: i32) -> Next {
        use super::skin::{
            EVENT_BACK, EVENT_IR_PAGE, EVENT_PANEL, EVENT_PRACTICE, EVENT_PROFILES,
            EVENT_READ_TEXT, EVENT_REPLAY, EVENT_SEARCH, EVENT_SIBLING, OPTION_EVENT,
        };
        let rivals = crate::scene::target::rivals(ctx);
        let p = &mut ctx.config.play;
        let done = match act {
            1..=9 => {
                self.toggle_panel(act as usize);
                true
            }
            43 => {
                p.random_2p = cycle(&LaneOption::ALL, p.random_2p, d);
                true
            }
            // beatoraja option buttons: as the panel keys that change them
            40 | 54 | 55 | 301..=307 | 75 | 78 => {
                let (panel, key) = match act {
                    40 => (1, 2),
                    54 => (1, 3),
                    55 => (1, 4),
                    75 => (3, 2),
                    78 => (3, 1),
                    n => (2, (n - 301) as usize),
                };
                option_panel::key(p, panel, key, &REFERENCE_BPMS, d);
                true
            }
            77 => {
                option_panel::turn(p, 1, -d, &rivals);
                true
            }
            72 => {
                p.bga = cycle(&BgaMode::ALL, p.bga, d);
                true
            }
            _ => false,
        };
        if done {
            ctx.play_sound(Sound::OptionChange);
            ctx.save_config();
            return Next::Stay;
        }
        match act {
            15 => self.decide(ctx, false),
            16 => self.decide(ctx, true),
            // beatoraja / LR2: the key config and skin select screens
            13 => {
                self.stop_bgm(ctx);
                Next::Push(crate::scene::keyconfig::open(ctx))
            }
            14 => Next::Push(crate::scene::skinselect::open(ctx, None)),
            // LR2 / beatoraja: the difficulty and key mode filters, the sort
            10 => self.option(ctx, SelectOption::DifficultyFilter, d),
            11 => self.option(ctx, SelectOption::KeyFilter, d),
            12 => self.option(ctx, SelectOption::Sort, d),
            42 => self.option(ctx, SelectOption::Random, d),
            308 => self.option(ctx, SelectOption::LnMode, d),
            EVENT_BACK => {
                self.back(ctx);
                Next::Stay
            }
            EVENT_REPLAY => self.watch_replay(ctx),
            EVENT_PRACTICE => self.practice(ctx),
            EVENT_IR_PAGE => {
                self.open_ir_page(ctx);
                Next::Stay
            }
            EVENT_PANEL => Next::Stay,
            EVENT_PROFILES => {
                use crate::scene::settings::{Page, SettingsScene};
                Next::Push(Box::new(SettingsScene::at(ctx, Page::System)))
            }
            EVENT_READ_TEXT => {
                self.read_text(ctx);
                Next::Stay
            }
            EVENT_SEARCH => {
                self.open_search();
                Next::Stay
            }
            n if (EVENT_SIBLING..EVENT_SIBLING + SIBLINGS as i32).contains(&n) => {
                self.pick_sibling(ctx, (n - EVENT_SIBLING) as usize);
                Next::Stay
            }
            n if (OPTION_EVENT..OPTION_EVENT + 100).contains(&n) => {
                match SelectOption::ALL.get((n - OPTION_EVENT) as usize) {
                    Some(o) => self.option(ctx, *o, d),
                    None => Next::Stay,
                }
            }
            _ => Next::Stay,
        }
    }

    /// START / SELECT pressed or released: open the panel they hold.
    pub(super) fn hold(&mut self, start: Option<bool>, select: Option<bool>) {
        if let Some(v) = start {
            self.start_held = v;
        }
        if let Some(v) = select {
            self.select_held = v;
        }
        let want = if self.num5_held {
            3
        } else {
            option_panel::panel_of(self.start_held, self.select_held)
        };
        if want != self.panel {
            self.toggle_panel(want);
            self.panel_repeat = None;
        }
    }

    pub(super) fn holding(&self) -> bool {
        self.start_held || self.select_held || self.num5_held
    }

    /// A held panel key let go: stop its repeat.
    pub(super) fn panel_release(&mut self, ctx: &mut Ctx, k: PanelKey) {
        if self.panel_repeat.is_some_and(|r| r.key == k) {
            self.panel_repeat = None;
            ctx.save_config();
        }
    }

    /// A key or turntable step with a panel held.
    pub(super) fn panel_input(&mut self, ctx: &mut Ctx, k: PanelKey) {
        let b = self.bpms(ctx);
        let rivals = crate::scene::target::rivals(ctx);
        let p = &mut ctx.config.play;
        let said = match k {
            PanelKey::Key(i) => option_panel::key(p, self.panel, i, &b, 1),
            PanelKey::Up => option_panel::turn(p, self.panel, 1, &rivals),
            PanelKey::Down => option_panel::turn(p, self.panel, -1, &rivals),
        };
        if let PanelKey::Key(i) = k {
            if option_panel::repeats(self.panel, i) {
                self.panel_repeat = Some(Repeat::new(k, Instant::now(), PANEL_REPEAT_DELAY));
            }
        }
        if let Some(msg) = said {
            ctx.play_sound(Sound::OptionChange);
            // the skin shows the change on its panel; without one, a toast
            // (an IR rival as TARGET too: no skin lists them)
            let rival = ctx.config.play.target == crate::config::Target::Rival;
            if !self.skin_has_panels() || rival {
                ctx.toast(msg);
            }
            ctx.save_config();
        }
    }

    /// A held green number key (panel 3): 1 ms steps, 10 ms after 50.
    pub(super) fn repeat_panel_key(&mut self, ctx: &mut Ctx, now: Instant) {
        let holding = self.holding();
        let Some(r) = self.panel_repeat.as_mut().filter(|_| holding) else {
            return;
        };
        let count = r.count;
        if !r.due(now, PANEL_REPEAT_EVERY) {
            return;
        }
        let PanelKey::Key(k) = r.key else {
            return;
        };
        let step = if count > 50 { 10.0 } else { 1.0 };
        let b = self.bpms(ctx);
        let msg = option_panel::repeat(&mut ctx.config.play, k, step, &b);
        if !self.skin_has_panels() {
            ctx.toast(msg);
        }
    }

    /// A tap on the skin at (`x`, `y`). Drawn smaller than 0.7 pt per skin
    /// pixel (a phone), its buttons take taps within 44 pt.
    pub(super) fn tap(&mut self, ctx: &mut Ctx, x: f32, y: f32) -> Option<Next> {
        let v = &ctx.view;
        let pt = self.host.xf(Rect::new(0.0, 0.0, v.width, v.height)).sx / v.scale.max(0.1);
        let slop = if pt < 0.7 { 44.0 * v.scale } else { 0.0 };
        match self.host.tap(x, y, slop) {
            Tap::Miss => None,
            Tap::Script => Some(Next::Stay),
            Tap::Event(act, d) => Some(self.skin_event(ctx, act, d)),
        }
    }
}
