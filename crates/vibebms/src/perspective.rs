//! Lanes seen in perspective (the touch perspective play skin).
//!
//! Each player's lanes form a fan: narrow at the far end, wider toward the
//! judge line and on to the bottom of the screen. A point of a fan is given
//! in lane space: `u` across the fan (0 left edge .. 1 right edge) and `d`
//! along it (0 at the base, 1 at the far end; below the base `d` is
//! negative). Notes travel in `d` (the green number is about `d`), and the
//! perspective only changes where a `d` is on the screen. The mapping is a
//! projective one, so lane edges are straight lines on the screen and a
//! lane's piece between two depths is a four-cornered shape whose corners
//! are mapped points: what is drawn and what touches hit are the same.
//!
//! The layout: the base (the judge line without LIFT) at 3/4 of the screen's
//! height, the far end near the top, the keys about a tenth of the screen
//! wide at the base. One line between two keys is upright, in the middle of
//! the screen (7 keys: 3|4, 5 keys: 2|3, with the scratch on the left; the
//! 2P side mirrors that), so the keys sit under the player's hands and the
//! scratch leans out to its side; its bottom may run off the screen.
//!
//! The touch zones (`Fan::touch_strips`) are like a beatmania controller's
//! keys: at the judge line each key's zone is its lane, and between a white
//! and a blue key the boundary bends so that the white key is wider below
//! the line and the blue key wider above it.

use crate::config::{PerspectiveConfig, LENGTH_RANGE, PERSPECTIVE_MAX};
use vibebms_core::PlayMode;

/// The canvases the perspective skins are made for.
pub const PORTRAIT: (f32, f32) = (720.0, 1280.0);
pub const LANDSCAPE: (f32, f32) = (1280.0, 720.0);

/// Width of the scratch lane and of the gap beside it, in key widths.
const SCRATCH_W: f32 = 1.7;
const SCRATCH_GAP: f32 = 0.12;

/// The base (the judge line without LIFT) and the far end (at the full
/// lane length, in landscape) as shares of the screen height from its top.
pub const BASE_Y: f32 = 0.75;
pub const TOP_Y: f32 = 0.06;
/// A key's width at the base, as a share of the screen width.
pub const KEY_W: f32 = 0.102;

/// The touch zones' boundary between a white and a blue key: how far above
/// and below the judge line it bends (in key widths at the line), and how
/// round the bend is: the boundary is `cos(pi x / key)` raised to the power
/// `1 / CURVE_ROUND`; under 1 the domes are rounder and the boundary
/// steeper where it crosses the line (1: a plain cosine wave).
pub const CURVE_H: f32 = 0.6;
pub const CURVE_ROUND: f32 = 0.5;

/// One lane of a fan: its range across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FanLane {
    pub lane: usize,
    pub u0: f32,
    pub u1: f32,
    pub scratch: bool,
    /// An even-numbered key (blue on a beatmania controller, the upper
    /// row of pop'n buttons).
    pub blue: bool,
}

/// Corners of a shape: top-left, top-right, bottom-right, bottom-left.
pub type Quad = [[f32; 2]; 4];

/// A touch zone as rows from the top down: at each row's `y`, the zone
/// spans `x0..x1`; between rows the edges are straight.
pub type Strip = Vec<[f32; 3]>;

/// One player's lanes on the screen (or canvas).
#[derive(Clone, Debug, PartialEq)]
pub struct Fan {
    pub player: usize,
    /// The x of the upright line between two keys (at `u` = `axis`), and
    /// the y of the base (`d` = 0).
    pub cx: f32,
    pub axis: f32,
    pub base_y: f32,
    /// Screen distance from the base to the far end (`d` = 1).
    pub len: f32,
    /// Width at the base.
    pub width: f32,
    /// Foreshortening: the far end is 1 / (1 + k) as wide as the base.
    pub k: f32,
    /// The y the lanes end at below the base (the screen's bottom).
    pub bottom: f32,
    pub lanes: Vec<FanLane>,
}

