//! The key config screen drawn by a KEY_CONFIG skin (LR2 / beatoraja skin
//! type 8), when one is chosen; without one the settings' KEY CONFIG page
//! is used.
//!
//! Rows are the bindings of a key mode (7 KEYS with 2P = 14 keys, 9 KEYS,
//! 5 KEYS: the order of LR2's mode button). ↑↓ (or the turntable) pick a
//! row, ←→ the mode, Enter (a white key, a tap on the chosen row) waits for
//! a key, controller button or turntable axis to bind, Delete clears, Esc
//! (SELECT, BACK) leaves and saves.
//!
//! Skin values: beatoraja's texts 40-49 / 240-283 (what row `i` is bound
//! to). LR2 / Lunatic Vibes: buttons 101-116 / 121-136 (the chosen key,
//! clicked to choose one), 140-142 (its index in 7 / 9 / 5 key mode), 143
//! (the mode), 150-159 (binding slot: 0 keyboard, 1 controller, 2
//! turntable axis), texts 40-49 (the chosen key's binding per slot),
//! options 400-402 (mode), Lunatic Vibes texts 300-384 (hints, key names
//! and bindings). beatoraja draws its rows over its (empty) skin; so does
//! this screen, for beatoraja skins and LR2 skins without key buttons.

use super::keybind::{self, Learn, Learned, Slot};
use super::skin_host::{NoHooks, SkinHost, Tap};
use super::{skinned, skinvals};
use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{with_alpha, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad_rows, AxisStepper, InputEv, Nav, Phase};
use crate::platform::Instant;
use crate::skin::render::Xf;
use crate::skin::{SkinState, SkinType};
use crate::sysound::Sound;
use crate::theme;
use crate::ui::{self, HitMap, ListView};
use std::collections::HashMap;
use winit::keyboard::KeyCode;

/// Key modes in LR2's order (button 143: 0, 1, 2).
pub const MODES: [&str; 3] = ["7 KEYS / 14 KEYS", "9 KEYS", "5 KEYS / 10 KEYS"];

/// Binding slots (LR2 buttons 150-159): keyboard, controller, axis.
const DEVICES: [&str; 3] = ["Keyboard", "Controller", "Axis"];

/// The rows of a mode, in beatoraja's order (keys, scratch, 2P, START,
/// SELECT), turntable axes last.
pub fn slots(mode: usize) -> Vec<Slot> {
    if mode == 1 {
        let mut v: Vec<Slot> = (0..9).map(Slot::Pms).collect();
        v.extend([Slot::Start, Slot::Select]);
        return v;
    }
    let keys = if mode == 0 { 7 } else { 5 };
    let mut v = Vec::new();
    for side in 0..2 {
        v.extend((0..keys).map(|idx| Slot::Key { side, idx }));
        v.extend([Slot::ScratchUp(side), Slot::ScratchDown(side)]);
    }
    v.extend([
        Slot::Start,
        Slot::Select,
        Slot::Start2,
        Slot::Select2,
        Slot::ScratchAxis(0),
        Slot::ScratchAxis(1),
    ]);
    v
}

/// The LR2 button of a binding (101.. 1P, 121.. 2P).
pub fn lr2_button(s: Slot) -> i32 {
    match s {
        Slot::Key { side, idx } => 101 + 20 * side as i32 + idx as i32,
        Slot::Pms(i) => 101 + i as i32,
        Slot::ScratchUp(sd) => 110 + 20 * sd as i32,
        Slot::ScratchDown(sd) => 111 + 20 * sd as i32,
        Slot::ScratchAxis(sd) => 116 + 20 * sd as i32,
        Slot::Start => 112,
        Slot::Select => 113,
        Slot::Start2 => 132,
        Slot::Select2 => 133,
    }
}

