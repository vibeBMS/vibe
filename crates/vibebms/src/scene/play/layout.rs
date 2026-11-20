//! Where the play goes on the screen: the skin's view, the touch controls
//! under it (or the perspective lanes as controls), START / SELECT.

use super::PlayScene;
use crate::app::Ctx;
use crate::config::Side;
use crate::gfx::Rect;
use crate::perspective::{Depths, Fan};
use crate::skin::render::Xf;
use crate::skin::LaneSkin;
use crate::touch::{self, TouchInput, Zone};

/// The touch controls, what is pressed, and where the play goes on the
/// screen: laid out again when what they follow changes (`Key`).
#[derive(Default)]
pub(super) struct Touch {
    pub input: TouchInput,
    /// Screen areas of the touch controls: (rect, player, scratch on the
    /// right).
    pub areas: Vec<(Rect, usize, bool)>,
    /// Where the play skin goes (above the touch controls).
    pub view: Rect,
    /// The touch controls (or the perspective lanes as controls), then
    /// START / SELECT.
    pub zones: Vec<Zone>,
    /// A perspective skin's fans on the screen.
    pub fans: Vec<Fan>,
    /// The skin's note fields in `view` (one per player).
    pub fields: Vec<Rect>,
    key: Option<Key>,
}

/// What the layout follows: the screen (its size, millimetres, touch
/// controls on), the skin (its size, perspective) and the judge line the
/// perspective lanes' touch area follows (LIFT).
#[derive(PartialEq)]
struct Key {
    size: (f32, f32),
    px_per_mm: f32,
    touch: bool,
    skin: Option<(f32, f32, bool)>,
    judge: f32,
}

impl PlayScene {
    /// The skin draws the lanes in perspective: they are the touch
    /// controls (no strip of controls under the play screen).
    pub(super) fn perspective(&self) -> bool {
        self.skin.as_ref().is_some_and(|s| s.skin.perspective)
    }

    /// Lay out the touch controls and the play's view again if what they
    /// follow changed.
    pub(super) fn lay_out(&mut self, ctx: &Ctx) {
        let key = Key {
            size: (ctx.view.width, ctx.view.height),
            px_per_mm: ctx.px_per_mm(),
            touch: ctx.touch_enabled(),
            skin: self
                .skin
                .as_ref()
                .map(|s| (s.skin.width, s.skin.height, s.skin.perspective)),
            judge: Depths::of(&ctx.config.play).judge,
        };
        if self.touch.key.as_ref() == Some(&key) {
            return;
        }
        let screen = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
        let fans = match self.perspective() {
            true => self.persp_fans(ctx, screen),
            false => Vec::new(),
        };
        let areas = self.touch_areas(ctx);
        let input_mode = self.facts.input_mode;
        let mut zones = Vec::new();
        if self.perspective() && key.touch {
            let top = self.persp_touch_top(ctx, &fans, screen);
            zones = touch::fan_zones(input_mode, &fans, top, key.judge, screen.x, screen.right());
        }
        if !areas.is_empty() {
            zones = touch::zones(
                input_mode,
                &self.touch_config(ctx),
                touch::controls_area(ctx.view.width, ctx.view.height, &ctx.config.touch),
                ctx.config.play.side == Side::P2,
                ctx.portrait(),
                ctx.px_per_mm(),
            );
        }
        let (view, buttons) = self.play_view(ctx, &areas);
        zones.extend(buttons);
        let fields = self.fields(view);
        let t = &mut self.touch;
        (t.areas, t.view, t.zones, t.fans, t.fields) = (areas, view, zones, fans, fields);
        t.key = Some(key);
    }

    /// The perspective skin's fans on the screen when it is drawn in `view`.
    fn persp_fans(&self, ctx: &Ctx, view: Rect) -> Vec<Fan> {
        let Some(sk) = self.skin.as_ref() else {
            return Vec::new();
        };
        let xf = Xf::fit(sk.skin.width, sk.skin.height, view);
        let side2 = ctx.config.play.side == Side::P2;
        crate::perspective::layout(
            self.session.mode,
            side2,
            sk.skin.width,
            sk.skin.height,
            &ctx.config.perspective,
        )
        .iter()
        .map(|f| f.transformed(xf.sx, xf.ox, xf.oy))
        .collect()
    }

    /// The top of the perspective lanes' touch area: the share of the
    /// screen set (by default half of it, the judge line in the middle of
    /// the touch area), at least a little above the judge line.
    fn persp_touch_top(&self, ctx: &Ctx, fans: &[Fan], view: Rect) -> f32 {
        let (lo, hi) = crate::config::TOUCH_HEIGHT_RANGE;
        let share = ctx.config.perspective.touch_height.clamp(lo, hi);
        let judge = Depths::of(&ctx.config.play).judge;
        let line = fans
            .iter()
            .map(|f| f.y(judge))
            .fold(view.bottom(), f32::min);
        (view.bottom() - share * view.h).min(line - 0.03 * view.h)
    }

