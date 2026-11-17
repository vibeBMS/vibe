//! In-game settings: every option lives here (no external launcher).
//!
//! Keyboard: ↑↓ select, ←→ change, Enter activate, Esc back.
//! Mouse/touch: tap a category, tap the left/right half of a value to change it.
//! Controller: the turntable selects, white keys decide / raise, blue keys
//! lower / go back, SELECT leaves.

mod audio;
mod display;
mod ir;
mod judge;
mod keys;
mod library;
mod play;
mod skins;
mod system;
mod tables;
mod touch;
#[cfg(target_arch = "wasm32")]
mod web_storage;

use super::keybind::{self, Learn, Learned, Slot};
use crate::app::{Ctx, Draw, Next, Scene};
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, AxisStepper, InputEv, Nav, Phase};
use crate::platform::{Duration, Instant};
use crate::skin::SkinType;
use crate::theme;
use crate::ui::{self, Armed, HitMap, ListView, TextEvent, TextInput};
use winit::keyboard::KeyCode;

/// A row's height (UI units).
const ROW_H: f32 = 40.0;
/// How often the rows are built again while nothing is done to them: they
/// show what goes on meanwhile (downloads, scans, update checks).
const REFRESH: Duration = Duration::from_millis(250);

/// The categories, in menu order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Play,
    Judge,
    Keys,
    Touch,
    Display,
    Audio,
    Library,
    Tables,
    Ir,
    Skins,
    System,
}

impl Page {
    const ALL: [Page; 11] = [
        Page::Play,
        Page::Judge,
        Page::Keys,
        Page::Touch,
        Page::Display,
        Page::Audio,
        Page::Library,
        Page::Tables,
        Page::Ir,
        Page::Skins,
        Page::System,
    ];

