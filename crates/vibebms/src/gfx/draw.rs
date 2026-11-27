//! What a scene draws with: the textures, the text, the batch of quads.

use super::text::{Text, TextStyle};
use super::{Batch, Gfx, Rect, TexId};

pub struct Draw<'a> {
    pub gfx: &'a Gfx,
    pub text: &'a mut Text,
    pub b: &'a mut Batch,
    /// Where the mouse is (`Ctx`'s `input.pointer`), for skins' hover images.
    pub pointer: Option<(f32, f32)>,
}

impl Draw<'_> {
    pub fn text(&mut self, s: &str, x: f32, y: f32, style: TextStyle) -> f32 {
        self.text.draw(self.gfx, self.b, s, x, y, style)
    }
    pub fn measure(&mut self, s: &str, size: f32) -> f32 {
        self.text.measure(self.gfx, s, size)
    }
    /// Draw a texture letterboxed into `r`.
    pub fn image_fit(&mut self, tex: TexId, r: Rect, alpha: f32) {
        let Some((w, h)) = self.gfx.texture_size(tex) else {
            return;
        };
        let s = (r.w / w as f32).min(r.h / h as f32);
        let (dw, dh) = (w as f32 * s, h as f32 * s);
        self.b.quad(
            tex,
            Rect::new(r.x + (r.w - dw) * 0.5, r.y + (r.h - dh) * 0.5, dw, dh),
            [0.0, 0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0, alpha],
        );
    }
    /// Draw a texture covering `r` (cropped).
    pub fn image_cover(&mut self, tex: TexId, r: Rect, alpha: f32) {
        let Some((w, h)) = self.gfx.texture_size(tex) else {
            return;
        };
        let (tw, th) = (w as f32, h as f32);
        let s = (r.w / tw).max(r.h / th);
        let (uw, uh) = (r.w / (tw * s), r.h / (th * s));
        let (u0, v0) = ((1.0 - uw) * 0.5, (1.0 - uh) * 0.5);
        self.b
            .quad(tex, r, [u0, v0, u0 + uw, v0 + uh], [1.0, 1.0, 1.0, alpha]);
    }
}
