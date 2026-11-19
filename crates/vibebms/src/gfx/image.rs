//! Image files decoded to RGBA (an animated one with all its frames).

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

/// An image (an animation's first frame), DDS and CIM too.
fn load_rgba_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if bytes.starts_with(b"DDS ") {
        return crate::formats::dds::decode(bytes);
    }
    if let Some(img) = crate::formats::cim::decode(bytes) {
        return Some(img);
    }
    // TGA has no signature to recognise it by
    let img = match image::load_from_memory(bytes).or_else(|e| {
        image::load_from_memory_with_format(bytes, image::ImageFormat::Tga).map_err(|_| e)
    }) {
        Ok(i) => i,
        Err(e) => {
            log::debug!("image decode: {e}");
            return None;
        }
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}
