//! How a skin sets its texts and numbers: styles and bitmap fonts.

use super::types::{Align, Overflow};
use std::collections::HashMap;
use std::path::PathBuf;

/// How a text is set.
#[derive(Clone, Debug, Default)]
pub struct TextStyle {
    pub align: Align,
    pub overflow: Overflow,
    /// Bitmap font (index into `Skin::fonts`: LR2, BMFont), else the UI
    /// font.
    pub font: Option<usize>,
    /// The skin's own font file (beatoraja TrueType / OpenType fonts).
    pub face: Option<PathBuf>,
    /// beatoraja outline and shadow (none in LR2).
    pub fx: TextFx,
    /// beatoraja font size (skin px): `.fnt` glyphs are scaled by it
    /// over the font's own size. `None`: by the destination height.
    pub size: Option<f32>,
}

/// How a number is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NumberStyle {
    /// Images of a set: 10 (0-9), 11 (and padding), 12 (signed: 24, a
    /// positive and a negative set of 0-9, padding, sign).
    pub digits: usize,
    /// Places (1 to [`NumberStyle::PLACES`]).
    pub keta: usize,
    pub align: Align,
    /// Glyph drawn in leading places (`None` = blank).
    pub pad: Option<usize>,
}

impl NumberStyle {
    /// The places a number may have.
    pub const PLACES: usize = 32;
}

/// beatoraja text `outlineColor`, `outlineWidth`, `shadowColor`,
/// `shadowOffsetX` / `Y` and `shadowSmoothness`. beatoraja draws them as
/// follows, and so does vibe: TrueType and plain bitmap fonts only have
/// the shadow, the text at the offset (display px) with its colour
/// halved; distance-field bitmap fonts (`.fnt` of type 1 / 2) have all of
/// it, the offset in texels of the glyph page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextFx {
    pub outline_color: [f32; 4],
    pub outline_width: f32,
    pub shadow_color: [f32; 4],
    pub shadow_offset: (f32, f32),
    pub shadow_smoothness: f32,
}

impl Default for TextFx {
    fn default() -> TextFx {
        // beatoraja's defaults: "ffffff00" colours, nothing else
        TextFx {
            outline_color: [1.0, 1.0, 1.0, 0.0],
            outline_width: 0.0,
            shadow_color: [1.0, 1.0, 1.0, 0.0],
            shadow_offset: (0.0, 0.0),
            shadow_smoothness: 0.0,
        }
    }
}

impl TextFx {
    pub fn has_shadow(&self) -> bool {
        self.shadow_offset != (0.0, 0.0)
    }
}

/// An LR2 bitmap font: glyph images at a design height.
#[derive(Clone, Debug, Default)]
pub struct BitmapFont {
    /// Design height of the glyphs (px).
    pub size: f32,
    /// Extra space after each glyph (px at design height; may be negative).
    pub margin: f32,
    /// Glyph: image (index into `Skin::images`) and its rect there.
    pub glyphs: HashMap<char, (usize, [f32; 4])>,
    /// Placement of a glyph (x offset, y offset from the top, advance) in
    /// fonts that give it (BMFont `.fnt`); else glyphs sit side by side.
    pub metrics: HashMap<char, [f32; 3]>,
    /// Glyph images are distance fields (beatoraja font `type` 1 / 2).
    pub sdf: bool,
    /// The `.fnt` font's own size (its `info size`), which beatoraja
    /// scales by the text's size; 0 for LR2 fonts.
    pub em: f32,
}
