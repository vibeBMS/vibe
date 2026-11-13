//! On-screen touch controls (phones, tablets, Surface, touch monitors).
//!
//! Two styles: a row of keys like a keyboard, or an IIDX-controller-like
//! turntable plus two rows of buttons. Fingers can slide between keys, and
//! circling the turntable reverses scratch direction (back-spin scratch).

use crate::config::TouchStyle;
use crate::gfx::{with_alpha, Batch, Color, Rect};
use crate::input::{LaneEv, Phase};
use crate::platform::Instant;
use std::collections::HashMap;
use vibebms_core::PlayMode;

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Rect(Rect),
    Circle { cx: f32, cy: f32, r: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub shape: Shape,
    pub lane: u8,
    pub scratch: bool,
    /// Even-numbered keys (blue on IIDX).
    pub black: bool,
}

impl Zone {
    fn hit(&self, x: f32, y: f32) -> bool {
        match self.shape {
            Shape::Rect(r) => r.contains(x, y),
            Shape::Circle { cx, cy, r } => (x - cx).powi(2) + (y - cy).powi(2) <= r * r,
        }
    }
}

/// The IIDX keypad in mm: key faces, the gap between neighbouring white
/// keys, the gap between the white row and the blue row above it, and the
/// turntable's distance to key 1.
const ARCADE_KEY_W: f32 = 29.0;
const ARCADE_KEY_H: f32 = 46.0;
const ARCADE_KEY_GAP: f32 = 12.0;
const ARCADE_ROW_GAP: f32 = 10.0;
const ARCADE_DISC_GAP: f32 = 12.0;

/// `area` narrowed to `w`, centred.
fn centred(area: Rect, w: f32) -> Rect {
    if area.w > w {
        Rect::new(area.x + (area.w - w) * 0.5, area.y, w, area.h)
    } else {
        area
    }
}

/// Build the zones for one player inside `area`.
pub fn layout(
    mode: PlayMode,
    style: TouchStyle,
    area: Rect,
    player: usize,
    scratch_right: bool,
) -> Vec<Zone> {
    // width units of a key and of the scratch
    let lane_width = [1.0, 1.7];
    let keys = mode.keys_per_player();
    let base = player * mode.lanes_per_player();
    let sc_lane = mode.scratch_lane(player).map(|l| l as u8);
    let gap = (area.w * 0.006).max(2.0);
    // a row of keys no wider than half the controls' height each: on a
    // wide screen the controls keep to the middle of the area
    let area = match style {
        TouchStyle::Keyboard => {
            let units: f32 = (0..mode.lanes_per_player())
                .map(|l| lane_width[usize::from(mode.is_scratch(base + l))])
                .sum();
            centred(area, units * area.h * 0.5)
        }
        TouchStyle::Iidx => area,
    };
    let mut zones = Vec::new();
    match style {
        TouchStyle::Keyboard => {
            let order = mode.display_order(player, scratch_right);
            let units: f32 = order
                .iter()
                .map(|l| {
                    if mode.is_scratch(*l) {
                        lane_width[1]
                    } else {
                        lane_width[0]
                    }
                })
                .sum();
            let mut x = area.x;
            for l in order {
                let sc = mode.is_scratch(l);
                let w = area.w * (if sc { lane_width[1] } else { lane_width[0] }) / units;
                let k = l - base;
                zones.push(Zone {
                    shape: Shape::Rect(Rect::new(x + gap * 0.5, area.y, w - gap, area.h)),
                    lane: l as u8,
                    scratch: sc,
                    black: !sc && k % 2 == 1,
                });
                x += w;
            }
        }
        TouchStyle::Iidx => {
            // the arcade keypad scaled to the controls' height (`u` px per
            // mm): white keys below, the blue ones above over the gaps
            let u = area.h / (ARCADE_KEY_H * 2.0 + ARCADE_ROW_GAP);
            let whites = keys.div_ceil(2) as f32;
            let pitch = (ARCADE_KEY_W + ARCADE_KEY_GAP) * u;
            let keypad = whites * pitch - ARCADE_KEY_GAP * u;
            let r = area.h * 0.47;
            let disc = sc_lane.map_or(0.0, |_| r * 2.0 + ARCADE_DISC_GAP * u);
            // too narrow (portrait, one side of double play): the same
            // keypad, narrower
            let fits = area.w >= disc + keypad;
            let area = if fits {
                centred(area, disc + keypad)
            } else {
                area
            };
            let mut keys_area = area;
            if let Some(sc) = sc_lane {
                let r = if fits { r } else { r.min(area.w * 0.2) };
                let d = if fits { disc } else { r * 2.0 + gap * 2.0 };
                let cx = if scratch_right {
                    area.right() - r
                } else {
                    area.x + r
                };
                zones.push(Zone {
                    shape: Shape::Circle {
                        cx,
                        cy: area.y + area.h * 0.5,
                        r,
                    },
                    lane: sc,
                    scratch: true,
                    black: false,
                });
                keys_area = Rect::new(
                    if scratch_right { area.x } else { area.x + d },
                    area.y,
                    area.w - d,
                    area.h,
                );
            }
            let pitch = if fits {
                pitch
            } else {
                keys_area.w / (whites - ARCADE_KEY_GAP / (ARCADE_KEY_W + ARCADE_KEY_GAP))
            };
            let kw = pitch * ARCADE_KEY_W / (ARCADE_KEY_W + ARCADE_KEY_GAP);
            let kh = ARCADE_KEY_H * u;
            for k in 0..keys {
                let black = k % 2 == 1;
                let x =
                    keys_area.x + (k / 2) as f32 * pitch + if black { pitch * 0.5 } else { 0.0 };
                let y = if black { area.y } else { area.bottom() - kh };
                zones.push(Zone {
                    shape: Shape::Rect(Rect::new(x, y, kw, kh)),
                    lane: (base + k) as u8,
                    scratch: false,
                    black,
                });
            }
        }
    }
    zones
}

