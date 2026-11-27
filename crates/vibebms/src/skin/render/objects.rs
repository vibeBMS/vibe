//! A skin's objects drawn, each by its kind.

use super::{Hooks, LoadedSkin, Xf};
use crate::gfx::{Draw, Rect, TexId};
use crate::skin::*;

/// A part `r` (an image's region `uv`) of an object drawn at `whole`:
/// with the object's blend, turned with it about its centre.
fn part(d: &mut Draw, t: TexId, r: Rect, uv: [f32; 4], fr: &Frame, whole: Rect) {
    let pivot = (
        (whole.x + whole.w * fr.center.0 - r.x) / r.w,
        (whole.y + whole.h * fr.center.1 - r.y) / r.h,
    );
    d.b.set_blend(fr.blend);
    d.b.quad_rot(t, r, uv, fr.color, fr.angle, pivot);
    d.b.set_blend(crate::gfx::Blend::Alpha);
}

/// 0..1 of a bar or slider: a rate, or a number within `minmax`.
fn value(st: &dyn SkinState, id: i32, minmax: Option<(f32, f32)>) -> f32 {
    match minmax {
        Some((lo, hi)) if hi != lo => {
            let n = st.number(id).unwrap_or(lo as i64) as f32;
            ((n - lo) / (hi - lo)).clamp(0.0, 1.0)
        }
        Some(_) => 0.0,
        None => st.rate(id).clamp(0.0, 1.0),
    }
}

/// Where an image of `w` x `h` goes in `r` under beatoraja stretch type
/// `mode`, and whether it is cut off at `r`.
pub fn stretched(mode: i32, r: Rect, w: f32, h: f32) -> (Rect, bool) {
    if w <= 0.0 || h <= 0.0 {
        return (r, false);
    }
    let (fx, fy) = (r.w / w, r.h / h);
    let (scale, trim) = match mode {
        1 => (fx.min(fy), false),
        2 => (fx.max(fy), false),
        3 => (fx.max(fy), true),
        4 => (fx, false),
        5 => (fx, true),
        6 => (fy, false),
        7 => (fy, true),
        8 => (fx.min(fy).min(1.0), false),
        9 => (1.0, false),
        10 => (1.0, true),
        _ => return (r, false),
    };
    let (sw, sh) = (w * scale, h * scale);
    (
        Rect::new(r.x + (r.w - sw) * 0.5, r.y + (r.h - sh) * 0.5, sw, sh),
        trim,
    )
}

impl LoadedSkin {
    /// A stacked bar of lamp counts (best lamps first, as beatoraja draws
    /// them) or rank counts (AAA first).
    #[allow(clippy::too_many_arguments)]
    pub fn distribution(
        &self,
        d: &mut Draw,
        src: Option<&Src>,
        counts: &[u32],
        rank: bool,
        now: i64,
        st: &dyn SkinState,
        r: Rect,
        fr: &Frame,
    ) {
        let total: u32 = counts.iter().sum();
        if total == 0 {
            return;
        }
        let order: Vec<usize> = if rank {
            (0..counts.len()).collect()
        } else {
            (0..counts.len()).rev().collect()
        };
        // animated image sets: one row of segment images per animation frame
        let set = src
            .filter(|s| s.cycle > 0 && s.frames.len() >= 22)
            .map(|s| {
                let sets = s.frames.len() / 11;
                let t = if s.timer > 0 {
                    st.timer(s.timer).map(|t0| now - t0).unwrap_or(0)
                } else {
                    now
                };
                Src::step(t, sets, s.cycle)
            })
            .unwrap_or(0);
        const RANK_COLORS: [u32; 9] = [
            0xffd34aff, 0xe6e6f0ff, 0xd29a5aff, 0x5fdc8aff, 0x4fa8ffff, 0xb58cffff, 0xff8a65ff,
            0xff5a6eff, 0x40444fff,
        ];
        let mut x = r.x;
        for k in order {
            let w = r.w * counts[k] as f32 / total as f32;
            if w <= 0.0 {
                continue;
            }
            let seg = Rect::new(x, r.y, w, r.h);
            match src {
                Some(s) if !rank => {
                    if let Some(f) = s.frames.get(set * 11 + k).or(s.frames.get(k)) {
                        if let Some((t, uv)) = self.uv(d, s, *f) {
                            part(d, t, seg, uv, fr, r);
                        }
                    }
                }
                _ => {
                    let c = crate::gfx::rgba(RANK_COLORS[k.min(8)]);
                    d.b.rect(seg, [c[0], c[1], c[2], c[3] * fr.color[3]]);
                }
            }
            x += w;
        }
    }