/// The chosen key as LR2's buttons 140 (7 keys), 141 (9 keys) and 142 (5
/// keys) show it: 1P keys, scratch left / right, START, SELECT, 2P ...,
/// then the turntable axes (Lunatic Vibes).
pub fn lr2_index(mode: usize, s: Slot) -> i64 {
    let k = if mode == 2 { 5 } else { 7 };
    let per = k as i64 + 4;
    match (mode, s) {
        (1, Slot::Pms(i)) => i as i64 + 1,
        (1, Slot::Start) => 10,
        (1, Slot::Select) => 11,
        (1, _) => 0,
        (_, Slot::Key { side, idx }) => side as i64 * per + idx as i64 + 1,
        (_, Slot::ScratchUp(sd)) => sd as i64 * per + k as i64 + 1,
        (_, Slot::ScratchDown(sd)) => sd as i64 * per + k as i64 + 2,
        (_, Slot::Start) => k as i64 + 3,
        (_, Slot::Select) => k as i64 + 4,
        (_, Slot::Start2) => per + k as i64 + 3,
        (_, Slot::Select2) => per + k as i64 + 4,
        (_, Slot::ScratchAxis(sd)) => 2 * per + 1 + sd as i64,
        (_, Slot::Pms(_)) => 0,
    }
}

/// Lunatic Vibes' text slots of a key (311.. names, 351.. bindings; +20
/// for 2P): keys 1-9, scratch left, right, START, SELECT, axis.
fn lv_text_slot(mode: usize, s: Slot) -> Option<i32> {
    Some(match s {
        Slot::Key { side, idx } => 20 * side as i32 + idx as i32,
        Slot::Pms(i) if mode == 1 => i as i32,
        Slot::ScratchUp(sd) => 20 * sd as i32 + 9,
        Slot::ScratchDown(sd) => 20 * sd as i32 + 10,
        Slot::Start => 11,
        Slot::Select => 12,
        Slot::Start2 => 31,
        Slot::Select2 => 32,
        Slot::ScratchAxis(sd) => 20 * sd as i32 + 13,
        Slot::Pms(_) => return None,
    })
}

/// The screen to open for the key config: the skinned one when a
/// KEY_CONFIG skin is chosen, else the settings' KEY CONFIG page.
pub fn open(ctx: &mut Ctx) -> Box<dyn Scene> {
    if skinned::configured_skin(ctx, SkinType::KeyConfig).is_some() {
        Box::new(KeyConfigScene::new())
    } else {
        Box::new(super::settings::SettingsScene::at(
            ctx,
            super::settings::Page::Keys,
        ))
    }
}

/// Rows shown at once (beatoraja's layout).
const ROWS: usize = 24;

pub struct KeyConfigScene {
    host: SkinHost,
    mode: usize,
    /// The chosen row (`sel`) and the first row shown.
    list: ListView,
    /// LR2 binding slot (0 keyboard, 1 controller, 2 axis).
    device: usize,
    /// Waiting for a key to bind to the chosen row.
    learn: Learn,
    /// The skin is an LR2 one (its texts 40-49 are the slots).
    lr2: bool,
    /// The rows are drawn here (beatoraja draws them over its skin).
    overlay: bool,
    rows: HitMap<usize>,
    mode_rects: [Rect; 2],
    back_rect: Rect,
    axis: AxisStepper,
    /// Keys and buttons down now, by row (LR2 key timers 100-139).
    pressed: HashMap<Slot, Instant>,
}

impl Default for KeyConfigScene {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyConfigScene {
    pub fn new() -> KeyConfigScene {
        KeyConfigScene {
            host: SkinHost::new(),
            mode: 0,
            list: ListView::new(),
            device: 0,
            learn: Learn::default(),
            lr2: false,
            overlay: true,
            rows: HitMap::default(),
            mode_rects: [Rect::default(); 2],
            back_rect: Rect::default(),
            axis: AxisStepper::default(),
            pressed: HashMap::new(),
        }
    }

    pub fn slot(&self) -> Slot {
        let v = slots(self.mode);
        v[self.list.sel.min(v.len() - 1)]
    }

    pub fn set_mode(&mut self, m: usize) {
        self.mode = m % MODES.len();
        self.list.select(0);
        self.learn.stop();
    }

    fn step(&mut self, d: i32) {
        self.list.step(d, slots(self.mode).len());
        self.learn.stop();
    }