/// `k` for a strength (0 flat .. `PERSPECTIVE_MAX`).
pub fn foreshortening(strength: f32) -> f32 {
    let r = 1.0 - strength.clamp(0.0, PERSPECTIVE_MAX);
    1.0 / r - 1.0
}

impl Fan {
    /// Size at depth `d` relative to the base.
    pub fn scale(&self, d: f32) -> f32 {
        1.0 / (1.0 + self.k * d)
    }

    /// Size at screen height `y` relative to the base (it changes linearly
    /// with `y`, 0 at the vanishing point).
    pub fn scale_at(&self, y: f32) -> f32 {
        1.0 - self.k * (self.base_y - y) / (self.len * (1.0 + self.k)).max(1e-3)
    }

    pub fn y(&self, d: f32) -> f32 {
        self.base_y - self.len * d * (1.0 + self.k) / (1.0 + self.k * d)
    }

    pub fn point(&self, u: f32, d: f32) -> [f32; 2] {
        [
            self.cx + (u - self.axis) * self.width * self.scale(d),
            self.y(d),
        ]
    }

    /// The x of `u` at screen height `y`.
    pub fn x_at(&self, u: f32, y: f32) -> f32 {
        self.cx + (u - self.axis) * self.width * self.scale_at(y)
    }

    /// The depth shown at screen height `y` (`None` at or above the
    /// vanishing point).
    pub fn depth(&self, y: f32) -> Option<f32> {
        let f = (self.base_y - y) / self.len.max(1e-3);
        let den = 1.0 + self.k - self.k * f;
        (den > 1e-6).then(|| f / den)
    }

    /// Lane space (u, d) of a screen point.
    pub fn locate(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let d = self.depth(y)?;
        let w = self.width * self.scale(d);
        Some((self.axis + (x - self.cx) / w.max(1e-3), d))
    }

    /// The shape of `u0..u1` between depths `d0` (near) and `d1` (far).
    pub fn quad(&self, u0: f32, u1: f32, d0: f32, d1: f32) -> Quad {
        [
            self.point(u0, d1),
            self.point(u1, d1),
            self.point(u1, d0),
            self.point(u0, d0),
        ]
    }

    /// The depth of the screen's bottom.
    pub fn d_bottom(&self) -> f32 {
        self.depth(self.bottom).unwrap_or(0.0)
    }

    /// A key's width at the base.
    fn key_width(&self) -> f32 {
        self.lanes
            .iter()
            .find(|l| !l.scratch)
            .map_or(0.0, |l| l.u1 - l.u0)
    }

    /// Where the white-and-blue boundaries bend: (y of the judge line,
    /// half the height of the bend).
    fn bend(&self, judge: f32) -> (f32, f32) {
        let yj = self.y(judge);
        let key = self.key_width() * self.width * self.scale(judge);
        (yj, CURVE_H * key)
    }

    /// The x of the left edge of touch column `i` (`lanes.len()`: the right
    /// edge of the last) at screen height `y`; the outer edges are `x0` and
    /// `x1`.
    fn side(&self, i: usize, y: f32, judge: f32, x0: f32, x1: f32) -> f32 {
        let n = self.lanes.len();
        if i == 0 {
            return x0;
        }
        if i >= n {
            return x1;
        }
        let (a, b) = (&self.lanes[i - 1], &self.lanes[i]);
        let mid = (a.u1 + b.u0) * 0.5;
        if a.scratch || b.scratch || a.blue == b.blue {
            return self.x_at(mid, y).clamp(x0, x1);
        }
        let key = a.u1 - a.u0;
        let (yj, h) = self.bend(judge);
        let t = ((yj - y) / h.max(1e-3)).clamp(-1.0, 1.0);
        let c = t.signum() * t.abs().powf(1.0 / CURVE_ROUND);
        let x = key / std::f32::consts::PI * c.clamp(-1.0, 1.0).acos();
        // the white key's top and the blue key's bottom are rounded
        let u = if a.blue {
            mid + key * 0.5 - x
        } else {
            mid - key * 0.5 + x
        };
        self.x_at(u, y).clamp(x0, x1)
    }

