//! Touch layout editor (KEY CONFIG / TOUCH): the touch buttons over the
//! screen as play shows them, with START / SELECT. Drag a button to move
//! it; drag its corners, or pinch it, to resize it; drag the top edge of
//! the controls to change their height. Editing a preset (flipping it
//! too) copies it into the custom layout of the key mode and orientation
//! shown.
//!
//! Keyboard: ←→ layout, ↑↓ key mode, +/- controls height, F flip,
//! S START/SELECT side, R reset, Enter save, Esc cancel.
//! Controller: key 1 layout, 2 key mode, 3/5 controls lower/higher,
//! 4 START/SELECT side, 6 flip, 7 reset, START save, SELECT cancel.

use crate::config::{Side, TouchConfig, TouchSide, TouchStyle};
use crate::ctx::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::theme;
use crate::gfx::{rgba, with_alpha, Draw, Gfx, Rect};
use crate::input::{nav_from_pad, InputEv, Nav, Phase};
use crate::platform::Instant;
use crate::scene::{Next, Scene};
use crate::touch::{self, TouchInput, Zone};
use crate::ui;
use std::collections::HashMap;
use vibebms_core::PlayMode;
use winit::keyboard::KeyCode;

/// Key modes in menu order.
const MODES: [PlayMode; 8] = [
    PlayMode::Beat7K,
    PlayMode::Beat5K,
    PlayMode::Beat14K,
    PlayMode::Beat10K,
    PlayMode::Popn9K,
    PlayMode::Key4,
    PlayMode::Key6,
    PlayMode::Key8,
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Mode,
    Layout,
    Height,
    Side,
    Flip,
    Reset,
    Cancel,
    Save,
}

#[derive(Clone, Copy, Debug)]
enum Grab {
    /// One pointer on a button: it moves once the pointer went past a slop.
    Move {
        id: u64,
        lane: u8,
        from: (f32, f32),
        orig: Rect,
        moved: bool,
    },
    /// A corner handle (0 top left, 1 top right, 2 bottom left, 3 bottom
    /// right): the opposite corner stays.
    Resize {
        id: u64,
        lane: u8,
        corner: usize,
        orig: Rect,
    },
    /// Two pointers: scale about their middle, which moves the button.
    Pinch {
        a: u64,
        b: u64,
        lane: u8,
        d0: f32,
        mid0: (f32, f32),
        orig: Rect,
    },
    /// The top edge of the controls: their height.
    Strip { id: u64 },
}

pub struct TouchLayoutScene {
    mode: usize,
    /// The touch settings when the editor opened (cancel restores them).
    orig: TouchConfig,
    sel: Option<u8>,
    ptrs: HashMap<u64, (f32, f32)>,
    grab: Option<Grab>,
    tools: Vec<(Rect, Tool)>,
    /// The play skin's aspect by key mode and orientation.
    aspects: HashMap<(usize, bool), f32>,
}

/// The smallest button: about a fingertip.
fn least(ctx: &Ctx) -> f32 {
    6.0 * ctx.px_per_mm()
}

/// `r` at least `least` across, no bigger than the screen, a quarter of it
/// on the screen at least.
fn on_screen(r: Rect, sw: f32, sh: f32, least: f32) -> Rect {
    let w = r.w.max(least).min(sw);
    let h = r.h.max(least).min(sh);
    Rect::new(
        r.x.clamp(-w * 0.75, sw - w * 0.25),
        r.y.clamp(-h * 0.75, sh - h * 0.25),
        w,
        h,
    )
}

/// `orig` with corner `corner` dragged to (x, y): the opposite corner
/// stays. A round button stays square.
fn resized(orig: Rect, corner: usize, x: f32, y: f32, least: f32, round: bool) -> Rect {
    let left = corner.is_multiple_of(2);
    let top = corner < 2;
    let fx = if left { orig.right() } else { orig.x };
    let fy = if top { orig.bottom() } else { orig.y };
    let mut w = (x - fx).abs().max(least);
    let mut h = (y - fy).abs().max(least);
    if round {
        w = w.max(h);
        h = w;
    }
    Rect::new(
        if left { fx - w } else { fx },
        if top { fy - h } else { fy },
        w,
        h,
    )
}

