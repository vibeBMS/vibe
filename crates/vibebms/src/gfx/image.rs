//! Image files decoded to RGBA.

pub fn load_rgba(path: &std::path::Path, color_key_black: bool) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = match crate::platform::vfs::read(path) {
        Ok(b) => b,
        Err(e) => {
            log::debug!("image {}: {e}", path.display());
            return None;
        }
    };
    load_rgba_bytes(&bytes, color_key_black)
}

pub fn load_rgba_bytes(bytes: &[u8], color_key_black: bool) -> Option<(u32, u32, Vec<u8>)> {
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
    let mut rgba = img.to_rgba8();
    if color_key_black {
        for p in rgba.pixels_mut() {
            if p[0] == 0 && p[1] == 0 && p[2] == 0 {
                p[3] = 0;
            }
        }
    }
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}
