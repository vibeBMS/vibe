//! On-screen touch controls (phones, tablets, Surface, touch monitors).
//!
//! Two presets: a row of keys like a keyboard, or a beatmania-controller-like
//! turntable plus two rows of buttons (BM); and the player's own layouts,
//! edited in `scene::touch_layout`. Fingers can slide between keys, and
//! circling the turntable reverses scratch direction (back-spin scratch).

use crate::config::{TouchButton, TouchConfig, TouchLayout, TouchSide, TouchStyle};
use crate::gfx::{with_alpha, Batch, Rect};
use crate::input::{LaneEv, Phase};
use crate::platform::Instant;
use std::collections::HashMap;
use vibebms_core::PlayMode;

#[derive(Clone, Debug)]
pub enum Shape {
    Rect(Rect),
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    /// Rows from the top down, each `[y, x0, x1]`: at `y` the shape spans
    /// `x0..x1`, its edges straight between rows. A lane of the perspective
    /// skin (`perspective::Fan::touch_strips`); its scratch is played with
    /// sideways swipes.
    Strip(std::sync::Arc<[[f32; 3]]>),
}

/// Pseudo lanes of the START and SELECT buttons (the hi-speed panel), and
/// the second player's in VS.
pub const START: u8 = 254;
pub const SELECT: u8 = 255;
pub const START2: u8 = 252;
pub const SELECT2: u8 = 253;
/// The first pseudo lane: lanes from here on are buttons, not keys.
pub const SYSTEM: u8 = START2;

#[derive(Clone, Debug)]
pub struct Zone {
    pub shape: Shape,
    pub lane: u8,
    pub scratch: bool,
    /// Even-numbered keys (blue on IIDX).
    pub black: bool,
    /// The button's own colour (pop'n buttons).
    pub color: Option<crate::gfx::Color>,
}

impl Zone {
    pub fn hit(&self, x: f32, y: f32) -> bool {
        match &self.shape {
            // the right edge belongs to the next strip: neighbours share
            // their edges, and a point is in one of them only
            Shape::Strip(rows) => strip_span(rows, y).is_some_and(|(a, b)| a <= x && x < b),
            _ => self.distance(x, y) <= 0.0,
        }
    }

    /// How far (x, y) is outside the zone (0 inside).
    fn distance(&self, x: f32, y: f32) -> f32 {
        match self.shape {
            Shape::Rect(r) => {
                let dx = (r.x - x).max(x - r.right()).max(0.0);
                let dy = (r.y - y).max(y - r.bottom()).max(0.0);
                dx.hypot(dy)
            }
            Shape::Circle { cx, cy, r } => ((x - cx).hypot(y - cy) - r).max(0.0),
            Shape::Strip(ref rows) => strip_distance(rows, x, y),
        }
    }

    /// How far outside a key a touch still counts for it (when no key is
    /// closer): half the key's size.
    fn reach(&self) -> f32 {
        match self.shape {
            Shape::Rect(r) => r.w.min(r.h) * 0.5,
            Shape::Circle { r, .. } => r * 0.5,
            // the perspective lanes' strips fill their area edge to edge
            Shape::Strip(_) => 0.0,
        }
    }

    /// The rectangle the zone is drawn in (a circle's bounding square).
    pub fn bounds(&self) -> Rect {
        match self.shape {
            Shape::Rect(r) => r,
            Shape::Circle { cx, cy, r } => Rect::new(cx - r, cy - r, r * 2.0, r * 2.0),
            Shape::Strip(ref rows) => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for &[y, a, b] in rows.iter() {
                    (x0, y0, x1, y1) = (x0.min(a), y0.min(y), x1.max(b), y1.max(y));
                }
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            }
        }
    }
}

/// A strip's span at height `y` (`None` above or below it).
pub fn strip_span(rows: &[[f32; 3]], y: f32) -> Option<(f32, f32)> {
    let (first, last) = (rows.first()?, rows.last()?);
    if y < first[0] || y > last[0] {
        return None;
    }
    let i = rows
        .partition_point(|r| r[0] <= y)
        .clamp(1, rows.len().max(2) - 1);
    let (a, b) = (rows[i - 1], rows.get(i).copied().unwrap_or(rows[i - 1]));
    let t = if b[0] > a[0] {
        ((y - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some((a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t))
}

/// How far (x, y) is outside a strip (0 inside or on its edge), near
/// enough: across at heights it spans, else down to its top or bottom row.
fn strip_distance(rows: &[[f32; 3]], x: f32, y: f32) -> f32 {
    if let Some((a, b)) = strip_span(rows, y) {
        return (a - x).max(x - b).max(0.0);
    }
    let Some(r) = (match (rows.first(), rows.last()) {
        (Some(f), _) if y < f[0] => Some(f),
        (_, l) => l,
    }) else {
        return f32::MAX;
    };
    let dx = (r[1] - x).max(x - r[2]).max(0.0);
    dx.hypot(y - r[0])
}

/// The zone a new touch at (x, y) presses: the one under it (the last
/// drawn where they overlap), else the nearest key within its reach.
/// Touchscreens give no feel for the gaps between keys, so a finger
/// landing in one still plays the key next to it.
pub fn pick(zones: &[Zone], x: f32, y: f32) -> Option<usize> {
    zones.iter().rposition(|z| z.hit(x, y)).or_else(|| {
        zones
            .iter()
            .enumerate()
            .filter(|(_, z)| !z.scratch && z.lane < SYSTEM)
            .map(|(i, z)| (i, z.distance(x, y), z.reach()))
            .filter(|(_, d, reach)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, ..)| i)
    })
}

/// pop'n music button colours, buttons 1..9.
const POPN_COLORS: [u32; 9] = [
    0xe8ecf2ff, 0xffd23cff, 0x46dc82ff, 0x3f8cffff, 0xff4d5eff, 0x3f8cffff, 0x46dc82ff, 0xffd23cff,
    0xe8ecf2ff,
];

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
    px_per_mm: f32,
) -> Vec<Zone> {
    // 4 / 6 / 8 keys have no arcade keypad: a row of keys
    let style = match mode.is_qwilight() {
        true => TouchStyle::Keyboard,
        false => style,
    };
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
        TouchStyle::Iidx | TouchStyle::Custom => area,
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
                    color: None,
                });
                x += w;
            }
        }
        TouchStyle::Iidx | TouchStyle::Custom if mode == PlayMode::Popn9K => {
            // the pop'n music panel: round buttons, 1 3 5 7 9 below and
            // 2 4 6 8 above between them, as wide as the area allows
            let h = area.h * 0.94;
            // neighbours (half a pitch across, `rise` up) stay apart
            let d = (area.w / 6.8).min(h / 1.82).min(60.0 * px_per_mm);
            let pitch = d / 0.7;
            let rise = d * 0.82;
            let x0 = area.x + (area.w - pitch * 4.0 - d) * 0.5 + d * 0.5;
            let y0 = area.y + (area.h - d - rise) * 0.5 + rise + d * 0.5;
            for k in 0..keys {
                let up = k % 2 == 1;
                zones.push(Zone {
                    shape: Shape::Circle {
                        cx: x0 + k as f32 * pitch * 0.5,
                        cy: if up { y0 - rise } else { y0 },
                        r: d * 0.5,
                    },
                    lane: (base + k) as u8,
                    scratch: false,
                    black: up,
                    color: Some(crate::gfx::rgba(POPN_COLORS[k.min(8)])),
                });
            }
        }
        // (Custom is resolved by `zones`: BM here)
        TouchStyle::Iidx | TouchStyle::Custom => {
            // the arcade keypad at its real width: white keys below, the
            // blue ones above over the gaps (the rows share the controls'
            // height, at most the real key height)
            let u = px_per_mm;
            let row_gap = (ARCADE_ROW_GAP * u).min(area.h * 0.15);
            let kh = (ARCADE_KEY_H * u).min((area.h - row_gap) * 0.5);
            let whites = keys.div_ceil(2) as f32;
            let pitch = (ARCADE_KEY_W + ARCADE_KEY_GAP) * u;
            let keypad = whites * pitch - ARCADE_KEY_GAP * u;
            // the turntable: all of it when there is room; else the keys
            // keep their width and the turntable goes partly off the edge
            // of the screen (scratching still turns around its centre),
            // down to a quarter of it showing; past that the keys get narrower
            let r = area.h * 0.47;
            let disc_gap = sc_lane.map_or(0.0, |_| ARCADE_DISC_GAP * u);
            let (whole, least) = sc_lane.map_or((0.0, 0.0), |_| (r * 2.0, r * 0.5));
            let (area, shown, pitch) = if area.w >= whole + disc_gap + keypad {
                (centred(area, whole + disc_gap + keypad), whole, pitch)
            } else if area.w >= least + disc_gap + keypad {
                (area, area.w - disc_gap - keypad, pitch)
            } else {
                let keys_w = area.w - least - disc_gap;
                let pitch = keys_w / (whites - ARCADE_KEY_GAP / (ARCADE_KEY_W + ARCADE_KEY_GAP));
                (area, least, pitch)
            };
            if let Some(sc) = sc_lane {
                let cx = if scratch_right {
                    area.right() - shown + r
                } else {
                    area.x + shown - r
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
                    color: None,
                });
            }
            let used = shown + disc_gap;
            let keys_area = Rect::new(
                if scratch_right { area.x } else { area.x + used },
                area.y,
                area.w - used,
                area.h,
            );
            let kw = pitch * ARCADE_KEY_W / (ARCADE_KEY_W + ARCADE_KEY_GAP);
            let top = area.y + (area.h - kh * 2.0 - row_gap) * 0.5;
            for k in 0..keys {
                let black = k % 2 == 1;
                let x =
                    keys_area.x + (k / 2) as f32 * pitch + if black { pitch * 0.5 } else { 0.0 };
                let y = if black { top } else { top + kh + row_gap };
                zones.push(Zone {
                    shape: Shape::Rect(Rect::new(x, y, kw, kh)),
                    lane: (base + k) as u8,
                    scratch: false,
                    black,
                    color: None,
                });
            }
        }
    }
    zones
}

