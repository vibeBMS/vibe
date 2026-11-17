//! Where the play goes on the screen: the skin's view, the touch controls
//! under it (or the perspective lanes as controls), START / SELECT.

use super::PlayScene;
use crate::app::Ctx;
use crate::config::Side;
use crate::gfx::Rect;
use crate::perspective::Fan;
use crate::skin::render::Xf;
use crate::touch::{self, TouchInput, Zone};

/// The touch controls: what is pressed, and the zones (laid out again when
/// the screen changes).
#[derive(Default)]
pub(super) struct Touch {
    pub input: TouchInput,
    pub zones: Vec<Zone>,
    /// The screen the zones were laid out for: size, touch on, skin loaded.
    key: (u32, u32, bool, bool),
}

impl PlayScene {
    /// The skin draws the lanes in perspective: they are the touch
    /// controls (no strip of controls under the play screen).
    pub(super) fn perspective(&self) -> bool {
        self.skin.as_ref().is_some_and(|s| s.skin.perspective)
    }

    /// The perspective skin's fans on the screen when it is drawn in `view`.
    pub(super) fn persp_fans(&self, ctx: &Ctx, view: Rect) -> Vec<Fan> {
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
    /// screen set, at least a little above the judge line.
    fn persp_touch_top(&self, ctx: &Ctx, fans: &[Fan], view: Rect) -> f32 {
        let (lo, hi) = crate::config::TOUCH_HEIGHT_RANGE;
        let share = ctx.config.perspective.touch_height.clamp(lo, hi);
        let judge = crate::perspective::Depths::of(&ctx.config.play).judge;
        let line = fans
            .iter()
            .map(|f| f.y(judge))
            .fold(view.bottom(), f32::min);
        (view.bottom() - share * view.h).min(line - 0.03 * view.h)
    }

    /// Screen areas of the touch controls: (rect, player, scratch on the right).
    pub(super) fn touch_areas(&self, ctx: &Ctx) -> Vec<(Rect, usize, bool)> {
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
    pub(super) fn play_view(
        &self,
        ctx: &Ctx,
        areas: &[(Rect, usize, bool)],
    ) -> (Rect, Option<Vec<Zone>>) {
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
                return (screen, None);
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
                Some(vec![zone(touch::START, b[0]), zone(touch::SELECT, b[1])]),
            );
        }
        if areas.is_empty() {
            return (screen, None);
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
            return (view, Some(one.into_iter().chain(two).collect()));
        }
        let side2 = ctx.config.play.side == Side::P2;
        let right = touch::system_right(&ctx.config.touch, input_mode, side2, ctx.portrait());
        let (view, buttons) = touch::system_buttons(screen, aspect, ctx.px_per_mm(), right);
        (view, Some(buttons.to_vec()))
    }

    pub(super) fn rebuild_zones(&mut self, ctx: &Ctx, areas: &[(Rect, usize, bool)]) {
        let key = (
            ctx.view.width as u32,
            ctx.view.height as u32,
            ctx.touch_enabled(),
            self.skin.is_some(),
        );
        // the perspective lanes follow the settings (LIFT, the touch area)
        if key == self.touch.key && !self.touch.zones.is_empty() && !self.perspective() {
            return;
        }
        self.touch.key = key;
        let input_mode = self.facts.input_mode;
        let mut zones = Vec::new();
        if self.perspective() && ctx.touch_enabled() {
            let view = Rect::new(0.0, 0.0, ctx.view.width, ctx.view.height);
            let fans = self.persp_fans(ctx, view);
            let top = self.persp_touch_top(ctx, &fans, view);
            zones = touch::fan_zones(input_mode, &fans, top, view.x, view.right());
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
        if let (_, Some(buttons)) = self.play_view(ctx, areas) {
            zones.extend(buttons);
        }
        self.touch.zones = zones;
    }

    /// The skin's note fields on screen (one per player).
    pub(super) fn fields(&self, view: Rect) -> Vec<Rect> {
        let Some(sk) = self.skin.as_ref() else {
            return Vec::new();
        };
        let xf = Xf::fit(sk.skin.width, sk.skin.height, view);
        let skin_mode = self.facts.skin_mode;
        let lpp = skin_mode.lanes_per_player();
        (0..skin_mode.players())
            .filter_map(|p| {
                sk.skin.lanes[p * lpp..((p + 1) * lpp).min(sk.skin.lanes.len())]
                    .iter()
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
}
