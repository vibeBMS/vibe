//! Drawing a loaded skin.

use super::script::Overlay;
use super::*;
use crate::app::Draw;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{Gfx, ImageTex, Picture, TexId};

/// A skin with its textures uploaded.
pub struct LoadedSkin {
    pub skin: Skin,
    pub tex: Vec<Option<TexId>>,
    /// Images loaded when first drawn: bitmap font sheets (a Japanese font
    /// has hundreds) and the skin's `lazy_images`. Which images are such
    /// (and not asked for yet), and the ones asked for since the last
    /// `load_pending`.
    lazy: Vec<bool>,
    wanted: std::cell::RefCell<Vec<usize>>,
    /// Images being decoded, with the images that show each file; the
    /// first is the skin's own, which it is not drawn without.
    decoding: Vec<(ImageJobs, Vec<Vec<usize>>)>,
    ready: bool,
    /// Images that are videos (looping from when the skin loaded).
    videos: Vec<(usize, crate::video::Video)>,
    /// Animated images (GIF, WebP, APNG), from when the skin loaded; their
    /// textures are in `tex`.
    anims: Vec<crate::gfx::ImageTex>,
    loaded_at: crate::platform::Instant,
}

/// An image file's pixels (an animation's frames); `key` turns that colour
/// transparent if the image has no alpha of its own (LR2 `#TRANSCOLOR`).
fn decode(path: &std::path::Path, key: Option<[u8; 3]>) -> Option<Picture> {
    let keyed = |px: &mut [u8]| {
        if let Some(k) = key {
            if px.as_chunks::<4>().0.iter().all(|p| p[3] == 255) {
                for p in px.as_chunks_mut::<4>().0 {
                    if p[..3] == k {
                        p[3] = 0;
                    }
                }
            }
        }
    };
    crate::gfx::load_picture(path, &keyed)
}

/// Image files decoded off the main thread, one after another on a worker
/// thread (so no more than a couple are held at once); on the web, which
/// has no threads, as many as fit in a few milliseconds per `poll`.
pub struct ImageJobs {
    total: usize,
    left: usize,
    source: Source,
}

enum Source {
    Worker(std::sync::mpsc::Receiver<Option<Picture>>),
    Here(Vec<PathBuf>, Option<[u8; 3]>),
}

impl ImageJobs {
    pub fn start(paths: Vec<PathBuf>, key: Option<[u8; 3]>) -> ImageJobs {
        let total = paths.len();
        let source = if crate::platform::IS_WEB {
            Source::Here(paths, key)
        } else {
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            crate::platform::spawn("image decode", move || {
                for p in &paths {
                    // the receiver is gone: the images are not wanted
                    if tx.send(decode(p, key)).is_err() {
                        break;
                    }
                }
            });
            Source::Worker(rx)
        };
        ImageJobs {
            total,
            left: total,
            source,
        }
    }

    pub fn done(&self) -> bool {
        self.left == 0
    }

    /// The images decoded since the last call, by index in the paths
    /// given, made textures by `upload` (`None`: not an image); `wait`
    /// waits for all.
    pub fn poll<T>(
        &mut self,
        wait: bool,
        mut upload: impl FnMut(Picture) -> T,
    ) -> Vec<(usize, Option<T>)> {
        use std::sync::mpsc::TryRecvError;
        let mut out = Vec::new();
        let t0 = crate::platform::Instant::now();
        while self.left > 0 {
            let i = self.total - self.left;
            let img = match &self.source {
                Source::Worker(rx) if wait => rx.recv().ok().flatten(),
                Source::Worker(rx) => match rx.try_recv() {
                    Ok(img) => img,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => None,
                },
                Source::Here(..) if !wait && !out.is_empty() && t0.elapsed().as_millis() >= 8 => {
                    break
                }
                Source::Here(paths, key) => decode(&paths[i], *key),
            };
            self.left -= 1;
            out.push((i, img.map(&mut upload)));
        }
        out
    }
}

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

