//! A skin with its textures: loading them, and drawing it.

use super::Hooks;
use super::Xf;
use crate::gfx::{Draw, Gfx, ImageJobs, ImageTex, Rect, TexId};
use crate::skin::script::Overlay;
use crate::skin::*;

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
    /// Something drawn was missing one of `lazy_images` (see
    /// `draw_objects`).
    pub(super) missing: std::cell::Cell<bool>,
    /// Images being decoded, with the images that show each file; the
    /// first is the skin's own, which it is not drawn without.
    decoding: Vec<(ImageJobs, Vec<Vec<usize>>)>,
    /// The skin's own images are in: it is drawn.
    ready: bool,
    /// Images that are videos (looping from when the skin loaded).
    videos: Vec<(usize, crate::gfx::video::Video)>,
    /// Animated images (GIF, WebP, APNG), from when the skin loaded; their
    /// textures are in `tex`.
    anims: Vec<crate::gfx::ImageTex>,
    loaded_at: crate::platform::Instant,
}

impl LoadedSkin {
    pub fn load(skin: Skin, gfx: &mut Gfx) -> LoadedSkin {
        let mut sk = LoadedSkin {
            skin,
            tex: Vec::new(),
            lazy: Vec::new(),
            wanted: Default::default(),
            missing: Default::default(),
            decoding: Vec::new(),
            ready: false,
            videos: Vec::new(),
            anims: Vec::new(),
            loaded_at: crate::platform::Instant::now(),
        };
        sk.start();
        sk.take_decoded(gfx, false);
        sk
    }

    /// Start decoding the images drawn from (in the background) and open
    /// the videos.
    fn start(&mut self) {
        let skin = &self.skin;
        let mut lazy = vec![false; skin.images.len()];
        let sheets = skin
            .fonts
            .iter()
            .flat_map(|f| f.glyphs.values().map(|(img, _)| img));
        for i in sheets.chain(&skin.lazy_images) {
            if let Some(l) = lazy.get_mut(*i) {
                *l = true;
            }
        }
        // only what is drawn, and an image several sources name once
        let used = skin.used_images();
        let mut files: Vec<PathBuf> = Vec::new();
        let mut users: Vec<Vec<usize>> = Vec::new();
        let mut file_of: HashMap<&PathBuf, usize> = HashMap::new();
        for (i, p) in skin.images.iter().enumerate() {
            let Some(p) = p.as_ref().filter(|_| !lazy[i] && used[i]) else {
                continue;
            };
            if crate::gfx::video::is_video(p) {
                if let Some(v) = crate::gfx::video::Video::open(p, true) {
                    self.videos.push((i, v));
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
        self.tex = vec![None; skin.images.len()];
        self.lazy = lazy;
        self.wanted.get_mut().clear();
        self.decoding = vec![(ImageJobs::start(files, skin.trans_color), users)];
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
        // (neither drawn nor loading: freed)
        if !self.ready && self.decoding.is_empty() {
            self.start();
        }
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

    /// Free the textures and videos (the skin is not drawn); the next
    /// [`Self::load_pending`] loads them again.
    pub fn free(&mut self, gfx: &mut Gfx) {
        self.decoding.clear();
        self.ready = false;
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
    pub(super) fn texture(&self, i: usize) -> Option<TexId> {
        let t = self.tex.get(i).copied().flatten();
        if t.is_none() {
            let unasked = self.lazy.get(i) == Some(&true);
            if unasked {
                let mut w = self.wanted.borrow_mut();
                if !w.contains(&i) {
                    w.push(i);
                }
            }
            let decoding = || {
                self.decoding
                    .iter()
                    .any(|(_, u)| u.iter().flatten().any(|&j| j == i))
            };
            if self.skin.lazy_images.contains(&i) && (unasked || decoding()) {
                self.missing.set(true);
            }
        }
        t
    }

    pub(super) fn uv(&self, d: &Draw, src: &Src, f: [f32; 4]) -> Option<(TexId, [f32; 4])> {
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
        if let Some(n) = &self.skin.note {
            n.drawn();
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
}
