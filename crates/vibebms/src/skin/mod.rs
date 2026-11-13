//! Skin support (LR2 `.lr2skin` / CSV play skins).
//!
//! The loader ([`lr2`]) turns a skin into a renderer-independent model; this
//! module evaluates the keyframe animations against a [`SkinState`] provided
//! by the scene. Semantics (coordinate system, easing, loop rules, number
//! layout, timer/option/number ids) follow LR2 as documented by beatoraja's
//! open-source LR2 skin loader.

pub mod beatoraja;
pub mod lr2;
pub mod render;

use crate::gfx::Rect;
use std::path::PathBuf;

/// (type, name) of any supported skin file.
pub fn header(path: &std::path::Path) -> Option<(SkinType, String)> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "lr2skin" => lr2::header(path),
        "json" | "luaskin" => beatoraja::header(path),
        _ => None,
    }
}

/// A source image region set (animation frames).
#[derive(Clone, Debug, Default)]
pub struct Src {
    pub image: usize,
    /// Frames as (x, y, w, h) in image pixels; `w/h < 0` = whole image.
    pub frames: Vec<[f32; 4]>,
    /// Animation cycle in ms (0 = no animation).
    pub cycle: i64,
    pub timer: i32,
}

impl Src {
    pub fn frame(&self, now: i64, st: &dyn SkinState) -> Option<[f32; 4]> {
        if self.frames.is_empty() {
            return None;
        }
        if self.cycle <= 0 || self.frames.len() == 1 {
            return Some(self.frames[0]);
        }
        let t = if self.timer > 0 {
            now - st.timer(self.timer)?
        } else {
            now
        };
        let n = self.frames.len() as i64;
        let i = (t.max(0) * n / self.cycle) % n;
        Some(self.frames[i as usize])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Key {
    pub time: i64,
    pub rect: Rect,
    pub color: [f32; 4],
    pub angle: f32,
}

/// Destination animation (`#DST_*` lines of one object).
#[derive(Clone, Debug, Default)]
pub struct Dst {
    pub keys: Vec<Key>,
    pub acc: i32,
    pub blend: i32,
    pub center: usize,
    pub loop_ms: i64,
    pub timer: i32,
    pub ops: Vec<i32>,
}

/// Evaluated destination for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub color: [f32; 4],
    pub angle: f32,
    pub additive: bool,
    /// Rotation centre as a fraction of the rect (y down).
    pub center: (f32, f32),
}

const CENTER_X: [f32; 10] = [0.5, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0];
/// LR2 centres follow the numeric keypad (1 = bottom-left); y grows downward here.
const CENTER_Y: [f32; 10] = [0.5, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0];

impl Dst {
    pub fn add_key(&mut self, k: Key) {
        let i = self.keys.partition_point(|e| e.time <= k.time);
        self.keys.insert(i, k);
    }