/// Screen px per font px of a BMFont text drawn into a rect `h` high
/// with font size `size` (screen px): size over the font's own size as in
/// beatoraja, else so a line fills the rect.
pub fn bmfont_scale(f: &BitmapFont, h: f32, size: Option<f32>) -> f32 {
    match size {
        Some(s) if f.em > 0.0 => s / f.em,
        _ => h / f.size,
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
    /// A side's gauge: (value, border, survival, gauge type in beatoraja
    /// order).
    fn gauge(&self, side: usize) -> (f32, f32, bool, usize) {
        (0.0, 80.0, false, 2)
    }
    /// Current judgement for a side: (judge 0..5, combo).
    fn judge(&self, side: usize) -> Option<(usize, u32)> {
        None
    }
    fn graph(&mut self, d: &mut Draw, graph: &Graph, rect: Rect, color: [f32; 4]) {}
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
    /// The skin select screen's preview of the chosen skin.
    fn preview(&mut self, d: &mut Draw, rect: Rect, color: [f32; 4]) {}
}

impl LoadedSkin {
    pub fn load(skin: Skin, gfx: &mut Gfx) -> LoadedSkin {
        let key = skin.trans_color;
        let mut lazy = vec![false; skin.images.len()];
        let sheets = skin.fonts.iter().flat_map(|f| f.glyphs.values().map(|(img, _)| img));
        for i in sheets.chain(&skin.lazy_images) {
            if let Some(l) = lazy.get_mut(*i) {
                *l = true;
            }
        }
        let mut videos = Vec::new();
        // only what is drawn, and an image several sources name once
        let used = skin.used_images();
        let mut files: Vec<PathBuf> = Vec::new();
        let mut users: Vec<Vec<usize>> = Vec::new();
        let mut file_of: HashMap<&PathBuf, usize> = HashMap::new();
        for (i, p) in skin.images.iter().enumerate() {
            let Some(p) = p.as_ref().filter(|_| !lazy[i] && used[i]) else {
                continue;
            };
            if crate::video::is_video(p) {
                if let Some(v) = crate::video::Video::open(p, true) {
                    videos.push((i, v));
                }
                continue;
            }
            let f = *file_of.entry(p).or_insert_with(|| {
                files.push(p.clone());
                users.push(Vec::new());
                files.len() - 1
            });
            users[f].push(i);
        }
        let mut sk = LoadedSkin {
            tex: vec![None; skin.images.len()],
            decoding: vec![(ImageJobs::start(files, key), users)],
            ready: false,
            skin,
            lazy,
            wanted: Default::default(),
            videos,
            anims: Vec::new(),
            loaded_at: crate::platform::Instant::now(),
        };
        sk.take_decoded(gfx, false);
        sk
    }

    /// Wait for the skin's images (else they come in while it is not
    /// drawn yet).
    pub fn wait(&mut self, gfx: &mut Gfx) {
        self.take_decoded(gfx, true);
    }

    fn take_decoded(&mut self, gfx: &mut Gfx, wait: bool) {
        for (jobs, users) in &mut self.decoding {
            for (f, t) in jobs.poll(wait, |pic| ImageTex::new(gfx, pic)) {
                for &i in &users[f] {
                    self.tex[i] = t.as_ref().map(|t| t.tex);
                }
                self.anims.extend(t.filter(|t| t.is_animated()));
            }
        }
        if self.decoding.first().is_some_and(|(j, _)| j.done()) {
            self.ready = true;
        }
        self.decoding.retain(|(j, _)| !j.done());
    }

    /// Load the font sheets drawing asked for and advance videos (call
    /// once per frame).
    pub fn load_pending(&mut self, gfx: &mut Gfx) {
        self.load_pending_at(gfx, crate::platform::Instant::now());
    }

    /// [`Self::load_pending`] with the videos and animations at `now`
    /// (a frame drawn offline: its own time).
    pub fn load_pending_at(&mut self, gfx: &mut Gfx, now: crate::platform::Instant) {
        let ms = now.saturating_duration_since(self.loaded_at).as_millis() as i64;
        for (i, v) in &mut self.videos {
            v.update(gfx, ms);
            self.tex[*i] = v.texture();
        }
        for a in &mut self.anims {
            a.show(gfx, ms);
        }
        self.take_decoded(gfx, false);
        // (asked for once: a sheet that cannot load is not asked for again)
        let mut wanted = std::mem::take(&mut *self.wanted.borrow_mut());
        wanted.retain(|&i| std::mem::take(&mut self.lazy[i]));
        let (files, users): (Vec<PathBuf>, Vec<Vec<usize>>) = wanted
            .into_iter()
            .filter_map(|i| Some((self.skin.images[i].clone()?, vec![i])))
            .unzip();
        if !files.is_empty() {
            let jobs = ImageJobs::start(files, self.skin.trans_color);
            self.decoding.push((jobs, users));
        }
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        self.decoding.clear();
        // video textures belong to the videos
        for (i, mut v) in self.videos.drain(..) {
            self.tex[i] = None;
            v.free(gfx);
        }
        // (their textures are freed with the others)
        self.anims.clear();
        let mut ids: Vec<TexId> = self.tex.drain(..).flatten().collect();
        ids.sort_unstable();
        ids.dedup();
        for t in ids {
            gfx.free_texture(t);
        }
        // archives opened for this skin's files
        crate::formats::dxa::clear_cache();
    }

    /// Image `i`'s texture; one loaded when first drawn is asked for.
    fn texture(&self, i: usize) -> Option<TexId> {
        let t = self.tex.get(i).copied().flatten();
        if t.is_none() && self.lazy.get(i) == Some(&true) {
            let mut w = self.wanted.borrow_mut();
            if !w.contains(&i) {
                w.push(i);
            }
        }
        t
    }

    fn uv(&self, d: &Draw, src: &Src, f: [f32; 4]) -> Option<(TexId, [f32; 4])> {
        let t = self.texture(src.image)?;
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
        if let Some((t, mut uv)) = self.uv(d, src, f) {
            // a negative width / height mirrors the image (beatoraja)
            let (mut r, mut center) = (r, fr.center);
            if r.w < 0.0 {
                r = Rect::new(r.x + r.w, r.y, -r.w, r.h);
                uv.swap(0, 2);
                center.0 = 1.0 - center.0;
            }
            if r.h < 0.0 {
                r = Rect::new(r.x, r.y + r.h, r.w, -r.h);
                uv.swap(1, 3);
                center.1 = 1.0 - center.1;
            }
            d.b.set_blend(fr.blend);
            d.b.quad_rot(t, r, uv, fr.color, fr.angle, center);
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

    /// Draw a text into `r` (font size = height; see `Kind::Text` overflow),
    /// with an LR2 bitmap font when one is given. `shown_ms`: how long the
    /// text has been the current one, for a marquee (overflow 3; `None`
    /// keeps it still). `fx`: its outline and shadow as beatoraja draws
    /// them (see `TextFx`); `size`: the beatoraja font size (skin px),
    /// which sizes `.fnt` glyphs; `scale`: screen px per skin px.
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
        shown_ms: Option<i64>,
        fx: &TextFx,
        size: Option<f32>,
        scale: f32,
    ) {
        let args = (align, overflow, font, shown_ms, size.map(|s| s * scale));
        if font
            .and_then(|i| self.skin.fonts.get(i))
            .is_some_and(|f| f.sdf)
        {
            // the shader draws the outline and shadow
            d.b.set_sdf(Some(fx.sdf()));
            self.text_plain(d, s, r, color, args);
            d.b.set_sdf(None);
            return;
        }
        if fx.has_shadow() {
            let (dx, dy) = (fx.shadow_offset.0 * scale, fx.shadow_offset.1 * scale);
            let half = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            self.text_plain(d, s, Rect::new(r.x + dx, r.y + dy, r.w, r.h), half, args);
        }
        self.text_plain(d, s, r, color, args);
    }

    /// `text` without outline or shadow; `args`: align, overflow, font,
    /// shown_ms, font size (screen px).
    fn text_plain(
        &self,
        d: &mut Draw,
        s: &str,
        r: Rect,
        color: crate::gfx::Color,
        args: (i32, i32, Option<usize>, Option<i64>, Option<f32>),
    ) {
        let (align, overflow, font, shown_ms, size) = args;
        // lines one under the other (beatoraja's fonts: 1.2 sizes apart)
        if s.contains('\n') {
            for (i, line) in s.split('\n').enumerate() {
                let y = r.y + r.h * 1.2 * i as f32;
                let lr = Rect::new(r.x, y, r.w, r.h);
                self.text_plain(d, line, lr, color, args);
            }
            return;
        }
        if let Some(f) = font.and_then(|i| self.skin.fonts.get(i)) {
            self.bitmap_text(d, f, s, r, align, color, size);
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
                d.text(s, x, r.y, TextStyle::new(size, color, al).fit(r.w));
            }
            2 if r.w > 1.0 => {
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, x, r.y, TextStyle::new(size, color, al));
                d.b.set_clip(None);
            }
            // vibeBMS: a long text shows its start, cut off; while it is
            // the current one it scrolls to its end and back (a marquee)
            3 if r.w > 1.0 => {
                let w = d.measure(s, size);
                if w <= r.w {
                    d.text(s, x, r.y, TextStyle::new(size, color, al));
                    return;
                }
                let off = shown_ms.map_or(0.0, |t| marquee_offset(t, w - r.w, size));
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, r.x - off, r.y, TextStyle::new(size, color, Align::Left));
                d.b.set_clip(None);
            }
            _ => {
                d.text(s, x, r.y, TextStyle::new(size, color, al));
            }
        }
    }

    /// LR2 bitmap text as beatoraja draws it: glyphs scaled to the rect
    /// height, the line shrunk to the rect width if wider, aligned around
    /// the anchor (left edge / centre / right edge of the rect).
    #[allow(clippy::too_many_arguments)]
    fn bitmap_text(
        &self,
        d: &mut Draw,
        f: &super::BitmapFont,
        s: &str,
        r: Rect,
        align: i32,
        color: crate::gfx::Color,
        size: Option<f32>,
    ) {
        let chars: Vec<char> = s.chars().filter(|c| f.glyphs.contains_key(c)).collect();
        let glyphs: Vec<&(usize, [f32; 4])> =
            chars.iter().filter_map(|c| f.glyphs.get(c)).collect();
        if glyphs.is_empty() || f.size <= 0.0 {
            return;
        }
        let k = r.h / f.size;
        if !f.metrics.is_empty() {
            self.bmfont_text(d, f, &chars, r, align, color, size);
            return;
        }
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
            if let Some(t) = self.texture(*img) {
                if let Some((w, h)) = d.gfx.texture_size(t) {
                    let (w, h) = (w as f32, h as f32);
                    let uv = [g[0] / w, g[1] / h, (g[0] + g[2]) / w, (g[1] + g[3]) / h];
                    d.b.quad(t, Rect::new(x, r.y, tw, r.h), uv, color);
                }
            }
            x += tw + f.margin * scale;
        }
    }

    /// A BMFont text: glyphs placed by their offsets and advances, scaled
    /// as beatoraja does by the text's font size over the font's own (else
    /// so a line fills the rect's height).
    #[allow(clippy::too_many_arguments)]
    fn bmfont_text(
        &self,
        d: &mut Draw,
        f: &super::BitmapFont,
        chars: &[char],
        r: Rect,
        align: i32,
        color: crate::gfx::Color,
        size: Option<f32>,
    ) {
        let k = bmfont_scale(f, r.h, size);
        let adv = |c: &char| f.metrics.get(c).map(|m| m[2]).unwrap_or(0.0);
        let width = chars.iter().map(adv).sum::<f32>() * k;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.clamp(0, 2) as f32 * 0.5;
        let mut x = r.x + r.w * a - width * scale * a;
        for c in chars {
            let (Some((img, g)), Some(m)) = (f.glyphs.get(c), f.metrics.get(c)) else {
                continue;
            };
            if let Some(t) = self.texture(*img) {
                if let Some((w, h)) = d.gfx.texture_size(t) {
                    let (w, h) = (w as f32, h as f32);
                    let uv = [g[0] / w, g[1] / h, (g[0] + g[2]) / w, (g[1] + g[3]) / h];
                    let gr = Rect::new(
                        x + m[0] * k * scale,
                        r.y + m[1] * k,
                        g[2] * k * scale,
                        g[3] * k,
                    );
                    d.b.quad(t, gr, uv, color);
                }
            }
            x += m[2] * k * scale;
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
        if !self.ready {
            return;
        }
        // nothing outside the skin's own screen (objects placed off-screen
        // would show in letterbox bars)
        let xf = Xf::fit(self.skin.width, self.skin.height, view);
        d.b.set_base_clip(Some(xf.rect(Rect::new(
            0.0,
            0.0,
            self.skin.width,
            self.skin.height,
        ))));
        let offsets = &self.skin.offsets;
        let rt = self.skin.script.as_deref();
        if let Some(rt) = rt {
            rt.update(
                &Overlay {
                    st,
                    rt: None,
                    offsets,
                },
                now,
            );
        }
        self.draw_objects(d, &xf, now, &Overlay { st, rt, offsets }, hooks);
        d.b.set_base_clip(None);
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
        xf: &Xf,
        now: i64,
        st: &dyn SkinState,
        hooks: &mut dyn Hooks,
    ) {
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

/// How far a marquee has scrolled (px) `t` ms after it started: it waits,
/// scrolls `travel` px, waits at the end, and starts again.
pub fn marquee_offset(t: i64, travel: f32, size: f32) -> f32 {
    const WAIT_MS: f32 = 1200.0;
    let speed = (size * 2.5).max(40.0); // px/s
    let scroll_ms = travel / speed * 1000.0;
    let t = t.max(0) as f32 % (WAIT_MS * 2.0 + scroll_ms);
    if t < WAIT_MS {
        0.0
    } else {
        ((t - WAIT_MS) / 1000.0 * speed).min(travel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_decode_off_the_main_thread_in_order() {
        let dir = std::env::temp_dir().join(format!("vibebms-imgjobs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut img = image::RgbaImage::new(3, 2);
        img.put_pixel(0, 0, image::Rgba([1, 2, 3, 255]));
        img.save(dir.join("a.png")).unwrap();
        image::RgbImage::new(5, 4).save(dir.join("b.png")).unwrap();
        let paths = ["a.png", "none.png", "b.png"].map(|f| dir.join(f)).to_vec();
        let size = |p: Picture| (p.w, p.h);
        let mut jobs = ImageJobs::start(paths.clone(), None);
        let mut got = Vec::new();
        while !jobs.done() {
            got.extend(jobs.poll(false, size));
        }
        assert_eq!(got, vec![(0, Some((3, 2))), (1, None), (2, Some((5, 4)))]);
        // the colour key (black): in images without alpha only
        let mut jobs = ImageJobs::start(paths, Some([0, 0, 0]));
        let px = jobs.poll(true, |p| p.px[3]);
        assert_eq!(
            px.iter().map(|p| p.1).collect::<Vec<_>>(),
            [Some(255), None, Some(0)]
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
