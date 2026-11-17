//! The chart's BGA images and videos on the GPU, and its stage file.

use super::loading::Rgba;
use super::session::Session;
use crate::gfx::{Gfx, TexId};
use crate::platform::Instant;
use crate::video::Video;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct Bga {
    /// By chart image: its texture, or its video.
    tex: Vec<Option<TexId>>,
    video: Vec<Option<Video>>,
    pub stage: Option<TexId>,
}

impl Bga {
    pub fn new(
        gfx: &mut Gfx,
        images: Vec<Option<Rgba>>,
        videos: &[Option<PathBuf>],
        stagefile: Option<Rgba>,
    ) -> Bga {
        Bga {
            tex: images
                .into_iter()
                .map(|i| i.map(|(w, h, px)| gfx.create_texture(w, h, &px)))
                .collect(),
            video: videos
                .iter()
                .map(|v| v.as_deref().and_then(|p| Video::open(p, false)))
                .collect(),
            stage: stagefile.map(|(w, h, px)| gfx.create_texture(w, h, &px)),
        }
    }

    /// The videos shown at `now`, each from the chart time it was first
    /// shown.
    pub fn update(&mut self, gfx: &mut Gfx, s: &Session, now: Instant) {
        if self.video.is_empty() {
            return;
        }
        let t = s.chart_us(now);
        for (v, start) in self.video.iter_mut().zip(&s.bga_start) {
            if let (Some(v), Some(start)) = (v, start) {
                v.update(gfx, (t - start) / 1000);
            }
        }
    }

    /// The layers' textures to show at `now` (the POOR layer replaces them
    /// briefly).
    pub fn textures<'a>(
        &'a self,
        s: &'a Session,
        now: Instant,
    ) -> impl Iterator<Item = TexId> + 'a {
        let poor = s.poor_until.is_some_and(|u| now < u) && s.bga[3].is_some();
        let layers: &[usize] = if poor { &[3] } else { &[0, 1, 2] };
        layers.iter().filter_map(|l| {
            let i = s.bga[*l]? as usize;
            match self.video.get(i) {
                Some(Some(v)) => v.texture(),
                _ => self.tex.get(i).copied().flatten(),
            }
        })
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        for t in self.tex.drain(..).flatten().chain(self.stage.take()) {
            gfx.free_texture(t);
        }
        for mut v in self.video.drain(..).flatten() {
            v.free(gfx);
        }
    }
}