    fn select(&mut self, s: Slot) {
        if let Some(i) = slots(self.mode).iter().position(|x| *x == s) {
            self.list.select(i);
        }
    }

    /// Wait for what to bind to the chosen row.
    fn start_learning(&mut self) {
        self.learn.start(self.slot());
    }

    /// An input while the chosen row waits: bound to it (then on to the
    /// next row), or it clears or cancels.
    pub fn learn_input(&mut self, ctx: &mut Ctx, ev: &InputEv) {
        let Some(slot) = self.learn.slot else {
            return;
        };
        let (key, pad, axis) = match self.learn.input(ev) {
            Learned::Waiting | Learned::Cancel => return,
            Learned::Clear => return self.clear(ctx),
            Learned::Bind(key, pad, axis) => (key, pad, axis),
        };
        if keybind::assign(&mut ctx.config.keys, slot, key, pad, axis) {
            self.learn.stop();
            ctx.save_config();
            ctx.play_sound(Sound::OptionChange);
            let n = slots(self.mode).len();
            self.list.select((self.list.sel + 1).min(n - 1));
        }
    }

    fn clear(&mut self, ctx: &mut Ctx) {
        let dev = self.lr2.then_some(self.device);
        keybind::clear(&mut ctx.config.keys, self.slot(), dev);
        self.learn.stop();
        ctx.save_config();
        ctx.play_sound(Sound::OptionChange);
    }

    fn leave(&mut self, ctx: &mut Ctx) -> Next {
        if self.host.leaving.is_none() {
            ctx.save_config();
            self.learn.stop();
            self.host.leave(Instant::now());
        }
        Next::Stay
    }

    /// A clicked skin button (LR2 / Lunatic Vibes ids).
    pub fn event(&mut self, ctx: &mut Ctx, act: i32) {
        match act {
            101..=136 => {
                let s = slots(self.mode).into_iter().find(|s| lr2_button(*s) == act);
                if let Some(s) = s {
                    self.select(s);
                    self.start_learning();
                }
            }
            143 => self.set_mode(self.mode + 1),
            150..=159 => self.device = (act - 150) as usize,
            _ => return,
        }
        ctx.play_sound(Sound::OptionChange);
    }

    /// Which row a key or button belongs to, in this mode.
    fn row_of(&self, ctx: &Ctx, key: Option<KeyCode>, pad: Option<u64>) -> Option<Slot> {
        slots(self.mode).into_iter().find(|s| {
            keybind::slot_keys_ref(&ctx.config.keys, *s).is_some_and(|lk| {
                key.is_some_and(|k| lk.keys.contains(&k))
                    || pad.is_some_and(|p| lk.pad.contains(&p))
            })
        })
    }

    fn press(&mut self, ctx: &Ctx, key: Option<KeyCode>, pad: Option<u64>, down: bool) {
        if let Some(s) = self.row_of(ctx, key, pad) {
            if down {
                self.pressed.entry(s).or_insert_with(Instant::now);
            } else {
                self.pressed.remove(&s);
            }
        }
    }

    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        match nav {
            Nav::Up => self.step(-1),
            Nav::Down => self.step(1),
            Nav::Decide => self.start_learning(),
            Nav::Close | Nav::Option => self.set_mode(self.mode + 1),
            Nav::Back | Nav::Settings => return self.leave(ctx),
        }
        Next::Stay
    }
}

struct KeyState<'a> {
    s: &'a KeyConfigScene,
    ctx: &'a Ctx,
}

impl KeyState<'_> {
    /// LR2 key timer index of a row (0 scratch, 1-9 keys; +20 for 2P).
    fn timer_slot(s: Slot) -> Option<i32> {
        Some(match s {
            Slot::Key { side, idx } => 20 * side as i32 + idx as i32 + 1,
            Slot::Pms(i) => i as i32 + 1,
            Slot::ScratchUp(sd) | Slot::ScratchDown(sd) => 20 * sd as i32,
            _ => return None,
        })
    }
}

