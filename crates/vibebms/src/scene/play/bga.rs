//! The chart's BGA images (animated ones too) and videos on the GPU, and
//! its stage file and back image.

use super::session::Session;
use crate::gfx::{Gfx, ImageTex, Picture, TexId};
use crate::platform::Instant;
use crate::video::Video;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct Bga {
    /// By chart image: its texture (animated from when it was first
    /// shown), or its video.
    tex: Vec<Option<ImageTex>>,
    video: Vec<Option<Video>>,
    pub stage: Option<TexId>,
    /// `#BACKBMP`, else the stage file.
    pub back: Option<TexId>,
    /// The stage file and back image, animated from when the play loaded.
    still: Vec<ImageTex>,
}

impl Bga {
    pub fn new(
        gfx: &mut Gfx,
        images: Vec<Option<Picture>>,
        videos: &[Option<PathBuf>],
        stagefile: Option<Picture>,
        backbmp: Option<Picture>,
    ) -> Bga {
        let stage = stagefile.map(|p| ImageTex::new(gfx, p));
        let back = backbmp.map(|p| ImageTex::new(gfx, p));
        let (stage_tex, back_tex) = (stage.as_ref().map(|t| t.tex), back.as_ref().map(|t| t.tex));
        Bga {
            tex: images
                .into_iter()
                .map(|i| i.map(|p| ImageTex::new(gfx, p)))
                .collect(),
            video: videos
                .iter()
                .map(|v| v.as_deref().and_then(|p| Video::open(p, false)))
                .collect(),
            stage: stage_tex,
            back: back_tex.or(stage_tex),
            still: stage.into_iter().chain(back).collect(),
        }
    }

    /// The videos and animated images shown at `now`, each from the chart
    /// time it was first shown.
    pub fn update(&mut self, gfx: &mut Gfx, s: &Session, now: Instant) {
        for t in &mut self.still {
            t.tick(gfx);
        }
        let t = s.chart_us(now);
        for (v, start) in self.video.iter_mut().zip(&s.bga_start) {
            if let (Some(v), Some(start)) = (v, start) {
                v.update(gfx, (t - start) / 1000);
            }
        }
        for (img, start) in self.tex.iter_mut().zip(&s.bga_start) {
            if let (Some(img), Some(start)) = (img, start) {
                img.show(gfx, (t - start) / 1000);
            }
        }
    }

    /// The layers' textures to show at `now`, with their opacity (the POOR
    /// layer replaces them briefly; a NONSTOP course's stages cross-fade).
    pub fn textures(&self, s: &Session, now: Instant) -> Vec<(TexId, f32)> {
        let poor = s.poor_until.is_some_and(|u| now < u) && s.bga[3].is_some();
        let layers: &[usize] = if poor { &[3] } else { &[0, 1, 2] };
        let tex = |i: u32| match self.video.get(i as usize) {
            Some(Some(v)) => v.texture(),
            _ => self
                .tex
                .get(i as usize)
                .and_then(|t| t.as_ref())
                .map(|t| t.tex),
        };
        let shown = |bga: &[Option<u32>; 4], alpha: f32| {
            layers
                .iter()
                .filter_map(move |l| Some((tex(bga[*l]?)?, alpha)))
                .collect::<Vec<_>>()
        };
        match &s.nonstop {
            Some(ns) if !poor => {
                let t = s.chart_us(now);
                s.stage_bga
                    .iter()
                    .enumerate()
                    .map(|(k, bga)| (bga, ns.bga_alpha(k, t)))
                    .filter(|(_, a)| *a > 0.0)
                    .flat_map(|(bga, a)| shown(bga, a))
                    .collect()
            }
            _ => shown(&s.bga, 1.0),
        }
    }

    pub fn free(&mut self, gfx: &mut Gfx) {
        self.stage = None;
        self.back = None;
        for t in self.tex.drain(..).flatten().chain(self.still.drain(..)) {
            t.free(gfx);
        }
        for mut v in self.video.drain(..).flatten() {
            v.free(gfx);
        }
    }
}