/// The range of the controls' share of the screen height.
pub const CONTROLS_HEIGHT: (f32, f32) = (0.12, 0.6);

/// The setting of `controls_share`.
pub fn controls_share_mut(cfg: &mut TouchConfig, portrait: bool) -> &mut f32 {
    if portrait {
        &mut cfg.portrait_height
    } else {
        &mut cfg.landscape_height
    }
}

/// The controls' share of the screen height in this orientation.
pub fn controls_share(cfg: &TouchConfig, portrait: bool) -> f32 {
    if portrait {
        cfg.portrait_height
    } else {
        cfg.landscape_height
    }
}

/// The strip at the bottom of a `w` x `h` screen the controls take.
pub fn controls_area(w: f32, h: f32, cfg: &TouchConfig) -> Rect {
    let share = controls_share(cfg, h > w);
    let ctrl_h = (h * share).max(80.0).min(h * CONTROLS_HEIGHT.1);
    Rect::new(0.0, h - ctrl_h, w, ctrl_h)
}

/// The presets' controls of each player in `area`: (rect, player, scratch
/// on the right). `side2`: the 2P side (single play).
pub fn player_areas(
    mode: PlayMode,
    area: Rect,
    cfg: &TouchConfig,
    side2: bool,
    px_per_mm: f32,
) -> Vec<(Rect, usize, bool)> {
    let dp = mode.is_double();
    if dp && cfg.allow_dp {
        // the two keypads a white key's gap (12 mm) apart
        let half = (area.w - 12.0 * px_per_mm).max(area.w * 0.9) * 0.5;
        vec![
            (Rect::new(area.x, area.y, half, area.h), 0, false),
            (
                Rect::new(area.right() - half, area.y, half, area.h),
                1,
                true,
            ),
        ]
    } else {
        vec![(area, 0, !dp && side2)]
    }
}

/// Where the custom layout of a key mode and orientation is kept in
/// `TouchConfig::layouts`.
pub fn slot(mode: PlayMode, portrait: bool) -> String {
    let o = if portrait { "portrait" } else { "landscape" };
    format!("{} {o}", mode.label())
}

/// The preset shown: the chosen one, or the one Custom starts from.
pub fn preset_of(cfg: &TouchConfig) -> TouchStyle {
    match cfg.style {
        TouchStyle::Custom if cfg.custom_from == TouchStyle::Custom => TouchStyle::Iidx,
        TouchStyle::Custom => cfg.custom_from,
        s => s,
    }
}

/// The zones of a preset (Keyboard or BM) for every player in `area`.
pub fn preset_zones(
    mode: PlayMode,
    style: TouchStyle,
    area: Rect,
    cfg: &TouchConfig,
    side2: bool,
    px_per_mm: f32,
) -> Vec<Zone> {
    player_areas(mode, area, cfg, side2, px_per_mm)
        .into_iter()
        .flat_map(|(a, player, right)| {
            let player = player.min(mode.players() - 1);
            layout(mode, style, a, player, right, px_per_mm)
        })
        .collect()
}

/// The custom layout of `mode` in this orientation, if Custom is chosen
/// and the mode has one.
pub fn custom_layout(cfg: &TouchConfig, mode: PlayMode, portrait: bool) -> Option<&TouchLayout> {
    (cfg.style == TouchStyle::Custom)
        .then(|| cfg.layouts.get(&slot(mode, portrait)))
        .flatten()
        .filter(|l| !l.buttons.is_empty())
}

/// The lane buttons the controls in `area` use: the chosen preset, or the
/// custom layout of this mode and orientation.
pub fn zones(
    mode: PlayMode,
    cfg: &TouchConfig,
    area: Rect,
    side2: bool,
    portrait: bool,
    px_per_mm: f32,
) -> Vec<Zone> {
    match custom_layout(cfg, mode, portrait) {
        Some(l) => custom_zones(mode, l, area),
        None => preset_zones(mode, preset_of(cfg), area, cfg, side2, px_per_mm),
    }
}

/// A custom button's rectangle on the screen.
pub fn button_rect(b: &TouchButton, area: Rect) -> Rect {
    Rect::new(
        area.x + b.x * area.w,
        area.y + b.y * area.h,
        b.w * area.w,
        b.h * area.h,
    )
}