impl SkinState for KeyState<'_> {
    fn timer(&self, id: i32) -> Option<i64> {
        let t = |i: Instant| self.s.host.ms(i);
        match id {
            // keys held down (on) and the others (off since the start)
            // LR2 key timers: 100-109 1P on (0 scratch, 1-9 keys), 110-119
            // 1P off, 120-139 the same for 2P
            100..=139 => {
                let g = (id - 100) / 10;
                let i = 20 * (g / 2) + (id - 100) % 10;
                let held = self
                    .s
                    .pressed
                    .iter()
                    .find(|(s, _)| Self::timer_slot(**s) == Some(i))
                    .map(|(_, at)| t(*at));
                match (g % 2 == 0, held) {
                    (true, h) => h,
                    (false, None) => Some(0),
                    (false, Some(_)) => None,
                }
            }
            _ => self.s.host.timer(id),
        }
    }

    fn option(&self, id: i32) -> bool {
        let v = match id.abs() {
            400..=402 => self.s.mode == (id.abs() - 400) as usize,
            _ => false,
        };
        v != (id < 0)
    }

    fn number(&self, id: i32) -> Option<i64> {
        skinvals::clock_number(id).or(match id {
            20 => Some(self.ctx.view.fps as i64),
            _ => None,
        })
    }

    fn text(&self, id: i32) -> Option<String> {
        let k = &self.ctx.config.keys;
        let rows = slots(self.s.mode);
        let lv = |base: i32| {
            rows.iter()
                .copied()
                .find(|s| lv_text_slot(self.s.mode, *s).map(|t| t + base) == Some(id))
        };
        Some(match id {
            2 => self.ctx.config.player_name.clone(),
            50 => self.s.host.skin.as_ref()?.skin.name.clone(),
            51 => self.s.host.skin.as_ref()?.skin.author.clone(),
            // LR2: the chosen key's binding per slot
            40..=49 if self.s.lr2 => {
                keybind::device_label(k, self.s.slot(), (id - 40) as usize).unwrap_or_default()
            }
            // beatoraja: what row i is bound to
            40..=49 | 240..=283 => {
                let i = if id < 50 { id - 40 } else { id - 230 } as usize;
                keybind::first_label(k, *rows.get(i)?)
            }
            300 => "KEY".into(),
            301 => "BINDING".into(),
            302 => "INPUT".into(),
            303 => "Enter: assign, Del: clear".into(),
            304 => "Left / Right: key mode".into(),
            305 => "Esc: back".into(),
            306 => "Spin the turntable to bind its axis".into(),
            311..=344 => lv(311)?.label(),
            351..=384 => keybind::first_label(k, lv(351)?),
            _ => return None,
        })
    }

    fn rate(&self, _: i32) -> f32 {
        0.0
    }

    fn button(&self, id: i32) -> Option<i64> {
        let s = self.s.slot();
        Some(match id {
            101..=136 => (lr2_button(s) == id) as i64,
            140..=142 => {
                let m = [0, 1, 2][(id - 140) as usize];
                // 140 7 keys, 141 9 keys, 142 5 keys
                if self.s.mode == m {
                    lr2_index(m, s)
                } else {
                    0
                }
            }
            143 => self.s.mode as i64,
            150..=159 => (self.s.device == (id - 150) as usize) as i64,
            _ => return skinvals::config_button(self.ctx, id).or_else(|| self.number(id)),
        })
    }
}

impl Scene for KeyConfigScene {
    fn update(&mut self, ctx: &mut Ctx, gfx: &mut Gfx, now: Instant) -> Next {
        if self.host.first_update() {
            let skin = {
                let st = KeyState { s: self, ctx };
                let stat = |id: i32| match id {
                    400..=402 => Some(id == 400),
                    _ => None,
                };
                skinned::load(ctx, gfx, SkinType::KeyConfig, &stat, &st)
            };
            let Some(skin) = skin else {
                // no usable skin: the settings' page instead
                return Next::Replace(Box::new(super::settings::SettingsScene::at(
                    ctx,
                    super::settings::Page::Keys,
                )));
            };
            let ext = skin.skin.path.extension().map(|e| e.to_ascii_lowercase());
            self.lr2 = ext.is_some_and(|e| e == "lr2skin" || e == "csv");
            let has_keys = skin
                .skin
                .objects
                .iter()
                .any(|o| o.act.is_some_and(|(a, _)| (101..=136).contains(&a)));
            self.overlay = !(self.lr2 && has_keys);
            self.host.skin = Some(skin);
            self.host.created = now;
        }
        self.host.tick(gfx);
        if self.host.faded(now) {
            return Next::Pop;
        }
        Next::Stay
    }

