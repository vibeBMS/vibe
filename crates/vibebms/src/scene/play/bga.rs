//! The chart's BGA images (animated ones too) and videos on the GPU, and
//! its stage file and back image.

use super::session::Session;
use crate::gfx::{Gfx, ImageTex, Picture, TexId};
use crate::platform::Instant;
use crate::video::Video;
use std::path::PathBuf;

/// How long before a video is first shown its decoder is started (µs of
/// chart time), so that its first frames are there in time.
const VIDEO_LEAD_US: i64 = 2_000_000;

/// A chart's BGA files of one kind (its pictures, its videos): each file
/// once, whatever number of the chart's images are it.
pub(super) struct Files<T> {
    pub files: Vec<T>,
    /// By chart image: which of `files` it is.
    pub of: Vec<Option<usize>>,
}

impl<T> Default for Files<T> {
    fn default() -> Self {
        Files {
            files: Vec::new(),
            of: Vec::new(),
        }
    }
}

impl<T> Files<T> {
    /// The file chart image `image` is.
    fn get(&self, image: u32) -> Option<&T> {
        self.files.get((*self.of.get(image as usize)?)?)
    }

    /// When each file was last first shown: the latest of the times its
    /// chart images first were (`starts`, by chart image).
    fn started(&self, starts: &[Option<i64>]) -> Vec<Option<i64>> {
        let mut out = vec![None; self.files.len()];
        for (k, start) in self.of.iter().zip(starts) {
            if let Some(o) = k.and_then(|k| out.get_mut(k)) {
                *o = (*o).max(*start);
            }
        }
        out
    }
}

#[derive(Default)]
pub(super) struct Bga {
    /// The chart's pictures on the GPU (an animated one plays from when it
    /// was last first shown) and its videos (`None`: not one that plays).
    tex: Files<ImageTex>,
    video: Files<Option<Video>>,
    /// The chart's BGA events whose videos were told to start: up to here.
    ahead: usize,
    pub stage: Option<TexId>,
    /// `#BACKBMP`, else the stage file.
    pub back: Option<TexId>,
    /// The stage file and back image, animated from when the play loaded.
    still: Vec<ImageTex>,
}

impl Bga {
    pub fn new(
        gfx: &mut Gfx,
        images: Files<Picture>,
        videos: &Files<PathBuf>,
        stagefile: Option<Picture>,
        backbmp: Option<Picture>,
    ) -> Bga {
        let stage = stagefile.map(|p| ImageTex::new(gfx, p));
        let back = backbmp.map(|p| ImageTex::new(gfx, p));
        let (stage_tex, back_tex) = (stage.as_ref().map(|t| t.tex), back.as_ref().map(|t| t.tex));
        let pictures = images.files.into_iter();
        Bga {
            tex: Files {
                files: pictures.map(|p| ImageTex::new(gfx, p)).collect(),
                of: images.of,
            },
            // (a video is read and decoded from when it is about to be shown)
            video: Files {
                files: videos.files.iter().map(|p| Video::open(p, false)).collect(),
                of: videos.of.clone(),
            },
            ahead: 0,
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
        while let Some(b) = s.chart.bga.get(self.ahead) {
            if b.time > t + VIDEO_LEAD_US {
                break;
            }
            let k = b.image.and_then(|i| *self.video.of.get(i as usize)?);
            if let Some(Some(v)) = k.and_then(|k| self.video.files.get_mut(k)) {
                v.prepare();
            }
            self.ahead += 1;
        }
        let starts = self.video.started(&s.bga_start);
        for (v, start) in self.video.files.iter_mut().zip(starts) {
            if let (Some(v), Some(start)) = (v, start) {
                v.update(gfx, (t - start) / 1000);
            }
        }
        let starts = self.tex.started(&s.bga_start);
        for (img, start) in self.tex.files.iter_mut().zip(starts) {
            if let Some(start) = start {
                img.show(gfx, (t - start) / 1000);
            }
        }
    }

    /// The layers' textures to show at `now`, with their opacity (the POOR
    /// layer replaces them briefly; a NONSTOP course's stages cross-fade).
    pub fn textures(&self, s: &Session, now: Instant) -> Vec<(TexId, f32)> {
        let poor = s.poor_until.is_some_and(|u| now < u) && s.bga[3].is_some();
        let layers: &[usize] = if poor { &[3] } else { &[0, 1, 2] };
        let tex = |i: u32| match self.video.get(i) {
            Some(Some(v)) => v.texture(),
            _ => self.tex.get(i).map(|t| t.tex),
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
        let tex = std::mem::take(&mut self.tex).files;
        for t in tex.into_iter().chain(self.still.drain(..)) {
            t.free(gfx);
        }
        for mut v in std::mem::take(&mut self.video).files.into_iter().flatten() {
            v.free(gfx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_plays_from_when_one_of_its_images_was_last_first_shown() {
        let f = Files {
            files: vec!['a', 'b', 'c'],
            of: vec![Some(0), None, Some(1), Some(0), Some(2), Some(0)],
        };
        assert_eq!(
            (f.get(3), f.get(1), f.get(4), f.get(9)),
            (Some(&'a'), None, Some(&'c'), None)
        );
        let starts = [Some(500), Some(9), None, Some(700), None, None];
        assert_eq!(f.started(&starts), [Some(700), None, None]);
    }
}