    /// The rows the touch strips are cut into from `top` to the bottom:
    /// evenly, and closer along the bends (evenly across them).
    fn strip_rows(&self, top: f32, judge: f32) -> Vec<f32> {
        const EVEN: usize = 16;
        const BEND: usize = 32;
        let bottom = self.bottom.max(top);
        let mut ys: Vec<f32> = (0..=EVEN)
            .map(|i| top + (bottom - top) * i as f32 / EVEN as f32)
            .collect();
        let (yj, h) = self.bend(judge);
        for j in 0..=BEND {
            let c = (std::f32::consts::PI * j as f32 / BEND as f32).cos();
            let y = yj - c.signum() * c.abs().powf(CURVE_ROUND) * h;
            if y > top && y < bottom {
                ys.push(y);
            }
        }
        ys.sort_by(f32::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        ys
    }

    /// The touch zones: each lane from the bottom up to screen height `top`,
    /// meeting its neighbours halfway across the scratch's gap and, between
    /// a white and a blue key, on a curve through their boundary at the
    /// judge line (at depth `judge`); the outer lanes reach out to `x0` and
    /// `x1` (the screen's edges, or halfway to the next fan). What is drawn
    /// as the touch area and what a touch hits.
    pub fn touch_strips(&self, top: f32, judge: f32, x0: f32, x1: f32) -> Vec<(FanLane, Strip)> {
        let ys = self.strip_rows(top, judge);
        let n = self.lanes.len();
        let sides: Vec<Vec<f32>> = (0..=n)
            .map(|i| ys.iter().map(|&y| self.side(i, y, judge, x0, x1)).collect())
            .collect();
        // keep the sides in order where clamping or rounding made them cross
        let mut sides = sides;
        for i in 1..=n {
            for r in 0..ys.len() {
                sides[i][r] = sides[i][r].max(sides[i - 1][r]);
            }
        }
        self.lanes
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let rows = ys
                    .iter()
                    .enumerate()
                    .map(|(r, &y)| [y, sides[i][r], sides[i + 1][r]])
                    .collect();
                (*l, rows)
            })
            .collect()
    }

    /// The fan with its coordinates scaled by `s` and moved by (ox, oy)
    /// (canvas to screen).
    pub fn transformed(&self, s: f32, ox: f32, oy: f32) -> Fan {
        Fan {
            cx: ox + self.cx * s,
            base_y: oy + self.base_y * s,
            len: self.len * s,
            width: self.width * s,
            bottom: oy + self.bottom * s,
            ..self.clone()
        }
    }
}

/// Lanes of one player across a fan, left to right: keys one unit wide,
/// the scratch wider at the outer edge with a gap beside it.
pub fn fan_lanes(mode: PlayMode, player: usize, scratch_right: bool) -> Vec<FanLane> {
    let order = mode.display_order(player, scratch_right);
    let width = |l: usize| {
        if mode.is_scratch(l) {
            SCRATCH_W
        } else {
            1.0
        }
    };
    let gap = if mode.has_scratch() { SCRATCH_GAP } else { 0.0 };
    let total: f32 = order.iter().map(|&l| width(l)).sum::<f32>() + gap;
    let mut x = 0.0;
    let mut out = Vec::with_capacity(order.len());
    for (i, &l) in order.iter().enumerate() {
        let sc = mode.is_scratch(l);
        // the gap after a scratch on the left, before one on the right
        if sc && i > 0 {
            x += gap;
        }
        out.push(FanLane {
            lane: l,
            u0: x / total,
            u1: (x + width(l)) / total,
            scratch: sc,
            blue: !sc && l % mode.lanes_per_player() % 2 == 1,
        });
        x += width(l);
        if sc && i == 0 {
            x += gap;
        }
    }
    out
}