    fn exit(&mut self, ctx: &mut Ctx, gfx: &mut Gfx) {
        ctx.save_config();
        self.host.free(gfx);
    }

    fn realtime(&self) -> bool {
        true
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        if self.host.leaving.is_some() {
            return Next::Stay;
        }
        // keys shown pressed (LR2 key timers)
        match ev {
            InputEv::Key {
                code,
                pressed,
                repeat: false,
                ..
            } => self.press(ctx, Some(*code), None, *pressed),
            InputEv::Pad { id, pressed, .. } => self.press(ctx, None, Some(*id), *pressed),
            _ => {}
        }
        if self.learn.on() {
            self.learn_input(ctx, ev);
            return Next::Stay;
        }
        let ready = self.host.ready(Instant::now());
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::ArrowLeft => self.set_mode(self.mode + MODES.len() - 1),
                KeyCode::ArrowRight => self.set_mode(self.mode + 1),
                KeyCode::Delete => self.clear(ctx),
                KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 => {
                    self.device = match code {
                        KeyCode::Digit1 => 0,
                        KeyCode::Digit2 => 1,
                        _ => 2,
                    }
                }
                c => {
                    if let Some(n) = nav_from_key(*c, &ctx.config.keys) {
                        if ready || matches!(n, Nav::Back | Nav::Settings) {
                            return self.nav(ctx, n);
                        }
                    }
                }
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(n) = nav_from_pad_rows(*id, &ctx.config.keys) {
                    return self.nav(ctx, n);
                }
            }
            // the turntable moves the cursor
            InputEv::Axis { id, value, .. } => match self.axis.step(*id, *value) {
                0 => {}
                d => self.step(d),
            },
            InputEv::Wheel { dy } => {
                let n = (-dy).round() as i32;
                if n != 0 {
                    self.step(n);
                }
            }
            InputEv::Pointer {
                phase: Phase::Up,
                x,
                y,
                ..
            } => {
                if self.back_rect.contains(*x, *y) {
                    return self.leave(ctx);
                }
                match self.host.tap(*x, *y, 0.0) {
                    Tap::Miss => {}
                    Tap::Script => return Next::Stay,
                    Tap::Event(act, _) => {
                        self.event(ctx, act);
                        return Next::Stay;
                    }
                }
                for (i, r) in self.mode_rects.iter().enumerate() {
                    if r.contains(*x, *y) {
                        self.set_mode(self.mode + if i == 0 { MODES.len() - 1 } else { 1 });
                        return Next::Stay;
                    }
                }
                if let Some(&i) = self.rows.at(*x, *y) {
                    // a tap chooses a row, a second one binds it
                    if i == self.list.sel {
                        self.start_learning();
                    } else {
                        self.list.select(i);
                    }
                }
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        self.host
            .draw(d, view, now, &KeyState { s: self, ctx }, &mut NoHooks);
        if self.overlay {
            let xf = self.host.xf(view);
            self.draw_rows(ctx, d, &xf, self.host.size().0);
        } else {
            self.rows.clear();
            self.mode_rects = [Rect::default(); 2];
        }
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        if self.learn.on() {
            let r = Rect::new(w * 0.2, h - 70.0 * ui, w * 0.6, 56.0 * ui);
            ui::panel(d, r);
            let what = format!(
                "{}: press a key or button{}  (Esc cancel, Del clear)",
                self.slot().label(),
                if self.slot().side_of_scratch().is_some() {
                    ", or spin the turntable"
                } else {
                    ""
                }
            );
            d.text(
                &what,
                r.x + r.w * 0.5,
                r.y + 18.0 * ui,
                TextStyle::new(17.0 * ui, theme::TEXT, Align::Center).fit(r.w - 16.0 * ui),
            );
        }
        self.back_rect = ui::back_button(d, w, ui);
    }
}