/// `orig` pinched from fingers `d0` apart around `mid0` to fingers at `pa`
/// and `pb`: scaled by their distance, moved with their middle.
fn pinched(orig: Rect, d0: f32, mid0: (f32, f32), pa: (f32, f32), pb: (f32, f32)) -> Rect {
    let s = ((pa.0 - pb.0).hypot(pa.1 - pb.1) / d0).clamp(0.2, 5.0);
    let mid = ((pa.0 + pb.0) * 0.5, (pa.1 + pb.1) * 0.5);
    let (cx, cy) = orig.center();
    let (cx, cy) = (cx + mid.0 - mid0.0, cy + mid.1 - mid0.1);
    let (w, h) = (orig.w * s, orig.h * s);
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

impl TouchLayoutScene {
    pub fn new(ctx: &mut Ctx) -> TouchLayoutScene {
        TouchLayoutScene {
            mode: 0,
            orig: ctx.config.touch.clone(),
            sel: None,
            ptrs: HashMap::new(),
            grab: None,
            tools: Vec::new(),
            aspects: HashMap::new(),
        }
    }

    fn mode(&self) -> PlayMode {
        MODES[self.mode]
    }

    fn area(ctx: &Ctx) -> Rect {
        touch::controls_area(ctx.view.width, ctx.view.height, &ctx.config.touch)
    }

    fn side2(ctx: &Ctx) -> bool {
        ctx.config.play.side == Side::P2
    }

    /// The buttons as play uses them.
    fn zones(&self, ctx: &Ctx) -> Vec<Zone> {
        touch::zones(
            self.mode(),
            &ctx.config.touch,
            Self::area(ctx),
            Self::side2(ctx),
            ctx.portrait(),
            ctx.px_per_mm(),
        )
    }

    /// The play screen and START / SELECT beside it, as play places them.
    fn system(&mut self, ctx: &Ctx) -> (Rect, [Zone; 2]) {
        let mode = self.mode();
        let aspect = *self
            .aspects
            .entry((self.mode, ctx.portrait()))
            .or_insert_with(|| super::play_skin::aspect(ctx, mode));
        let screen = Rect::new(0.0, 0.0, ctx.view.width, Self::area(ctx).y);
        let right = touch::system_right(&ctx.config.touch, mode, Self::side2(ctx), ctx.portrait());
        touch::system_buttons(screen, aspect, ctx.px_per_mm(), right)
    }

    /// Where a touch takes the top edge of the controls: a finger's width
    /// around it, mostly above (keys may start right at it).
    fn edge(ctx: &Ctx) -> Rect {
        let y = Self::area(ctx).y;
        let mm = ctx.px_per_mm();
        Rect::new(0.0, y - 5.0 * mm, ctx.view.width, 6.5 * mm)
    }

    fn custom(&self, ctx: &Ctx) -> bool {
        touch::custom_layout(&ctx.config.touch, self.mode(), ctx.portrait()).is_some()
    }

    /// Make the shown layout this mode's custom one: a preset is copied.
    fn make_custom(&mut self, ctx: &mut Ctx) {
        if self.custom(ctx) {
            return;
        }
        let area = Self::area(ctx);
        let t = &ctx.config.touch;
        let base = touch::preset_of(t);
        let zones = touch::preset_zones(
            self.mode(),
            base,
            area,
            t,
            Self::side2(ctx),
            ctx.px_per_mm(),
        );
        let slot = touch::slot(self.mode(), ctx.portrait());
        let t = &mut ctx.config.touch;
        t.layouts.insert(slot, touch::to_layout(&zones, area));
        t.style = TouchStyle::Custom;
        t.custom_from = base;
    }

    /// Put button `lane` at `r` (on the screen), no smaller than a
    /// fingertip and partly on the screen.
    fn set_rect(&mut self, ctx: &mut Ctx, lane: u8, r: Rect) {
        self.make_custom(ctx);
        let r = on_screen(r, ctx.view.width, ctx.view.height, least(ctx));
        let area = Self::area(ctx);
        let slot = touch::slot(self.mode(), ctx.portrait());
        if let Some(b) = ctx
            .config
            .touch
            .layouts
            .get_mut(&slot)
            .and_then(|l| l.buttons.iter_mut().find(|b| b.lane == lane))
        {
            touch::set_button_rect(b, r, area);
        }
    }

    fn rect_of(&self, ctx: &Ctx, lane: u8) -> Option<(Rect, bool)> {
        self.zones(ctx)
            .iter()
            .find(|z| z.lane == lane)
            .map(|z| (z.bounds(), matches!(z.shape, touch::Shape::Circle { .. })))
    }

    fn handle_size(ctx: &Ctx) -> f32 {
        (5.0 * ctx.px_per_mm()).max(16.0 * ctx.ui())
    }

    /// The corner handles of a button's rect.
    fn handles(r: Rect, s: f32) -> [Rect; 4] {
        let at = |x: f32, y: f32| Rect::new(x - s * 0.5, y - s * 0.5, s, s);
        [
            at(r.x, r.y),
            at(r.right(), r.y),
            at(r.x, r.bottom()),
            at(r.right(), r.bottom()),
        ]
    }

    /// The controls' share of the screen height, in steps of 1%.
    fn set_height(&mut self, ctx: &mut Ctx, share: f32) {
        let (lo, hi) = touch::CONTROLS_HEIGHT;
        let portrait = ctx.portrait();
        let h = touch::controls_share_mut(&mut ctx.config.touch, portrait);
        *h = ((share * 100.0).round() / 100.0).clamp(lo, hi);
    }

    fn step_height(&mut self, ctx: &mut Ctx, d: i32) {
        let h = touch::controls_share(&ctx.config.touch, ctx.portrait());
        self.set_height(ctx, h + d as f32 * 0.01);
        self.grab = None;
    }

    fn step_side(&mut self, ctx: &mut Ctx, d: i32) {
        let s = &mut ctx.config.touch.start_select_side;
        let all = TouchSide::ALL;
        let i = all.iter().position(|x| x == s).unwrap_or(0) as i32;
        *s = all[(i + d).rem_euclid(all.len() as i32) as usize];
    }

    /// Mirror the layout shown left to right.
    fn flip(&mut self, ctx: &mut Ctx) {
        self.make_custom(ctx);
        let slot = touch::slot(self.mode(), ctx.portrait());
        if let Some(l) = ctx.config.touch.layouts.get_mut(&slot) {
            touch::flip(l);
        }
        self.grab = None;
    }

    fn step_mode(&mut self, d: i32) {
        let n = MODES.len() as i32;
        self.mode = (self.mode as i32 + d).rem_euclid(n) as usize;
        self.sel = None;
        self.grab = None;
    }

    fn step_layout(&mut self, ctx: &mut Ctx, d: i32) {
        let slot = touch::slot(self.mode(), ctx.portrait());
        let t = &mut ctx.config.touch;
        let all = TouchStyle::ALL;
        let i = all.iter().position(|s| *s == t.style).unwrap_or(0) as i32;
        let next = all[(i + d).rem_euclid(all.len() as i32) as usize];
        // a mode with no layout of its own starts Custom from the preset
        // just shown
        if next == TouchStyle::Custom
            && t.style != TouchStyle::Custom
            && !t.layouts.contains_key(&slot)
        {
            t.custom_from = t.style;
        }
        t.style = next;
        self.grab = None;
    }

    fn reset(&mut self, ctx: &mut Ctx) {
        // only the layout shown: a preset leaves the custom one alone
        if self.custom(ctx) {
            let slot = touch::slot(self.mode(), ctx.portrait());
            ctx.config.touch.layouts.remove(&slot);
        } else {
            ctx.toast(tr!("This layout is a preset already"));
        }
        self.grab = None;
    }

    fn save(&mut self, ctx: &mut Ctx) -> Next {
        ctx.save_config();
        Next::Pop
    }

    fn cancel(&mut self, ctx: &mut Ctx) -> Next {
        ctx.config.touch = self.orig.clone();
        Next::Pop
    }

    fn tool(&mut self, ctx: &mut Ctx, tool: Tool, r: Rect, x: f32) -> Next {
        let d = if x < r.x + r.w * 0.5 { -1 } else { 1 };
        match tool {
            Tool::Mode => self.step_mode(d),
            Tool::Layout => self.step_layout(ctx, d),
            Tool::Height => self.step_height(ctx, d),
            Tool::Side => self.step_side(ctx, d),
            Tool::Flip => self.flip(ctx),
            Tool::Reset => self.reset(ctx),
            Tool::Cancel => return self.cancel(ctx),
            Tool::Save => return self.save(ctx),
        }
        Next::Stay
    }

    fn down(&mut self, ctx: &mut Ctx, id: u64, x: f32, y: f32) -> Next {
        self.ptrs.insert(id, (x, y));
        // a second finger on a held button pinches it
        if let Some(Grab::Move { id: a, lane, .. }) = self.grab {
            if a != id {
                if let (Some(&pa), Some((orig, _))) = (self.ptrs.get(&a), self.rect_of(ctx, lane)) {
                    self.grab = Some(Grab::Pinch {
                        a,
                        b: id,
                        lane,
                        d0: (pa.0 - x).hypot(pa.1 - y).max(1.0),
                        mid0: ((pa.0 + x) * 0.5, (pa.1 + y) * 0.5),
                        orig,
                    });
                }
                return Next::Stay;
            }
        }
        if self.grab.is_some() {
            return Next::Stay;
        }
        if let Some(&(r, tool)) = self.tools.iter().find(|(r, _)| r.contains(x, y)) {
            return self.tool(ctx, tool, r, x);
        }
        // the selected button's corners
        if let Some((lane, (r, _))) = self.sel.and_then(|l| Some((l, self.rect_of(ctx, l)?))) {
            let hs = Self::handle_size(ctx) * 1.4;
            if let Some(corner) = Self::handles(r, hs).iter().position(|h| h.contains(x, y)) {
                self.grab = Some(Grab::Resize {
                    id,
                    lane,
                    corner,
                    orig: r,
                });
                return Next::Stay;
            }
        }
        let zones = self.zones(ctx);
        let hit = zones.iter().rposition(|z| z.hit(x, y));
        if hit.is_none() && Self::edge(ctx).contains(x, y) {
            self.grab = Some(Grab::Strip { id });
            return Next::Stay;
        }
        match hit.or_else(|| touch::pick(&zones, x, y)) {
            Some(i) => {
                let z = &zones[i];
                self.sel = Some(z.lane);
                self.grab = Some(Grab::Move {
                    id,
                    lane: z.lane,
                    from: (x, y),
                    orig: z.bounds(),
                    moved: false,
                });
            }
            None => self.sel = None,
        }
        Next::Stay
    }

    fn moved(&mut self, ctx: &mut Ctx, id: u64, x: f32, y: f32) {
        if let Some(p) = self.ptrs.get_mut(&id) {
            *p = (x, y);
        }
        let Some(grab) = self.grab else { return };
        match grab {
            Grab::Move {
                id: g,
                lane,
                from,
                orig,
                moved,
            } if g == id => {
                let (dx, dy) = (x - from.0, y - from.1);
                if !moved && dx.hypot(dy) < 2.0 * ctx.px_per_mm() {
                    return;
                }
                self.grab = Some(Grab::Move {
                    id,
                    lane,
                    from,
                    orig,
                    moved: true,
                });
                self.set_rect(
                    ctx,
                    lane,
                    Rect::new(orig.x + dx, orig.y + dy, orig.w, orig.h),
                );
            }
            Grab::Resize {
                id: g,
                lane,
                corner,
                orig,
            } if g == id => {
                let round = self.rect_of(ctx, lane).is_some_and(|(_, round)| round);
                let r = resized(orig, corner, x, y, least(ctx), round);
                self.set_rect(ctx, lane, r);
            }
            Grab::Pinch {
                a,
                b,
                lane,
                d0,
                mid0,
                orig,
            } if a == id || b == id => {
                let (Some(&pa), Some(&pb)) = (self.ptrs.get(&a), self.ptrs.get(&b)) else {
                    return;
                };
                self.set_rect(ctx, lane, pinched(orig, d0, mid0, pa, pb));
            }
            Grab::Strip { id: g } if g == id => {
                let h = ctx.view.height;
                self.set_height(ctx, (h - y) / h.max(1.0));
            }
            _ => {}
        }
    }

    fn up(&mut self, ctx: &mut Ctx, id: u64) {
        self.ptrs.remove(&id);
        match self.grab {
            Some(Grab::Pinch { a, b, lane, .. }) if a == id || b == id => {
                // the finger left goes on moving the button
                let other = if a == id { b } else { a };
                self.grab = match (self.ptrs.get(&other), self.rect_of(ctx, lane)) {
                    (Some(&from), Some((orig, _))) => Some(Grab::Move {
                        id: other,
                        lane,
                        from,
                        orig,
                        moved: true,
                    }),
                    _ => None,
                };
            }
            Some(Grab::Move { id: g, .. } | Grab::Resize { id: g, .. } | Grab::Strip { id: g })
                if g == id =>
            {
                self.grab = None
            }
            _ => {}
        }
    }

    fn layout_label(ctx: &Ctx) -> String {
        let t = &ctx.config.touch;
        match t.style {
            TouchStyle::Custom => tr!("CUSTOM").into(),
            s => s.label().into(),
        }
    }
}

impl Scene for TouchLayoutScene {
    fn update(&mut self, _ctx: &mut Ctx, _gfx: &mut Gfx, _now: Instant) -> Next {
        Next::Stay
    }

    fn full_screen(&self) -> bool {
        true
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::Escape | KeyCode::Backspace => return self.cancel(ctx),
                KeyCode::Enter | KeyCode::NumpadEnter => return self.save(ctx),
                KeyCode::ArrowLeft => self.step_layout(ctx, -1),
                KeyCode::ArrowRight => self.step_layout(ctx, 1),
                KeyCode::ArrowUp => self.step_mode(-1),
                KeyCode::ArrowDown => self.step_mode(1),
                KeyCode::KeyR => self.reset(ctx),
                KeyCode::KeyF => self.flip(ctx),
                KeyCode::KeyS => self.step_side(ctx, 1),
                KeyCode::Equal | KeyCode::NumpadAdd | KeyCode::PageUp => self.step_height(ctx, 1),
                KeyCode::Minus | KeyCode::NumpadSubtract | KeyCode::PageDown => {
                    self.step_height(ctx, -1)
                }
                _ => {}
            },
            // a touch editor: a controller only leaves it
            InputEv::Pad {
                id, pressed: true, ..
            } => match nav_from_pad(*id, &ctx.config.keys) {
                Some(Nav::Option) => return self.save(ctx),
                Some(Nav::Settings) => return self.cancel(ctx),
                _ => {}
            },
            InputEv::Pointer {
                id, phase, x, y, ..
            } => match phase {
                Phase::Down => return self.down(ctx, *id, *x, *y),
                Phase::Move => self.moved(ctx, *id, *x, *y),
                Phase::Up => self.up(ctx, *id),
            },
            InputEv::Released { .. } => {
                self.ptrs.clear();
                self.grab = None;
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &Ctx, d: &mut Draw, _now: Instant) {
        let ui = ctx.ui();
        let mm = ctx.px_per_mm();
        let (w, h) = (ctx.view.width, ctx.view.height);
        let area = Self::area(ctx);
        d.b.rect(Rect::new(0.0, 0.0, w, h), theme::BACKGROUND);
        // where the play screen goes, START / SELECT beside it
        let (view, system) = self.system(ctx);
        d.b.gradient(
            view,
            ui::lift(theme::BACKGROUND, 0.06),
            ui::lift(theme::BACKGROUND, 0.02),
        );
        d.text(
            tr!("PLAY SCREEN"),
            view.x + view.w * 0.5,
            view.y + (view.h - 20.0 * ui) * 0.5 + 30.0 * ui,
            TextStyle::new(20.0 * ui, with_alpha(theme::TEXT_DIM, 0.5), Align::Center),
        );
        let opacity = ctx.config.touch.opacity;
        TouchInput::default().draw(d.b, &system, opacity);
        for (z, label) in system.iter().zip(["START", "SELECT"]) {
            let r = z.bounds();
            let size = (r.w * 0.22).min(14.0 * ui);
            d.text(
                label,
                r.x + r.w * 0.5,
                r.y + (r.h - size) * 0.5,
                TextStyle::new(size, theme::TEXT, Align::Center).fit(r.w * 0.9),
            );
        }
        let side2 = Self::side2(ctx);
        for (a, _, _) in touch::player_areas(self.mode(), area, &ctx.config.touch, side2, mm) {
            d.b.rect(a, rgba(0x05060aff));
        }
        // the top edge of the controls, with a grip to drag
        let dragging = matches!(self.grab, Some(Grab::Strip { .. }));
        let edge = if dragging {
            theme::ACCENT
        } else {
            with_alpha(theme::TEXT_DIM, 0.6)
        };
        d.b.rect(Rect::new(0.0, area.y - 1.0, w, 2.0), edge);
        let (gw, gh) = (16.0 * mm, 2.5 * mm);
        d.b.rect(
            Rect::new((w - gw) * 0.5, area.y - gh - 1.5 * mm, gw, gh),
            edge,
        );
        if dragging {
            let share = touch::controls_share(&ctx.config.touch, ctx.portrait());
            let size = 16.0 * ui;
            d.text(
                &tr!(
                    "CONTROLS {percent}%",
                    percent = format!("{:.0}", share * 100.0)
                ),
                w * 0.5,
                area.y - gh - 3.0 * mm - size * 1.2,
                TextStyle::new(size, theme::ACCENT, Align::Center),
            );
        }
        // the buttons, drawn as in play
        let zones = self.zones(ctx);
        TouchInput::default().draw(d.b, &zones, opacity);
        let mode = self.mode();
        for z in &zones {
            let r = z.bounds();
            let k = z.lane as usize % mode.lanes_per_player();
            let label = if z.scratch {
                "SC".to_string()
            } else {
                (k + 1).to_string()
            };
            let size = (r.w.min(r.h) * 0.3).min(28.0 * ui);
            d.text(
                &label,
                r.x + r.w * 0.5,
                r.y + (r.h - size) * 0.5,
                TextStyle::new(size, with_alpha(theme::TEXT, 0.8), Align::Center),
            );
        }
        if let Some((r, _)) = self.sel.and_then(|l| self.rect_of(ctx, l)) {
            d.b.rect_outline(r, 2.0, theme::ACCENT);
            for hr in Self::handles(r, Self::handle_size(ctx)) {
                d.b.rect(hr, theme::ACCENT);
            }
        }
        // the toolbar, over the play screen (START / SELECT stay in view),
        // finger-sized on phones
        let bh = (40.0 * ui).max(8.0 * mm).min(h * 0.1);
        let gap = 8.0 * ui;
        let tw = (view.w - 16.0 * ui).min(780.0 * ui);
        let x0 = view.x + (view.w - tw) * 0.5;
        let y0 = 8.0 * ui;
        let custom = self.custom(ctx);
        let t = &ctx.config.touch;
        let share = touch::controls_share(t, ctx.portrait());
        let height = format!(
            "◀ {} ▶",
            tr!(
                "CONTROLS {percent}%",
                percent = format!("{:.0}", share * 100.0)
            )
        );
        let side = format!("◀ START/SELECT {} ▶", t.start_select_side.label());
        let layout = format!("◀ {} ▶", Self::layout_label(ctx));
        let mode_label = format!("◀ {} ▶", mode.label());
        // two rows, three on narrow screens (phones in portrait)
        let rows: Vec<Vec<(Tool, f32, &str)>> = if tw >= (520.0 * ui).max(90.0 * mm) {
            vec![
                vec![
                    (Tool::Mode, 0.23, &mode_label),
                    (Tool::Layout, 0.23, &layout),
                    (Tool::Reset, 0.16, tr!("RESET")),
                    (Tool::Cancel, 0.18, tr!("CANCEL")),
                    (Tool::Save, 0.20, tr!("SAVE")),
                ],
                vec![
                    (Tool::Height, 0.36, &height),
                    (Tool::Side, 0.40, &side),
                    (Tool::Flip, 0.24, tr!("FLIP")),
                ],
            ]
        } else {
            vec![
                vec![(Tool::Mode, 0.5, &mode_label), (Tool::Layout, 0.5, &layout)],
                vec![(Tool::Height, 0.45, &height), (Tool::Side, 0.55, &side)],
                vec![
                    (Tool::Flip, 0.22, tr!("FLIP")),
                    (Tool::Reset, 0.24, tr!("RESET")),
                    (Tool::Cancel, 0.27, tr!("CANCEL")),
                    (Tool::Save, 0.27, tr!("SAVE")),
                ],
            ]
        };
        self.tools.clear();
        let mut y = y0;
        for row in &rows {
            let inner = tw - gap * (row.len() - 1) as f32;
            let mut x = x0;
            for &(tool, share, label) in row {
                let r = Rect::new(x, y, inner * share, bh);
                ui::button(d, r, label, ui, tool == Tool::Save);
                if tool == Tool::Reset && !custom {
                    d.b.rect(r, with_alpha(theme::BACKGROUND, 0.5));
                }
                self.tools.push((r, tool));
                x += r.w + gap;
            }
            y += bh + gap;
        }
        let preset = touch::preset_of(&ctx.config.touch).label();
        let state = match (custom, ctx.portrait()) {
            (true, true) => tr!("Custom portrait layout").to_string(),
            (true, false) => tr!("Custom landscape layout").to_string(),
            (false, true) => tr!(
                "{preset} preset · editing it makes a custom portrait layout",
                preset = preset
            ),
            (false, false) => tr!(
                "{preset} preset · editing it makes a custom landscape layout",
                preset = preset
            ),
        };
        let small = 14.0 * ui;
        let lines = [
            (state.as_str(), theme::TEXT),
            (
                tr!("Drag a button to move it · drag its corners or pinch it to resize"),
                theme::TEXT_DIM,
            ),
            (
                tr!("Drag the line above the controls to change their height"),
                theme::TEXT_DIM,
            ),
        ];
        for (i, (text, color)) in lines.into_iter().enumerate() {
            d.text(
                text,
                view.x + view.w * 0.5,
                y + i as f32 * small * 1.4,
                TextStyle::new(small, color, Align::Center).fit(view.w - 16.0 * ui),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_sit_on_the_corners() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let h = TouchLayoutScene::handles(r, 10.0);
        assert_eq!(h[0].center(), (10.0, 20.0));
        assert_eq!(h[3].center(), (110.0, 70.0));
    }

    #[test]
    fn resizing_keeps_the_opposite_corner() {
        let r = Rect::new(100.0, 100.0, 50.0, 40.0);
        // bottom right dragged out; top left dragged in past the least size
        let a = resized(r, 3, 200.0, 180.0, 20.0, false);
        assert_eq!((a.x, a.y, a.w, a.h), (100.0, 100.0, 100.0, 80.0));
        let b = resized(r, 0, 145.0, 135.0, 20.0, false);
        assert_eq!(
            (b.right(), b.bottom(), b.w, b.h),
            (150.0, 140.0, 20.0, 20.0)
        );
        // a turntable stays round
        let c = resized(r, 1, 300.0, 120.0, 20.0, true);
        assert_eq!((c.x, c.bottom(), c.w, c.h), (100.0, 140.0, 200.0, 200.0));
    }

    #[test]
    fn pinching_scales_about_the_fingers() {
        let r = Rect::new(100.0, 100.0, 40.0, 20.0);
        // fingers 100 px apart at (120, 110) spread to 200 px, moved 10 px right
        let p = pinched(r, 100.0, (120.0, 110.0), (30.0, 110.0), (230.0, 110.0));
        assert_eq!((p.w, p.h), (80.0, 40.0));
        assert_eq!(p.center(), (130.0, 110.0));
    }

    #[test]
    fn buttons_stay_partly_on_the_screen() {
        let r = on_screen(Rect::new(-500.0, 900.0, 100.0, 2.0), 1000.0, 600.0, 30.0);
        assert_eq!((r.x, r.y, r.w, r.h), (-75.0, 592.5, 100.0, 30.0));
    }
}
