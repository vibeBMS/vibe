//! The play options of the select screen: its own option list (skin ids
//! 9000 on), the skin's option buttons and panels, and the START / SELECT
//! panels (`option_panel`).

use super::input::PanelKey;
use super::{SelectScene, REFERENCE_BPMS};
use crate::app::{Ctx, Next};
use crate::config::{
    Bpms, DifficultyFilter, HsFix, KeyFilter, RandomSelect, Side, SortMode, Versus, HISPEED_STEP,
};
use crate::input::Repeat;
use crate::library::{Favorites, Song};
use crate::platform::{Duration, Instant};
use crate::scene::option_panel::{self, cycle, onoff};
use crate::scene::skin_host::Tap;
use crate::sysound::Sound;
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::LaneOption;
use vibebms_core::LnType;

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
}

impl SelectOption {
    pub const ALL: [SelectOption; 18] = [
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
        }
    }

    /// Its value; `bpm` is the selected chart's (for its green number),
    /// `song` the selected chart (for the favourites).
    pub fn value(self, ctx: &Ctx, bpm: Option<Bpms>, song: Option<&Song>) -> String {
        let p = &ctx.config.play;
        let sel = &ctx.config.select;
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
            SelectOption::Screenshot => "SAVE".into(),
            SelectOption::Random => p.random_1p.label().into(),
            SelectOption::Gauge => p.gauge.label().into(),
            SelectOption::AutoShift => p.gauge_auto_shift.label().into(),
            SelectOption::LnMode => format!("{:?}", p.ln_mode).to_uppercase(),
            SelectOption::HiSpeedDown => {
                format!("{:.2}", p.hispeed_at(&bpm.unwrap_or(REFERENCE_BPMS)))
            }
            SelectOption::HiSpeedUp => match bpm {
                _ if p.hs_fix != HsFix::Off => {
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
                2 => "BATTLE",
                3 => "BATTLE AS",
                4 => "G-BEST",
                5 => "G-LAST",
                6 => "VS",
                _ => "OFF",
            }
            .into(),
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
    /// Change option `o` (the next value).
    pub(super) fn option(&mut self, ctx: &mut Ctx, o: SelectOption) -> Next {
        let bpm = self.bpms(ctx);
        let sel = &mut ctx.config.select;
        // the list: its order, what it shows, the records it is sorted by
        let relist = match o {
            SelectOption::Sort => {
                sel.sort = cycle(&SortMode::ALL, sel.sort, 1);
                true
            }
            SelectOption::KeyFilter => {
                sel.key_filter = cycle(&KeyFilter::ALL, sel.key_filter, 1);
                true
            }
            SelectOption::DifficultyFilter => {
                sel.difficulty_filter = cycle(&DifficultyFilter::ALL, sel.difficulty_filter, 1);
                true
            }
            SelectOption::RandomSelect => {
                sel.random = cycle(&RandomSelect::ALL, sel.random, 1);
                false
            }
            _ => matches!(o, SelectOption::Judge | SelectOption::LnMode),
        };
        let p = &mut ctx.config.play;
        match o {
            SelectOption::Random => p.random_1p = cycle(&LaneOption::ALL, p.random_1p, 1),
            SelectOption::Gauge => p.gauge = cycle(p.gauge_preset.selectable(), p.gauge, 1),
            SelectOption::AutoShift => {
                p.gauge_auto_shift = cycle(&GaugeAutoShift::ALL, p.gauge_auto_shift, 1)
            }
            SelectOption::LnMode => {
                p.ln_mode = cycle(&[LnType::Ln, LnType::Cn, LnType::Hcn], p.ln_mode, 1)
            }
            SelectOption::HiSpeedDown => p.add_hispeed(-HISPEED_STEP, &bpm),
            SelectOption::HiSpeedUp => p.add_hispeed(HISPEED_STEP, &bpm),
            SelectOption::Side => p.side = cycle(&[Side::P1, Side::P2], p.side, 1),
            SelectOption::Judge => p.judge_preset = cycle(&JudgePreset::ALL, p.judge_preset, 1),
            SelectOption::Autoplay => p.autoplay ^= true,
            SelectOption::Settings => {
                return Next::Push(Box::new(crate::scene::settings::SettingsScene::new(ctx)))
            }
            SelectOption::Battle => {
                // OFF, BATTLE, BATTLE AS, G-BATTLE (MY BEST, LAST), VS
                let all = option_panel::battle_choices(true);
                let row: Vec<i64> = all.iter().copied().filter(|i| *i != 1).collect();
                let label = option_panel::step_battle(p, &row, 1);
                // (skins other than the default show no such row)
                ctx.toast(format!("BATTLE: {label}"));
                if ctx.config.play.versus == Versus::Local {
                    vs_hint(ctx);
                }
            }
            SelectOption::Sort
            | SelectOption::KeyFilter
            | SelectOption::DifficultyFilter
            | SelectOption::RandomSelect => {
                // (skins without these show nothing of it)
                ctx.toast(format!("{}: {}", o.name(), o.value(ctx, None, None)));
            }
            SelectOption::Screenshot => {
                ctx.view.want_screenshot = true;
                return Next::Stay;
            }
            SelectOption::FavoriteChart | SelectOption::FavoriteSong => {
                self.toggle_favorite(ctx, o == SelectOption::FavoriteSong)
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

    /// Open / close a skin panel (1..=6; 0 closes the open one).
    pub(crate) fn toggle_panel(&mut self, n: usize) {
        let now = Instant::now();
        if self.panel > 0 {
            self.panel_off[self.panel - 1] = Some(now);
        }
        self.panel = if n == self.panel || n == 0 {
            0
        } else {
            n.min(6)
        };
        if self.panel > 0 {
            self.panel_on[self.panel - 1] = Some(now);
        }
    }

    /// Does the skin have panels (objects shown only with a panel option)?
    pub(super) fn skin_has_panels(&self) -> bool {
        self.host.skin.as_ref().is_some_and(|s| {
            s.skin
                .objects
                .iter()
                .any(|o| o.dst.ops.iter().any(|op| (21..=26).contains(op)))
        })
    }

    /// A clicked skin object (LR2 / beatoraja button ids, or vibeBMS ids).
    pub(super) fn skin_event(&mut self, ctx: &mut Ctx, act: i32) -> Next {
        use super::skin::{EVENT_BACK, EVENT_REPLAY, OPTION_EVENT};
        let p = &mut ctx.config.play;
        let done = match act {
            1..=9 => {
                self.toggle_panel(act as usize);
                true
            }
            43 => {
                p.random_2p = cycle(&LaneOption::ALL, p.random_2p, 1);
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
                option_panel::key(p, panel, key, &REFERENCE_BPMS);
                true
            }
            77 => {
                option_panel::turn(p, 1, -1, &REFERENCE_BPMS);
                true
            }
            72 => {
                p.bga ^= true;
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
            10 => self.option(ctx, SelectOption::DifficultyFilter),
            11 => self.option(ctx, SelectOption::KeyFilter),
            12 => self.option(ctx, SelectOption::Sort),
            42 => self.option(ctx, SelectOption::Random),
            40 => self.option(ctx, SelectOption::Gauge),
            308 => self.option(ctx, SelectOption::LnMode),
            EVENT_BACK => {
                self.back(ctx);
                Next::Stay
            }
            EVENT_REPLAY => self.watch_replay(ctx),
            n if (OPTION_EVENT..OPTION_EVENT + 100).contains(&n) => {
                match SelectOption::ALL.get((n - OPTION_EVENT) as usize) {
                    Some(o) => self.option(ctx, *o),
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
        let p = &mut ctx.config.play;
        let said = match k {
            PanelKey::Key(i) => option_panel::key(p, self.panel, i, &b),
            PanelKey::Up => option_panel::turn(p, self.panel, 1, &b),
            PanelKey::Down => option_panel::turn(p, self.panel, -1, &b),
        };
        if let PanelKey::Key(i) = k {
            if option_panel::repeats(self.panel, i) {
                self.panel_repeat = Some(Repeat::new(k, Instant::now(), PANEL_REPEAT_DELAY));
            }
        }
        if let Some(msg) = said {
            ctx.play_sound(Sound::OptionChange);
            // the skin shows the change on its panel; without one, a toast
            if !self.skin_has_panels() {
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

    /// A tap on the skin at (`x`, `y`).
    pub(super) fn tap(&mut self, ctx: &mut Ctx, x: f32, y: f32) -> Option<Next> {
        match self.host.tap(x, y) {
            Tap::Miss => None,
            Tap::Script => Some(Next::Stay),
            Tap::Event(act, _) => Some(self.skin_event(ctx, act)),
        }
    }
}
