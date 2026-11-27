//! A skin's images while it loads: measured, within what a load may take.

use super::load::{Includes, MEDIA_MAX};
use super::model::Src;
use std::path::PathBuf;

/// A skin that asked for more than a load may take (includes, animation
/// frames) is not loaded.
pub(super) fn within_limits(
    path: &std::path::Path,
    includes: &Includes,
    images: &ImageFiles,
) -> Result<(), String> {
    let what = match (includes.over, images.frames_left.get()) {
        (true, _) => "includes",
        (_, None) => "animation frames",
        _ => return Ok(()),
    };
    Err(format!("{}: too many {what}", path.display()))
}

/// Pixel size of an image file (through the VFS).
pub fn image_size(p: &std::path::Path) -> Option<(u32, u32)> {
    let video = crate::gfx::video::is_video(p);
    if !crate::platform::vfs::readable(p, if video { u64::MAX } else { MEDIA_MAX }) {
        return None;
    }
    if video {
        return crate::gfx::video::size(p);
    }
    // the start of the file says, but for the odd JPEG with a large
    // preview ahead of its frame header
    const HEAD: usize = 64 << 10;
    let head = crate::platform::vfs::read_head(p, HEAD).ok()?;
    image_size_of(&head, p).or_else(|| {
        let whole = (head.len() == HEAD).then(|| crate::platform::vfs::read(p).ok())??;
        image_size_of(&whole, p)
    })
}

/// Animation frames the objects of a skin may have in all (one object:
/// [`Src::grid`]'s 65,536).
const FRAMES: usize = 1 << 20;

/// A skin's image files while it loads, each measured when something first
/// draws from it (a skin may list images for options not chosen, or
/// videos, which take a process to measure).
pub(super) struct ImageFiles<'a> {
    paths: Vec<Option<PathBuf>>,
    sizes: Vec<std::cell::OnceCell<Option<(u32, u32)>>>,
    pub(super) measure: &'a dyn Fn(&std::path::Path) -> Option<(u32, u32)>,
    /// Frames the skin may still cut its images into (`None`: it asked
    /// for more than [`FRAMES`]).
    frames_left: std::cell::Cell<Option<usize>>,
}

impl<'a> ImageFiles<'a> {
    pub(super) fn new(measure: &'a dyn Fn(&std::path::Path) -> Option<(u32, u32)>) -> Self {
        ImageFiles {
            paths: Vec::new(),
            sizes: Vec::new(),
            measure,
            frames_left: Some(FRAMES).into(),
        }
    }

    /// [`Src::grid`], within the frames the skin may have in all.
    pub(super) fn grid(&self, rect: [f32; 4], div_x: i64, div_y: i64) -> Option<Vec<[f32; 4]>> {
        let frames = Src::grid(rect, div_x, div_y);
        let left = self.frames_left.get()?.checked_sub(frames.len());
        self.frames_left.set(left);
        left.map(|_| frames)
    }

    pub(super) fn len(&self) -> usize {
        self.paths.len()
    }

    pub(super) fn push(&mut self, p: Option<PathBuf>) {
        self.paths.push(p);
        self.sizes.push(Default::default());
    }

    /// An image measured already: kept only when it is there.
    pub(super) fn push_measured(&mut self, p: PathBuf, size: Option<(u32, u32)>) {
        self.paths.push(size.map(|_| p));
        self.sizes.push(size.into());
    }

    pub(super) fn size(&self, i: usize) -> Option<(u32, u32)> {
        let p = self.paths.get(i)?.as_deref();
        *self.sizes[i].get_or_init(|| p.and_then(self.measure))
    }

    /// The paths, without the images found missing.
    pub(super) fn finish(self) -> Vec<Option<PathBuf>> {
        self.paths
            .into_iter()
            .zip(self.sizes)
            .map(|(p, s)| p.filter(|_| s.get() != Some(&None)))
            .collect()
    }
}

fn image_size_of(b: &[u8], p: &std::path::Path) -> Option<(u32, u32)> {
    if let Some(s) = crate::formats::dds::size(b).or_else(|| crate::formats::cim::size(b)) {
        return Some(s);
    }
    let r = image::ImageReader::new(std::io::Cursor::new(b))
        .with_guessed_format()
        .ok()?;
    if r.format().is_some() {
        return r.into_dimensions().ok();
    }
    // TGA has no signature: trust the extension
    let tga = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tga"));
    tga.then(|| {
        image::ImageReader::with_format(std::io::Cursor::new(b), image::ImageFormat::Tga)
            .into_dimensions()
            .ok()
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sizes_from_the_header_or_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("vibebms-imgsize-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbImage::new(300, 200);
        let (png, jpg) = (dir.join("a.png"), dir.join("a.jpg"));
        img.save(&png).unwrap();
        // a JPEG whose frame header is past the first 64 KiB
        let mut b = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Jpeg)
            .unwrap();
        let mut big = b[..2].to_vec();
        for _ in 0..2 {
            big.extend([0xFF, 0xE1, 0xFF, 0xFF]);
            big.resize(big.len() + 0xFFFD, 0);
        }
        big.extend(&b[2..]);
        std::fs::write(&jpg, big).unwrap();
        assert_eq!(image_size(&png), Some((300, 200)));
        assert_eq!(image_size(&jpg), Some((300, 200)));
        assert_eq!(image_size(&dir.join("none.png")), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