    fn name(self) -> &'static str {
        match self {
            Page::Play => "PLAY",
            Page::Judge => "JUDGE & GAUGE",
            Page::Keys => "KEY CONFIG",
            Page::Touch => "TOUCH",
            Page::Display => "DISPLAY",
            Page::Audio => "AUDIO",
            Page::Library => "LIBRARY",
            Page::Tables => "TABLES",
            Page::Ir => "IR",
            Page::Skins => "SKINS",
            Page::System => "SYSTEM",
        }
    }

    fn rows(self, ctx: &Ctx, armed: &Armed<Confirm>) -> Vec<Row> {
        match self {
            Page::Play => play::rows(ctx),
            Page::Judge => judge::rows(ctx),
            Page::Keys => keys::rows(ctx),
            Page::Touch => touch::rows(ctx),
            Page::Display => display::rows(ctx),
            Page::Audio => audio::rows(ctx),
            Page::Library => library::rows(ctx, armed),
            Page::Tables => tables::rows(ctx),
            Page::Ir => ir::rows(ctx),
            Page::Skins => skins::rows(ctx, armed),
            Page::System => system::rows(ctx),
        }
    }

    fn step(self, d: i32) -> Page {
        crate::scene::option_panel::cycle(&Page::ALL, self, d)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextField {
    PlayerName,
    AddFolder,
    AddTable,
    IrUrl,
    IrName,
    IrToken,
}

/// The screens rows open.
#[derive(Clone, Copy)]
enum Screen {
    KeyConfig,
    SkinSelect,
    /// The skin select on the sound sets.
    SoundSets,
    TouchLayout,
}

/// What choosing a row does.
#[derive(Clone, Copy)]
enum Act {
    None,
    /// Change a value by a step (←→; Enter and taps step up).
    Adjust(fn(&mut Ctx, i32)),
    /// Do `.0` for item `.1` of the page's list (a folder, a table, ...).
    Run(fn(&mut Ctx, usize), usize),
    Text(TextField),
    Bind(Slot),
    Open(Screen),
    /// The skin chosen for a type: the default, then the installed ones.
    CycleSkin(SkinType),
    /// Custom option `.1` of the skin chosen for `.0`.
    SkinOption(SkinType, usize),
    /// Field `.2` (x, y, w, h, angle, alpha) of offset `.1` of that skin.
    SkinOffset(SkinType, usize, usize),
    /// Install catalog skin `.0` (an installed one again on a second
    /// press; a downloading one: cancel).
    SkinGet(usize),
    /// Delete catalog skin `.0`, on a second press.
    SkinDelete(usize),
    /// Web: delete kept song folder `.0` (see `web_storage`), on a second
    /// press.
    #[cfg(target_arch = "wasm32")]
    DeleteKept(usize),
    /// Leave and start the updated executable.
    Restart,
}

impl Act {
    /// ←→ step the value (else they do nothing, and Enter acts).
    fn adjusts(self) -> bool {
        matches!(
            self,
            Act::Adjust(_) | Act::CycleSkin(_) | Act::SkinOption(..) | Act::SkinOffset(..)
        )
    }

    /// The act waits for a second press ([`Confirm`]).
    fn confirms(self) -> bool {
        match self {
            Act::SkinGet(_) | Act::SkinDelete(_) => true,
            #[cfg(target_arch = "wasm32")]
            Act::DeleteKept(_) => true,
            _ => false,
        }
    }
}

/// A row pressed once that a second press confirms.
#[derive(Debug, PartialEq)]
enum Confirm {
    /// Install the installed catalog skin with this id again.
    Reinstall(String),
    DeleteSkin(String),
    #[cfg(target_arch = "wasm32")]
    DeleteKept(usize),
}

struct Row {
    label: String,
    /// Shown at the right (with ◀ ▶ around it when ←→ change it).
    value: String,
    act: Act,
}

impl Row {
    fn new(label: impl Into<String>, value: impl Into<String>, act: Act) -> Row {
        let value = value.into();
        Row {
            label: label.into(),
            value: if act.adjusts() {
                format!("◀ {value} ▶")
            } else {
                value
            },
            act,
        }
    }

    fn adjust(label: impl Into<String>, value: impl Into<String>, f: fn(&mut Ctx, i32)) -> Row {
        Row::new(label, value, Act::Adjust(f))
    }

    /// A line that only says something.
    fn note(label: impl Into<String>, value: impl Into<String>) -> Row {
        Row::new(label, value, Act::None)
    }
}

/// `v` (0..1) in percent.
fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// `n` bytes as MB for people.
fn mb(n: u64) -> String {
    format!("{} MB", (n + (1 << 19)) >> 20)
}

pub struct SettingsScene {
    page: Page,
    rows: Vec<Row>,
    /// When `rows` were built.
    built: Instant,
    /// The rows (scrolled in pixels).
    list: ListView,
    focus_rows: bool,
    edit: Option<(TextField, TextInput)>,
    learn: Learn,
    armed: Armed<Confirm>,
    page_rects: HitMap<Page>,
    /// Each row and the middle of its value: a tap left of it lowers the
    /// value, right of it raises it.
    row_rects: HitMap<(usize, f32)>,
    /// The turntable moves the selection.
    axis: AxisStepper,
    back_rect: Rect,
    /// Where an act asked to go (another screen, or out of the app).
    then: Option<Next>,
}

impl SettingsScene {
    pub fn new(ctx: &mut Ctx) -> SettingsScene {
        let mut s = SettingsScene {
            page: Page::Play,
            rows: Vec::new(),
            built: Instant::now(),
            list: ListView::new(),
            focus_rows: false,
            edit: None,
            learn: Learn::default(),
            armed: Armed::default(),
            page_rects: HitMap::default(),
            row_rects: HitMap::default(),
            axis: AxisStepper::default(),
            back_rect: Rect::default(),
            then: None,
        };
        s.refresh(ctx);
        s
    }

    /// Opened on `page` (its rows chosen).
    pub fn at(ctx: &mut Ctx, page: Page) -> SettingsScene {
        let mut s = SettingsScene::new(ctx);
        s.focus_rows = true;
        s.show(ctx, page);
        s
    }

    fn refresh(&mut self, ctx: &Ctx) {
        self.rows = self.page.rows(ctx, &self.armed);
        self.built = Instant::now();
    }

    fn show(&mut self, ctx: &Ctx, page: Page) {
        self.page = page;
        self.list.sel = 0;
        self.refresh(ctx);
    }

    fn act(&self) -> Act {
        self.rows.get(self.list.sel).map_or(Act::None, |r| r.act)
    }

    /// Choose the selected row: ←→ (`d`), or Enter / a tap (`enter`).
    fn activate(&mut self, ctx: &mut Ctx, d: i32, enter: bool) {
        let act = self.act();
        if !act.confirms() {
            self.armed.clear();
        }
        let step = if d == 0 { 1 } else { d };
        match act {
            Act::None => return,
            Act::Adjust(f) => f(ctx, step),
            Act::Run(f, i) => f(ctx, i),
            Act::CycleSkin(t) => display::cycle_skin(ctx, t, step),
            Act::SkinOption(t, i) => display::step_skin_option(ctx, t, i, step),
            Act::SkinOffset(t, i, f) => display::step_skin_offset(ctx, t, i, f, step),
            Act::SkinGet(i) => skins::get(ctx, &mut self.armed, i),
            Act::SkinDelete(i) => skins::delete(ctx, &mut self.armed, i),
            #[cfg(target_arch = "wasm32")]
            Act::DeleteKept(i) => web_storage::delete(ctx, &mut self.armed, i),
            Act::Text(tf) if enter || d != 0 => {
                let cur = match tf {
                    TextField::PlayerName => ctx.config.player_name.clone(),
                    TextField::IrUrl => ctx.config.ir_server.url.clone(),
                    TextField::IrName => ctx.config.ir_server.name.clone(),
                    _ => String::new(),
                };
                self.edit = Some((tf, TextInput::new(cur)));
            }
            Act::Bind(slot) if enter || d != 0 => self.learn.start(slot),
            Act::Text(_) | Act::Bind(_) => {}
            Act::Open(s) => {
                ctx.save_config();
                use super::skinselect::SkinSelectScene;
                self.then = Some(Next::Push(match s {
                    Screen::KeyConfig => Box::new(super::keyconfig::KeyConfigScene::new()),
                    Screen::SkinSelect => Box::new(SkinSelectScene::new(None)),
                    Screen::SoundSets => Box::new(SkinSelectScene::new(Some(SkinType::SoundSet))),
                    Screen::TouchLayout => {
                        Box::new(super::touch_layout::TouchLayoutScene::new(ctx))
                    }
                }));
            }
            Act::Restart => {
                ctx.net.updater.relaunch_on_exit();
                self.then = Some(Next::Quit);
            }
        }
        ctx.save_config();
        self.refresh(ctx);
    }

    /// Move the selection by `d` rows (or categories, when they have the
    /// focus).
    fn step(&mut self, ctx: &Ctx, d: i32) {
        if self.focus_rows {
            self.list.step(d, self.rows.len());
        } else {
            self.show(ctx, self.page.step(d));
        }
    }

    fn leave(ctx: &mut Ctx) -> Next {
        ctx.save_config();
        Next::Pop
    }

    /// A controller (or the keys bound to the lanes), as on the music
    /// select: the turntable moves, a white key decides (raises a value),
    /// a blue key lowers a value or goes back to the categories, START
    /// goes back to them too, SELECT leaves.
    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        self.list.follow = true;
        let adjust = self.act().adjusts();
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide if !self.focus_rows => self.focus_rows = true,
            Nav::Decide if adjust => self.activate(ctx, 1, false),
            Nav::Decide => self.activate(ctx, 0, true),
            Nav::Close if self.focus_rows && adjust => self.activate(ctx, -1, false),
            Nav::Close | Nav::Option => self.focus_rows = false,
            Nav::Back if self.focus_rows => self.focus_rows = false,
            Nav::Back | Nav::Settings => return Self::leave(ctx),
        }
        Next::Stay
    }

    fn commit_text(&mut self, ctx: &mut Ctx, tf: TextField, s: String) {
        let s = s.trim().to_string();
        match tf {
            TextField::PlayerName => ctx.config.player_name = s,
            TextField::AddFolder => {
                if !s.is_empty() {
                    if !std::path::Path::new(&s).is_dir() && !ctx.paths.root.join(&s).is_dir() {
                        ctx.toast(format!("Not a folder: {s}"));
                        return;
                    }
                    ctx.config.bms_dirs.push(s);
                    ctx.start_scan();
                }
            }
            TextField::AddTable => {
                if !s.is_empty() {
                    ctx.config.tables.push(crate::config::TableConfig {
                        name: String::new(),
                        url: s,
                        enabled: true,
                    });
                    ctx.tables.refresh(&ctx.config.tables, false);
                }
            }
            TextField::IrUrl => ctx.config.ir_server.url = s,
            TextField::IrName => ctx.config.ir_server.name = s,
            TextField::IrToken => ctx.config.ir_server.token = s,
        }
        ctx.save_config();
        self.refresh(ctx);
    }

    /// An input while a row waits for its key.
    fn learn_input(&mut self, ctx: &mut Ctx, ev: &InputEv) {
        let Some(slot) = self.learn.slot else {
            return;
        };
        match self.learn.input(ev) {
            Learned::Waiting | Learned::Cancel => return,
            Learned::Clear => keybind::clear(&mut ctx.config.keys, slot, None),
            Learned::Bind(key, pad, axis) => {
                keybind::assign(&mut ctx.config.keys, slot, key, pad, axis);
                // on to the next row for quick setup
                self.list.sel = (self.list.sel + 1).min(self.rows.len().saturating_sub(1));
            }
        }
        self.learn.stop();
        ctx.save_config();
        self.refresh(ctx);
    }

    fn input_inner(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        if self.learn.on() {
            self.learn_input(ctx, ev);
            return Next::Stay;
        }
        if let Some((tf, t)) = self.edit.as_mut() {
            if let InputEv::Dropped(p) = ev {
                t.buf.push_str(&p.to_string_lossy());
                return Next::Stay;
            }
            match t.input(ev) {
                TextEvent::Editing => {}
                TextEvent::Cancel => self.edit = None,
                TextEvent::Commit(s) => {
                    let tf = *tf;
                    self.edit = None;
                    self.commit_text(ctx, tf, s);
                }
            }
            return Next::Stay;
        }
        if matches!(ev, InputEv::Key { pressed: true, .. }) {
            self.list.follow = true;
        }
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::Escape | KeyCode::Backspace if self.focus_rows => self.focus_rows = false,
                KeyCode::Escape | KeyCode::Backspace | KeyCode::F1 | KeyCode::Tab => {
                    return Self::leave(ctx)
                }
                KeyCode::ArrowUp => self.step(ctx, -1),
                KeyCode::ArrowDown => self.step(ctx, 1),
                KeyCode::ArrowLeft if self.focus_rows => self.activate(ctx, -1, false),
                KeyCode::ArrowRight if self.focus_rows => self.activate(ctx, 1, false),
                KeyCode::ArrowLeft => {}
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space if self.focus_rows => {
                    self.activate(ctx, 0, true)
                }
                KeyCode::ArrowRight | KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                    self.focus_rows = true
                }
                KeyCode::PageUp => self.show(ctx, self.page.step(-1)),
                KeyCode::PageDown => self.show(ctx, self.page.step(1)),
                // keys bound to the lanes (a controller seen as a keyboard)
                other => {
                    if let Some(nav) = nav_from_key(*other, &ctx.config.keys) {
                        return self.nav(ctx, nav);
                    }
                }
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(nav) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.nav(ctx, nav);
                }
            }
            InputEv::Axis { id, value, .. } if ctx.config.keys.any_axis_is_scratch => {
                match self.axis.step(*id, *value) {
                    0 => {}
                    d if d > 0 => return self.nav(ctx, Nav::Down),
                    _ => return self.nav(ctx, Nav::Up),
                }
            }
            InputEv::Wheel { dy } => self.list.wheel(-dy * ROW_H),
            // the rows: a drag scrolls them, a tap (lifted where it went
            // down) acts on the row
            InputEv::Pointer {
                id,
                phase: Phase::Move,
                y,
                ..
            } => {
                self.list.drag_to(*id, *y, ui::DRAG_SLOP * ctx.ui());
            }
            InputEv::Pointer {
                id,
                phase: Phase::Up,
                ..
            } => {
                if let Some((x, y)) = self.list.release(*id) {
                    self.tap_row(ctx, x, y);
                }
            }
            InputEv::Pointer {
                id,
                phase: Phase::Down,
                x,
                y,
                ..
            } => {
                if self.back_rect.contains(*x, *y) {
                    return Self::leave(ctx);
                }
                if let Some(&p) = self.page_rects.at(*x, *y) {
                    self.show(ctx, p);
                    self.list.scroll = 0.0;
                    self.focus_rows = false;
                } else if self.row_rects.at(*x, *y).is_some() {
                    self.list.press(*id, *x, *y);
                }
            }
            _ => {}
        }
        Next::Stay
    }

    /// A tap on the rows at (`x`, `y`): the row's action, by where on it.
    fn tap_row(&mut self, ctx: &mut Ctx, x: f32, y: f32) {
        let Some((r, &(i, mid))) = self.row_rects.hit(x, y) else {
            return;
        };
        self.focus_rows = true;
        self.list.sel = i;
        if !self.act().adjusts() {
            self.activate(ctx, 0, true);
        } else if x > r.x + r.w * 0.55 {
            // on the value: its left half lowers it
            self.activate(ctx, if x < mid { -1 } else { 1 }, false);
        } else {
            self.activate(ctx, 1, false);
        }
    }

    fn draw_inner(&mut self, ctx: &mut Ctx, d: &mut Draw) {
        let ui = ctx.ui();
        let (w, h) = (ctx.view.width, ctx.view.height);
        d.b.gradient(
            Rect::new(0.0, 0.0, w, h),
            ui::lift(theme::BACKGROUND, 0.04),
            theme::BACKGROUND,
        );
        let header = 48.0 * ui;
        d.b.rect(Rect::new(0.0, 0.0, w, header), theme::PANEL);
        d.b.rect(
            Rect::new(0.0, header - 2.0, w, 2.0),
            with_alpha(theme::ACCENT, 0.6),
        );
        d.text(
            "SETTINGS",
            16.0 * ui,
            12.0 * ui,
            22.0 * ui,
            theme::ACCENT,
            Align::Left,
        );
        d.text(
            "↑↓ select  ←→ change  Enter edit  Esc back",
            w * 0.5,
            15.0 * ui,
            15.0 * ui,
            theme::TEXT_DIM,
            Align::Center,
        );
        self.back_rect = ui::back_button(d, w, ui);
        let rh = ROW_H * ui;
        let portrait = ctx.portrait();
        // portrait: the categories in rows of five over the list
        let per_line = 5;
        let (cat_r, list_r) = if portrait {
            let ch = Page::ALL.len().div_ceil(per_line) as f32 * rh;
            (
                Rect::new(0.0, header, w, ch),
                Rect::new(0.0, header + ch, w, h - header - ch),
            )
        } else {
            let cw = 220.0 * ui;
            (
                Rect::new(0.0, header, cw, h - header),
                Rect::new(cw, header, w - cw, h - header),
            )
        };
        self.page_rects.clear();
        for (i, p) in Page::ALL.into_iter().enumerate() {
            let r = if portrait {
                let bw = w / per_line as f32;
                Rect::new(
                    (i % per_line) as f32 * bw + 2.0,
                    cat_r.y + (i / per_line) as f32 * rh + 2.0,
                    bw - 4.0,
                    rh - 4.0 * ui,
                )
            } else {
                Rect::new(
                    cat_r.x + 8.0 * ui,
                    cat_r.y + 8.0 * ui + i as f32 * (rh + 4.0 * ui),
                    cat_r.w - 16.0 * ui,
                    rh,
                )
            };
            ui::button(d, r, p.name(), ui * 0.95, p == self.page);
            if p == self.page && !self.focus_rows {
                // keyboard focus on the categories: accent tab on the left
                d.b.rect(
                    Rect::new(r.x - 4.0, r.y + 6.0, 3.0, r.h - 12.0),
                    theme::ACCENT,
                );
            }
            self.page_rects.push(r, p);
        }
        // keep the chosen row in view after keys (with one below it); never
        // past the last row
        let sel_y = self.list.sel as f32 * rh;
        let content = 16.0 * ui + self.rows.len() as f32 * rh;
        self.list.fit(sel_y, sel_y + rh * 2.0, list_r.h, content);
        self.row_rects.clear();
        d.b.set_clip(Some(list_r));
        let size = 17.0 * ui;
        for (i, row) in self.rows.iter().enumerate() {
            let y = list_r.y + 8.0 * ui + i as f32 * rh - self.list.scroll;
            if y + rh < list_r.y || y > list_r.bottom() {
                continue;
            }
            let r = Rect::new(list_r.x + 10.0 * ui, y, list_r.w - 20.0 * ui, rh - 4.0);
            ui::row(d, r, i == self.list.sel && self.focus_rows);
            let ty = r.y + (r.h - size) * 0.5;
            d.text_fit(
                &row.label,
                r.x + 12.0 * ui,
                ty,
                size,
                theme::TEXT,
                Align::Left,
                r.w * 0.55,
            );
            let color = match row.act {
                Act::None => theme::TEXT_DIM,
                _ => theme::ACCENT,
            };
            let right = r.right() - 12.0 * ui;
            let drawn = d.text_fit(&row.value, right, ty, size, color, Align::Right, r.w * 0.42);
            self.row_rects.push(r, (i, right - drawn * 0.5));
        }
        d.b.set_clip(None);
        if let Some((tf, t)) = self.edit.as_mut() {
            t.draw_modal(ctx, d, &format!("{tf:?}"), *tf == TextField::IrToken);
        }
        if let Some(slot) = self.learn.slot {
            let r = Rect::new(w * 0.2, h * 0.38, w * 0.6, 90.0 * ui);
            ui::modal(d, w, h, r);
            let what = if matches!(slot, Slot::ScratchAxis(_)) {
                "Spin the turntable"
            } else {
                "Press a key or controller button"
            };
            let x = r.x + r.w * 0.5;
            d.text(
                what,
                x,
                r.y + 18.0 * ui,
                22.0 * ui,
                theme::TEXT,
                Align::Center,
            );
            d.text(
                "Esc = cancel, Delete = clear",
                x,
                r.y + 54.0 * ui,
                15.0 * ui,
                theme::TEXT_DIM,
                Align::Center,
            );
        }
    }
}

