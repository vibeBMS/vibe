//! Numbers drawn with a skin's digit images.

use super::LoadedSkin;
use crate::gfx::{Draw, Rect};
use crate::skin::*;

impl LoadedSkin {
    #[allow(clippy::too_many_arguments)]
    pub fn number(
        &self,
        d: &mut Draw,
        src: &Src,
        digits: usize,
        keta: usize,
        align: i32,
        pad: Option<usize>,
        value: i64,
        now: i64,
        st: &dyn SkinState,
        r: Rect,
        fr: &Frame,
    ) {
        // (none while its timer is off)
        if src.frame(now, st).is_none() {
            return;
        }
        let neg = value < 0;
        let mut v = value.unsigned_abs();
        let mut slots: Vec<Option<usize>> = vec![None; keta];
        for j in (0..keta).rev() {
            if v > 0 || j == keta - 1 {
                slots[j] = Some((v % 10) as usize);
            } else if pad.is_some() {
                slots[j] = pad;
            }
            v /= 10;
        }
        // signed fonts (24 images): a positive and a negative set of
        // 0-9, padding, sign; the sign goes in front of the first digit
        let signed = digits == 12;
        if signed {
            // the sign goes right before the first real digit (not padding)
            let len = value.unsigned_abs().max(1).ilog10() as usize + 1;
            if len < keta {
                slots[keta - len - 1] = Some(11);
            }
        }
        let blanks = slots.iter().filter(|s| s.is_none()).count() as f32;
        let shift = match align {
            1 => r.w * blanks,
            2 => r.w * blanks * 0.5,
            _ => 0.0,
        };
        // animated digit sets: frames are grouped by `digits` (x2 when signed)
        let group = if signed { 24 } else { digits };
        let sign_off = if signed && neg { 12 } else { 0 };
        let sets = (src.frames.len() / group).max(1);
        let set = if src.cycle > 0 && sets > 1 {
            let t = if src.timer > 0 {
                st.timer(src.timer).map(|s| now - s).unwrap_or(0)
            } else {
                now
            };
            Src::step(t, sets, src.cycle)
        } else {
            0
        };
        for (j, s) in slots.iter().enumerate() {
            let Some(dg) = s else { continue };
            let Some(f) = src
                .frames
                .get(set * group + sign_off + dg)
                .or_else(|| src.frames.get(*dg))
            else {
                continue;
            };
            if let Some((t, uv)) = self.uv(d, src, *f) {
                d.b.set_blend(fr.blend);
                d.b.quad(
                    t,
                    Rect::new(r.x + r.w * j as f32 - shift, r.y, r.w, r.h),
                    uv,
                    fr.color,
                );
                d.b.set_blend(crate::gfx::Blend::Alpha);
            }
        }
    }
}