/// The `u` of a fan's upright line: between the middle keys, the scratch's
/// side having the fewer (3|4 of 7 keys and 2|3 of 5 with the scratch on
/// the left, 4|5 and 3|4 with it on the right); without a scratch the
/// middle of the fan.
fn axis_of(lanes: &[FanLane]) -> f32 {
    let keys: Vec<&FanLane> = lanes.iter().filter(|l| !l.scratch).collect();
    let Some(sc) = lanes.iter().position(|l| l.scratch) else {
        return 0.5;
    };
    let near = keys.len() / 2;
    let left = if sc == 0 { near } else { keys.len() - near };
    match left {
        0 => keys.first().map_or(0.5, |k| k.u0),
        n => keys[n - 1].u1,
    }
}

/// The top of the room the lanes can reach: in portrait under the skin's
/// title, gauge and score rows, in landscape near the top of the screen.
pub fn top_of_lanes(portrait: bool, h: f32) -> f32 {
    if portrait {
        212.0
    } else {
        TOP_Y * h
    }
}

/// The widest screen the keys grow with, in screen heights: on a very
/// wide screen they stop growing rather than getting flat.
const MAX_ASPECT: f32 = 2.4;

/// The fans of `mode` on a canvas of `w` x `h` (y down), one per player.
/// `side2`: the 2P side in single play (the scratch on the right).
///
/// One player's fan has its upright line in the middle of the screen; in
/// double play each player has a half, the line in its middle, and the
/// keys are narrowed so that the fans meet at the bottom of the screen.
pub fn layout(mode: PlayMode, side2: bool, w: f32, h: f32, cfg: &PerspectiveConfig) -> Vec<Fan> {
    let portrait = h > w;
    let double = mode.is_double();
    let k = foreshortening(cfg.strength);
    let base_y = BASE_Y * h;
    let top = top_of_lanes(portrait, h).min(base_y - 40.0);
    let length = cfg.length.clamp(LENGTH_RANGE.0, LENGTH_RANGE.1);
    let len = (base_y - top) * length;
    let n = mode.players();
    let region = w / n as f32;
    // the size at the bottom relative to the base
    let s_bottom = 1.0 + k * (h - base_y) / (len * (1.0 + k));
    (0..n)
        .map(|player| {
            let lanes = fan_lanes(mode, player, if double { player == 1 } else { side2 });
            let axis = axis_of(&lanes);
            let key_u = lanes
                .iter()
                .find(|l| !l.scratch)
                .map_or(1.0, |l| l.u1 - l.u0);
            let mut key = KEY_W * w.min(h * MAX_ASPECT);
            if double {
                // the inner side (away from the scratch) ends at the
                // middle of the screen at its bottom, the outer side (the
                // scratch) at the screen's edge at the judge line
                let (inner, outer) = if player == 0 {
                    (1.0 - axis, axis)
                } else {
                    (axis, 1.0 - axis)
                };
                key = key
                    .min(region * 0.5 * key_u / (inner * s_bottom))
                    .min(region * 0.5 * key_u / outer);
            }
            Fan {
                player,
                cx: (player as f32 + 0.5) * region,
                axis,
                base_y,
                len,
                width: key / key_u,
                k,
                bottom: h,
                lanes,
            }
        })
        .collect()
}

/// START / SELECT in a top corner of the canvas, side by side (they go
/// where the skins leave room for them): their rects as x, y, w, h.
pub fn system_buttons(w: f32, h: f32, right: bool) -> [[f32; 4]; 2] {
    let portrait = h > w;
    let (b, m) = if portrait { (84.0, 8.0) } else { (72.0, 8.0) };
    let x0 = if right { w - m - b * 2.0 - m } else { m };
    [[x0, m, b, b], [x0 + b + m, m, b, b]]
}

