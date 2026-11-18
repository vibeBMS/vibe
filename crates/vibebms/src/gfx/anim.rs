//! Animated images (GIF, WebP, APNG): every frame decoded to RGBA and kept
//! in memory, shown at its own frame times on one texture that is updated
//! when the frame changes (so an animation costs the GPU one image).
//!
//! Frames are held within a budget: each animation at most `PER_ANIM`
//! bytes and all animations together at most `ALL_ANIMS`. An animation
//! larger than its share keeps every second frame (each shown as long as
//! the two were), then every fourth and so on, so it plays at its own speed
//! with fewer frames; one that cannot keep two frames is a still image.

use super::{Gfx, TexId};
use std::sync::atomic::{AtomicUsize, Ordering};

const SMALL: bool = cfg!(any(
    target_arch = "wasm32",
    target_os = "android",
    target_os = "ios"
));
const MB: usize = 1 << 20;
/// The frames one animation keeps (bytes of RGBA).
const PER_ANIM: usize = if SMALL { 32 * MB } else { 128 * MB };
/// The frames all animations keep together.
const ALL_ANIMS: usize = if SMALL { 96 * MB } else { 512 * MB };

/// Bytes of frames the animations alive hold.
static HELD: AtomicUsize = AtomicUsize::new(0);

/// An animation's frames (all `w`×`h` RGBA, composed) and their times.
pub struct Animation {
    pub w: u32,
    pub h: u32,
    frames: Vec<Vec<u8>>,
    /// When each frame ends, in ms from the start of a loop.
    ends: Vec<u32>,
    /// How many times it plays (`None`: forever).
    plays: Option<u32>,
    bytes: usize,
}