    /// Evaluate at scene time `now` (ms).
    pub fn eval(&self, now: i64, st: &dyn SkinState) -> Option<Frame> {
        if self.keys.is_empty() || !self.ops.iter().all(|o| st.option(*o)) {
            return None;
        }
        let mut t = now;
        if self.timer > 0 {
            t -= st.timer(self.timer)?;
        }
        let start = self.keys[0].time;
        let end = self.keys[self.keys.len() - 1].time;
        if self.loop_ms == -1 {
            if t > end {
                return None;
            }
        } else if end > 0 && t > self.loop_ms {
            t = if end == self.loop_ms {
                self.loop_ms
            } else {
                (t - self.loop_ms) % (end - self.loop_ms) + self.loop_ms
            };
        }
        if t < start {
            return None;
        }
        let i = self.keys.partition_point(|k| k.time <= t).saturating_sub(1);
        let a = &self.keys[i];
        let (rect, color, angle) = match self.keys.get(i + 1) {
            Some(b) if self.acc != 3 && b.time > a.time => {
                let mut r = (t - a.time) as f32 / (b.time - a.time) as f32;
                match self.acc {
                    1 => r *= r,
                    2 => r = 1.0 - (r - 1.0) * (r - 1.0),
                    _ => {}
                }
                let l = |x: f32, y: f32| x + (y - x) * r;
                (
                    Rect::new(
                        l(a.rect.x, b.rect.x),
                        l(a.rect.y, b.rect.y),
                        l(a.rect.w, b.rect.w),
                        l(a.rect.h, b.rect.h),
                    ),
                    [
                        l(a.color[0], b.color[0]),
                        l(a.color[1], b.color[1]),
                        l(a.color[2], b.color[2]),
                        l(a.color[3], b.color[3]),
                    ],
                    l(a.angle, b.angle),
                )
            }
            _ => (a.rect, a.color, a.angle),
        };
        let c = self.center.min(9);
        Some(Frame {
            rect,
            color,
            angle,
            additive: self.blend == 2,
            center: (CENTER_X[c], CENTER_Y[c]),
        })
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Image(Src),
    /// Image supplied by the game (stagefile 100, backbmp 101, banner 102 ...).
    RefImage(i32),
    Number {
        src: Src,
        digits: usize,
        id: i32,
        align: i32,
        keta: usize,
        /// Glyph drawn in leading positions (`None` = blank).
        pad: Option<usize>,
    },
    Text {
        id: i32,
        align: i32,
    },
    Bar {
        src: Src,
        id: i32,
        dir: i32,
    },
    Slider {
        src: Src,
        id: i32,
        dir: i32,
        range: f32,
    },
    Gauge {
        src: Src,
        part_w: f32,
        part_h: f32,
        step: (f32, f32),
        parts: usize,
        /// beatoraja style: the destination covers the whole gauge and is
        /// split into `parts` cells (LR2 places one cell per step).
        split: bool,
    },
    Bga,
    /// The note field (lanes from `#DST_NOTE`).
    Notes,
    JudgeLine(Src),
    /// `#SRC_NOWJUDGE_*` / `#SRC_NOWCOMBO_*` for one player side.
    Judge {
        side: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub kind: Kind,
    pub dst: Dst,
}

/// Per-judgement image + combo number of a `NOWJUDGE` group.
#[derive(Clone, Debug, Default)]
pub struct JudgeSkin {
    pub judge: [Option<(Src, Dst)>; 6],
    pub combo: [Option<(Src, Dst, usize)>; 6],
    pub shift: bool,
}

/// Note sprites for one lane.
#[derive(Clone, Debug, Default)]
pub struct LaneSkin {
    pub rect: Option<Rect>,
    pub note: Option<Src>,
    pub ln_start: Option<Src>,
    pub ln_end: Option<Src>,
    pub ln_body: Option<Src>,
    pub ln_body_active: Option<Src>,
    pub mine: Option<Src>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinType {
    Play7,
    Play5,
    Play14,
    Play10,
    Play9,
    Other(i32),
}

impl SkinType {
    pub fn from_id(i: i32) -> SkinType {
        match i {
            0 => SkinType::Play7,
            1 => SkinType::Play5,
            2 => SkinType::Play14,
            3 => SkinType::Play10,
            4 => SkinType::Play9,
            n => SkinType::Other(n),
        }
    }

    pub fn mode(self) -> Option<vibebms_core::PlayMode> {
        use vibebms_core::PlayMode::*;
        Some(match self {
            SkinType::Play7 => Beat7K,
            SkinType::Play5 => Beat5K,
            SkinType::Play14 => Beat14K,
            SkinType::Play10 => Beat10K,
            SkinType::Play9 => Popn9K,
            SkinType::Other(_) => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct CustomOption {
    pub name: String,
    pub first_id: i32,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Skin {
    pub path: PathBuf,
    pub name: String,
    pub author: String,
    pub kind: SkinType,
    pub width: f32,
    pub height: f32,
    pub images: Vec<Option<PathBuf>>,
    pub objects: Vec<Obj>,
    /// Lane skins indexed by our lane numbering.
    pub lanes: Vec<LaneSkin>,
    pub judges: [JudgeSkin; 2],
    /// Bar (measure) lines, one per lane group (both sides in DP).
    pub bar_lines: Vec<(Src, Dst)>,
    pub custom_options: Vec<CustomOption>,
    pub scene_ms: i64,
    pub input_ms: i64,
    pub fadeout_ms: i64,
    pub finish_margin_ms: i64,
}

/// Game state queried by the skin each frame.
pub trait SkinState {
    /// Start time (scene ms) of a timer, `None` if off. Timer 0 = scene start.
    fn timer(&self, id: i32) -> Option<i64>;
    /// LR2 option (negative id = negated).
    fn option(&self, id: i32) -> bool;
    fn number(&self, id: i32) -> Option<i64>;
    fn text(&self, id: i32) -> Option<String>;
    /// 0..1 for bar graphs and sliders.
    fn rate(&self, id: i32) -> f32;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct St;
    impl SkinState for St {
        fn timer(&self, id: i32) -> Option<i64> {
            (id == 41).then_some(1000)
        }
        fn option(&self, id: i32) -> bool {
            id >= 0
        }
        fn number(&self, _: i32) -> Option<i64> {
            None
        }
        fn text(&self, _: i32) -> Option<String> {
            None
        }
        fn rate(&self, _: i32) -> f32 {
            0.0
        }
    }

    fn key(t: i64, x: f32) -> Key {
        Key {
            time: t,
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            color: [1.0; 4],
            angle: 0.0,
        }
    }

    #[test]
    fn keyframes_loop_and_timer() {
        let mut d = Dst {
            loop_ms: 0,
            ..Default::default()
        };
        d.add_key(key(0, 0.0));
        d.add_key(key(1000, 100.0));
        assert_eq!(d.eval(500, &St).unwrap().rect.x, 50.0);
        assert_eq!(d.eval(1500, &St).unwrap().rect.x, 50.0); // looped
        d.loop_ms = -1;
        assert!(d.eval(1500, &St).is_none());
        d.timer = 41;
        d.loop_ms = 0;
        assert_eq!(d.eval(1250, &St).unwrap().rect.x, 25.0);
        d.timer = 46; // off
        assert!(d.eval(1250, &St).is_none());
        d.timer = 0;
        d.ops = vec![-5];
        assert!(d.eval(10, &St).is_none());
        d.ops.clear();
        d.acc = 1;
        assert_eq!(d.eval(500, &St).unwrap().rect.x, 25.0);
    }
}
