//! What the scene draws over and beside the play skin: the touch controls,
//! the hi-speed panel for skins without one, VS's FAILED, notices, the
//! fade at the end.

use super::skin::{PlayHooks, PlayState};
use super::{PlayScene, Stage};
use crate::app::{Ctx, Draw};
use crate::config::Bpms;
use crate::gfx::text::Align;
use crate::gfx::{rgba, with_alpha, Rect};
use crate::platform::Instant;
use crate::scene::skinvals::ms_since;
use crate::theme;
use crate::touch;
use crate::ui;

impl PlayScene {
    pub(super) fn draw_play(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let areas = self.touch_areas(ctx);
        self.rebuild_zones(ctx, &areas);
        let (view, _) = self.play_view(ctx, &areas);
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
            for (area, _, _) in &areas {
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
                    d.text_fit(
                        label,
                        r.x + r.w * 0.5,
                        r.y + (r.h - size) * 0.5,
                        size,
                        theme::TEXT,
                        Align::Center,
                        r.w * 0.9,
                    );
                }
            }
        }
        for who in 0..2 {
            // (a skin's own panel is the first player's)
            if self.controls.panel[who].open() && (!self.skin_panel || who == 1) {
                self.draw_panel(ctx, d, view, now, who);
            }
        }
        // VS: a player who failed plays no more
        if self.session.vs {
            let fields = self.fields(view);
            for (side, f) in self.session.sides.iter().zip(&fields) {
                if side.failed.is_some() {
                    d.b.rect(*f, with_alpha(rgba(0x000000ff), 0.6));
                    d.text(
                        "FAILED",
                        f.x + f.w * 0.5,
                        f.y + f.h * 0.4,
                        32.0 * ui,
                        theme::ACCENT,
                        Align::Center,
                    );
                }
            }
        }
        let notices = [
            (self.session.ghost_desync(), "GHOST DESYNC"),
            (self.session.older_replay(), "REPLAY OF AN OLDER ENGINE"),
        ];
        let mut y = view.y + 8.0 * ui;
        for (_, what) in notices.iter().filter(|(on, _)| *on) {
            d.text(
                what,
                view.x + view.w * 0.5,
                y,
                16.0 * ui,
                theme::ACCENT,
                Align::Center,
            );
            y += 20.0 * ui;
        }
        if let Stage::Ending { since, .. } = self.stage {
            let a = (now.duration_since(since).as_secs_f32() / 1.2).min(1.0);
            d.b.rect(
                Rect::new(0.0, 0.0, ctx.width, ctx.height),
                with_alpha(rgba(0x000000ff), a * 0.85),
            );
        }
        self.session.advance_cursors(self.session.chart_us(now));
    }

    /// The hi-speed panel for skins without their own, beside the note
    /// field (between the fields in double play), never over the lanes.
    fn draw_panel(&self, ctx: &Ctx, d: &mut Draw, view: Rect, now: Instant, who: usize) {
        let ui = ctx.ui();
        let fields = self.fields(view);
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
        let bpms = Bpms::of_meta(&self.session.chart.meta);
        let hs = p.hispeed_at(&bpms);
        let cx = r.x + w * 0.5;
        let small = (11.0 * ui).min(w * 0.12);
        let mut y = r.y + 10.0 * ui;
        let line = |d: &mut Draw, s: &str, size: f32, color, y: &mut f32| {
            d.text_fit(s, cx, *y, size, color, Align::Center, w - 8.0 * ui);
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
            d.text("10.0", cx, y, small * 0.85, theme::TEXT_DIM, Align::Center);
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
                small * 0.85,
                theme::TEXT_DIM,
                Align::Center,
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
            (true, true) => ["HOLD:", "QUIT"],
            (true, false) => ["WHITE -0.25 / BLUE +0.25", "TURNTABLE: COVER"],
            _ => ["WHITE -0.01 / BLUE +0.01", "TURNTABLE: HI-SPEED"],
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
