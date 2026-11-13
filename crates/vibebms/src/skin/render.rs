//! Drawing a loaded skin.

use super::*;
use crate::app::Draw;
use crate::gfx::text::Align;
use crate::gfx::{Gfx, TexId};

/// A skin with its textures uploaded.
pub struct LoadedSkin {
    pub skin: Skin,
    pub tex: Vec<Option<TexId>>,
    /// Bitmap font sheets load on first use (a Japanese font has hundreds):
    /// which images are such sheets, and the ones asked for since the
    /// last `load_pending`.
    lazy: Vec<bool>,
    wanted: std::cell::RefCell<Vec<usize>>,
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

/// Scene-specific pieces the generic renderer cannot draw itself. Every
/// method has a no-op default so a scene implements only what it shows.
#[allow(unused_variables)]
pub trait Hooks {
    fn notes(&mut self, d: &mut Draw, skin: &LoadedSkin, xf: &Xf, now: i64, st: &dyn SkinState) {}
    fn bga(&mut self, d: &mut Draw, rect: Rect) {}
    fn ref_image(&self, id: i32) -> Option<TexId> {
        None
    }
    /// (value, border, survival, gauge type in beatoraja order).
    fn gauge(&self) -> (f32, f32, bool, usize) {
        (0.0, 80.0, false, 2)
    }
    /// Current judgement for a side: (judge 0..5, combo).
    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        None
    }
    fn graph(
        &mut self,
        d: &mut Draw,
        kind: GraphKind,
        style: &GraphStyle,
        rect: Rect,
        color: [f32; 4],
    ) {
    }
    /// The music select bar list.
    fn song_list(
        &mut self,
        d: &mut Draw,
        skin: &LoadedSkin,
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
    ) {
    }
    /// A clickable object was drawn at `rect` (screen space).
    fn clickable(&mut self, rect: Rect, act: i32, click: i32) {}
}

impl LoadedSkin {
    pub fn load(skin: Skin, gfx: &mut Gfx) -> LoadedSkin {
        let mut lazy = vec![false; skin.images.len()];
        for f in &skin.fonts {
            for (img, _) in f.glyphs.values() {
                if let Some(l) = lazy.get_mut(*img) {
                    *l = true;
                }
            }
        }
        let tex = skin
            .images
            .iter()
            .zip(&lazy)
            .map(|(p, lazy)| {
                p.as_ref()
                    .filter(|_| !lazy)
                    .and_then(|p| gfx.load_image(p, false))
            })
            .collect();
        LoadedSkin {
            skin,
            tex,
            lazy,
            wanted: Default::default(),
        }
    }

