//! Image and archive formats skins come in: DirectDraw surfaces (LR2),
//! libGDX `.cim` images (beatoraja) and DxLib `.dxa` archives (LR2).

pub mod cim;
pub mod dds;
pub mod dxa;

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