/// In steps of 1/10000 of the area, which keep the config file short.
fn q(v: f32) -> f32 {
    (v * 10_000.0).round() / 10_000.0
}

/// Set a custom button's rectangle from one on the screen.
pub fn set_button_rect(b: &mut TouchButton, r: Rect, area: Rect) {
    b.x = q((r.x - area.x) / area.w);
    b.y = q((r.y - area.y) / area.h);
    b.w = q(r.w / area.w);
    b.h = q(r.h / area.h);
}

/// The zones of a custom layout: a lane's colour and kind come from the
/// mode, as in the presets.
pub fn custom_zones(mode: PlayMode, l: &TouchLayout, area: Rect) -> Vec<Zone> {
    let mut seen = Vec::new();
    l.buttons
        .iter()
        .filter(|b| (b.lane as usize) < mode.lane_count() && b.w > 0.0 && b.h > 0.0)
        .filter(|b| {
            let new = !seen.contains(&b.lane);
            seen.push(b.lane);
            new
        })
        .map(|b| {
            let r = button_rect(b, area);
            let lane = b.lane as usize;
            let scratch = mode.is_scratch(lane);
            let k = lane % mode.lanes_per_player();
            let shape = if b.round {
                let (cx, cy) = r.center();
                Shape::Circle {
                    cx,
                    cy,
                    r: r.w.min(r.h) * 0.5,
                }
            } else {
                Shape::Rect(r)
            };
            Zone {
                shape,
                lane: b.lane,
                scratch,
                black: !scratch && k % 2 == 1,
                color: (b.round && mode == PlayMode::Popn9K)
                    .then(|| crate::gfx::rgba(POPN_COLORS[k.min(8)])),
            }
        })
        .collect()
}

/// A custom layout with the lane buttons of `zones` (a preset) in `area`.
pub fn to_layout(zones: &[Zone], area: Rect) -> TouchLayout {
    TouchLayout {
        buttons: zones
            .iter()
            .filter(|z| z.lane < SYSTEM)
            .map(|z| {
                let mut b = TouchButton {
                    lane: z.lane,
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                    round: matches!(z.shape, Shape::Circle { .. }),
                };
                set_button_rect(&mut b, z.bounds(), area);
                b
            })
            .collect(),
    }
}

/// A custom layout mirrored left to right: each button keeps its lane, so
/// the turntable goes to the other side.
pub fn flip(l: &mut TouchLayout) {
    for b in &mut l.buttons {
        b.x = q(1.0 - b.x - b.w);
    }
}

/// The perspective skin's lanes as touch zones: columns from the bottom of
/// the screen up to `top` (as drawn) that fill the width `x0..x1`, split
/// halfway between the fans in double play.
pub fn fan_zones(
    mode: PlayMode,
    fans: &[crate::perspective::Fan],
    top: f32,
    judge: f32,
    x0: f32,
    x1: f32,
) -> Vec<Zone> {
    let mid = |i: usize| (fans[i - 1].cx + fans[i].cx) * 0.5;
    fans.iter()
        .enumerate()
        .flat_map(|(i, f)| {
            let l = if i == 0 { x0 } else { mid(i) };
            let r = if i + 1 == fans.len() { x1 } else { mid(i + 1) };
            f.touch_strips(top, judge, l, r)
        })
        .map(|(l, rows)| {
            let k = l.lane % mode.lanes_per_player();
            Zone {
                shape: Shape::Strip(rows.into()),
                lane: l.lane as u8,
                scratch: l.scratch,
                black: l.blue,
                color: (mode == PlayMode::Popn9K).then(|| crate::gfx::rgba(POPN_COLORS[k.min(8)])),
            }
        })
        .collect()
}

/// Whether START / SELECT go on the right of the screen: the side chosen,
/// else the scratch side, which moves the note field towards the middle
/// (left for 1P and double play, right for 2P).
pub fn system_right(cfg: &TouchConfig, mode: PlayMode, side2: bool, portrait: bool) -> bool {
    match cfg.start_select_side {
        // a custom layout's turntables all on one half: that side
        TouchSide::Auto => match scratch_half(cfg, mode, portrait) {
            Some(right) => right,
            None => !mode.is_double() && side2,
        },
        TouchSide::Left => false,
        TouchSide::Right => true,
    }
}

/// Which half of the screen a custom layout's turntables are on (true:
/// right), when they are all on the same half.
fn scratch_half(cfg: &TouchConfig, mode: PlayMode, portrait: bool) -> Option<bool> {
    let layout = custom_layout(cfg, mode, portrait)?;
    let lanes: Vec<u8> = (0..mode.players())
        .filter_map(|p| mode.scratch_lane(p).map(|l| l as u8))
        .collect();
    let halves: Vec<bool> = layout
        .buttons
        .iter()
        .filter(|b| lanes.contains(&b.lane))
        .map(|b| b.x + b.w * 0.5 > 0.5)
        .collect();
    let first = *halves.first()?;
    halves.iter().all(|&h| h == first).then_some(first)
}

/// START above SELECT, two square buttons 1.5 buttons apart, away from the
/// keys: in the letterbox beside the play screen, on the `right` (or left)
/// side. `screen` is the space above the controls and `aspect` the play
/// skin's width / height. Returns the rect left for the play screen and the
/// two buttons. The play screen moves aside to leave all of the letterbox
/// on the buttons' side, and gives up room where that is still too narrow.
pub fn system_buttons(screen: Rect, aspect: f32, px_per_mm: f32, right: bool) -> (Rect, [Zone; 2]) {
    let mm = px_per_mm;
    let button = |lane, r| Zone {
        shape: Shape::Rect(r),
        lane,
        scratch: false,
        black: false,
        color: None,
    };
    let margin = 2.5 * mm;
    let shown = (screen.h * aspect).min(screen.w);
    let strip = (screen.w - shown)
        .max(18.0 * mm + margin * 2.0)
        .min(screen.w * 0.2);
    let view = Rect::new(
        if right { screen.x } else { screen.x + strip },
        screen.y,
        screen.w - strip,
        screen.h,
    );
    let b = (strip - margin * 2.0)
        .min(22.0 * mm)
        .min((screen.h - margin * 2.0) / 3.5);
    let x = if right { view.right() } else { screen.x } + (strip - b) * 0.5;
    let y = screen.y + (screen.h - b * 3.5) * 0.5;
    (
        view,
        [
            button(START, Rect::new(x, y, b, b)),
            button(SELECT, Rect::new(x, y + b * 2.5, b, b)),
        ],
    )
}

/// A strip filled, a colour per row (by its y).
pub fn draw_strip(batch: &mut Batch, rows: &[[f32; 3]], color: impl Fn(f32) -> crate::gfx::Color) {
    for w in rows.windows(2) {
        let ([y0, a0, b0], [y1, a1, b1]) = (w[0], w[1]);
        let (c0, c1) = (color(y0), color(y1));
        batch.quad4([[a0, y0], [b0, y0], [b1, y1], [a1, y1]], [c0, c0, c1, c1]);
    }
}