    pub(super) fn draw_objects(
        &self,
        d: &mut Draw,
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
        // the objects of an option show together: none while an image one
        // of them is drawn from is loading (a panel's page as it opens)
        let mut held: Vec<i32> = Vec::new();
        for o in &self.skin.objects {
            match &o.kind {
                Kind::Notes => {
                    hooks.notes(d, self, xf, now, st);
                    continue;
                }
                Kind::Judge { side } => {
                    self.draw_judge(d, xf, *side, now, st, hooks);
                    continue;
                }
                Kind::SongList => {
                    hooks.song_list(d, self, xf, now, st);
                    continue;
                }
                _ => {}
            }
            let Some(fr) = o.dst.eval(now, st) else {
                continue;
            };
            let r = xf.rect(fr.rect);
            // where it is on screen (a mirrored image has a negative size)
            let area = Rect::new(r.x.min(r.x + r.w), r.y.min(r.y + r.h), r.w.abs(), r.h.abs());
            if let Some(h) = o.dst.hover {
                let over = Rect::new(
                    area.x + h.x * xf.sx,
                    area.y + h.y * xf.sy,
                    h.w * xf.sx,
                    h.h * xf.sy,
                );
                if !d.pointer.is_some_and(|(x, y)| over.contains(x, y)) {
                    continue;
                }
            }
            if let Some((act, click)) = o.act {
                hooks.clickable(area, act, click);
            }
            if !held.is_empty() && o.dst.ops.iter().any(|op| held.contains(op)) {
                continue;
            }
            self.missing.set(false);
            match &o.kind {
                Kind::Image(src) | Kind::JudgeLine(src) => {
                    let size = src.frames.first().map(|f| (f[2] * xf.sx, f[3] * xf.sy));
                    match size.filter(|_| o.dst.stretch != 0) {
                        Some((w, h)) => {
                            let (rr, trim) = stretched(o.dst.stretch, r, w, h);
                            if trim {
                                d.b.set_clip(Some(r));
                            }
                            self.blit(d, src, now, st, rr, &fr);
                            if trim {
                                d.b.set_clip(None);
                            }
                        }
                        None => self.blit(d, src, now, st, r, &fr),
                    }
                }
                Kind::Distribution { src, rank } => {
                    if let Some(counts) = st.distribution(*rank) {
                        self.distribution(d, src.as_ref(), &counts, *rank, now, st, r, &fr);
                    }
                }
                // beatoraja's plain black / white
                Kind::RefImage(110) => d.b.rect(r, [0.0, 0.0, 0.0, fr.color[3]]),
                Kind::RefImage(111) => d.b.rect(r, fr.color),
                Kind::Cover {
                    src,
                    line,
                    link_lift,
                } => {
                    // drawn only above the disappear line (which LIFT raises)
                    let clip = line.map(|l| {
                        let lift = if *link_lift {
                            st.offset(OFFSET_LIFT).map(|o| o[1]).unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        let ly = xf.oy + (l - lift) * xf.sy;
                        Rect::new(r.x, r.y.min(ly), r.w, (ly - r.y).max(0.0))
                    });
                    if clip.map(|c| c.h > 0.0).unwrap_or(true) {
                        d.b.set_clip(clip);
                        self.blit(d, src, now, st, r, &fr);
                        d.b.set_clip(None);
                    }
                }
                Kind::RefImage(REF_SKIN_PREVIEW) => hooks.preview(d, r, fr.color),
                Kind::RefImage(id) => {
                    if let Some(t) = hooks.ref_image(*id) {
                        part(d, t, r, [0.0, 0.0, 1.0, 1.0], &fr, r);
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
                Kind::Text {
                    id,
                    font,
                    align,
                    overflow,
                    constant,
                    face,
                    fx,
                    size,
                } => {
                    if let Some(s) = constant.clone().or_else(|| st.text(*id)) {
                        // texts follow the selection (timer 11), else the scene
                        let shown = now - st.timer(11).unwrap_or(0);
                        let f = face.as_deref().and_then(|p| d.text.face(p));
                        d.text.set_face(f.unwrap_or(0));
                        let (al, of, c) = (*align, *overflow, fr.color);
                        let sh = Some(shown);
                        self.text(d, &s, r, al, of, c, *font, sh, fx, *size, xf.sy);
                        d.text.set_face(0);
                    }
                }
                Kind::Bar {
                    src,
                    id,
                    dir,
                    minmax,
                } => {
                    let v = value(st, *id, *minmax);
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
                        part(d, t, rr, uv, &fr, r);
                    }
                }
                Kind::Slider {
                    src,
                    id,
                    dir,
                    range,
                    minmax,
                } => {
                    let off = value(st, *id, *minmax) * range;
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
                    side,
                    ..
                } => {
                    let (value, border, survival, ty) = hooks.gauge(*side);
                    let n = *parts as f32;
                    let per = 100.0 / n;
                    // as beatoraja: whole cells, at least one while alive,
                    // and the cell ending on the border is already red (a
                    // NORMAL gauge at 80% shows red)
                    let lit = if value > 0.0 {
                        ((value / per) as usize).max(1)
                    } else {
                        0
                    };
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
                        let above = survival || (i as f32 + 1.0) * per >= border;
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
                            part(d, t, pr, uv, &fr, r);
                        }
                    }
                }
                Kind::Bga => hooks.bga(d, r),
                Kind::ImageSet { srcs, id } => {
                    let i = st.button(*id).unwrap_or(0).max(0) as usize;
                    if let Some(src) = srcs.get(i) {
                        self.blit(d, src, now, st, r, &fr);
                    }
                }
                Kind::Graph(g) => hooks.graph(d, g, r, fr.color),
                Kind::Notes | Kind::Judge { .. } | Kind::SongList => {}
            }
            if self.missing.get() {
                held.extend(o.dst.ops.iter().filter(|&&op| op > 0));
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
        let js = &self.skin.judges[side.min(2)];
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
