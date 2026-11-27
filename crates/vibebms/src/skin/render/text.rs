//! Texts: in the UI's fonts, or in the skin's bitmap fonts.

use super::LoadedSkin;
use crate::gfx::text as screen;
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
    /// Draw text `s` in `style` into `fr`'s rect (screen px; font size =
    /// its height), in `fr`'s colour. `shown_ms`: how long the text has
    /// been the current one, for a marquee (`None` keeps it still);
    /// `scale`: screen px per skin px (the font size and the shadow's
    /// offset are in skin px).
    pub fn text(
        &self,
        d: &mut Draw,
        s: &str,
        style: &TextStyle,
        fr: &Frame,
        shown_ms: Option<i64>,
        scale: f32,
    ) {
        let size = style.size.map(|s| s * scale);
        let fx = &style.fx;
        if (style.font)
            .and_then(|i| self.skin.fonts.get(i))
            .is_some_and(|f| f.sdf)
        {
            // the shader draws the outline and shadow
            d.b.set_sdf(Some(fx.sdf()));
            self.text_plain(d, s, style, fr, shown_ms, size);
            d.b.set_sdf(None);
            return;
        }
        if fx.has_shadow() {
            let (r, color) = (fr.rect, fr.color);
            let (dx, dy) = (fx.shadow_offset.0 * scale, fx.shadow_offset.1 * scale);
            let shadow = Frame {
                rect: Rect::new(r.x + dx, r.y + dy, r.w, r.h),
                color: [color[0] * 0.5, color[1] * 0.5, color[2] * 0.5, color[3]],
                ..*fr
            };
            self.text_plain(d, s, style, &shadow, shown_ms, size);
        }
        self.text_plain(d, s, style, fr, shown_ms, size);
    }

    /// `text` without outline or shadow; `size`: the font size (screen
    /// px) if it has one.
    fn text_plain(
        &self,
        d: &mut Draw,
        s: &str,
        style: &TextStyle,
        fr: &Frame,
        shown_ms: Option<i64>,
        size: Option<f32>,
    ) {
        let (r, color) = (fr.rect, fr.color);
        // lines one under the other (beatoraja's fonts: 1.2 sizes apart)
        if s.contains('\n') {
            for (i, line) in s.split('\n').enumerate() {
                let y = r.y + r.h * 1.2 * i as f32;
                let lr = Frame {
                    rect: Rect::new(r.x, y, r.w, r.h),
                    ..*fr
                };
                self.text_plain(d, line, style, &lr, shown_ms, size);
            }
            return;
        }
        if let Some(f) = style.font.and_then(|i| self.skin.fonts.get(i)) {
            self.bitmap_text(d, f, s, fr, style.align, size);
            return;
        }
        let (x, al) = match style.align {
            Align::Center => (r.x + r.w * 0.5, screen::Align::Center),
            Align::Right => (r.right(), screen::Align::Right),
            Align::Left => (r.x, screen::Align::Left),
        };
        let size = r.h.max(4.0);
        let look = |a| screen::TextStyle::new(size, color, a);
        match style.overflow {
            Overflow::Shrink if r.w > 1.0 => {
                d.text(s, x, r.y, look(al).fit(r.w));
            }
            Overflow::Cut if r.w > 1.0 => {
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, x, r.y, look(al));
                d.b.set_clip(None);
            }
            // a long text shows its start, cut off; while it is the
            // current one it scrolls to its end and back
            Overflow::Marquee if r.w > 1.0 => {
                let w = d.measure(s, size);
                if w <= r.w {
                    d.text(s, x, r.y, look(al));
                    return;
                }
                let off = shown_ms.map_or(0.0, |t| marquee_offset(t, w - r.w, size));
                d.b.set_clip(Some(Rect::new(r.x, r.y - size, r.w, size * 3.0)));
                d.text(s, r.x - off, r.y, look(screen::Align::Left));
                d.b.set_clip(None);
            }
            _ => {
                d.text(s, x, r.y, look(al));
            }
        }
    }

    /// LR2 bitmap text as beatoraja draws it: glyphs scaled to the rect
    /// height, the line shrunk to the rect width if wider, aligned around
    /// the anchor (left edge / centre / right edge of the rect).
    fn bitmap_text(
        &self,
        d: &mut Draw,
        f: &BitmapFont,
        s: &str,
        fr: &Frame,
        align: Align,
        size: Option<f32>,
    ) {
        let (r, color) = (fr.rect, fr.color);
        let chars: Vec<char> = s.chars().filter(|c| f.glyphs.contains_key(c)).collect();
        let glyphs: Vec<&(usize, [f32; 4])> =
            chars.iter().filter_map(|c| f.glyphs.get(c)).collect();
        if glyphs.is_empty() || f.size <= 0.0 {
            return;
        }
        let k = r.h / f.size;
        if !f.metrics.is_empty() {
            self.bmfont_text(d, f, &chars, fr, align, size);
            return;
        }
        let width = glyphs.iter().map(|g| g.1[2]).sum::<f32>() * k + f.margin * glyphs.len() as f32;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.share();
        let mut x = r.x + r.w * a - width * scale * a;
        for (img, g) in glyphs {
            let tw = g[2] * scale * k;
            if let Some((t, uv)) = self.uv(*img, *g) {
                d.b.quad(t, Rect::new(x, r.y, tw, r.h), uv, color);
            }
            x += tw + f.margin * scale;
        }
    }

    /// A BMFont text: glyphs placed by their offsets and advances, scaled
    /// as beatoraja does by the text's font size over the font's own (else
    /// so a line fills the rect's height).
    fn bmfont_text(
        &self,
        d: &mut Draw,
        f: &BitmapFont,
        chars: &[char],
        fr: &Frame,
        align: Align,
        size: Option<f32>,
    ) {
        let (r, color) = (fr.rect, fr.color);
        let k = bmfont_scale(f, r.h, size);
        let adv = |c: &char| f.metrics.get(c).map(|m| m[2]).unwrap_or(0.0);
        let width = chars.iter().map(adv).sum::<f32>() * k;
        let scale = if r.w > 0.0 && r.w < width {
            r.w / width
        } else {
            1.0
        };
        let a = align.share();
        let mut x = r.x + r.w * a - width * scale * a;
        for c in chars {
            let (Some((img, g)), Some(m)) = (f.glyphs.get(c), f.metrics.get(c)) else {
                continue;
            };
            if let Some((t, uv)) = self.uv(*img, *g) {
                let gr = Rect::new(
                    x + m[0] * k * scale,
                    r.y + m[1] * k,
                    g[2] * k * scale,
                    g[3] * k,
                );
                d.b.quad(t, gr, uv, color);
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
