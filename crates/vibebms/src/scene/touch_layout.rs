//! Touch layout editor (KEY CONFIG / TOUCH): the touch buttons over the
//! screen as play shows them. Drag a button to move it; drag its corners,
//! or pinch it, to resize it. Editing a preset copies it into the custom
//! layout of the key mode and orientation shown.
//!
//! Keyboard: ←→ layout, ↑↓ key mode, R reset, Enter save, Esc cancel.
//! Controller: white keys layout, blue keys key mode, START save,
//! SELECT cancel.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{Side, TouchConfig, TouchStyle};
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Gfx, Rect};
use crate::input::{nav_from_pad, InputEv, Nav, Phase};
use crate::platform::Instant;
use crate::theme;
use crate::touch::{self, TouchInput, Zone};
use crate::ui;
use std::collections::HashMap;
use vibebms_core::PlayMode;
use winit::keyboard::KeyCode;

/// Key modes in menu order.
const MODES: [PlayMode; 5] = [
    PlayMode::Beat7K,
    PlayMode::Beat5K,
    PlayMode::Beat14K,
    PlayMode::Beat10K,
    PlayMode::Popn9K,
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Mode,
    Layout,
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
}

pub struct TouchLayoutScene {
    mode: usize,
    /// The touch settings when the editor opened (cancel restores them).
    orig: TouchConfig,
    sel: Option<u8>,
    ptrs: HashMap<u64, (f32, f32)>,
    grab: Option<Grab>,
    tools: Vec<(Rect, Tool)>,
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
    let left = corner % 2 == 0;
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
        }
    }

    fn mode(&self) -> PlayMode {
        MODES[self.mode]
    }

    fn area(ctx: &Ctx) -> Rect {
        touch::controls_area(ctx.width, ctx.height, &ctx.config.touch)
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
        let r = on_screen(r, ctx.width, ctx.height, least(ctx));
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
            ctx.toast("This layout is a preset already");
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
        match touch::pick(&zones, x, y) {
            Some(i) => {
                let z = zones[i];
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
            Some(Grab::Move { id: g, .. } | Grab::Resize { id: g, .. }) if g == id => {
                self.grab = None
            }
            _ => {}
        }
    }

    fn layout_label(ctx: &Ctx) -> String {
        let t = &ctx.config.touch;
        match t.style {
            TouchStyle::Custom => "CUSTOM".into(),
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
                _ => {}
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => match nav_from_pad(*id, &ctx.config.keys) {
                Some(Nav::Decide) => self.step_layout(ctx, 1),
                Some(Nav::Close) => self.step_mode(1),
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

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, _now: Instant) {
        let ui = ctx.ui();
        let (w, h) = (ctx.width, ctx.height);
        let area = Self::area(ctx);
        d.b.rect(Rect::new(0.0, 0.0, w, h), theme::BACKGROUND);
        // where the play screen goes
        let view = Rect::new(0.0, 0.0, w, area.y);
        d.b.gradient(
            view,
            ui::lift(theme::BACKGROUND, 0.06),
            ui::lift(theme::BACKGROUND, 0.02),
        );
        d.text(
            "PLAY SCREEN",
            w * 0.5,
            (view.h - 20.0 * ui) * 0.5 + 30.0 * ui,
            20.0 * ui,
            with_alpha(theme::TEXT_DIM, 0.5),
            Align::Center,
        );
        let side2 = Self::side2(ctx);
        for (a, _, _) in
            touch::player_areas(self.mode(), area, &ctx.config.touch, side2, ctx.px_per_mm())
        {
            d.b.rect(a, rgba(0x05060aff));
        }
        // the buttons, drawn as in play
        let zones = self.zones(ctx);
        TouchInput::default().draw(d.b, &zones, ctx.config.touch.opacity);
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
                size,
                with_alpha(theme::TEXT, 0.8),
                Align::Center,
            );
        }
        if let Some((r, _)) = self.sel.and_then(|l| self.rect_of(ctx, l)) {
            d.b.rect_outline(r, 2.0, theme::ACCENT);
            for hr in Self::handles(r, Self::handle_size(ctx)) {
                d.b.rect(hr, theme::ACCENT);
            }
        }
        // the toolbar
        // finger-sized on phones
        let bh = (40.0 * ui).max(8.0 * ctx.px_per_mm()).min(h * 0.1);
        let gap = 8.0 * ui;
        let tw = (w - 16.0 * ui).min(780.0 * ui);
        let x0 = (w - tw) * 0.5;
        let y0 = 8.0 * ui;
        let custom = self.custom(ctx);
        let parts: [(Tool, f32, String, bool); 5] = [
            (Tool::Mode, 0.23, format!("◀ {} ▶", mode.label()), false),
            (
                Tool::Layout,
                0.23,
                format!("◀ {} ▶", Self::layout_label(ctx)),
                false,
            ),
            (Tool::Reset, 0.16, "RESET".into(), false),
            (Tool::Cancel, 0.18, "CANCEL".into(), false),
            (Tool::Save, 0.20, "SAVE".into(), true),
        ];
        let inner = tw - gap * (parts.len() - 1) as f32;
        let mut x = x0;
        self.tools.clear();
        for (tool, share, label, active) in parts {
            let r = Rect::new(x, y0, inner * share, bh);
            let dim = tool == Tool::Reset && !custom;
            ui::button(d, r, &label, ui, active);
            if dim {
                d.b.rect(r, with_alpha(theme::BACKGROUND, 0.5));
            }
            self.tools.push((r, tool));
            x += r.w + gap;
        }
        let orient = if ctx.portrait() {
            "PORTRAIT"
        } else {
            "LANDSCAPE"
        };
        let state = if custom {
            format!("Custom {orient} layout")
        } else {
            format!(
                "{} preset · editing it makes a custom {orient} layout",
                touch::preset_of(&ctx.config.touch).label()
            )
        };
        let small = 14.0 * ui;
        d.text_fit(
            &state,
            w * 0.5,
            y0 + bh + 8.0 * ui,
            small,
            theme::TEXT,
            Align::Center,
            w - 16.0 * ui,
        );
        d.text_fit(
            "Drag a button to move it · drag its corners or pinch it to resize",
            w * 0.5,
            y0 + bh + 10.0 * ui + small * 1.4,
            small,
            theme::TEXT_DIM,
            Align::Center,
            w - 16.0 * ui,
        );
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
