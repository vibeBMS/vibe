//! What the scene draws over and beside the play skin: the touch controls,
//! the hi-speed panel for skins without one, VS's FAILED, notices, the
//! fade at the end.

use super::skin::{PlayHooks, PlayState};
use super::{PlayScene, Stage};
use crate::app::{Ctx, Draw};
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{rgba, with_alpha, Rect};
use crate::platform::Instant;
use crate::scene::skinvals::ms_since;
use crate::theme;
use crate::touch;
use crate::ui;

impl PlayScene {
    pub(super) fn draw_play(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        self.sync_sides(&ctx.config.play);
        self.lay_out(ctx);
        let view = self.touch.view;
        let mut notes_graph = std::mem::take(&mut self.notes_graph);
        if let Some(sk) = self.skin.as_ref() {
            let st = PlayState::new(self, ctx, now);
            let mut hooks = PlayHooks {
                st: &st,
                notes_graph: &mut notes_graph,
            };
            let ms = ms_since(self.scene_start, now).max(0);
            sk.draw(d, view, ms, &st, &mut hooks);
        }
        self.notes_graph = notes_graph;
        let ui = ctx.ui();
        if ctx.touch_enabled() {
            let zones = &self.touch.zones;
            for (area, _, _) in &self.touch.areas {
                d.b.rect(*area, rgba(0x05060aff));
            }
            // (the perspective lanes draw their own touch areas)
            let shown = if self.perspective() {
                let first = zones.iter().position(|z| z.lane >= touch::SYSTEM);
                &zones[first.unwrap_or(zones.len())..]
            } else {
                &zones[..]
            };
            self.touch.input.draw(d.b, shown, ctx.config.touch.opacity);
            for z in zones.iter().filter(|z| z.lane >= touch::SYSTEM) {
                if let touch::Shape::Rect(r) = z.shape {
                    let label = match z.lane {
                        touch::START | touch::START2 => "START",
                        _ => "SELECT",
                    };
                    let size = (r.w * 0.22).min(14.0 * ui);
                    d.text(
                        label,
                        r.x + r.w * 0.5,
                        r.y + (r.h - size) * 0.5,
                        TextStyle::new(size, theme::TEXT, Align::Center).fit(r.w * 0.9),
                    );
                }
            }
        }
        for who in 0..2 {
            // (a skin's own panel is the first player's)
            if self.controls.panel[who].open() && (!self.skin_panel || who == 1) {
                self.draw_panel(ctx, d, view, now, who);
            } else if who == 0 || self.session.vs {
                self.draw_hs_change(ctx, d, now, who);
            }
        }
        // VS: a player who failed plays no more
        if self.session.vs {
            for (side, f) in self.session.sides.iter().zip(&self.touch.fields) {
                if side.failed.is_some() {
                    d.b.rect(*f, with_alpha(rgba(0x000000ff), 0.6));
                    d.text(
                        "FAILED",
                        f.x + f.w * 0.5,
                        f.y + f.h * 0.4,
                        TextStyle::new(32.0 * ui, theme::ACCENT, Align::Center),
                    );
                }
            }
        }
        let notices = [
            (self.session.ghost_desync(), "GHOST DESYNC"),
            (
                self.session.older_replay(),
                tr!("REPLAY OF AN OLDER ENGINE"),
            ),
        ];
        let mut y = view.y + 8.0 * ui;
        for (_, what) in notices.iter().filter(|(on, _)| *on) {
            d.text(
                what,
                view.x + view.w * 0.5,
                y,
                TextStyle::new(16.0 * ui, theme::ACCENT, Align::Center),
            );
            y += 20.0 * ui;
        }
        self.draw_practice(ctx, d, view);
        if let Stage::Ending { since, .. } = self.stage {
            let a = (now.duration_since(since).as_secs_f32() / 1.2).min(1.0);
            d.b.rect(
                Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height),
                with_alpha(rgba(0x000000ff), a * 0.85),
            );
        }
        self.session.advance_cursors(self.session.chart_us(now));
    }

    /// A hi-speed change the player made, shown over the top of their
    /// note field for a moment (`hsview::SHOWN`), fading out.
    fn draw_hs_change(&self, ctx: &Ctx, d: &mut Draw, now: Instant, who: usize) {
        let p = self.side_config(who);
        let Some((text, fade)) = self.hs_view[who].shown(p, &self.bpms_at(now), now) else {
            return;
        };
        let fields = &self.touch.fields;
        let f = match self.session.vs {
            true => fields.get(who),
            false => fields.first(),
        };
        let Some(f) = f.copied() else {
            return;
        };
        // (a double play's two fields: across both)
        let f = match (self.session.vs, fields.last()) {
            (false, Some(l)) => Rect::new(f.x, f.y, l.right() - f.x, f.h),
            _ => f,
        };
        let ui = ctx.ui();
        let size = (15.0 * ui).min(f.w * 0.08);
        let (w, h) = ((size * 13.0).min(f.w * 0.95), size * 2.0);
        let top = f.y + f.h * (0.06 + p.sudden * (p.lane_cover || p.sudden > 0.0) as u8 as f32);
        let r = Rect::new(f.x + (f.w - w) * 0.5, top.min(f.bottom() - h), w, h);
        let a = 1.0 - fade;
        d.b.rect(r, with_alpha(rgba(0x05060aff), 0.8 * a));
        d.text(
            &text,
            r.x + r.w * 0.5,
            r.y + (h - size) * 0.5,
            TextStyle::new(size, with_alpha(theme::ACCENT, a), Align::Center).fit(w - 8.0 * ui),
        );
    }

    /// The hi-speed panel for skins without their own, beside the note
    /// field (between the fields in double play), never over the lanes.
    fn draw_panel(&self, ctx: &Ctx, d: &mut Draw, view: Rect, now: Instant, who: usize) {
        let ui = ctx.ui();
        let fields = &self.touch.fields;
        let (Some(first), Some(last)) = (fields.first(), fields.last()) else {
            return;
        };
        // free columns: between the fields, right of them, left of them;
        // the panel goes against the field (centred between two)
        #[derive(Clone, Copy)]
        enum Side {
            Between,
            Right,
            Left,
        }
        let mut spaces = Vec::new();
        if fields.len() == 2 {
            spaces.push((first.right(), last.x, Side::Between));
        }
        let right = (last.right(), view.right(), Side::Right);
        let left = (view.x, first.x, Side::Left);
        if self.session.vs {
            // VS: each player's beside their own field
            spaces.insert(0, if who == 0 { left } else { right });
        } else if first.x - view.x > view.right() - last.right() {
            spaces.extend([left, right]);
        } else {
            spaces.extend([right, left]);
        }
        let (want, gap) = (120.0 * ui, 6.0 * ui);
        let Some((x0, x1, side)) = spaces
            .iter()
            .copied()
            .find(|(a, b, _)| b - a >= want * 0.6 + gap * 2.0)
            .or_else(|| {
                spaces
                    .iter()
                    .copied()
                    .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
            })
        else {
            return;
        };
        let w = (x1 - x0 - gap * 2.0).min(want).max(40.0 * ui);
        let x = match side {
            Side::Between => (x0 + x1 - w) * 0.5,
            Side::Right => x0 + gap,
            Side::Left => x1 - gap - w,
        };
        let h = (first.h * 0.7).min(340.0 * ui);
        let r = Rect::new(x, first.y + first.h * 0.12, w, h);
        ui::panel(d, r);
        let p = &ctx.config.play.for_side(who);
        let bpms = self.bpms_at(now);
        let hs = p.hispeed_shown(&bpms);
        let cx = r.x + w * 0.5;
        let small = (11.0 * ui).min(w * 0.12);
        let mut y = r.y + 10.0 * ui;
        let line = |d: &mut Draw, s: &str, size: f32, color, y: &mut f32| {
            d.text(
                s,
                cx,
                *y,
                TextStyle::new(size, color, Align::Center).fit(w - 8.0 * ui),
            );
            *y += size * 1.3;
        };
        line(d, "HI-SPEED", small, theme::TEXT_DIM, &mut y);
        line(d, &format!("{hs:.2}"), small * 2.2, theme::ACCENT, &mut y);
        line(
            d,
            &format!("GN {}", p.green(&bpms)),
            small,
            theme::TEXT,
            &mut y,
        );
        // 0.5 .. 10.0 track with the current value
        let bottom_rows = 4.0 * small * 1.3 + 10.0 * ui;
        let (t0, t1) = (y + small * 1.4, r.bottom() - bottom_rows - small * 1.6);
        if t1 - t0 > 20.0 * ui {
            d.text(
                "10.0",
                cx,
                y,
                TextStyle::new(small * 0.85, theme::TEXT_DIM, Align::Center),
            );
            d.b.rect(
                Rect::new(cx - 2.0 * ui, t0, 4.0 * ui, t1 - t0),
                with_alpha(theme::TEXT_DIM, 0.4),
            );
            let k = ((hs - 0.5) / 9.5).clamp(0.0, 1.0);
            let my = t1 - (t1 - t0) * k;
            d.b.rect(
                Rect::new(cx - w * 0.3, my - 3.0 * ui, w * 0.6, 6.0 * ui),
                theme::ACCENT,
            );
            d.text(
                "0.5",
                cx,
                t1 + 3.0 * ui,
                TextStyle::new(small * 0.85, theme::TEXT_DIM, Align::Center),
            );
        }
        let mut y = r.bottom() - bottom_rows;
        let cover = if p.lane_cover || p.sudden > 0.0 {
            format!("SUD+ {:.0}%", p.sudden * 100.0)
        } else {
            "SUD+ OFF".into()
        };
        line(d, &cover, small, theme::TEXT, &mut y);
        line(
            d,
            &format!("LIFT {:.0}%", p.lift * 100.0),
            small,
            theme::TEXT,
            &mut y,
        );
        let c = &self.controls.panel[who];
        let hint = match (c.start, c.select) {
            (true, true) => [tr!("HOLD:"), tr!("QUIT")],
            (true, false) => [tr!("WHITE -0.25 / BLUE +0.25"), tr!("TURNTABLE: COVER")],
            _ => [tr!("WHITE -0.01 / BLUE +0.01"), tr!("TURNTABLE: HI-SPEED")],
        };
        for h in hint {
            line(d, h, small * 0.8, theme::TEXT_DIM, &mut y);
        }
        if let Some(t) = c.both_since {
            let k = now.duration_since(t).as_secs_f32().min(1.0);
            d.b.rect(
                Rect::new(r.x, r.bottom() - 4.0 * ui, r.w * k, 4.0 * ui),
                theme::ACCENT,
            );
        }
    }
}