    /// Load the font sheets drawing asked for (call once per frame).
    pub fn load_pending(&mut self, gfx: &mut Gfx) {
        let wanted = std::mem::take(&mut *self.wanted.borrow_mut());
        for i in wanted {
            if self.tex.get(i).is_some_and(Option::is_none) {
                let t = self.skin.images[i]
                    .as_ref()
                    .and_then(|p| gfx.load_image(p, false));
                self.tex[i] = t;
                // a sheet that cannot load is not asked for again
                self.lazy[i] = false;
            }
        }
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        for t in self.tex.drain(..).flatten() {
            gfx.free_texture(t);
        }
        // archives opened for this skin's files
        crate::platform::dxa::clear_cache();
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
            d.b.set_blend(fr.blend);
            d.b.quad_rot(t, r, uv, fr.color, fr.angle, fr.center);
            d.b.set_additive(false);
        }
    }

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
                ((t.max(0) * sets as i64 / s.cycle) % sets as i64) as usize
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
                            d.b.quad(t, seg, uv, fr.color);
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

    /// Draw a text into `r` (font size = height; see `Kind::Text` overflow),
    /// with an LR2 bitmap font when one is given.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        d: &mut Draw,
        s: &str,
        r: Rect,
        align: i32,
        overflow: i32,
        color: crate::gfx::Color,
        font: Option<usize>,
    ) {
        if let Some(f) = font.and_then(|i| self.skin.fonts.get(i)) {
            self.bitmap_text(d, f, s, r, align, color);
            return;
        }
        let (x, al) = match align {
            1 => (r.x + r.w * 0.5, Align::Center),
            2 => (r.right(), Align::Right),
            _ => (r.x, Align::Left),
        };
        let size = r.h.max(4.0);
        match overflow {
            1 if r.w > 1.0 => {
                d.text_fit(s, x, r.y, size, color, al, r.w);
            }
            2 if r.w > 1.0 => {
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, x, r.y, size, color, al);
                d.b.set_clip(None);
            }
            _ => {
                d.text(s, x, r.y, size, color, al);
            }
        }
    }

    /// LR2 bitmap text as beatoraja draws it: glyphs scaled to the rect
    /// height, the line shrunk to the rect width if wider, aligned around
    /// the anchor (left edge / centre / right edge of the rect).
    fn bitmap_text(
        &self,
        d: &mut Draw,
        f: &super::BitmapFont,
        s: &str,
        r: Rect,
        align: i32,
        color: crate::gfx::Color,
    ) {
        let glyphs: Vec<&(usize, [f32; 4])> = s.chars().filter_map(|c| f.glyphs.get(&c)).collect();
        if glyphs.is_empty() || f.size <= 0.0 {
            return;
        }
        let k = r.h / f.size;
        let width = glyphs.iter().map(|g| g.1[2]).sum::<f32>() * k + f.margin * glyphs.len() as f32;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.clamp(0, 2) as f32 * 0.5;
        let mut x = r.x + r.w * a - width * scale * a;
        for (img, g) in glyphs {
            let tw = g[2] * scale * k;
            if self.lazy.get(*img) == Some(&true) && self.tex[*img].is_none() {
                let mut w = self.wanted.borrow_mut();
                if !w.contains(img) {
                    w.push(*img);
                }
            }
            if let Some(t) = self.tex.get(*img).copied().flatten() {
                if let Some((w, h)) = d.gfx.texture_size(t) {
                    let (w, h) = (w as f32, h as f32);
                    let uv = [g[0] / w, g[1] / h, (g[0] + g[2]) / w, (g[1] + g[3]) / h];
                    d.b.quad(t, Rect::new(x, r.y, tw, r.h), uv, color);
                }
            }
            x += tw + f.margin * scale;
        }
    }

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
                d.b.set_blend(fr.blend);
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
        // nothing outside the skin's own screen (objects placed off-screen
        // would show in letterbox bars)
        let xf = Xf::fit(self.skin.width, self.skin.height, view);
        d.b.set_base_clip(Some(xf.rect(Rect::new(
            0.0,
            0.0,
            self.skin.width,
            self.skin.height,
        ))));
        self.draw_scripted(d, view, now, st, hooks);
        d.b.set_base_clip(None);
    }

    fn draw_scripted(
        &self,
        d: &mut Draw,
        view: Rect,
        now: i64,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
        match &self.skin.script {
            Some(rt) => {
                rt.update(st, now);
                let st = super::script::Scripted { st, rt };
                self.draw_objects(d, view, now, &st, hooks);
            }
            None => self.draw_objects(d, view, now, st, hooks),
        }
    }

    /// A click on an object with event `act`: script events are taken
    /// care of here (`true`); other events are the scene's.
    pub fn click(&self, act: i32) -> bool {
        self.skin.script.as_ref().is_some_and(|rt| rt.click(act))
    }

    /// Game events the skin's scripts asked for since the last call.
    pub fn script_events(&self) -> Vec<i32> {
        self.skin
            .script
            .as_ref()
            .map(|rt| rt.take_events())
            .unwrap_or_default()
    }

    fn draw_objects(
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
                Kind::SongList => {
                    hooks.song_list(d, self, &xf, now, st);
                    continue;
                }
                _ => {}
            }
            let Some(fr) = o.dst.eval(now, st) else {
                continue;
            };
            let r = xf.rect(fr.rect);
            if let Some((act, click)) = o.act {
                hooks.clickable(r, act, click);
            }
            match &o.kind {
                Kind::Image(src) | Kind::JudgeLine(src) => self.blit(d, src, now, st, r, &fr),
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
                Kind::Text {
                    id,
                    font,
                    align,
                    overflow,
                } => {
                    if let Some(s) = st.text(*id) {
                        self.text(d, &s, r, *align, *overflow, fr.color, *font);
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
                Kind::ImageSet { srcs, id } => {
                    let i = st.button(*id).unwrap_or(0).max(0) as usize;
                    if let Some(src) = srcs.get(i) {
                        self.blit(d, src, now, st, r, &fr);
                    }
                }
                Kind::Graph(g, style) => hooks.graph(d, *g, style, r, fr.color),
                Kind::Notes | Kind::Judge { .. } | Kind::SongList => {}
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