    /// Screen areas of the touch controls: (rect, player, scratch on the right).
    fn touch_areas(&self, ctx: &Ctx) -> Vec<(Rect, usize, bool)> {
        if !ctx.touch_enabled() || self.perspective() {
            return Vec::new();
        }
        let t = &self.touch_config(ctx);
        let area = touch::controls_area(ctx.view.width, ctx.view.height, t);
        let side2 = ctx.config.play.side == Side::P2;
        touch::player_areas(self.facts.input_mode, area, t, side2, ctx.px_per_mm())
    }

    /// The touch settings for this play (VS: two control sets).
    fn touch_config(&self, ctx: &Ctx) -> crate::config::TouchConfig {
        let mut t = ctx.config.touch.clone();
        t.allow_dp |= self.session.vs;
        t
    }

    /// Where the play skin goes (above the touch controls) and, with touch
    /// controls, the START / SELECT buttons beside it.
    fn play_view(&self, ctx: &Ctx, areas: &[(Rect, usize, bool)]) -> (Rect, Vec<Zone>) {
        let screen = Rect::new(
            0.0,
            0.0,
            ctx.view.width,
            areas.first().map(|t| t.0.y).unwrap_or(ctx.view.height),
        );
        let input_mode = self.facts.input_mode;
        if self.perspective() {
            // START / SELECT in the top corner the skin leaves for them
            let Some(sk) = self.skin.as_ref().filter(|_| ctx.touch_enabled()) else {
                return (screen, Vec::new());
            };
            let xf = Xf::fit(sk.skin.width, sk.skin.height, screen);
            let side2 = ctx.config.play.side == Side::P2;
            let right = touch::system_right(&ctx.config.touch, input_mode, side2, ctx.portrait());
            let b = crate::perspective::system_buttons(sk.skin.width, sk.skin.height, right);
            let zone = |lane, [x, y, w, h]: [f32; 4]| Zone {
                shape: touch::Shape::Rect(xf.rect(Rect::new(x, y, w, h))),
                lane,
                scratch: false,
                black: false,
                color: None,
            };
            return (
                screen,
                vec![zone(touch::START, b[0]), zone(touch::SELECT, b[1])],
            );
        }
        if areas.is_empty() {
            return (screen, Vec::new());
        }
        let aspect = self
            .skin
            .as_ref()
            .map(|s| s.skin.width / s.skin.height.max(1.0))
            .unwrap_or(16.0 / 9.0);
        if self.session.vs {
            // VS: 1P's buttons on the left, 2P's on the right
            let mm = ctx.px_per_mm();
            let (view, one) = touch::system_buttons(screen, aspect, mm, false);
            let (view, mut two) = touch::system_buttons(view, aspect, mm, true);
            two[0].lane = touch::START2;
            two[1].lane = touch::SELECT2;
            return (view, one.into_iter().chain(two).collect());
        }
        let side2 = ctx.config.play.side == Side::P2;
        let right = touch::system_right(&ctx.config.touch, input_mode, side2, ctx.portrait());
        let (view, buttons) = touch::system_buttons(screen, aspect, ctx.px_per_mm(), right);
        (view, buttons.to_vec())
    }

    /// The skin's note fields on screen (one per player).
    fn fields(&self, view: Rect) -> Vec<Rect> {
        let Some(sk) = self.skin.as_ref() else {
            return Vec::new();
        };
        let xf = Xf::fit(sk.skin.width, sk.skin.height, view);
        let skin_mode = self.facts.skin_mode;
        let lpp = skin_mode.lanes_per_player();
        lane_fields(&sk.skin.lanes, skin_mode.players(), lpp, &xf)
    }
}

/// The note field of each of `players` (`lpp` lanes each) as `xf` puts a
/// skin's `lanes` on the screen; a skin may have fewer lanes than the mode
/// (none of a side: no field for it).
fn lane_fields(lanes: &[LaneSkin], players: usize, lpp: usize, xf: &Xf) -> Vec<Rect> {
    (0..players)
        .filter_map(|p| {
            (lanes.iter().skip(p * lpp).take(lpp))
                .filter_map(|l| l.rect)
                .map(|r| xf.rect(r))
                .reduce(|a, b| {
                    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
                    Rect::new(
                        x,
                        y,
                        a.right().max(b.right()) - x,
                        a.bottom().max(b.bottom()) - y,
                    )
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A double play skin with the lanes of one side only (or none) has
    /// that side's field.
    #[test]
    fn a_skin_with_fewer_lanes_than_the_mode_has_fewer_fields() {
        let xf = Xf::fit(100.0, 100.0, Rect::new(0.0, 0.0, 100.0, 100.0));
        let lane = |x: f32| LaneSkin {
            rect: Some(Rect::new(x, 0.0, 10.0, 50.0)),
            ..Default::default()
        };
        let lanes: Vec<LaneSkin> = (0..16).map(|i| lane(i as f32 * 5.0)).collect();
        let w = |n: usize| -> Vec<f32> {
            let fields = lane_fields(&lanes[..n], 2, 8, &xf);
            fields.iter().map(|r| r.w).collect()
        };
        assert_eq!(w(16), [45.0, 45.0]);
        assert_eq!(w(9), [45.0, 10.0]);
        assert_eq!(w(7), [40.0]);
        assert_eq!(w(0), Vec::<f32>::new());
    }
}
