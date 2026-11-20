//! Image files decoded to RGBA (an animated one with all its frames).

use image::DynamicImage;

/// The largest image that is decoded: no side longer than [`MAX_SIDE`] (as
/// `.dds` and `.cim` files were kept to; what a GPU cannot hold is scaled
/// down for it, `Gfx::create_texture`) and no more than [`MAX_PIXELS`] in
/// all, 256 MiB as RGBA: an 8192×8192 sheet, four times the largest that
/// skins are known to bring. A file whose header says more is not decoded,
/// whatever its format and however small it is.
pub const MAX_SIDE: u32 = 16_384;
pub const MAX_PIXELS: u64 = 1 << 26;

/// Whether a `w`×`h` image is one that is decoded.
pub fn fits(w: u32, h: u32) -> bool {
    w <= MAX_SIDE && h <= MAX_SIDE && w as u64 * h as u64 <= MAX_PIXELS
}

/// An image file's pixels (`w`×`h` RGBA; an animation's first frame) and,
/// if it is animated, its frames.
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
    pub anim: Option<super::anim::Animation>,
}

/// An image file, animated GIF / WebP / APNG with its frames, each frame
/// passed through `key` (transparency keys such as `key_black`).
pub fn load_picture(path: &std::path::Path, key: &dyn Fn(&mut [u8])) -> Option<Picture> {
    let bytes = match crate::platform::vfs::read_shared(path) {
        Ok(b) => b,
        Err(e) => {
            log::debug!("image {}: {e}", path.display());
            return None;
        }
    };
    load_picture_bytes(&bytes, key)
}

pub fn load_picture_bytes(bytes: &[u8], key: &dyn Fn(&mut [u8])) -> Option<Picture> {
    if let Some(a) = super::anim::decode(bytes, key) {
        return Some(Picture {
            w: a.w,
            h: a.h,
            px: a.frame(0).to_vec(),
            anim: Some(a),
        });
    }
    let (w, h, mut px) = load_rgba_bytes(bytes)?;
    key(&mut px);
    Some(Picture {
        w,
        h,
        px,
        anim: None,
    })
}

/// Pure black made transparent (the BMS BGA layer convention).
pub fn key_black(px: &mut [u8]) {
    for p in px.as_chunks_mut::<4>().0 {
        if p[..3] == [0, 0, 0] {
            p[3] = 0;
        }
    }
}

/// `bytes` decoded as `format` (`None`: the one its signature says), if
/// the size in its header [`fits`].
fn decode(bytes: &[u8], format: Option<image::ImageFormat>) -> image::ImageResult<DynamicImage> {
    use image::error::{ImageError, LimitError, LimitErrorKind};
    use image::ImageDecoder;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    match format {
        Some(f) => reader.set_format(f),
        None => reader = reader.with_guessed_format()?,
    }
    let decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    if !fits(w, h) {
        let kind = LimitErrorKind::DimensionError;
        return Err(ImageError::Limits(LimitError::from_kind(kind)));
    }
    DynamicImage::from_decoder(decoder)
}

/// An image (an animation's first frame), DDS and CIM too.
fn load_rgba_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.starts_with(b"DDS ") {
        return crate::formats::dds::decode(bytes);
    }
    if let Some(img) = crate::formats::cim::decode(bytes) {
        return Some(img);
    }
    // TGA has no signature to recognise it by
    let tga = Some(image::ImageFormat::Tga);
    let img = match decode(bytes, None).or_else(|e| decode(bytes, tga).map_err(|_| e)) {
        Ok(i) => i,
        Err(e) => {
            log::debug!("image decode: {e}");
            return None;
        }
    };
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::GrayImage::new(w, h)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn an_image_past_the_limits_is_not_decoded() {
        assert!(fits(MAX_SIDE, 4096) && fits(8192, 8192));
        assert!(!fits(MAX_SIDE + 1, 1) && !fits(1, MAX_SIDE + 1));
        // few bytes of file, and more pixels than any image has
        assert!(!fits(8192, 8193) && !fits(23_170, 23_170));
        let ok = load_picture_bytes(&grey_png(MAX_SIDE, 1), &|_| {}).unwrap();
        assert_eq!(
            (ok.w, ok.h, ok.px.len()),
            (MAX_SIDE, 1, MAX_SIDE as usize * 4)
        );
        assert!(load_picture_bytes(&grey_png(MAX_SIDE + 1, 1), &|_| {}).is_none());
        assert!(load_picture_bytes(&grey_png(1, MAX_SIDE + 1), &|_| {}).is_none());
    }
}