impl Scene for SettingsScene {
    fn update(&mut self, ctx: &mut Ctx, _gfx: &mut Gfx, now: Instant) -> Next {
        if now.saturating_duration_since(self.built) >= REFRESH {
            self.refresh(ctx);
        }
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        let next = self.input_inner(ctx, ev);
        self.then.take().unwrap_or(next)
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, _now: Instant) {
        self.draw_inner(ctx, d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Instant;

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
    fn rows_adjust_or_act() {
        let r = Row::adjust("Lane cover", "ON", |c, _| c.config.play.lane_cover ^= true);
        assert_eq!(r.value, "◀ ON ▶");
        let r = Row::new(
            "Rescan library",
            "10 charts",
            Act::Run(|c, _| c.start_scan(), 0),
        );
        assert_eq!(r.value, "10 charts");
        assert!(!r.act.adjusts());
        assert_eq!(Page::Play.step(-1), Page::System);
        assert_eq!(Page::System.step(1), Page::Play);
    }

    #[test]
    fn every_page_has_rows_and_keys_change_them() {
        let root = std::env::temp_dir().join(format!("vibebms-settings-{}", std::process::id()));
        let mut ctx = Ctx::for_test(root.clone());
        let mut s = SettingsScene::at(&mut ctx, Page::Play);
        for p in Page::ALL {
            s.show(&ctx, p);
            assert!(!s.rows.is_empty(), "{p:?}");
        }
        // PLAY: the third row is RANDOM (2P); → steps it and the row shows it
        s.show(&ctx, Page::Play);
        s.input(&mut ctx, &key(KeyCode::ArrowDown));
        s.input(&mut ctx, &key(KeyCode::ArrowDown));
        let before = ctx.config.play.random_2p;
        s.input(&mut ctx, &key(KeyCode::ArrowRight));
        assert_ne!(ctx.config.play.random_2p, before);
        assert!(s.rows[2].value.contains(ctx.config.play.random_2p.label()));
        // KEY CONFIG: Enter waits for a key, the key binds, on to the next row
        s.show(&ctx, Page::Keys);
        s.input(&mut ctx, &key(KeyCode::Enter));
        assert!(s.learn.on());
        s.input(&mut ctx, &key(KeyCode::KeyQ));
        assert_eq!(ctx.config.keys.p1.keys[0].keys, vec![KeyCode::KeyQ]);
        assert!(!s.learn.on());
        assert_eq!(s.list.sel, 1);
        std::fs::remove_dir_all(&root).ok();
    }
}
