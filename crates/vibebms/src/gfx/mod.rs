//! Minimal, fast 2D renderer on wgpu: one textured-quad pipeline, batched by
//! texture. Works on Vulkan / Metal / DX12 / GL(ES).

mod batch;
mod capture;
mod device;
mod image;
pub mod text;
mod texture;

pub use self::image::{load_rgba, load_rgba_bytes};
pub use batch::{Batch, Blend, SdfStyle};
pub use device::Gfx;

pub type TexId = usize;
/// Texture 0 is the glyph atlas; its top-left 4x4 pixels are opaque white.
pub const ATLAS: TexId = 0;
pub const WHITE_UV: [f32; 4] = [0.5 / 2048.0, 0.5 / 2048.0, 1.5 / 2048.0, 1.5 / 2048.0];
pub const ATLAS_SIZE: u32 = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

pub type Color = [f32; 4];

pub const fn rgba(hex: u32) -> Color {
    [
        ((hex >> 24) & 0xff) as f32 / 255.0,
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}
