//! Texts: in the UI's fonts, or in the skin's bitmap fonts.

use super::LoadedSkin;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::{Draw, Rect};
use crate::skin::*;

impl TextFx {
    /// The distance-field style (beatoraja's shader uniforms).
    pub fn sdf(&self) -> crate::gfx::SdfStyle {
        crate::gfx::SdfStyle {
            outline_color: self.outline_color,
            shadow_color: self.shadow_color,
            shadow_offset: [self.shadow_offset.0, self.shadow_offset.1],
            outline_distance: (0.5 - self.outline_width / 2.0).max(0.1),
            shadow_smoothing: self.shadow_smoothness / 2.0,
        }
    }
}

/// Screen px per font px of a BMFont text drawn into a rect `h` high
/// with font size `size` (screen px): size over the font's own size as in
/// beatoraja, else so a line fills the rect.
pub fn bmfont_scale(f: &BitmapFont, h: f32, size: Option<f32>) -> f32 {
    match size {
        Some(s) if f.em > 0.0 => s / f.em,
        _ => h / f.size,
    }
}

impl LoadedSkin {
    /// Draw a text into `r` (font size = height; see `Kind::Text` overflow),
    /// with an LR2 bitmap font when one is given. `shown_ms`: how long the
    /// text has been the current one, for a marquee (overflow 3; `None`
    /// keeps it still). `fx`: its outline and shadow as beatoraja draws
    /// them (see `TextFx`); `size`: the beatoraja font size (skin px),
    /// which sizes `.fnt` glyphs; `scale`: screen px per skin px.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        d: &mut Draw,
        s: &str,
        r: Rect,
        align: i32,
        overflow: i32,
        color: crate::gfx::Color,
        font: Option<usize>,
        shown_ms: Option<i64>,
        fx: &TextFx,
        size: Option<f32>,
        scale: f32,
    ) {
        let args = (align, overflow, font, shown_ms, size.map(|s| s * scale));
        if font
            .and_then(|i| self.skin.fonts.get(i))
            .is_some_and(|f| f.sdf)
        {
            // the shader draws the outline and shadow
            d.b.set_sdf(Some(fx.sdf()));
            self.text_plain(d, s, r, color, args);
            d.b.set_sdf(None);
            return;
        }
        if fx.has_shadow() {
            let (dx, dy) = (fx.shadow_offset.0 * scale, fx.shadow_offset.1 * scale);
            let half = [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]];
            self.text_plain(d, s, Rect::new(r.x + dx, r.y + dy, r.w, r.h), half, args);
        }
        self.text_plain(d, s, r, color, args);
    }

    /// `text` without outline or shadow; `args`: align, overflow, font,
    /// shown_ms, font size (screen px).
    fn text_plain(
        &self,
        d: &mut Draw,
        s: &str,
        r: Rect,
        color: crate::gfx::Color,
        args: (i32, i32, Option<usize>, Option<i64>, Option<f32>),
    ) {
        let (align, overflow, font, shown_ms, size) = args;
        // lines one under the other (beatoraja's fonts: 1.2 sizes apart)
        if s.contains('\n') {
            for (i, line) in s.split('\n').enumerate() {
                let y = r.y + r.h * 1.2 * i as f32;
                let lr = Rect::new(r.x, y, r.w, r.h);
                self.text_plain(d, line, lr, color, args);
            }
            return;
        }
        if let Some(f) = font.and_then(|i| self.skin.fonts.get(i)) {
            self.bitmap_text(d, f, s, r, align, color, size);
            return;
        }
        let (x, al) = match align {
            1 => (r.x + r.w * 0.5, Align::Center),
            2 => (r.right(), Align::Right),
            _ => (r.x, Align::Left),
        };
        let size = r.h.max(4.0);
        match overflow {
            1 if r.w > 1.0 => {
                d.text(s, x, r.y, TextStyle::new(size, color, al).fit(r.w));
            }
            2 if r.w > 1.0 => {
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, x, r.y, TextStyle::new(size, color, al));
                d.b.set_clip(None);
            }
            // vibeBMS: a long text shows its start, cut off; while it is
            // the current one it scrolls to its end and back (a marquee)
            3 if r.w > 1.0 => {
                let w = d.measure(s, size);
                if w <= r.w {
                    d.text(s, x, r.y, TextStyle::new(size, color, al));
                    return;
                }
                let off = shown_ms.map_or(0.0, |t| marquee_offset(t, w - r.w, size));
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, r.x - off, r.y, TextStyle::new(size, color, Align::Left));
                d.b.set_clip(None);
            }
            _ => {
                d.text(s, x, r.y, TextStyle::new(size, color, al));
            }
        }
    }

    /// LR2 bitmap text as beatoraja draws it: glyphs scaled to the rect
    /// height, the line shrunk to the rect width if wider, aligned around
    /// the anchor (left edge / centre / right edge of the rect).
    #[allow(clippy::too_many_arguments)]
    fn bitmap_text(
        &self,
        d: &mut Draw,
        f: &BitmapFont,
        s: &str,
        r: Rect,
        align: i32,
        color: crate::gfx::Color,
        size: Option<f32>,
    ) {
        let chars: Vec<char> = s.chars().filter(|c| f.glyphs.contains_key(c)).collect();
        let glyphs: Vec<&(usize, [f32; 4])> =
            chars.iter().filter_map(|c| f.glyphs.get(c)).collect();
        if glyphs.is_empty() || f.size <= 0.0 {
            return;
        }
        let k = r.h / f.size;
        if !f.metrics.is_empty() {
            self.bmfont_text(d, f, &chars, r, align, color, size);
            return;
        }
        let width = glyphs.iter().map(|g| g.1[2]).sum::<f32>() * k + f.margin * glyphs.len() as f32;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.clamp(0, 2) as f32 * 0.5;
        let mut x = r.x + r.w * a - width * scale * a;
        for (img, g) in glyphs {
            let tw = g[2] * scale * k;
            if let Some(t) = self.texture(*img) {
                if let Some((w, h)) = d.gfx.texture_size(t) {
                    let (w, h) = (w as f32, h as f32);
                    let uv = [g[0] / w, g[1] / h, (g[0] + g[2]) / w, (g[1] + g[3]) / h];
                    d.b.quad(t, Rect::new(x, r.y, tw, r.h), uv, color);
                }
            }
            x += tw + f.margin * scale;
        }
    }

    /// A BMFont text: glyphs placed by their offsets and advances, scaled
    /// as beatoraja does by the text's font size over the font's own (else
    /// so a line fills the rect's height).
    #[allow(clippy::too_many_arguments)]
    fn bmfont_text(
        &self,
        d: &mut Draw,
        f: &BitmapFont,
        chars: &[char],
        r: Rect,
        align: i32,
        color: crate::gfx::Color,
        size: Option<f32>,
    ) {
        let k = bmfont_scale(f, r.h, size);
        let adv = |c: &char| f.metrics.get(c).map(|m| m[2]).unwrap_or(0.0);
        let width = chars.iter().map(adv).sum::<f32>() * k;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.clamp(0, 2) as f32 * 0.5;
        let mut x = r.x + r.w * a - width * scale * a;
        for c in chars {
            let (Some((img, g)), Some(m)) = (f.glyphs.get(c), f.metrics.get(c)) else {
                continue;
            };
            if let Some(t) = self.texture(*img) {
                if let Some((w, h)) = d.gfx.texture_size(t) {
                    let (w, h) = (w as f32, h as f32);
                    let uv = [g[0] / w, g[1] / h, (g[0] + g[2]) / w, (g[1] + g[3]) / h];
                    let gr = Rect::new(
                        x + m[0] * k * scale,
                        r.y + m[1] * k,
                        g[2] * k * scale,
                        g[3] * k,
                    );
                    d.b.quad(t, gr, uv, color);
                }
            }
            x += m[2] * k * scale;
        }
    }
}

/// How far a marquee has scrolled (px) `t` ms after it started: it waits,
/// scrolls `travel` px, waits at the end, and starts again.
pub fn marquee_offset(t: i64, travel: f32, size: f32) -> f32 {
    const WAIT_MS: f32 = 1200.0;
    let speed = (size * 2.5).max(40.0); // px/s
    let scroll_ms = travel / speed * 1000.0;
    let t = t.max(0) as f32 % (WAIT_MS * 2.0 + scroll_ms);
    if t < WAIT_MS {
        0.0
    } else {
        ((t - WAIT_MS) / 1000.0 * speed).min(travel)
    }
}
