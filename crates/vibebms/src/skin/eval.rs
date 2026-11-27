//! A skin's animations evaluated against the game's state at a time.

use super::model::{Dst, Key, Src};
use super::state::SkinState;
use super::types::Acc;
use crate::gfx::Rect;

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
        Some(self.frames[Src::step(t, self.frames.len(), self.cycle)])
    }

    /// Which of the `n` steps of an animation of `cycle` ms shows `t` ms
    /// into it (a script's timer may put `t` anywhere an `i64` reaches).
    pub fn step(t: i64, n: usize, cycle: i64) -> usize {
        (t.max(0) as i128 * n as i128 / cycle as i128 % n as i128) as usize
    }
}

/// Evaluated destination for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub color: [f32; 4],
    pub angle: f32,
    pub blend: crate::gfx::Blend,
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
        } else if end > 0 && end > start && t > self.loop_ms {
            // (one keyframe: nothing to loop, it stays from its time on)
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
            Some(b) if self.acc != Acc::Hold && b.time > a.time => {
                let mut r = (t - a.time) as f32 / (b.time - a.time) as f32;
                match self.acc {
                    Acc::Accelerate => r *= r,
                    Acc::Decelerate => r = 1.0 - (r - 1.0) * (r - 1.0),
                    Acc::Linear | Acc::Hold => {}
                }
                (
                    a.rect.lerp(b.rect, r),
                    crate::gfx::mix(a.color, b.color, r),
                    a.angle + (b.angle - a.angle) * r,
                )
            }
            _ => (a.rect, a.color, a.angle),
        };
        let (mut rect, mut color, mut angle) = (rect, color, angle);
        for id in &self.offsets {
            if let Some([x, y, w, h, r, a]) = st.offset(*id) {
                // offsets are y-up like beatoraja destinations
                rect.x += x;
                rect.y -= y + h;
                rect.w += w;
                rect.h += h;
                angle -= r;
                color[3] = (color[3] + a / 255.0).clamp(0.0, 1.0);
            }
        }
        let c = self.center.min(9);
        Some(Frame {
            rect,
            color,
            angle,
            blend: self.blend,
            center: (CENTER_X[c], CENTER_Y[c]),
        })
    }
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

    /// A script's timer may be any number: one far in the past still
    /// gives a frame of the animation.
    #[test]
    fn a_frame_for_any_timer() {
        struct Far;
        impl SkinState for Far {
            fn timer(&self, _: i32) -> Option<i64> {
                Some(i64::MIN / 1000 + 1000)
            }
            fn option(&self, _: i32) -> bool {
                false
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
        let src = Src {
            image: 0,
            frames: Src::grid([0.0, 0.0, 1024.0, 1.0], 1024, 1),
            cycle: 1000,
            timer: 41,
        };
        assert!(src.frame(0, &Far).is_some());
        assert_eq!(Src::step(1500, 4, 1000), 2);
        assert_eq!(Src::step(-5, 4, 1000), 0);
        assert!(Src::step(i64::MAX, 1024, 1) < 1024);
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
        d.acc = Acc::Accelerate;
        assert_eq!(d.eval(500, &St).unwrap().rect.x, 25.0);
        // a single keyframe at 30 ms, loop 0: shown from 30 ms on (LR2)
        let mut one = Dst {
            loop_ms: 0,
            ..Default::default()
        };
        one.add_key(key(30, 7.0));
        assert!(one.eval(10, &St).is_none());
        for t in [30, 59, 60, 1000, 123_457] {
            assert_eq!(one.eval(t, &St).unwrap().rect.x, 7.0, "{t}");
        }
    }
}
