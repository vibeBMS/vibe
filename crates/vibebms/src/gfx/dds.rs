//! DirectDraw Surface (`.dds`) textures as LR2 skins use them: uncompressed
//! RGB(A) with channel masks, and DXT1 / DXT3 / DXT5. First mip level only.

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// (width, height) of a DDS file.
pub fn size(b: &[u8]) -> Option<(u32, u32)> {
    (b.get(0..4)? == b"DDS ").then(|| Some((u32_at(b, 16)?, u32_at(b, 12)?)))?
}

/// Decode to RGBA8.
pub fn decode(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = size(b)?;
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return None;
    }
    let flags = u32_at(b, 80)?;
    let four = b.get(84..88)?;
    let data = b.get(128..)?;
    let (w_, h_) = (w as usize, h as usize);
    if flags & 0x4 != 0 {
        let block = match four {
            b"DXT1" => 8,
            b"DXT2" | b"DXT3" | b"DXT4" | b"DXT5" => 16,
            _ => return None,
        };
        let mut out = vec![0u8; w_ * h_ * 4];
        let (bw, bh) = (w_.div_ceil(4), h_.div_ceil(4));
        for by in 0..bh {
            for bx in 0..bw {
                let o = (by * bw + bx) * block;
                let blk = data.get(o..o + block)?;
                let px = match four {
                    b"DXT1" => bc1(blk, true),
                    b"DXT2" | b"DXT3" => bc2(blk),
                    _ => bc3(blk),
                };
                for (i, p) in px.iter().enumerate() {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    if x < w_ && y < h_ {
                        out[(y * w_ + x) * 4..][..4].copy_from_slice(p);
                    }
                }
            }
        }
        return Some((w, h, out));
    }
    // uncompressed, described by channel masks
    let bits = u32_at(b, 88)? as usize;
    let bpp = bits / 8;
    if !(2..=4).contains(&bpp) {
        return None;
    }
    let masks = [
        u32_at(b, 92)?,
        u32_at(b, 96)?,
        u32_at(b, 100)?,
        u32_at(b, 104)?,
    ];
    let has_alpha = flags & 0x1 != 0 && masks[3] != 0;
    let chan = |v: u32, m: u32| -> u8 {
        if m == 0 {
            return 0;
        }
        let shift = m.trailing_zeros();
        let max = m >> shift;
        (((v & m) >> shift) * 255 / max) as u8
    };
    let mut out = Vec::with_capacity(w_ * h_ * 4);
    for i in 0..w_ * h_ {
        let p = data.get(i * bpp..i * bpp + bpp)?;
        let mut v = 0u32;
        for (k, byte) in p.iter().enumerate() {
            v |= (*byte as u32) << (8 * k);
        }
        out.extend_from_slice(&[
            chan(v, masks[0]),
            chan(v, masks[1]),
            chan(v, masks[2]),
            if has_alpha { chan(v, masks[3]) } else { 255 },
        ]);
    }
    Some((w, h, out))
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = (c >> 11) & 31;
    let g = (c >> 5) & 63;
    let b = c & 31;
    [
        (r * 255 / 31) as u8,
        (g * 255 / 63) as u8,
        (b * 255 / 31) as u8,
    ]
}

/// A colour block: 16 pixels. `one_bit_alpha` = DXT1 rules (transparent
/// fourth colour when c0 <= c1).
fn bc1(b: &[u8], one_bit_alpha: bool) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u16, wb: u16| ((a as u16 * wa + b as u16 * wb) / (wa + wb)) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || !one_bit_alpha {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 2, 1);
            pal[3][k] = mix(p0[k], p1[k], 1, 2);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 1, 1);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let idx = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| pal[((idx >> (2 * i)) & 3) as usize])
}

fn bc2(b: &[u8]) -> [[u8; 4]; 16] {
    let mut px = bc1(&b[8..16], false);
    for (i, p) in px.iter_mut().enumerate() {
        let a = (b[i / 2] >> (4 * (i % 2))) & 0xf;
        p[3] = a * 17;
    }
    px
}

fn bc3(b: &[u8]) -> [[u8; 4]; 16] {
    let mut px = bc1(&b[8..16], false);
    let (a0, a1) = (b[0] as u16, b[1] as u16);
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for k in 1..7u16 {
            pal[k as usize + 1] = (((7 - k) * a0 + k * a1) / 7) as u8;
        }
    } else {
        for k in 1..5u16 {
            pal[k as usize + 1] = (((5 - k) * a0 + k * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for (k, byte) in b[2..8].iter().enumerate() {
        bits |= (*byte as u64) << (8 * k);
    }
    for (i, p) in px.iter_mut().enumerate() {
        p[3] = pal[((bits >> (3 * i)) & 7) as usize];
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(w: u32, h: u32, flags: u32, four: &[u8; 4], bits: u32, masks: [u32; 4]) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0..4].copy_from_slice(b"DDS ");
        b[4..8].copy_from_slice(&124u32.to_le_bytes());
        b[12..16].copy_from_slice(&h.to_le_bytes());
        b[16..20].copy_from_slice(&w.to_le_bytes());
        b[76..80].copy_from_slice(&32u32.to_le_bytes());
        b[80..84].copy_from_slice(&flags.to_le_bytes());
        b[84..88].copy_from_slice(four);
        b[88..92].copy_from_slice(&bits.to_le_bytes());
        for (k, m) in masks.iter().enumerate() {
            b[92 + 4 * k..96 + 4 * k].copy_from_slice(&m.to_le_bytes());
        }
        b
    }

    #[test]
    fn uncompressed_bgra() {
        let mut b = header(
            2,
            1,
            0x41,
            &[0; 4],
            32,
            [0xff0000, 0xff00, 0xff, 0xff00_0000],
        );
        b.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // B G R A
        let (w, h, px) = decode(&b).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(px, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn dxt1_block() {
        let mut b = header(4, 4, 0x4, b"DXT1", 0, [0; 4]);
        // c0 = red, c1 = blue; first pixel index 0, second 1, rest 0
        b.extend_from_slice(&0xf800u16.to_le_bytes());
        b.extend_from_slice(&0x001fu16.to_le_bytes());
        b.extend_from_slice(&[0b0000_0100, 0, 0, 0]);
        let (_, _, px) = decode(&b).unwrap();
        assert_eq!(&px[0..4], &[255, 0, 0, 255]);
        assert_eq!(&px[4..8], &[0, 0, 255, 255]);
    }
}
