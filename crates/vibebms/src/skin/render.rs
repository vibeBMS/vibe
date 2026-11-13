//! Drawing a loaded skin.

use super::*;
use crate::app::Draw;
use crate::gfx::text::Align;
use crate::gfx::{Gfx, TexId};

/// A skin with its textures uploaded.
pub struct LoadedSkin {
    pub skin: Skin,
    pub tex: Vec<Option<TexId>>,
}

/// Skin → screen transform (letterboxed, aspect preserved).
#[derive(Clone, Copy, Debug)]
pub struct Xf {
    pub sx: f32,
    pub sy: f32,
    pub ox: f32,
    pub oy: f32,
}

impl Xf {
    pub fn fit(skin_w: f32, skin_h: f32, view: Rect) -> Xf {
        let s = (view.w / skin_w).min(view.h / skin_h);
        Xf {
            sx: s,
            sy: s,
            ox: view.x + (view.w - skin_w * s) * 0.5,
            oy: view.y + (view.h - skin_h * s) * 0.5,
        }
    }
    pub fn rect(&self, r: Rect) -> Rect {
        Rect::new(
            self.ox + r.x * self.sx,
            self.oy + r.y * self.sy,
            r.w * self.sx,
            r.h * self.sy,
        )
    }
}

/// Scene-specific pieces the generic renderer cannot draw itself.
pub trait Hooks {
    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState);
    fn bga(&mut self, d: &mut Draw, rect: Rect);
    fn ref_image(&self, id: i32) -> Option<TexId>;
    /// (value 0..100, border, survival gauge)
    /// (value, border, survival, gauge type in beatoraja order).
    fn gauge(&self) -> (f32, f32, bool, usize);
    /// Current judgement for a side: (judge 0..5, combo).
    fn judge(&self, side: usize) -> Option<(usize, u32)>;
}

impl LoadedSkin {
    pub fn load(skin: Skin, gfx: &mut Gfx) -> LoadedSkin {
        let tex = skin
            .images
            .iter()
            .map(|p| p.as_ref().and_then(|p| gfx.load_image(p, false)))
            .collect();
        LoadedSkin { skin, tex }
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        for t in self.tex.drain(..).flatten() {
            gfx.free_texture(t);
        }
    }

    fn uv(&self, d: &Draw, src: &Src, f: [f32; 4]) -> Option<(TexId, [f32; 4])> {
        let t = (*self.tex.get(src.image)?)?;
        let (w, h) = d.gfx.texture_size(t)?;
        let (w, h) = (w as f32, h as f32);
        Some((
            t,
            [f[0] / w, f[1] / h, (f[0] + f[2]) / w, (f[1] + f[3]) / h],
        ))
    }