struct Ptr {
    zone: usize,
    lane: u8,
    sub: u8,
    last_angle: f32,
    last_y: f32,
    accum: f32,
    dir: i8,
}

#[derive(Default)]
pub struct TouchInput {
    ptrs: HashMap<u64, Ptr>,
    held: HashMap<(u8, u8), u32>,
}

impl TouchInput {
    pub fn is_pressed(&self, lane: u8) -> bool {
        self.held.iter().any(|((l, _), c)| *l == lane && *c > 0)
    }

    fn press(&mut self, lane: u8, sub: u8, time: Instant, out: &mut Vec<LaneEv>) {
        let c = self.held.entry((lane, sub)).or_insert(0);
        *c += 1;
        if *c == 1 {
            out.push(LaneEv {
                lane,
                sub,
                pressed: true,
                time,
            });
        }
    }

    fn release(&mut self, lane: u8, sub: u8, time: Instant, out: &mut Vec<LaneEv>) {
        if let Some(c) = self.held.get_mut(&(lane, sub)) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                out.push(LaneEv {
                    lane,
                    sub,
                    pressed: false,
                    time,
                });
            }
        }
    }

    /// Returns true if the pointer is handled by the touch controls.
    #[allow(clippy::too_many_arguments)]
    pub fn pointer(
        &mut self,
        zones: &[Zone],
        id: u64,
        phase: Phase,
        x: f32,
        y: f32,
        time: Instant,
        threshold_deg: f32,
        out: &mut Vec<LaneEv>,
    ) -> bool {
        match phase {
            Phase::Down => {
                let Some(zi) = zones.iter().position(|z| z.hit(x, y)) else {
                    return false;
                };
                let z = zones[zi];
                let angle = match z.shape {
                    Shape::Circle { cx, cy, .. } => (y - cy).atan2(x - cx),
                    _ => 0.0,
                };
                self.press(z.lane, 0, time, out);
                self.ptrs.insert(
                    id,
                    Ptr {
                        zone: zi,
                        lane: z.lane,
                        sub: 0,
                        last_angle: angle,
                        last_y: y,
                        accum: 0.0,
                        dir: 0,
                    },
                );
                true
            }
            Phase::Move => {
                let Some(p) = self.ptrs.get(&id) else {
                    return false;
                };
                let zi = p.zone;
                let Some(z) = zones.get(zi).copied() else {
                    return true;
                };
                if z.scratch {
                    let (delta, nangle) = match z.shape {
                        Shape::Circle { cx, cy, .. } => {
                            let a = (y - cy).atan2(x - cx);
                            let mut d = a - p.last_angle;
                            if d > std::f32::consts::PI {
                                d -= std::f32::consts::TAU;
                            } else if d < -std::f32::consts::PI {
                                d += std::f32::consts::TAU;
                            }
                            (d.to_degrees(), a)
                        }
                        // keyboard style scratch: vertical swipes, ~3 px per "degree"
                        Shape::Rect(_) => ((y - p.last_y) / 3.0, 0.0),
                    };
                    let p = self.ptrs.get_mut(&id).unwrap();
                    p.last_angle = nangle;
                    p.last_y = y;
                    if delta.signum() != p.accum.signum() {
                        p.accum = 0.0;
                    }
                    p.accum += delta;
                    if p.accum.abs() >= threshold_deg {
                        let dir: i8 = if p.accum > 0.0 { 1 } else { -1 };
                        p.accum = 0.0;
                        if p.dir != 0 && dir != p.dir {
                            // direction change: release and press the other sub-key (BSS end)
                            let (lane, old) = (p.lane, p.sub);
                            let new = 1 - old;
                            p.sub = new;
                            p.dir = dir;
                            self.release(lane, old, time, out);
                            self.press(lane, new, time, out);
                        } else {
                            p.dir = dir;
                        }
                    }
                    return true;
                }
                // slide between keys
                if let Some(ni) = zones.iter().position(|nz| !nz.scratch && nz.hit(x, y)) {
                    if ni != zi {
                        let (lane, sub) = (p.lane, p.sub);
                        let nl = zones[ni].lane;
                        self.release(lane, sub, time, out);
                        self.press(nl, 0, time, out);
                        let p = self.ptrs.get_mut(&id).unwrap();
                        p.zone = ni;
                        p.lane = nl;
                        p.sub = 0;
                    }
                }
                true
            }
            Phase::Up => {
                let Some(p) = self.ptrs.remove(&id) else {
                    return false;
                };
                self.release(p.lane, p.sub, time, out);
                true
            }
        }
    }

    pub fn draw(
        &self,
        batch: &mut Batch,
        zones: &[Zone],
        opacity: f32,
        white: Color,
        blue: Color,
        scratch: Color,
    ) {
        for z in zones {
            let pressed = self.is_pressed(z.lane);
            let base = if z.scratch {
                scratch
            } else if z.black {
                blue
            } else {
                white
            };
            let fill = with_alpha(base, opacity * if pressed { 0.85 } else { 0.28 });
            let edge = with_alpha(base, opacity * 0.9);
            match z.shape {
                Shape::Rect(r) => {
                    batch.rect(r, fill);
                    batch.rect_outline(r, 2.0, edge);
                }
                Shape::Circle { cx, cy, r } => {
                    batch.circle(cx, cy, r, 0.0, fill, 64);
                    batch.circle(cx, cy, r, r - 3.0, edge, 64);
                    batch.circle(cx, cy, r * 0.18, 0.0, with_alpha(edge, 0.8), 24);
                    // rotation hint marks
                    for i in 0..12 {
                        let a = i as f32 / 12.0 * std::f32::consts::TAU;
                        let (s, c) = a.sin_cos();
                        let rr = r * 0.82;
                        batch.rect(
                            Rect::new(cx + c * rr - 2.0, cy + s * rr - 2.0, 4.0, 4.0),
                            with_alpha(edge, 0.6),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iidx_layout_has_all_lanes() {
        let z = layout(
            PlayMode::Beat7K,
            TouchStyle::Iidx,
            Rect::new(0.0, 500.0, 800.0, 300.0),
            0,
            false,
        );
        assert_eq!(z.len(), 8);
        let mut lanes: Vec<u8> = z.iter().map(|z| z.lane).collect();
        lanes.sort();
        assert_eq!(lanes, (0..8).collect::<Vec<u8>>());
        assert!(z[0].scratch);
        let k = layout(
            PlayMode::Popn9K,
            TouchStyle::Keyboard,
            Rect::new(0.0, 0.0, 900.0, 100.0),
            0,
            false,
        );
        assert_eq!(k.len(), 9);
    }

    #[test]
    fn wide_areas_keep_keys_together() {
        let area = Rect::new(0.0, 500.0, 2400.0, 240.0);
        let z = layout(PlayMode::Beat7K, TouchStyle::Iidx, area, 0, false);
        let xs: Vec<f32> = z
            .iter()
            .map(|z| match z.shape {
                Shape::Rect(r) => r.right(),
                Shape::Circle { cx, r, .. } => cx - r,
            })
            .collect();
        let width_of = |z: &Zone| match z.shape {
            Shape::Rect(r) => r.w,
            Shape::Circle { r, .. } => r * 2.0,
        };
        // arcade proportions (29 x 46, two rows in a 102 high box), centred
        let key = z.iter().find(|z| !z.scratch).map(width_of).unwrap();
        assert!((key - 240.0 * 29.0 / 102.0).abs() < 0.5, "{key}");
        let (left, right) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(0.0, f32::max),
        );
        assert!((left - (2400.0 - right)).abs() < 20.0, "{left} {right}");
    }

    #[test]
    fn slide_and_scratch_reverse() {
        let zones = layout(
            PlayMode::Beat7K,
            TouchStyle::Keyboard,
            Rect::new(0.0, 0.0, 870.0, 200.0),
            0,
            false,
        );
        let mut t = TouchInput::default();
        let now = Instant::now();
        let mut out = Vec::new();
        // keyboard layout: scratch (1.7 units = 170px) then 7 keys of 100px
        assert!(t.pointer(&zones, 1, Phase::Down, 175.0, 50.0, now, 6.0, &mut out));
        assert_eq!(out.last().unwrap().lane, 0);
        t.pointer(&zones, 1, Phase::Move, 275.0, 50.0, now, 6.0, &mut out);
        assert_eq!(out.len(), 3); // press 0, release 0, press 1
        assert_eq!(out[2].lane, 1);
        out.clear();
        // scratch: swipe down then up => press, then release+press (BSS)
        t.pointer(&zones, 2, Phase::Down, 50.0, 10.0, now, 6.0, &mut out);
        t.pointer(&zones, 2, Phase::Move, 50.0, 40.0, now, 6.0, &mut out);
        t.pointer(&zones, 2, Phase::Move, 50.0, 10.0, now, 6.0, &mut out);
        let sc: Vec<_> = out.iter().map(|e| (e.lane, e.sub, e.pressed)).collect();
        assert_eq!(sc, vec![(7, 0, true), (7, 0, false), (7, 1, true)]);
        out.clear();
        t.pointer(&zones, 2, Phase::Up, 50.0, 10.0, now, 6.0, &mut out);
        assert_eq!(out[0].sub, 1);
        assert!(!out[0].pressed);
    }
}