impl KeyConfigScene {
    /// beatoraja's rows over the skin (its 1280×720 layout).
    fn draw_rows(&mut self, ctx: &Ctx, d: &mut Draw, xf: &Xf, skin_w: f32) {
        let k = skin_w / 1280.0;
        let (sx, sy) = (xf.sx * k, xf.sy * k);
        let p = |x: f32, y: f32| (xf.ox + x * sx, xf.oy + y * sy);
        let size = 20.0 * sy;
        let keys = &ctx.config.keys;
        let rows = slots(self.mode);
        // title with arrows to change the mode
        let (tx, ty) = p(80.0, 50.0);
        let title = format!("<--  {}  -->", MODES[self.mode]);
        let tw = d.measure(&title, size);
        d.text(
            &title,
            tx,
            ty,
            TextStyle::new(size, [0.3, 1.0, 1.0, 1.0], Align::Left),
        );
        self.mode_rects = [
            Rect::new(tx, ty, tw * 0.25, size * 1.4),
            Rect::new(tx + tw * 0.75, ty, tw * 0.25, size * 1.4),
        ];
        for (i, name) in DEVICES.iter().enumerate() {
            let (x, y) = p(200.0 + 170.0 * i as f32, 80.0);
            d.text(
                name,
                x,
                y,
                TextStyle::new(size, [1.0, 1.0, 0.3, 1.0], Align::Left),
            );
        }
        let hint = "↑↓ row  Enter assign  ←→ mode  Del clear  Esc back";
        let (hx, hy) = p(760.0, 80.0);
        d.text(
            hint,
            hx,
            hy,
            TextStyle::new(size * 0.8, [1.0, 0.65, 0.2, 1.0], Align::Left),
        );
        let sel = self.list.sel;
        self.list
            .fit(sel as f32, sel as f32 + 1.0, ROWS as f32, rows.len() as f32);
        let first = self.list.scroll as usize;
        self.rows.clear();
        for (i, s) in rows.iter().enumerate().skip(first).take(ROWS) {
            let (x, y) = p(50.0, 110.0 + 24.0 * (i - first) as f32);
            let row = Rect::new(x, y, 700.0 * sx, 24.0 * sy);
            self.rows.push(row, i);
            if i == sel {
                let c = if self.learn.on() {
                    [0.8, 0.1, 0.1, 0.8]
                } else {
                    [0.1, 0.2, 0.9, 0.8]
                };
                for dx in 0..3 {
                    let (cx, _) = p(200.0 + 170.0 * dx as f32, 0.0);
                    d.b.rect(Rect::new(cx, y, 160.0 * sx, 24.0 * sy), c);
                }
            } else if i % 2 == 0 {
                d.b.rect(row, with_alpha(theme::PANEL, 0.25));
            }
            d.text(
                &s.label(),
                x,
                y + 2.0 * sy,
                TextStyle::new(size, theme::TEXT, Align::Left),
            );
            for dev in 0..3 {
                if dev == 2 && s.side_of_scratch().is_none() {
                    continue;
                }
                let v = keybind::device_label(keys, *s, dev).unwrap_or_else(|| "----".into());
                let (cx, _) = p(202.0 + 170.0 * dev as f32, 0.0);
                d.text(
                    &v,
                    cx,
                    y + 2.0 * sy,
                    TextStyle::new(size, theme::TEXT, Align::Left).fit(155.0 * sx),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lr2_ids() {
        // 7 keys: 1P 1..7, scratch, START, SELECT, then 2P
        assert_eq!(lr2_index(0, Slot::Key { side: 0, idx: 0 }), 1);
        assert_eq!(lr2_index(0, Slot::ScratchDown(0)), 9);
        assert_eq!(lr2_index(0, Slot::Select), 11);
        assert_eq!(lr2_index(0, Slot::Key { side: 1, idx: 6 }), 18);
        assert_eq!(lr2_index(0, Slot::ScratchDown(1)), 20);
        assert_eq!(lr2_index(0, Slot::ScratchAxis(1)), 24);
        // 5 keys: 2P from 10
        assert_eq!(lr2_index(2, Slot::Key { side: 1, idx: 0 }), 10);
        assert_eq!(lr2_index(2, Slot::ScratchUp(1)), 15);
        assert_eq!(lr2_index(2, Slot::ScratchAxis(0)), 19);
        assert_eq!(lr2_index(1, Slot::Pms(8)), 9);
        assert_eq!(lr2_button(Slot::Key { side: 1, idx: 0 }), 121);
        assert_eq!(lr2_button(Slot::Pms(8)), 109);
        assert_eq!(lr2_button(Slot::ScratchAxis(0)), 116);
        // every row of every mode has its own button
        for m in 0..3 {
            let mut b: Vec<i32> = slots(m).into_iter().map(lr2_button).collect();
            let n = b.len();
            b.sort();
            b.dedup();
            assert_eq!(b.len(), n);
        }
    }

    fn key(code: KeyCode) -> InputEv {
        InputEv::Key {
            code,
            pressed: true,
            repeat: false,
            time: Instant::now(),
            text: None,
        }
    }

    #[test]
    fn assign_through_the_screen() {
        let root = std::env::temp_dir().join(format!("vibebms-kcs-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let mut s = KeyConfigScene::new();
        // beatoraja texts: what each row is bound to
        let st = KeyState { s: &s, ctx: &ctx };
        assert_eq!(st.text(40).as_deref(), Some("Z"));
        assert_eq!(st.text(47).as_deref(), Some("ShiftLeft"));
        assert_eq!(st.button(101), Some(1));
        assert_eq!(st.button(140), Some(1));
        assert_eq!(st.button(143), Some(0));
        assert!(st.option(400) && !st.option(401) && st.option(-402));
        // LR2's button 123 chooses 2P key 3 and waits for a key
        s.event(&mut ctx, 123);
        assert_eq!(s.slot(), Slot::Key { side: 1, idx: 2 });
        assert!(s.learn.on());
        s.input(&mut ctx, &key(KeyCode::KeyQ));
        assert_eq!(ctx.config.keys.p2.keys[2].keys, vec![KeyCode::KeyQ]);
        // on to the next row, no longer waiting
        assert_eq!(s.slot(), Slot::Key { side: 1, idx: 3 });
        assert!(!s.learn.on());
        // the keyboard: ↓, Enter, a controller button
        s.input(&mut ctx, &key(KeyCode::ArrowDown));
        s.input(&mut ctx, &key(KeyCode::Enter));
        s.input(
            &mut ctx,
            &InputEv::Pad {
                id: 2_000_005,
                pressed: true,
                time: Instant::now(),
            },
        );
        assert_eq!(ctx.config.keys.p2.keys[4].pad, vec![2_000_005]);
        // LR2 texts 40-49: the chosen key's binding per slot
        s.lr2 = true;
        s.list.sel = 9 + 2;
        let st = KeyState { s: &s, ctx: &ctx };
        assert_eq!(st.text(40).as_deref(), Some("Q"));
        assert_eq!(st.button(140), Some(14));
        assert_eq!(st.button(123), Some(1));
        // 9 keys: another mode, other rows
        s.input(&mut ctx, &key(KeyCode::ArrowRight));
        assert_eq!((s.mode, s.slot()), (1, Slot::Pms(0)));
        // saved with the settings
        let back = crate::config::Config::load(&ctx.paths.config_file());
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(back.keys, ctx.config.keys);
    }

    #[test]
    fn beatoraja_rows() {
        let r = slots(0);
        assert_eq!(r.len(), 24);
        assert_eq!(r[7], Slot::ScratchUp(0));
        assert_eq!(r[18], Slot::Start);
        // the second player's START / SELECT (VS)
        assert_eq!(r[20], Slot::Start2);
        assert_eq!(lr2_index(0, Slot::Start2), 21);
        assert_eq!(slots(1).len(), 11);
        assert_eq!(slots(2)[5], Slot::ScratchUp(0));
    }
}