/// The canvas a perspective skin of `w0` x `h0` is widened or heightened to
/// so that it has the screen's aspect ratio.
pub fn reflowed_size(w0: f32, h0: f32, aspect: f32) -> (f32, f32) {
    if aspect <= 0.0 || !aspect.is_finite() {
        return (w0, h0);
    }
    if aspect > w0 / h0 {
        (h0 * aspect, h0)
    } else {
        (w0, w0 / aspect)
    }
}

/// Lane-space depths of what the play options set: the judge line (LIFT),
/// the lower edge of the lane cover (SUDDEN+) and of HIDDEN+'s cover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Depths {
    pub judge: f32,
    pub cover: f32,
    pub hidden: f32,
}

impl Depths {
    pub fn of(p: &crate::config::PlayConfig) -> Depths {
        let judge = p.lift.clamp(0.0, 0.9);
        let sudden = if p.lane_cover || p.sudden > 0.0 {
            p.sudden
        } else {
            0.0
        };
        let cover = (1.0 - sudden).max(judge + 0.05);
        Depths {
            judge,
            cover,
            hidden: judge + p.hidden.clamp(0.0, 1.0) * (cover - judge),
        }
    }

    /// The depth of a note `beats` ahead of the line, the lane showing
    /// `visible_beats` between the line and the cover (the same share as
    /// a flat lane's pixels: the green number holds).
    pub fn at(&self, ahead: f64, visible: f64) -> f32 {
        self.judge + (ahead / visible.max(1e-9)) as f32 * (self.cover - self.judge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fan(strength: f32) -> Fan {
        let cfg = PerspectiveConfig {
            strength,
            ..PerspectiveConfig::default()
        };
        layout(PlayMode::Beat7K, false, 720.0, 1280.0, &cfg).remove(0)
    }

    #[test]
    fn mapping_is_monotonic_and_narrows() {
        for strength in [0.0, 0.3, 0.65, 0.9] {
            let f = fan(strength);
            let mut last = (f32::MAX, f32::MAX);
            let d0 = f.d_bottom();
            assert!(d0 < 0.0 && f.y(d0) > f.base_y);
            for i in 0..=100 {
                let d = d0 + (1.0 - d0) * i as f32 / 100.0;
                let y = f.y(d);
                let w = f.point(1.0, d)[0] - f.point(0.0, d)[0];
                assert!(y < last.0, "y falls as d grows ({strength}, {d})");
                assert!(w <= last.1 + 1e-3, "lanes narrow ({strength}, {d})");
                last = (y, w);
            }
            // the far end: 1 - strength as wide as the base
            let top = f.point(1.0, 1.0)[0] - f.point(0.0, 1.0)[0];
            assert!((top / f.width - (1.0 - strength)).abs() < 1e-4);
            assert!((f.y(1.0) - (f.base_y - f.len)).abs() < 1e-3);
        }
    }

    #[test]
    fn flat_at_zero() {
        let f = fan(0.0);
        assert_eq!(f.k, 0.0);
        for i in 0..=10 {
            let d = i as f32 / 10.0;
            assert!((f.y(d) - (f.base_y - f.len * d)).abs() < 1e-3);
            assert!((f.point(0.0, d)[0] - f.point(0.0, 0.0)[0]).abs() < 1e-3);
        }
        // a key a tenth of the screen wide
        let key = f.lanes[1].u1 - f.lanes[1].u0;
        assert!((key * f.width - KEY_W * 720.0).abs() < 1e-3);
    }

    #[test]
    fn locate_inverts_point() {
        for strength in [0.0, 0.5, 0.9] {
            let f = fan(strength);
            for (u, d) in [(0.1, -0.1), (0.5, 0.0), (0.9, 0.5), (0.3, 1.0), (0.7, 0.95)] {
                let [x, y] = f.point(u, d);
                let (u2, d2) = f.locate(x, y).unwrap();
                assert!((u - u2).abs() < 1e-4 && (d - d2).abs() < 1e-4, "{u} {d}");
            }
        }
        // the vanishing point and above it: nothing
        let f = fan(0.65);
        let vy = f.base_y - f.len * (1.0 + f.k) / f.k;
        assert!(f.locate(360.0, vy - 1.0).is_none());
    }

    const SCREENS: [(f32, f32); 6] = [
        (2340.0, 1080.0),
        (2048.0, 1536.0),
        (1280.0, 720.0),
        (1080.0, 2340.0),
        (720.0, 1280.0),
        (1536.0, 2048.0),
    ];

    #[test]
    fn lanes_fit_the_screen() {
        for (w, h) in SCREENS {
            for mode in PlayMode::ALL {
                for side2 in [false, true] {
                    let fans = layout(mode, side2, w, h, &PerspectiveConfig::default());
                    assert_eq!(fans.len(), mode.players());
                    let lanes: usize = fans.iter().map(|f| f.lanes.len()).sum();
                    assert_eq!(lanes, mode.lane_count());
                    let what = format!("{mode:?} {side2} {w}x{h}");
                    for f in &fans {
                        // the judge line at 3/4 of the height, all of it
                        // on the screen; the far end near the top
                        assert!((f.base_y - 0.75 * h).abs() < 1e-3, "{what}");
                        let (a, b) = (f.point(0.0, 0.0)[0], f.point(1.0, 0.0)[0]);
                        assert!(a >= 0.0 && b <= w + 0.01, "{what}: {a} {b}");
                        let far = f.y(1.0);
                        assert!((far - top_of_lanes(h > w, h)).abs() < 0.1, "{what}");
                        if h < w {
                            assert!((far - 0.06 * h).abs() < 0.1, "{what}");
                        }
                    }
                    // double play: the fans meet at the bottom
                    if let [one, two] = &fans[..] {
                        let d = one.d_bottom();
                        let (a, b) = (one.point(1.0, d)[0], two.point(0.0, d)[0]);
                        assert!(a <= w * 0.5 + 0.01 && a > w * 0.49, "{what}: {a}");
                        assert!((a + b - w).abs() < 0.01, "{what}: {b}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_middle_keys_meet_upright_in_the_middle() {
        // 7 keys: 3|4 (2P: 4|5), 5 keys: 2|3 (2P: 3|4), at the middle of
        // the screen from the far end to the bottom
        for (w, h) in SCREENS {
            for (mode, left) in [(PlayMode::Beat7K, 3), (PlayMode::Beat5K, 2)] {
                for side2 in [false, true] {
                    let f = &layout(mode, side2, w, h, &PerspectiveConfig::default())[0];
                    let keys: Vec<&FanLane> = f.lanes.iter().filter(|l| !l.scratch).collect();
                    let n = if side2 { keys.len() - left } else { left };
                    let (a, b) = (keys[n - 1], keys[n]);
                    assert_eq!((a.lane, b.lane), (n - 1, n));
                    for d in [f.d_bottom(), 0.0, 0.5, 1.0] {
                        let x = f.point(a.u1, d)[0];
                        assert!((x - w * 0.5).abs() < 0.01, "{mode:?} {w}x{h} {d}: {x}");
                        assert!((f.point(b.u0, d)[0] - w * 0.5).abs() < 0.01);
                    }
                    // a key a tenth of the screen wide at the judge line
                    let key = f.point(a.u1, 0.0)[0] - f.point(a.u0, 0.0)[0];
                    assert!((key - KEY_W * w).abs() < 0.01, "{key}");
                    // the scratch on the outside, the far end a quarter
                    // as wide as the base
                    let sc = f.lanes.iter().position(|l| l.scratch).unwrap();
                    assert_eq!(sc, if side2 { f.lanes.len() - 1 } else { 0 });
                    assert!((f.scale(1.0) - 0.25).abs() < 1e-4);
                }
            }
        }
    }

    #[test]
    fn the_2p_side_mirrors() {
        for (w, h) in SCREENS {
            for mode in [PlayMode::Beat7K, PlayMode::Beat5K] {
                let cfg = PerspectiveConfig::default();
                let (one, two) = (
                    layout(mode, false, w, h, &cfg).remove(0),
                    layout(mode, true, w, h, &cfg).remove(0),
                );
                let n = one.lanes.len();
                for (i, a) in one.lanes.iter().enumerate() {
                    let b = &two.lanes[n - 1 - i];
                    assert_eq!((a.scratch, a.blue), (b.scratch, b.blue));
                    for d in [one.d_bottom(), 0.0, 1.0] {
                        let (a0, a1) = (one.point(a.u0, d)[0], one.point(a.u1, d)[0]);
                        let (b0, b1) = (two.point(b.u0, d)[0], two.point(b.u1, d)[0]);
                        assert!((a0 - (w - b1)).abs() < 0.01 && (a1 - (w - b0)).abs() < 0.01);
                    }
                }
            }
        }
    }

    #[test]
    fn scratch_on_the_play_side() {
        let l = fan_lanes(PlayMode::Beat7K, 0, false);
        assert!(l[0].scratch && l[0].lane == 7);
        assert!(l[1].u0 > l[0].u1, "a gap after the scratch");
        assert!((l.last().unwrap().u1 - 1.0).abs() < 1e-6);
        let r = fan_lanes(PlayMode::Beat7K, 0, true);
        assert!(r[7].scratch && r[0].lane == 0 && r[7].u1 == 1.0);
        // double play: 1P's on the left, 2P's on the right
        let fans = layout(PlayMode::Beat14K, false, 1280.0, 720.0, &Default::default());
        assert!(fans[0].lanes[0].scratch && fans[1].lanes[7].scratch);
        assert_eq!(fans[1].lanes[0].lane, 8);
        // no scratch: equal lanes, no gap
        let p = fan_lanes(PlayMode::Popn9K, 0, false);
        assert!(p.windows(2).all(|w| (w[0].u1 - w[1].u0).abs() < 1e-6));
    }

    #[test]
    fn green_number_holds_in_lane_space() {
        // a note at the main BPM takes the green number's time from the
        // cover's edge to the judge line, whatever the perspective
        let mut p = crate::config::PlayConfig {
            sudden: 0.2,
            lift: 0.1,
            lane_cover: true,
            ..Default::default()
        };
        let b = crate::config::Bpms::flat(150.0);
        let dur = p.duration_at(&b, 150.0, true);
        let beats = dur / 1000.0 * 150.0 / 60.0;
        let dep = Depths::of(&p);
        assert!((dep.at(beats, beats) - dep.cover).abs() < 1e-6);
        assert!((dep.at(0.0, beats) - dep.judge).abs() < 1e-6);
        // the lane holds 4 / hi-speed beats over its whole length, as a
        // flat lane does
        let whole = 4.0 / p.hispeed_at(&b) as f64;
        assert!((dep.at(whole, beats) - (dep.judge + 1.0)).abs() < 1e-4);
        // and the same for the screen: what the perspective maps is d only
        let f = fan(0.65);
        let flat = fan(0.0);
        for fan in [&f, &flat] {
            assert!((fan.y(dep.at(beats, beats)) - fan.y(dep.cover)).abs() < 1e-3);
        }
        p.hidden = 0.5;
        let dep = Depths::of(&p);
        assert!((dep.hidden - (dep.judge + 0.5 * (dep.cover - dep.judge))).abs() < 1e-6);
    }

    #[test]
    fn reflow_keeps_the_aspect() {
        assert_eq!(reflowed_size(720.0, 1280.0, 9.0 / 19.5), (720.0, 1560.0));
        assert_eq!(reflowed_size(1280.0, 720.0, 4.0 / 3.0), (1280.0, 960.0));
        let (w, h) = reflowed_size(1280.0, 720.0, 19.5 / 9.0);
        assert!((w - 1560.0).abs() < 0.01 && h == 720.0);
    }
}