/// A strip's outline, `t` thick inside it.
pub fn strip_outline(batch: &mut Batch, rows: &[[f32; 3]], t: f32, color: crate::gfx::Color) {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return;
    };
    for w in rows.windows(2) {
        let ([y0, a0, b0], [y1, a1, b1]) = (w[0], w[1]);
        batch.quad4([[a0, y0], [a0 + t, y0], [a1 + t, y1], [a1, y1]], [color; 4]);
        batch.quad4([[b0 - t, y0], [b0, y0], [b1, y1], [b1 - t, y1]], [color; 4]);
    }
    for (r, dy) in [(first, t), (last, -t)] {
        let (y0, y1) = (r[0].min(r[0] + dy), r[0].max(r[0] + dy));
        if r[2] > r[1] {
            batch.rect(Rect::new(r[1], y0, r[2] - r[1], y1 - y0), color);
        }
    }
}

struct Ptr {
    /// When it went down.
    down: Instant,
    zone: usize,
    lane: u8,
    sub: u8,
    last_angle: f32,
    last_x: f32,
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

    /// Let go of the keys held by pointers down by `time`, whose releases
    /// were lost (the page lost the focus, or no finger was on the screen
    /// at `time`), and of keys no pointer holds.
    pub fn release_all(&mut self, time: Instant, out: &mut Vec<LaneEv>) {
        self.ptrs.retain(|_, p| p.down > time);
        let mut gone: Vec<(u8, u8)> = Vec::new();
        for (key, c) in self.held.iter_mut() {
            let left = self
                .ptrs
                .values()
                .filter(|p| (p.lane, p.sub) == *key)
                .count() as u32;
            if *c > 0 && left == 0 {
                gone.push(*key);
            }
            *c = left;
        }
        gone.sort_unstable();
        for (lane, sub) in gone {
            out.push(LaneEv {
                lane,
                sub,
                pressed: false,
                time,
            });
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
                // the same pointer down again: its release was lost
                if let Some(p) = self.ptrs.remove(&id) {
                    self.release(p.lane, p.sub, time, out);
                }
                let Some(zi) = pick(zones, x, y) else {
                    return false;
                };
                let z = &zones[zi];
                let angle = match z.shape {
                    Shape::Circle { cx, cy, .. } => (y - cy).atan2(x - cx),
                    _ => 0.0,
                };
                self.press(z.lane, 0, time, out);
                self.ptrs.insert(
                    id,
                    Ptr {
                        down: time,
                        zone: zi,
                        lane: z.lane,
                        sub: 0,
                        last_angle: angle,
                        last_x: x,
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
                let Some(z) = zones.get(zi) else {
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
                        // perspective scratch lane: sideways swipes
                        Shape::Strip(_) => ((x - p.last_x) / 3.0, 0.0),
                    };
                    let p = self.ptrs.get_mut(&id).unwrap();
                    p.last_angle = nangle;
                    p.last_x = x;
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
                // slide between keys (not onto START / SELECT)
                if let Some(ni) = zones
                    .iter()
                    .rposition(|nz| !nz.scratch && nz.lane < SYSTEM && nz.hit(x, y))
                {
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

    pub fn draw(&self, batch: &mut Batch, zones: &[Zone], opacity: f32) {
        use crate::theme::{NOTE_BLUE, NOTE_SCRATCH, NOTE_WHITE};
        for z in zones {
            let pressed = self.is_pressed(z.lane);
            let base = if let Some(c) = z.color {
                c
            } else if z.scratch {
                NOTE_SCRATCH
            } else if z.black {
                NOTE_BLUE
            } else {
                NOTE_WHITE
            };
            let fill = with_alpha(base, opacity * if pressed { 0.85 } else { 0.28 });
            let edge = with_alpha(base, opacity * 0.9);
            match z.shape {
                Shape::Rect(r) => {
                    batch.rect(r, fill);
                    batch.rect_outline(r, 2.0, edge);
                }
                Shape::Strip(ref rows) => {
                    draw_strip(batch, rows, |_| fill);
                    strip_outline(batch, rows, 2.0, edge);
                }
                Shape::Circle { cx, cy, r } if !z.scratch => {
                    batch.circle(cx, cy, r, 0.0, fill, 48);
                    batch.circle(cx, cy, r, r - 3.0, edge, 48);
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
    use crate::perspective::Fan;

    #[test]
    fn iidx_layout_has_all_lanes() {
        let z = layout(
            PlayMode::Beat7K,
            TouchStyle::Iidx,
            Rect::new(0.0, 500.0, 800.0, 300.0),
            0,
            false,
            4.0,
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
            4.0,
        );
        assert_eq!(k.len(), 9);
    }

    #[test]
    fn system_buttons_sit_beside_the_play_screen() {
        let rect = |z: &Zone| match z.shape {
            Shape::Rect(r) => r,
            _ => panic!("square buttons"),
        };
        // a phone in landscape: the play screen keeps its size at the left,
        // START over SELECT in the letterbox on the right
        let screen = Rect::new(0.0, 0.0, 2000.0, 500.0);
        let (view, [start, select]) = system_buttons(screen, 1280.0 / 528.0, 6.0, true);
        let shown = 500.0 * 1280.0 / 528.0;
        assert!(view.x == 0.0 && view.w >= shown, "{view:?}");
        let (a, b) = (rect(&start), rect(&select));
        assert!(a.x >= view.right() && (a.x - b.x).abs() < 0.1 && b.y > a.bottom());
        assert!((a.w - a.h).abs() < 0.1 && (a.w - 22.0 * 6.0).abs() < 0.1);
        // a tablet: no letterbox, the play screen gives up one strip (2P: left)
        let screen = Rect::new(0.0, 0.0, 1280.0, 528.0);
        let (view, [start, _]) = system_buttons(screen, 1280.0 / 528.0, 6.0, false);
        assert!(
            (view.x - 138.0).abs() < 0.5 && view.right() == 1280.0,
            "{view:?}"
        );
        assert!(rect(&start).right() <= view.x);
        // portrait: the letterbox beside it as well
        let screen = Rect::new(0.0, 0.0, 700.0, 900.0);
        let (view, [start, select]) = system_buttons(screen, 720.0 / 1280.0, 6.0, true);
        let (a, b) = (rect(&start), rect(&select));
        assert!(
            view.h == 900.0 && a.x >= view.right() && (b.y - a.bottom() - a.h * 1.5).abs() < 0.1
        );
    }

    #[test]
    fn wide_areas_keep_keys_together() {
        let area = Rect::new(0.0, 500.0, 2400.0, 240.0);
        let z = layout(PlayMode::Beat7K, TouchStyle::Iidx, area, 0, false, 4.0);
        let xs: Vec<f32> = z
            .iter()
            .map(|z| match z.shape {
                Shape::Rect(r) => r.right(),
                Shape::Circle { cx, r, .. } => cx - r,
                Shape::Strip(_) => unreachable!(),
            })
            .collect();
        let width_of = |z: &Zone| match z.shape {
            Shape::Rect(r) => r.w,
            Shape::Circle { r, .. } => r * 2.0,
            Shape::Strip(_) => unreachable!(),
        };
        // the real key width (29 mm at 4 px per mm), the set centred
        let key = z
            .iter()
            .find(|z| !z.scratch && z.lane < SYSTEM)
            .map(width_of)
            .unwrap();
        assert!((key - 29.0 * 4.0).abs() < 0.5, "{key}");
        let (left, right) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(0.0, f32::max),
        );
        assert!((left - (2400.0 - right)).abs() < 20.0, "{left} {right}");
    }

    #[test]
    fn narrow_areas_push_the_turntable_off_the_edge() {
        // 850 px: the keys (608 px at 4 px/mm) keep their width and only
        // part of the turntable shows
        let area = Rect::new(0.0, 0.0, 850.0, 400.0);
        let z = layout(PlayMode::Beat7K, TouchStyle::Iidx, area, 0, false, 4.0);
        let Shape::Circle { cx, r, .. } = z[0].shape else {
            panic!("turntable first");
        };
        assert!(cx - r < 0.0 && cx + r > 0.5 * r);
        assert!(z[1..]
            .iter()
            .filter(|z| z.lane < SYSTEM)
            .all(|z| match z.shape {
                Shape::Rect(k) => (k.w - 116.0).abs() < 0.5 && k.right() <= 850.5,
                _ => false,
            }));
    }

    fn fans() -> Vec<crate::perspective::Fan> {
        let cfg = crate::config::PerspectiveConfig::default();
        crate::perspective::layout(PlayMode::Beat7K, false, 720.0, 1280.0, &cfg)
    }

    /// The lane a zone hits at (x, y), as drawn (no nearest-key fallback).
    fn hit_lane(zones: &[Zone], x: f32, y: f32) -> Option<u8> {
        zones
            .iter()
            .rposition(|z| z.hit(x, y))
            .map(|i| zones[i].lane)
    }

    /// The zones of `mode` on a `w` x `h` screen, the touch area its bottom
    /// half (the default), and the fans.
    fn persp(mode: PlayMode, side2: bool, w: f32, h: f32) -> (Vec<Zone>, Vec<Fan>) {
        let cfg = crate::config::PerspectiveConfig::default();
        let fans = crate::perspective::layout(mode, side2, w, h, &cfg);
        let top = h * (1.0 - cfg.touch_height);
        (fan_zones(mode, &fans, top, 0.0, 0.0, w), fans)
    }

    const SCREENS: [(f32, f32); 5] = [
        (2340.0, 1080.0),
        (2048.0, 1536.0),
        (1280.0, 720.0),
        (1080.0, 2340.0),
        (1536.0, 2048.0),
    ];

    /// The span of `lane`'s zone at height `y`.
    fn span(zones: &[Zone], lane: usize, y: f32) -> (f32, f32) {
        let z = zones.iter().find(|z| z.lane as usize == lane).unwrap();
        let Shape::Strip(rows) = &z.shape else {
            panic!("a strip");
        };
        strip_span(rows, y).unwrap()
    }

    #[test]
    fn fan_zones_are_the_lanes_at_the_judge_line() {
        for (w, h) in SCREENS {
            for mode in PlayMode::ALL {
                for side2 in [false, true] {
                    let (zones, fans) = persp(mode, side2, w, h);
                    assert_eq!(zones.len(), mode.lane_count());
                    for f in &fans {
                        let y = f.y(0.0);
                        for (i, l) in f.lanes.iter().enumerate() {
                            let n = f.lanes.len();
                            if l.scratch
                                || i == 0
                                || i + 1 == n
                                || f.lanes[i - 1].scratch
                                || f.lanes[i + 1].scratch
                            {
                                continue;
                            }
                            let (a, b) = span(&zones, l.lane, y);
                            let (x0, x1) = (f.point(l.u0, 0.0)[0], f.point(l.u1, 0.0)[0]);
                            let what = format!("{mode:?} {w}x{h} {side2} lane {}", l.lane);
                            assert!((a - x0).abs() < 0.5 && (b - x1).abs() < 0.5, "{what}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn white_keys_wider_below_blue_keys_above() {
        for (w, h) in SCREENS {
            for mode in PlayMode::ALL {
                let (zones, fans) = persp(mode, false, w, h);
                for f in &fans {
                    let yj = f.y(0.0);
                    let key = f.point(f.lanes[1].u1, 0.0)[0] - f.point(f.lanes[1].u0, 0.0)[0];
                    let bend = crate::perspective::CURVE_H * key;
                    // inner keys (both neighbours keys)
                    for win in f.lanes.windows(3) {
                        let l = win[1];
                        if win.iter().any(|l| l.scratch) {
                            continue;
                        }
                        let width = |y: f32| {
                            let (a, b) = span(&zones, l.lane, y);
                            let lane = f.x_at(l.u1, y) - f.x_at(l.u0, y);
                            (b - a) / lane
                        };
                        let (above, below) = (width(yj - bend * 0.6), width(yj + bend * 0.6));
                        let what = format!("{mode:?} {w}x{h} lane {}: {above} {below}", l.lane);
                        if l.blue {
                            assert!(above > 1.1 && below < 0.9, "{what}");
                        } else {
                            assert!(above < 0.9 && below > 1.1, "{what}");
                        }
                        // beyond the bend: a white key's zone above it is
                        // its lane's middle line, a blue one's the same below
                        let (above, below) = (width(yj - bend * 1.1), width(yj + bend * 1.1));
                        let what = format!("{what} / {above} {below}");
                        if l.blue {
                            assert!((above - 2.0).abs() < 1e-2 && below.abs() < 1e-2, "{what}");
                        } else {
                            assert!(above.abs() < 1e-2 && (below - 2.0).abs() < 1e-2, "{what}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn fan_zones_partition_the_touch_area() {
        for (w, h) in SCREENS {
            for mode in PlayMode::ALL {
                for side2 in [false, true] {
                    let (zones, _) = persp(mode, side2, w, h);
                    let top = h * 0.5;
                    for i in 0..160 {
                        for j in 0..=80 {
                            let x = (i as f32 + 0.37) * w / 160.0;
                            let y = top + j as f32 * (h - top) / 80.0;
                            let n = zones.iter().filter(|z| z.hit(x, y)).count();
                            assert_eq!(n, 1, "{mode:?} {w}x{h} {side2}: {x}, {y}");
                            // double play: each player's half
                            let player = usize::from(x > w * 0.5 && mode.players() == 2);
                            let lane = hit_lane(&zones, x, y).unwrap() as usize;
                            assert_eq!(lane / mode.lanes_per_player(), player);
                        }
                    }
                    // above the touch area nothing is hit
                    for i in 0..20 {
                        assert!(pick(&zones, (i as f32 + 0.5) * w / 20.0, top - 2.0).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn fan_zones_of_2p_mirror_1p() {
        for (w, h) in SCREENS {
            for mode in [PlayMode::Beat7K, PlayMode::Beat5K] {
                let (one, _) = persp(mode, false, w, h);
                let (two, _) = persp(mode, true, w, h);
                let keys = mode.keys_per_player();
                // key k mirrors key keys + 1 - k, the scratch the scratch
                let mirror = |l: u8| match l as usize {
                    l if l < keys => (keys - 1 - l) as u8,
                    l => l as u8,
                };
                for i in 0..120 {
                    for j in 0..=40 {
                        let x = (i as f32 + 0.37) * w / 120.0;
                        let y = h * 0.5 + j as f32 * h * 0.5 / 40.0;
                        assert_eq!(
                            hit_lane(&one, x, y).map(mirror),
                            hit_lane(&two, w - x, y),
                            "{mode:?} {w}x{h}: {x}, {y}"
                        );
                    }
                }
            }
        }
        // double play: the second player's fan mirrors the first's
        let (dp, _) = persp(PlayMode::Beat14K, false, 2340.0, 1080.0);
        for i in 0..117 {
            let (x, y) = ((i as f32 + 0.37) * 10.0, 900.0);
            let a = hit_lane(&dp, x, y).unwrap();
            let b = hit_lane(&dp, 2340.0 - x, y).unwrap();
            assert_eq!(b, if a == 7 { 15 } else { 8 + 6 - a }, "{x}");
        }
    }

    #[test]
    fn fan_zones_hit_what_is_drawn() {
        let fans = fans();
        let f = &fans[0];
        let top = 1280.0 * 0.5;
        let zones = fan_zones(PlayMode::Beat7K, &fans, top, 0.0, 0.0, 720.0);
        assert_eq!(zones.len(), 8);
        assert!(zones[0].scratch && zones[0].lane == 7);
        // the gap beside the scratch: each side plays the lane beside it
        let (g0, g1) = (f.lanes[0].u1, f.lanes[1].u0);
        let at = |u: f32| f.point(u, f.d_bottom() * 0.5);
        let [x, y] = at(g0 + (g1 - g0) * 0.25);
        assert_eq!(hit_lane(&zones, x, y), Some(7));
        let [x, y] = at(g0 + (g1 - g0) * 0.75);
        assert_eq!(hit_lane(&zones, x, y), Some(0));
        // the upright line: keys 3 and 4 either side of the middle at the
        // judge line, down to the bottom and up to the touch area's top
        // (above the bends: the blue key 4, below: the white key 3)
        let yj = f.y(0.0);
        assert_eq!(hit_lane(&zones, 359.0, yj), Some(2));
        assert_eq!(hit_lane(&zones, 361.0, yj), Some(3));
        assert_eq!(hit_lane(&zones, 359.0, top + 1.0), Some(3));
        assert_eq!(hit_lane(&zones, 361.0, 1279.0), Some(2));
    }

    #[test]
    fn fan_zones_fill_the_width() {
        // from the screen's bottom to above the judge line, edge to edge;
        // in double play each player's half
        let cfg = crate::config::PerspectiveConfig::default();
        for (w, h) in [
            (1280.0, 720.0),
            (1560.0, 720.0),
            (1280.0, 960.0),
            (720.0, 1560.0),
        ] {
            for mode in PlayMode::ALL {
                let fans = crate::perspective::layout(mode, false, w, h, &cfg);
                let line = fans[0].base_y;
                let zones = fan_zones(mode, &fans, line - 0.1 * h, 0.0, 0.0, w);
                for y in [h - 1.0, line, line - 0.09 * h] {
                    for i in 0..200 {
                        let x = (i as f32 + 0.5) * w / 200.0;
                        let lane = hit_lane(&zones, x, y)
                            .unwrap_or_else(|| panic!("{mode:?} {w}x{h}: {x}, {y}"));
                        let player = usize::from(x > w * 0.5 && mode.players() == 2);
                        assert_eq!(lane as usize / mode.lanes_per_player(), player);
                    }
                }
            }
        }
    }

    #[test]
    fn fan_lanes_slide_and_scratch_swipes() {
        let fans = fans();
        let f = &fans[0];
        let zones = fan_zones(PlayMode::Beat7K, &fans, 1280.0 * 0.5, 0.0, 0.0, 720.0);
        let now = Instant::now();
        let mut t = TouchInput::default();
        let mut out = Vec::new();
        let at = |u: f32| f.point(u, f.d_bottom() * 0.5);
        // a finger on key 1 slides to key 2 (on the judge line)
        let (l1, l2) = (f.lanes[1], f.lanes[2]);
        let p = f.point((l1.u0 + l1.u1) * 0.5, 0.0);
        t.pointer(&zones, 1, Phase::Down, p[0], p[1], now, 6.0, &mut out);
        let p = f.point((l2.u0 + l2.u1) * 0.5, 0.0);
        t.pointer(&zones, 1, Phase::Move, p[0], p[1], now, 6.0, &mut out);
        let evs: Vec<(u8, bool)> = out.iter().map(|e| (e.lane, e.pressed)).collect();
        assert_eq!(evs, [(0, true), (0, false), (1, true)]);
        // the scratch: a tap presses it; swiping back and forth sideways
        // turns it the other way (another press)
        out.clear();
        let sc = f.lanes[0];
        let p = at((sc.u0 + sc.u1) * 0.5);
        t.pointer(&zones, 2, Phase::Down, p[0], p[1], now, 6.0, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].lane == 7 && out[0].pressed && out[0].sub == 0);
        for dx in [30.0, 60.0, 30.0, 0.0] {
            t.pointer(
                &zones,
                2,
                Phase::Move,
                p[0] + dx - 30.0,
                p[1],
                now,
                6.0,
                &mut out,
            );
        }
        let subs: Vec<(u8, bool)> = out.iter().map(|e| (e.sub, e.pressed)).collect();
        assert_eq!(subs, [(0, true), (0, false), (1, true)]);
        // vertical moves in the scratch lane do nothing
        out.clear();
        t.pointer(
            &zones,
            2,
            Phase::Move,
            p[0] - 30.0,
            p[1] - 40.0,
            now,
            6.0,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn touches_between_keys_play_the_nearest() {
        let area = Rect::new(0.0, 0.0, 1200.0, 400.0);
        let z = layout(PlayMode::Beat7K, TouchStyle::Iidx, area, 0, false, 4.0);
        let key = |lane: u8| match z.iter().find(|z| z.lane == lane).unwrap().shape {
            Shape::Rect(r) => r,
            _ => panic!(),
        };
        // lanes 0 and 2 are keys 1 and 3 (white): in the gap between them,
        // closer to 3; just below key 1; far from every key
        let (k1, k3) = (key(0), key(2));
        assert_eq!(z[pick(&z, k3.x - 10.0, k3.y + 10.0).unwrap()].lane, 2);
        assert_eq!(z[pick(&z, k1.x + 5.0, k1.bottom() + 8.0).unwrap()].lane, 0);
        assert!(pick(&z, k1.x, k1.bottom() + 200.0).is_none());
    }

    #[test]
    fn popn_buttons_zigzag_across_the_width() {
        let area = Rect::new(0.0, 0.0, 1000.0, 300.0);
        let z = layout(PlayMode::Popn9K, TouchStyle::Iidx, area, 0, false, 4.0);
        assert_eq!(z.len(), 9);
        let c: Vec<(f32, f32, f32)> = z
            .iter()
            .map(|z| match z.shape {
                Shape::Circle { cx, cy, r } => (cx, cy, r),
                _ => panic!("round buttons"),
            })
            .collect();
        for (k, w) in c.windows(2).enumerate() {
            assert!(w[1].0 > w[0].0);
            // even buttons (2, 4, ...) above their neighbours
            assert_eq!(w[1].1 < w[0].1, k % 2 == 0);
        }
        assert!(c
            .iter()
            .all(|(x, y, r)| x - r >= 0.0 && x + r <= 1000.0 && y - r >= 0.0 && y + r <= 300.0));
    }

    #[test]
    fn slide_and_scratch_reverse() {
        let zones = layout(
            PlayMode::Beat7K,
            TouchStyle::Keyboard,
            Rect::new(0.0, 0.0, 870.0, 200.0),
            0,
            false,
            4.0,
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
        t.pointer(&zones, 2, Phase::Down, 50.0, 60.0, now, 6.0, &mut out);
        t.pointer(&zones, 2, Phase::Move, 50.0, 90.0, now, 6.0, &mut out);
        t.pointer(&zones, 2, Phase::Move, 50.0, 60.0, now, 6.0, &mut out);
        let sc: Vec<_> = out.iter().map(|e| (e.lane, e.sub, e.pressed)).collect();
        assert_eq!(sc, vec![(7, 0, true), (7, 0, false), (7, 1, true)]);
        out.clear();
        t.pointer(&zones, 2, Phase::Up, 50.0, 60.0, now, 6.0, &mut out);
        assert_eq!(out[0].sub, 1);
        assert!(!out[0].pressed);
    }

    #[test]
    fn lost_releases() {
        let zones = layout(
            PlayMode::Beat7K,
            TouchStyle::Keyboard,
            Rect::new(0.0, 0.0, 870.0, 200.0),
            0,
            false,
            4.0,
        );
        let now = Instant::now();
        let mut t = TouchInput::default();
        let mut out = Vec::new();
        // key 1, right of the scratch (170 px)
        let (x, y) = (220.0, 50.0);
        assert!(t.pointer(&zones, 1, Phase::Down, x, y, now, 6.0, &mut out));
        let lane = out[0].lane;
        // its release never came; another finger on the key presses it again
        // only once it was let go
        t.release_all(now, &mut out);
        assert!(!t.is_pressed(lane));
        assert_eq!(out.last().map(|e| (e.lane, e.pressed)), Some((lane, false)));
        out.clear();
        t.pointer(&zones, 2, Phase::Down, x, y, now, 6.0, &mut out);
        assert_eq!(out.iter().filter(|e| e.pressed).count(), 1);
        // the same id down again, its up lost: released, then pressed
        out.clear();
        t.pointer(&zones, 2, Phase::Down, x, y, now, 6.0, &mut out);
        assert_eq!(
            out.iter().map(|e| e.pressed).collect::<Vec<_>>(),
            [false, true]
        );
        t.pointer(&zones, 2, Phase::Up, x, y, now, 6.0, &mut out);
        assert!(!t.is_pressed(lane));
        // a finger down after the moment no finger was on the screen stays
        let gone = Instant::now();
        let later = gone + std::time::Duration::from_millis(5);
        t.pointer(&zones, 3, Phase::Down, x, y, later, 6.0, &mut out);
        out.clear();
        t.release_all(gone, &mut out);
        assert!(out.is_empty() && t.is_pressed(lane));
    }
    fn shapes_match(a: &[Zone], b: &[Zone]) {
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(b) {
            assert_eq!((a.lane, a.scratch, a.black), (b.lane, b.scratch, b.black));
            assert_eq!(a.color.is_some(), b.color.is_some());
            let (ra, rb) = (a.bounds(), b.bounds());
            for (u, v) in [(ra.x, rb.x), (ra.y, rb.y), (ra.w, rb.w), (ra.h, rb.h)] {
                assert!((u - v).abs() < 0.5, "{ra:?} {rb:?}");
            }
            assert_eq!(
                matches!(a.shape, Shape::Circle { .. }),
                matches!(b.shape, Shape::Circle { .. })
            );
        }
    }

    #[test]
    fn presets_are_the_chosen_layout_until_custom() {
        let area = Rect::new(0.0, 400.0, 1600.0, 360.0);
        let mut cfg = TouchConfig::default();
        // the default is the BM preset, as before custom layouts
        let bm = zones(PlayMode::Beat7K, &cfg, area, false, false, 4.0);
        let old = layout(PlayMode::Beat7K, TouchStyle::Iidx, area, 0, false, 4.0);
        shapes_match(&bm, &old);
        // BM: the turntable on the scratch side, white keys low, black raised
        let disc = bm.iter().find(|z| z.scratch).unwrap();
        assert!(matches!(disc.shape, Shape::Circle { .. }));
        let keys: Vec<Zone> = bm.iter().filter(|z| !z.scratch).cloned().collect();
        assert!(keys.iter().all(|k| k.bounds().x > disc.bounds().right()));
        let (white, black): (Vec<&Zone>, Vec<&Zone>) = keys.iter().partition(|k| !k.black);
        assert_eq!((white.len(), black.len()), (4, 3));
        assert!(black
            .iter()
            .all(|b| white.iter().all(|w| b.bounds().bottom() < w.bounds().y)));
        // each black key over the gap between two white ones
        for b in &black {
            let c = b.bounds().center().0;
            assert!(white.iter().any(|w| w.bounds().right() < c));
            assert!(white.iter().any(|w| w.bounds().x > c));
        }
        // 2P side: the turntable on the right
        let p2 = zones(PlayMode::Beat7K, &cfg, area, true, false, 4.0);
        let disc2 = p2.iter().find(|z| z.scratch).unwrap().bounds();
        assert!(p2
            .iter()
            .filter(|z| !z.scratch)
            .all(|k| k.bounds().right() < disc2.x));
        cfg.style = TouchStyle::Keyboard;
        let kb = zones(PlayMode::Beat7K, &cfg, area, false, false, 4.0);
        shapes_match(
            &kb,
            &layout(PlayMode::Beat7K, TouchStyle::Keyboard, area, 0, false, 4.0),
        );
        // Custom with nothing edited shows the preset it starts from
        cfg.style = TouchStyle::Custom;
        cfg.custom_from = TouchStyle::Keyboard;
        shapes_match(&zones(PlayMode::Beat7K, &cfg, area, false, false, 4.0), &kb);
        // double play: two sets
        cfg.style = TouchStyle::Iidx;
        let dp = zones(PlayMode::Beat14K, &cfg, area, false, false, 4.0);
        assert_eq!(dp.len(), 16);
        cfg.allow_dp = false;
        assert_eq!(
            zones(PlayMode::Beat14K, &cfg, area, false, false, 4.0).len(),
            8
        );
    }

    #[test]
    fn presets_copy_into_custom_layouts() {
        let area = Rect::new(0.0, 500.0, 2340.0, 580.0);
        let mut cfg = TouchConfig {
            style: TouchStyle::Custom,
            ..Default::default()
        };
        for mode in PlayMode::ALL {
            for style in [TouchStyle::Iidx, TouchStyle::Keyboard] {
                let preset = preset_zones(mode, style, area, &cfg, false, 5.0);
                let l = to_layout(&preset, area);
                assert_eq!(l.buttons.len(), mode.lane_count());
                cfg.layouts.insert(slot(mode, false), l);
                let custom = zones(mode, &cfg, area, false, false, 5.0);
                shapes_match(&custom, &preset);
                // the other orientation has none: the preset
                assert!(custom_layout(&cfg, mode, true).is_none());
            }
        }
        // a layout made on one screen scales with the controls area
        let l = cfg.layouts.get(&slot(PlayMode::Beat7K, false)).unwrap();
        let small = Rect::new(0.0, 200.0, 1170.0, 290.0);
        let z = custom_zones(PlayMode::Beat7K, l, small);
        let big = custom_zones(PlayMode::Beat7K, l, area);
        assert!((z[3].bounds().x * 2.0 - big[3].bounds().x).abs() < 1.0);
    }

    #[test]
    fn custom_buttons_are_hit_where_drawn() {
        let area = Rect::new(0.0, 600.0, 1000.0, 400.0);
        let b = |lane, x, y, w, h, round| TouchButton {
            lane,
            x,
            y,
            w,
            h,
            round,
        };
        let l = TouchLayout {
            buttons: vec![
                // a turntable in a 300 x 400 rect: a circle of radius 150
                b(7, 0.0, 0.0, 0.3, 1.0, true),
                b(0, 0.4, 0.5, 0.2, 0.5, false),
                // overlapping key 1, drawn over it
                b(1, 0.5, 0.25, 0.2, 0.5, false),
                // above the controls area, over the play screen
                b(2, 0.8, -0.5, 0.2, 0.25, false),
                // unknown lane and a duplicate: left out
                b(40, 0.0, 0.0, 0.1, 0.1, false),
                b(0, 0.9, 0.9, 0.1, 0.1, false),
            ],
        };
        let z = custom_zones(PlayMode::Beat7K, &l, area);
        assert_eq!(z.len(), 4);
        let Shape::Circle { cx, cy, r } = z[0].shape else {
            panic!("round turntable")
        };
        assert_eq!((cx, cy, r), (150.0, 800.0, 150.0));
        assert!(z[0].scratch && !z[1].black && z[2].black);
        let lane = |x, y| pick(&z, x, y).map(|i| z[i].lane);
        assert_eq!(lane(150.0, 800.0), Some(7));
        // the rect's corner is outside the circle drawn
        assert_ne!(lane(5.0, 605.0), Some(7));
        assert_eq!(lane(450.0, 950.0), Some(0));
        // where keys 1 and 2 overlap, the one drawn last
        assert_eq!(lane(550.0, 850.0), Some(1));
        assert_eq!(lane(900.0, 450.0), Some(2));
        // touches play them as in the presets
        let mut t = TouchInput::default();
        let mut out = Vec::new();
        let now = Instant::now();
        assert!(t.pointer(&z, 1, Phase::Down, 900.0, 450.0, now, 6.0, &mut out));
        assert_eq!((out[0].lane, out[0].pressed), (2, true));
    }

    #[test]
    fn flipping_mirrors_the_layout() {
        let area = Rect::new(0.0, 500.0, 2340.0, 580.0);
        let cfg = TouchConfig::default();
        for mode in PlayMode::ALL {
            for style in [TouchStyle::Iidx, TouchStyle::Keyboard] {
                let l = to_layout(&preset_zones(mode, style, area, &cfg, false, 5.0), area);
                let mut f = l.clone();
                flip(&mut f);
                // each button mirrored about the middle, with its lane
                for (a, b) in l.buttons.iter().zip(&f.buttons) {
                    assert_eq!(
                        (a.lane, a.y, a.w, a.h, a.round),
                        (b.lane, b.y, b.w, b.h, b.round)
                    );
                    assert!((a.x + a.w * 0.5 - (1.0 - b.x - b.w * 0.5)).abs() < 1e-4);
                }
                // twice: back as it was
                flip(&mut f);
                for (a, b) in l.buttons.iter().zip(&f.buttons) {
                    assert!((a.x - b.x).abs() < 1e-6, "{mode:?} {a:?} {b:?}");
                }
            }
        }
        // BM 1P: the turntable goes from the left of the keys to the right
        let l = to_layout(
            &preset_zones(PlayMode::Beat7K, TouchStyle::Iidx, area, &cfg, false, 5.0),
            area,
        );
        let mut f = l.clone();
        flip(&mut f);
        let z = custom_zones(PlayMode::Beat7K, &f, area);
        let disc = z.iter().find(|z| z.scratch).unwrap().bounds();
        let keys: Vec<Rect> = z
            .iter()
            .filter(|z| !z.scratch)
            .map(|z| z.bounds())
            .collect();
        assert_eq!(keys.len(), 7);
        assert!(keys.iter().all(|k| k.right() < disc.x));
        // key 1 next to it
        let k1 = z.iter().find(|z| z.lane == 0).unwrap().bounds();
        assert!(keys.iter().all(|k| k.x <= k1.x));
    }

    #[test]
    fn start_select_side() {
        let mut cfg = TouchConfig::default();
        // auto: the scratch side, left for double play
        assert!(!system_right(&cfg, PlayMode::Beat7K, false, false));
        assert!(system_right(&cfg, PlayMode::Beat7K, true, false));
        assert!(!system_right(&cfg, PlayMode::Beat14K, true, false));
        // auto with a custom layout: the turntable's side, whichever player
        let button = |lane, x| crate::config::TouchButton {
            lane,
            x,
            y: 0.0,
            w: 0.2,
            h: 1.0,
            round: true,
        };
        let sc = PlayMode::Beat7K.scratch_lane(0).unwrap() as u8;
        cfg.style = TouchStyle::Custom;
        cfg.layouts.insert(
            slot(PlayMode::Beat7K, false),
            TouchLayout {
                buttons: vec![button(sc, 0.75), button(0, 0.1)],
            },
        );
        assert!(system_right(&cfg, PlayMode::Beat7K, false, false));
        cfg.layouts
            .get_mut(&slot(PlayMode::Beat7K, false))
            .unwrap()
            .buttons[0]
            .x = 0.05;
        assert!(!system_right(&cfg, PlayMode::Beat7K, true, false));
        // not this orientation's: the player's side
        assert!(system_right(&cfg, PlayMode::Beat7K, true, true));
        cfg.style = TouchStyle::Iidx;
        cfg.start_select_side = TouchSide::Left;
        assert!(!system_right(&cfg, PlayMode::Beat7K, true, false));
        cfg.start_select_side = TouchSide::Right;
        assert!(system_right(&cfg, PlayMode::Beat7K, false, false));
        assert!(system_right(&cfg, PlayMode::Beat14K, false, false));
        // the buttons on that side of the play screen
        let screen = Rect::new(0.0, 0.0, 2000.0, 500.0);
        let (view, [start, _]) = system_buttons(screen, 1280.0 / 528.0, 6.0, true);
        assert!(start.bounds().x >= view.right());
        let (view, [start, _]) = system_buttons(screen, 1280.0 / 528.0, 6.0, false);
        assert!(start.bounds().right() <= view.x);
    }

    #[test]
    fn slots_by_mode_and_orientation() {
        assert_eq!(slot(PlayMode::Beat7K, false), "7KEYS landscape");
        assert_eq!(slot(PlayMode::Popn9K, true), "9KEYS portrait");
    }
}
