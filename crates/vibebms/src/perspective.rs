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

use crate::config::{PerspectiveConfig, LENGTH_RANGE, PERSPECTIVE_MAX};
use vibebms_core::PlayMode;

/// The canvases the perspective skins are made for.
pub const PORTRAIT: (f32, f32) = (720.0, 1280.0);
pub const LANDSCAPE: (f32, f32) = (1280.0, 720.0);

/// Width of the scratch lane and of the gap beside it, in key widths.
const SCRATCH_W: f32 = 1.45;
const SCRATCH_GAP: f32 = 0.25;

/// One lane of a fan: its range across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FanLane {
    pub lane: usize,
    pub u0: f32,
    pub u1: f32,
    pub scratch: bool,
}

/// Corners of a shape: top-left, top-right, bottom-right, bottom-left.
pub type Quad = [[f32; 2]; 4];

/// One player's lanes on the screen (or canvas).
#[derive(Clone, Debug, PartialEq)]
pub struct Fan {
    pub player: usize,
    /// Centre x and y of the base (`d` = 0).
    pub cx: f32,
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

    pub fn y(&self, d: f32) -> f32 {
        self.base_y - self.len * d * (1.0 + self.k) / (1.0 + self.k * d)
    }

    pub fn point(&self, u: f32, d: f32) -> [f32; 2] {
        [self.cx + (u - 0.5) * self.width * self.scale(d), self.y(d)]
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
        Some((0.5 + (x - self.cx) / w.max(1e-3), d))
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

    /// The touch columns: each lane from the bottom up to screen height
    /// `top`, widened to meet its neighbours halfway across a gap, the
    /// outer lanes reaching out to `x0` and `x1` (the screen's edges, or
    /// halfway to the next fan). What is drawn as the touch area and what
    /// a touch hits.
    pub fn touch_quads(&self, top: f32, x0: f32, x1: f32) -> Vec<(FanLane, Quad)> {
        let d0 = self.d_bottom();
        let d1 = self.depth(top).unwrap_or(1.0).clamp(d0, 1.0);
        let n = self.lanes.len();
        // a side of column `i` (its left edge; `n`: the right edge of the last)
        let side = |i: usize, d: f32| match i {
            0 => [x0, self.y(d)],
            i if i == n => [x1, self.y(d)],
            i => self.point((self.lanes[i - 1].u1 + self.lanes[i].u0) * 0.5, d),
        };
        self.lanes
            .iter()
            .enumerate()
            .map(|(i, l)| {
                (
                    *l,
                    [side(i, d1), side(i + 1, d1), side(i + 1, d0), side(i, d0)],
                )
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

    /// Bounding box: x, y, w, h.
    pub fn bounds(&self) -> [f32; 4] {
        let d0 = self.d_bottom();
        let (a, b) = (self.point(0.0, d0), self.point(1.0, d0));
        let (c, e) = (self.point(0.0, 1.0), self.point(1.0, 1.0));
        let x0 = a[0].min(c[0]);
        let x1 = b[0].max(e[0]);
        [x0, c[1], x1 - x0, a[1] - c[1]]
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
        });
        x += width(l);
        if sc && i == 0 {
            x += gap;
        }
    }
    out
}

/// Where the base (`d` = 0) is: a share of the canvas height above its
/// bottom, the room the touch area and the lanes below the line take.
fn base_gap(portrait: bool) -> f32 {
    if portrait {
        0.14
    } else {
        0.16
    }
}

/// The top of the room the lanes can reach (under the skin's title, gauge
/// and score rows; in landscape double play under the middle column).
pub fn top_of_lanes(portrait: bool, double: bool) -> f32 {
    match (portrait, double) {
        (true, _) => 212.0,
        (false, false) => 96.0,
        (false, true) => 96.0,
    }
}

/// The widest a landscape fan gets at the bottom, in screen heights: on a
/// very wide screen the lanes stop growing rather than getting flat.
const MAX_FAN_ASPECT: f32 = 2.4;

/// The fans of `mode` on a canvas of `w` x `h` (y down), one per player.
/// `side2`: the 2P side in single play (the scratch on the right).
///
/// The lanes are laid out like the mobile perspective games (BanG Dream!,
/// Project SEKAI, Arcaea): the fans reach the screen's bottom corners, so
/// the judge line spans about 85 % of the width (two fans side by side in
/// double play), and the touch columns are as wide as the lanes there.
pub fn layout(mode: PlayMode, side2: bool, w: f32, h: f32, cfg: &PerspectiveConfig) -> Vec<Fan> {
    let portrait = h > w;
    let double = mode.is_double();
    let k = foreshortening(cfg.strength);
    let base_y = h - base_gap(portrait) * h;
    let top = top_of_lanes(portrait, double).min(base_y - 40.0);
    let length = cfg.length.clamp(LENGTH_RANGE.0, LENGTH_RANGE.1);
    let len = (base_y - top) * length;
    // the width at the bottom of the screen of all the fans side by side
    let all = if portrait {
        w
    } else {
        w.min(h * MAX_FAN_ASPECT * mode.players() as f32)
    };
    let n = mode.players();
    let bw = all / n as f32;
    // the size at the bottom relative to the base
    let s_bottom = 1.0 + k * (h - base_y) / (len * (1.0 + k));
    (0..n)
        .map(|player| Fan {
            player,
            cx: w * 0.5 + (player as f32 + 0.5 - n as f32 * 0.5) * bw,
            base_y,
            len,
            width: bw / s_bottom,
            k,
            bottom: h,
            lanes: fan_lanes(mode, player, if double { player == 1 } else { side2 }),
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
        // the bottom of the screen is the bottom of the lanes, as wide
        assert!((f.width - 720.0).abs() < 1e-3);
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

    #[test]
    fn the_screen_bottom_fits() {
        for (w, h) in [
            (720.0, 1280.0),
            (720.0, 1560.0),
            (1280.0, 720.0),
            (1560.0, 720.0),
        ] {
            for mode in [PlayMode::Beat7K, PlayMode::Beat14K, PlayMode::Popn9K] {
                let fans = layout(mode, false, w, h, &PerspectiveConfig::default());
                assert_eq!(fans.len(), mode.players());
                let lanes: usize = fans.iter().map(|f| f.lanes.len()).sum();
                assert_eq!(lanes, mode.lane_count());
                for f in &fans {
                    let [x, y, bw, bh] = f.bounds();
                    assert!(x >= 0.0 && x + bw <= w + 0.01, "{mode:?} {w}x{h}: {x} {bw}");
                    assert!(y >= 0.0 && y + bh <= h + 0.01);
                    assert!(
                        f.base_y < h && f.y(1.0) >= top_of_lanes(h > w, mode.is_double()) - 0.1
                    );
                }
            }
        }
    }

    #[test]
    fn judge_line_spans_most_of_the_width() {
        // as in the mobile perspective games: about 85 % of the width at
        // the judge line (both fans in double play), in landscape too
        for (w, h) in [
            (1280.0, 720.0),
            (1560.0, 720.0),
            (1280.0, 960.0),
            (720.0, 1280.0),
            (720.0, 1560.0),
        ] {
            for mode in PlayMode::ALL {
                let fans = layout(mode, false, w, h, &PerspectiveConfig::default());
                let share = fans.iter().map(|f| f.width).sum::<f32>() / w;
                assert!((0.83..0.9).contains(&share), "{mode:?} {w}x{h}: {share}");
            }
        }
        // a very wide screen: the lanes stop growing
        let f = layout(PlayMode::Beat7K, false, 4000.0, 720.0, &Default::default());
        assert!(f[0].bounds()[2] <= 720.0 * MAX_FAN_ASPECT + 0.01);
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