    /// Draw one source frame into a screen rect.
    pub fn blit(&self, d: &mut Draw, src: &Src, now: i64, st: &dyn SkinState, r: Rect, fr: &Frame) {
        let Some(f) = src.frame(now, st) else { return };
        if let Some((t, uv)) = self.uv(d, src, f) {
            d.b.set_additive(fr.additive);
            d.b.quad_rot(t, r, uv, fr.color, fr.angle, fr.center);
            d.b.set_additive(false);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn number(
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
        let Some(base) = src.frame(now, st) else {
            return;
        };
        let _ = base;
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
            if let Some(j) = slots.iter().position(|s| s.is_some()) {
                if j > 0 {
                    slots[j - 1] = Some(11);
                }
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
            ((t.max(0) * sets as i64 / src.cycle) % sets as i64) as usize
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
                d.b.set_additive(fr.additive);
                d.b.quad(
                    t,
                    Rect::new(r.x + r.w * j as f32 - shift, r.y, r.w, r.h),
                    uv,
                    fr.color,
                );
                d.b.set_additive(false);
            }
        }
    }

    pub fn draw(
        &self,
        d: &mut Draw,
        view: Rect,
        now: i64,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
        let xf = Xf::fit(self.skin.width, self.skin.height, view);
        for o in &self.skin.objects {
            match &o.kind {
                Kind::Notes => {
                    hooks.notes(d, self, &xf, now, st);
                    continue;
                }
                Kind::Judge { side } => {
                    self.draw_judge(d, &xf, *side, now, st, hooks);
                    continue;
                }
                _ => {}
            }
            let Some(fr) = o.dst.eval(now, st) else {
                continue;
            };
            let r = xf.rect(fr.rect);
            match &o.kind {
                Kind::Image(src) | Kind::JudgeLine(src) => self.blit(d, src, now, st, r, &fr),
                Kind::RefImage(id) => {
                    if let Some(t) = hooks.ref_image(*id) {
                        d.b.quad(t, r, [0.0, 0.0, 1.0, 1.0], fr.color);
                    }
                }
                Kind::Number {
                    src,
                    digits,
                    id,
                    align,
                    keta,
                    pad,
                } => {
                    if let Some(v) = st.number(*id) {
                        self.number(d, src, *digits, *keta, *align, *pad, v, now, st, r, &fr);
                    }
                }
                Kind::Text { id, align } => {
                    if let Some(s) = st.text(*id) {
                        let (x, al) = match align {
                            1 => (r.x + r.w * 0.5, Align::Center),
                            2 => (r.right(), Align::Right),
                            _ => (r.x, Align::Left),
                        };
                        let max_w = if r.w > 1.0 {
                            r.w.max(r.h * 4.0)
                        } else {
                            f32::INFINITY
                        };
                        d.text_fit(&s, x, r.y, r.h.max(4.0), fr.color, al, max_w);
                    }
                }
                Kind::Bar { src, id, dir } => {
                    let v = st.rate(*id).clamp(0.0, 1.0);
                    let Some(f) = src.frame(now, st) else {
                        continue;
                    };
                    // direction 1 fills bottom-to-top
                    let (rr, ff) = if *dir == 1 {
                        (
                            Rect::new(r.x, r.bottom() - r.h * v, r.w, r.h * v),
                            [f[0], f[1] + f[3] * (1.0 - v), f[2], f[3] * v],
                        )
                    } else {
                        (
                            Rect::new(r.x, r.y, r.w * v, r.h),
                            [f[0], f[1], f[2] * v, f[3]],
                        )
                    };
                    if let Some((t, uv)) = self.uv(d, src, ff) {
                        d.b.quad(t, rr, uv, fr.color);
                    }
                }
                Kind::Slider {
                    src,
                    id,
                    dir,
                    range,
                } => {
                    let off = st.rate(*id).clamp(0.0, 1.0) * range;
                    let (dx, dy) = match dir {
                        0 => (0.0, -off),
                        1 => (off, 0.0),
                        2 => (0.0, off),
                        _ => (-off, 0.0),
                    };
                    let rr = Rect::new(r.x + dx * xf.sx, r.y + dy * xf.sy, r.w, r.h);
                    self.blit(d, src, now, st, rr, &fr);
                }
                Kind::Gauge {
                    src,
                    step,
                    parts,
                    split,
                    ..
                } => {
                    let (value, border, survival, ty) = hooks.gauge();
                    let n = *parts as f32;
                    let lit = (value / 100.0 * n).floor() as usize;
                    let per = 100.0 / n;
                    let groups = src.frames.len() / 4;
                    // one 4-image group per gauge type (beatoraja order), or
                    // groove + survival, or a single group
                    let group = if groups > ty {
                        ty
                    } else if groups > 1 && survival {
                        1
                    } else {
                        0
                    };
                    let pw = if *split { fr.rect.w / n } else { fr.rect.w };
                    for i in 0..*parts {
                        let above = survival || (i as f32 + 1.0) * per > border;
                        let idx = match (i < lit, above) {
                            (true, true) => 0,
                            (true, false) => 1,
                            (false, true) => 2,
                            (false, false) => 3,
                        };
                        let Some(f) = src.frames.get(idx + group * 4).or(src.frames.get(idx))
                        else {
                            continue;
                        };
                        let (sx, sy) = if step.0.abs() >= 1.0 || step.1.abs() >= 1.0 {
                            (step.0, step.1)
                        } else {
                            (pw, 0.0)
                        };
                        let pr = xf.rect(Rect::new(
                            fr.rect.x + sx * i as f32,
                            fr.rect.y + sy * i as f32,
                            pw,
                            fr.rect.h,
                        ));
                        if let Some((t, uv)) = self.uv(d, src, *f) {
                            d.b.quad(t, pr, uv, fr.color);
                        }
                    }
                }
                Kind::Bga => hooks.bga(d, r),
                Kind::Notes | Kind::Judge { .. } => {}
            }
        }
    }

    fn draw_judge(
        &self,
        d: &mut Draw,
        xf: &Xf,
        side: usize,
        now: i64,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
        let Some((j, combo)) = hooks.judge(side) else {
            return;
        };
        let js = &self.skin.judges[side.min(1)];
        let Some((src, dst)) = js.judge[j.min(5)].as_ref() else {
            return;
        };
        let Some(fr) = dst.eval(now, st) else { return };
        let mut jr = xf.rect(fr.rect);
        let combo_part = (j <= 2 && combo > 0)
            .then(|| js.combo[j.min(5)].as_ref())
            .flatten();
        if let Some((csrc, cdst, keta)) = combo_part {
            if let Some(cf) = cdst.eval(now, st) {
                let digits = combo.to_string().len().min(*keta);
                let cw = cf.rect.w * xf.sx * digits as f32;
                if js.shift {
                    jr.x -= cw * 0.5;
                }
                // combo position is relative to the judge image
                let cr = Rect::new(
                    jr.x + cf.rect.x * xf.sx,
                    jr.y + cf.rect.y * xf.sy,
                    cf.rect.w * xf.sx,
                    cf.rect.h * xf.sy,
                );
                let n = if csrc.frames.len() % 24 == 0 {
                    12
                } else if csrc.frames.len() % 11 == 0 {
                    11
                } else {
                    10
                };
                let cr = Rect::new(cr.x - cr.w * (*keta - digits) as f32, cr.y, cr.w, cr.h);
                self.number(d, csrc, n, *keta, 0, None, combo as i64, now, st, cr, &cf);
            }
        }
        self.blit(d, src, now, st, jr, &fr);
    }
}
