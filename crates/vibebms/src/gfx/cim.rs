//! libGDX "CIM" images (beatoraja skins ship them, e.g. LITONE5 for
//! beatoraja): a zlib stream of width, height and pixel format (big-endian
//! 32-bit), then the raw pixels.

use std::io::Read;

/// Gdx2DPixmap formats: bytes per pixel.
fn bpp(format: i32) -> Option<usize> {
    match format {
        1 => Some(1), // alpha
        2 => Some(2), // luminance + alpha
        3 => Some(3), // RGB888
        4 => Some(4), // RGBA8888
        5 => Some(2), // RGB565
        6 => Some(2), // RGBA4444
        _ => None,
    }
}

fn header(bytes: &[u8]) -> Option<(flate2::read::ZlibDecoder<&[u8]>, u32, u32, i32)> {
    // a zlib stream: CMF 0x78, and the header checksum
    if bytes.len() < 2
        || bytes[0] != 0x78
        || (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 31 != 0
    {
        return None;
    }
    let mut z = flate2::read::ZlibDecoder::new(bytes);
    let mut h = [0u8; 12];
    z.read_exact(&mut h).ok()?;
    let n = |i: usize| i32::from_be_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    let (w, hgt, f) = (n(0), n(4), n(8));
    if w <= 0 || hgt <= 0 || w > 16384 || hgt > 16384 {
        return None;
    }
    bpp(f)?;
    Some((z, w as u32, hgt as u32, f))
}

pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    header(bytes).map(|(_, w, h, _)| (w, h))
}

/// RGBA8 pixels.
pub fn decode(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (mut z, w, h, f) = header(bytes)?;
    let n = w as usize * h as usize;
    let mut raw = vec![0u8; n * bpp(f)?];
    z.read_exact(&mut raw).ok()?;
    let rgba = match f {
        4 => raw,
        3 => raw
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        1 => raw.iter().flat_map(|&a| [255, 255, 255, a]).collect(),
        2 => raw
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        5 => raw
            .chunks_exact(2)
            .flat_map(|p| {
                let v = u16::from_be_bytes([p[0], p[1]]);
                let c = |x: u16, bits: u32| ((x as u32 * 255) / ((1 << bits) - 1)) as u8;
                [c(v >> 11, 5), c((v >> 5) & 63, 6), c(v & 31, 5), 255]
            })
            .collect(),
        _ => raw
            .chunks_exact(2)
            .flat_map(|p| {
                let v = u16::from_be_bytes([p[0], p[1]]);
                let c = |x: u16| (x & 15) as u8 * 17;
                [c(v >> 12), c(v >> 8), c(v >> 4), c(v)]
            })
            .collect(),
    };
    Some((w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn decodes_rgb888() {
        let mut body = Vec::new();
        for v in [2i32, 1, 3] {
            body.extend(v.to_be_bytes());
        }
        body.extend([10, 20, 30, 40, 50, 60]);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&body).unwrap();
        let bytes = z.finish().unwrap();
        assert_eq!(size(&bytes), Some((2, 1)));
        let (w, h, px) = decode(&bytes).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(px, vec![10, 20, 30, 255, 40, 50, 60, 255]);
        assert!(size(b"\x89PNG....").is_none());
    }
}