impl Drop for Animation {
    fn drop(&mut self) {
        HELD.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

impl Animation {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn frame(&self, i: usize) -> &[u8] {
        &self.frames[i]
    }

    /// One loop's length in ms.
    pub fn loop_ms(&self) -> u32 {
        self.ends.last().copied().unwrap_or(0)
    }

    /// The frame shown `t_ms` after the animation started: it loops, or
    /// after its last play stays on its last frame.
    pub fn frame_at(&self, t_ms: i64) -> usize {
        let len = self.loop_ms() as i64;
        if t_ms <= 0 || len == 0 {
            return 0;
        }
        if self.plays.is_some_and(|n| t_ms >= len * n as i64) {
            return self.frames.len() - 1;
        }
        let t = (t_ms % len) as u32;
        self.ends
            .partition_point(|&e| e <= t)
            .min(self.frames.len() - 1)
    }

    /// The animation of `frames` (pixels, ms each) taking `bytes`.
    fn new(w: u32, h: u32, frames: Vec<(Vec<u8>, u32)>, plays: Option<u32>) -> Animation {
        let bytes = frames.iter().map(|(f, _)| f.len()).sum();
        HELD.fetch_add(bytes, Ordering::Relaxed);
        let mut end = 0u32;
        let (frames, ends) = frames
            .into_iter()
            .map(|(f, ms)| {
                end = end.saturating_add(ms);
                (f, end)
            })
            .unzip();
        Animation {
            w,
            h,
            frames,
            ends,
            plays,
            bytes,
        }
    }
}

/// What an animation of this size may keep now.
fn budget() -> usize {
    ALL_ANIMS
        .saturating_sub(HELD.load(Ordering::Relaxed))
        .min(PER_ANIM)
}

/// A frame's time as browsers show it: GIF and WebP frames of 10 ms or less
/// last 100 ms (most such files were made for that), APNG ones at least 10.
fn frame_ms(delay: image::Delay, gif_like: bool) -> u32 {
    let (n, d) = delay.numer_denom_ms();
    let ms = (n as u64).checked_div(d as u64).unwrap_or(0) as u32;
    match gif_like {
        true if ms <= 10 => 100,
        _ => ms.max(10),
    }
}

/// The frames of an animated GIF, WebP or APNG in `bytes`, each passed
/// through `key` (transparency keys); `None` if it is not one, has one
/// frame, or is too large to keep two.
pub fn decode(bytes: &[u8], key: &dyn Fn(&mut [u8])) -> Option<Animation> {
    decode_within(bytes, key, budget())
}

fn decode_within(bytes: &[u8], key: &dyn Fn(&mut [u8]), budget: usize) -> Option<Animation> {
    use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
    use image::metadata::LoopCount;
    use image::{AnimationDecoder, ImageDecoder};
    let cur = || std::io::Cursor::new(bytes);
    let ((w, h), count, frames, gif_like) = if bytes.starts_with(b"GIF8") {
        let d = GifDecoder::new(cur()).ok()?;
        (d.dimensions(), d.loop_count(), d.into_frames(), true)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let d = PngDecoder::new(cur()).ok()?;
        if !d.is_apng().ok()? {
            return None;
        }
        let size = d.dimensions();
        let d = d.apng().ok()?;
        (size, d.loop_count(), d.into_frames(), false)
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let d = WebPDecoder::new(cur()).ok()?;
        if !d.has_animation() {
            return None;
        }
        (d.dimensions(), d.loop_count(), d.into_frames(), true)
    } else {
        return None;
    };
    let frame_bytes = (w as usize * h as usize * 4).max(1);
    if frame_bytes * 2 > budget {
        return None;
    }
    let max_frames = budget / frame_bytes;
    // (pixels, ms) of the frames kept: every `stride`-th, each lasting
    // until the next kept one
    let mut kept: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut stride = 1usize;
    for (i, f) in frames.enumerate() {
        let Ok(f) = f else { break };
        let ms = frame_ms(f.delay(), gif_like);
        if i % stride != 0 {
            if let Some(last) = kept.last_mut() {
                last.1 = last.1.saturating_add(ms);
            }
            continue;
        }
        let buf = f.into_buffer();
        if buf.dimensions() != (w, h) {
            break;
        }
        let mut px = buf.into_raw();
        key(&mut px);
        kept.push((px, ms));
        if kept.len() > max_frames {
            // every second one of those kept, and of those to come
            kept = kept
                .chunks_mut(2)
                .map(|pair| {
                    let ms = pair.iter().map(|p| p.1).fold(0u32, u32::saturating_add);
                    (std::mem::take(&mut pair[0].0), ms)
                })
                .collect();
            stride *= 2;
        }
    }
    if kept.len() < 2 {
        return None;
    }
    let plays = match count {
        LoopCount::Infinite => None,
        LoopCount::Finite(n) => Some(n.get()),
    };
    Some(Animation::new(w, h, kept, plays))
}

/// An image on the GPU, animated if its file is: `tex` shows the frame due.
pub struct ImageTex {
    pub tex: TexId,
    anim: Option<Animation>,
    shown: usize,
    since: crate::platform::Instant,
}

impl ImageTex {
    pub fn new(gfx: &mut Gfx, pic: super::Picture) -> ImageTex {
        let tex = gfx.create_texture(pic.w, pic.h, &pic.px);
        ImageTex {
            tex,
            anim: pic.anim,
            shown: 0,
            since: crate::platform::Instant::now(),
        }
    }

    /// An image file's texture (`key`: see `load_picture`).
    pub fn load(gfx: &mut Gfx, path: &std::path::Path, key: &dyn Fn(&mut [u8])) -> Option<Self> {
        Some(ImageTex::new(gfx, super::load_picture(path, key)?))
    }

    pub fn is_animated(&self) -> bool {
        self.anim.is_some()
    }

    /// Show the frame due `t_ms` after the animation started.
    pub fn show(&mut self, gfx: &Gfx, t_ms: i64) {
        let Some(a) = &self.anim else { return };
        let i = a.frame_at(t_ms);
        if i != self.shown {
            self.shown = i;
            gfx.update_texture(self.tex, 0, 0, a.w, a.h, a.frame(i));
        }
    }

    /// Show the frame due now, the animation started when the image loaded.
    pub fn tick(&mut self, gfx: &Gfx) {
        if self.anim.is_some() {
            self.show(gfx, self.since.elapsed().as_millis() as i64);
        }
    }

    pub fn free(self, gfx: &mut Gfx) {
        gfx.free_texture(self.tex);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{Delay, Frame, Rgba, RgbaImage};

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    /// An animated GIF of `frames` (colour, ms), looping forever.
    pub fn gif(w: u32, h: u32, frames: &[([u8; 4], u32)]) -> Vec<u8> {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut out = Vec::new();
        {
            let mut e = GifEncoder::new(&mut out);
            e.set_repeat(Repeat::Infinite).unwrap();
            e.encode_frames(frames.iter().map(|(c, ms)| {
                Frame::from_parts(solid(w, h, *c), 0, 0, Delay::from_numer_denom_ms(*ms, 1))
            }))
            .unwrap();
        }
        out
    }

    /// An APNG of `frames` (colour, ms) played `plays` times (0: forever).
    pub fn apng(w: u32, h: u32, frames: &[([u8; 4], u16)], plays: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.set_animated(frames.len() as u32, plays).unwrap();
            let mut wr = e.write_header().unwrap();
            for (c, ms) in frames {
                wr.set_frame_delay(*ms, 1000).unwrap();
                wr.write_image_data(solid(w, h, *c).as_raw()).unwrap();
            }
            wr.finish().unwrap();
        }
        out
    }

    /// A lossless WebP of one image, and the VP8L chunk inside it.
    fn webp_still(img: &RgbaImage) -> (Vec<u8>, Vec<u8>) {
        let mut out = Vec::new();
        image_webp::WebPEncoder::new(&mut out)
            .encode(
                img.as_raw(),
                img.width(),
                img.height(),
                image_webp::ColorType::Rgba8,
            )
            .unwrap();
        // RIFF size WEBP, then the one VP8L chunk
        assert_eq!(&out[12..16], b"VP8L");
        let chunk = out[12..].to_vec();
        (out, chunk)
    }

    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend((data.len() as u32).to_le_bytes());
        c.extend(data);
        if data.len() % 2 == 1 {
            c.push(0);
        }
        c
    }

    fn u24(v: u32) -> [u8; 3] {
        let b = v.to_le_bytes();
        [b[0], b[1], b[2]]
    }

    /// An animated WebP of `frames` (colour, ms), looping forever: a VP8X
    /// header, ANIM and one ANMF per frame around lossless images.
    pub fn webp_anim(w: u32, h: u32, frames: &[([u8; 4], u32)]) -> Vec<u8> {
        let mut vp8x = vec![0x02 | 0x10, 0, 0, 0]; // animation, alpha
        vp8x.extend(u24(w - 1));
        vp8x.extend(u24(h - 1));
        let mut body = b"WEBP".to_vec();
        body.extend(chunk(b"VP8X", &vp8x));
        body.extend(chunk(b"ANIM", &[0, 0, 0, 0, 0, 0]));
        for (c, ms) in frames {
            let (_, vp8l) = webp_still(&solid(w, h, *c));
            let mut f = Vec::new();
            f.extend(u24(0));
            f.extend(u24(0));
            f.extend(u24(w - 1));
            f.extend(u24(h - 1));
            f.extend(u24(*ms));
            f.push(0b10); // no blending, no disposal
            f.extend(vp8l);
            body.extend(chunk(b"ANMF", &f));
        }
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];

    fn colours(a: &Animation) -> Vec<[u8; 4]> {
        (0..a.len())
            .map(|i| a.frame(i)[..4].try_into().unwrap())
            .collect()
    }

    #[test]
    fn gif_frames_at_their_own_times() {
        let b = gif(4, 3, &[(RED, 100), (GREEN, 200), (BLUE, 50)]);
        let a = decode(&b, &|_| {}).expect("animated");
        assert_eq!((a.w, a.h, a.len()), (4, 3, 3));
        assert_eq!(colours(&a), [RED, GREEN, BLUE]);
        assert_eq!(a.loop_ms(), 350);
        let at = |t| a.frame_at(t);
        assert_eq!(
            [at(-5), at(0), at(99), at(100), at(299), at(300)],
            [0, 0, 0, 1, 1, 2]
        );
        // it loops
        assert_eq!([at(350), at(460), at(700 + 320)], [0, 1, 2]);
    }

    #[test]
    fn short_gif_frames_last_100_ms() {
        let b = gif(2, 2, &[(RED, 0), (GREEN, 10), (BLUE, 20)]);
        let a = decode(&b, &|_| {}).unwrap();
        assert_eq!(a.loop_ms(), 220);
    }

    #[test]
    fn a_one_frame_gif_is_still() {
        let b = gif(2, 2, &[(RED, 100)]);
        assert!(decode(&b, &|_| {}).is_none());
        let p = crate::gfx::load_picture_bytes(&b, &|_| {}).unwrap();
        assert!(p.anim.is_none());
        assert_eq!(&p.px[..4], &RED);
    }

    #[test]
    fn apng_frames_and_play_count() {
        let b = apng(3, 3, &[(RED, 40), (GREEN, 60)], 2);
        let a = decode(&b, &|_| {}).expect("animated");
        assert_eq!(colours(&a), [RED, GREEN]);
        assert_eq!(a.loop_ms(), 100);
        assert_eq!([a.frame_at(30), a.frame_at(50), a.frame_at(130)], [0, 1, 0]);
        // after two plays it stays on its last frame
        assert_eq!([a.frame_at(200), a.frame_at(10_000)], [1, 1]);
        // a PNG that is not animated is still
        let mut still = Vec::new();
        solid(2, 2, BLUE)
            .write_to(
                &mut std::io::Cursor::new(&mut still),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert!(decode(&still, &|_| {}).is_none());
    }

    #[test]
    fn webp_still_and_animated() {
        let (still, _) = webp_still(&solid(5, 4, GREEN));
        assert!(decode(&still, &|_| {}).is_none());
        let p = crate::gfx::load_picture_bytes(&still, &|_| {}).expect("WebP decodes");
        assert_eq!((p.w, p.h), (5, 4));
        assert_eq!(&p.px[..4], &GREEN);
        let b = webp_anim(5, 4, &[(RED, 80), (BLUE, 120)]);
        let p = crate::gfx::load_picture_bytes(&b, &|_| {}).expect("animated WebP decodes");
        let a = p.anim.as_ref().expect("animated");
        assert_eq!(colours(a), [RED, BLUE]);
        assert_eq!(a.loop_ms(), 200);
        assert_eq!(&p.px[..4], &RED);
    }

    #[test]
    fn keys_apply_to_every_frame() {
        let b = gif(
            2,
            2,
            &[([0, 0, 0, 255], 100), (RED, 100), ([0, 0, 0, 255], 100)],
        );
        let a = decode(&b, &crate::gfx::key_black).unwrap();
        assert_eq!(a.frame(0)[3], 0);
        assert_eq!(a.frame(1)[3], 255);
        assert_eq!(a.frame(2)[3], 0);
    }

    #[test]
    fn over_budget_keeps_fewer_frames_at_the_same_speed() {
        let cols = [RED, GREEN, BLUE, RED, GREEN, BLUE, RED];
        let frames: Vec<_> = cols.iter().map(|c| (*c, 100)).collect();
        let b = gif(8, 8, &frames);
        let one = 8 * 8 * 4;
        // room for 3 frames: every second one is kept, then every fourth
        let a = decode_within(&b, &|_| {}, one * 3).unwrap();
        assert_eq!(colours(&a), [RED, GREEN]);
        assert_eq!(a.loop_ms(), 700);
        assert_eq!(
            [a.frame_at(399), a.frame_at(400), a.frame_at(699)],
            [0, 1, 1]
        );
        let a = decode_within(&b, &|_| {}, one * 4).unwrap();
        assert_eq!(colours(&a), [RED, BLUE, GREEN, RED]);
        assert_eq!(a.loop_ms(), 700);
        // no room for two: a still image
        assert!(decode_within(&b, &|_| {}, one * 2 - 1).is_none());
        assert_eq!(decode_within(&b, &|_| {}, one * 100).unwrap().len(), 7);
    }
}
