//! A font face drawn glyph by glyph: an outline is read from the font file
//! when its glyph is drawn, nothing is parsed up front (a CJK font has some
//! 65 000 glyphs, of which a screen draws a few hundred), and the faces of
//! one file share its bytes.
//!
//! Glyphs come out as fontdue drew them (the renderer vibeBMS used before):
//! the same bounds, bitmap sizes and advances, the outline's curves split
//! into lines as fontdue split them, and the lines' coverage summed by
//! `ab_glyph_rasterizer`.

use std::sync::Arc;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// A font file's bytes, shared by the faces read from it.
pub enum Bytes {
    Static(&'static [u8]),
    Owned(Vec<u8>),
    /// A system font mapped from its file: its pages are the file's, read
    /// when drawn with, and the system can drop them again (Android counts
    /// an app's own memory, not mapped files).
    #[cfg(not(target_arch = "wasm32"))]
    Mapped(memmap2::Mmap),
    /// Held by fontdb.
    Shared(Arc<dyn AsRef<[u8]> + Send + Sync>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Static(b) => b,
            Bytes::Owned(b) => b,
            #[cfg(not(target_arch = "wasm32"))]
            Bytes::Mapped(m) => m,
            Bytes::Shared(b) => (**b).as_ref(),
        }
    }
}

impl Bytes {
    /// A system font file: mapped (read when it cannot be).
    pub fn system_file(path: &std::path::Path) -> Option<Bytes> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(file) = std::fs::File::open(path) {
            // SAFETY: a system font is not written while it is in use (fontdb
            // maps the same files to read their names)
            if let Ok(m) = unsafe { memmap2::Mmap::map(&file) } {
                return Some(Bytes::Mapped(m));
            }
        }
        std::fs::read(path).ok().map(Bytes::Owned)
    }
}

/// Where a glyph's bitmap goes, as fontdue's `Metrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// The bitmap's left edge from the pen, in whole pixels.
    pub xmin: i32,
    /// The bitmap's bottom edge from the baseline (up), in whole pixels.
    pub ymin: i32,
    pub width: usize,
    pub height: usize,
    pub advance_width: f32,
}

/// One face of a font file.
pub struct Font {
    data: Arc<Bytes>,
    index: u32,
    units_per_em: f32,
    ascender: f32,
}

impl Font {
    /// Face `index` of `data`; `None` when it is no font.
    pub fn new(data: Arc<Bytes>, index: u32) -> Option<Font> {
        let face = Face::parse(&data, index).ok()?;
        let units_per_em = face.units_per_em() as f32;
        let ascender = face.ascender() as f32;
        Some(Font {
            data,
            index,
            units_per_em,
            ascender,
        })
    }

    /// The file's bytes (shared with its other faces).
    #[cfg(test)]
    pub fn data(&self) -> &Arc<Bytes> {
        &self.data
    }

    /// The face's tables. Reading them anew each time takes under a
    /// microsecond (the table directory and headers; outlines and character
    /// maps are read where they are), and keeps no borrow of `data`.
    fn face(&self) -> Option<Face<'_>> {
        Face::parse(&self.data, self.index).ok()
    }

    /// The glyph `c` is drawn with, 0 (none) when the font lacks it. As
    /// fontdue maps characters: the last character map table that has it.
    fn glyph_of(face: &Face, c: char) -> u16 {
        let Some(cmap) = face.tables().cmap else {
            return 0;
        };
        let glyphs = face.number_of_glyphs();
        (0..cmap.subtables.len())
            .rev()
            .filter_map(|i| cmap.subtables.get(i))
            // (not a glyph the font has: as missing)
            .find_map(|t| t.glyph_index(c as u32).filter(|g| g.0 != 0 && g.0 < glyphs))
            .map_or(0, |g| g.0)
    }

    pub fn has(&self, c: char) -> bool {
        self.face().is_some_and(|f| Font::glyph_of(&f, c) != 0)
    }

    /// The height above the baseline lines leave for glyphs at `px`.
    pub fn ascent(&self, px: f32) -> f32 {
        self.ascender * (px / self.units_per_em)
    }

    /// `c` drawn `px` pixels to the em: its metrics and coverage, rows from
    /// the top. A character the font lacks is drawn as its glyph 0 (the
    /// "missing" box), as fontdue draws it.
    pub fn rasterize(&self, c: char, px: f32) -> (Metrics, Vec<u8>) {
        let Some(face) = self.face().filter(|_| px > 0.0) else {
            return (Metrics::default(), Vec::new());
        };
        let glyph = GlyphId(Font::glyph_of(&face, c));
        let mut outline = Outline::new(self.units_per_em);
        face.outline_glyph(glyph, &mut outline);
        let advance = face.glyph_hor_advance(glyph).unwrap_or(0) as f32;
        let b = outline.bounds();
        let scale = px / self.units_per_em;
        // fontdue's metrics: the bounds scaled, the bitmap the outline
        // covers at their fraction of a pixel
        let [xmin, ymin, width, height] = [b.xmin, b.ymin, b.xmax - b.xmin, b.ymax - b.ymin];
        let [xmin, ymin, width, height] =
            [xmin * scale, ymin * scale, width * scale, height * scale];
        let mut offset_x = fract(xmin);
        let mut offset_y = fract(1.0 - fract(height) - fract(ymin));
        if offset_x.is_sign_negative() {
            offset_x += 1.0;
        }
        if offset_y.is_sign_negative() {
            offset_y += 1.0;
        }
        let m = Metrics {
            xmin: xmin.floor() as i32,
            ymin: ymin.floor() as i32,
            width: (width + offset_x).ceil() as usize,
            height: (height + offset_y).ceil() as usize,
            advance_width: scale * advance,
        };
        if m.width == 0 || m.height == 0 {
            return (m, vec![0; m.width * m.height]);
        }
        let mut raster = ab_glyph_rasterizer::Rasterizer::new(m.width, m.height);
        let at = |x: f32, y: f32| {
            ab_glyph_rasterizer::point(
                (x - b.xmin) * scale + offset_x,
                (b.ymax - y) * scale + offset_y,
            )
        };
        for &[x0, y0, x1, y1] in &outline.lines {
            raster.draw_line(at(x0, y0), at(x1, y1));
        }
        let mut bitmap = vec![0; m.width * m.height];
        raster.for_each_pixel(|i, a| bitmap[i] = (a * 255.9).clamp(0.0, 255.0) as u8);
        (m, bitmap)
    }
}

/// fontdue's `fract` (on x86): what is left of `x` after its whole part.
fn fract(x: f32) -> f32 {
    x - (x as i32 as f32)
}

#[derive(Clone, Copy)]
struct Bounds {
    xmin: f32,
    xmax: f32,
    ymin: f32,
    ymax: f32,
}

/// A glyph's outline as lines (font units), its curves split as fontdue
/// splits them for 32 px; lines that do not go up or down are left out (they
/// cover nothing).
struct Outline {
    lines: Vec<[f32; 4]>,
    bounds: Bounds,
    start: (f32, f32),
    last: (f32, f32),
    /// Twice the area of the triangle a curve's piece leaves out (font
    /// units) above which it is split again.
    max_area: f32,
}

impl Outline {
    fn new(units_per_em: f32) -> Outline {
        // fontdue: 3 px at the 32 px it is set to
        let max_area = 3.0 * 2.0 * (units_per_em / 32.0);
        Outline {
            lines: Vec::new(),
            bounds: Bounds {
                xmin: f32::MAX,
                xmax: f32::MIN,
                ymin: f32::MAX,
                ymax: f32::MIN,
            },
            start: (0.0, 0.0),
            last: (0.0, 0.0),
            max_area,
        }
    }

    fn bounds(&self) -> Bounds {
        if self.lines.is_empty() {
            Bounds {
                xmin: 0.0,
                xmax: 0.0,
                ymin: 0.0,
                ymax: 0.0,
            }
        } else {
            self.bounds
        }
    }

    fn push(&mut self, a: (f32, f32), b: (f32, f32)) {
        if a.1.to_bits() == b.1.to_bits() {
            return;
        }
        self.lines.push([a.0, a.1, b.0, b.1]);
        for (x, y) in [a, b] {
            let r = &mut self.bounds;
            r.xmin = r.xmin.min(x);
            r.xmax = r.xmax.max(x);
            r.ymin = r.ymin.min(y);
            r.ymax = r.ymax.max(y);
        }
    }

    /// The curve `point` from the last point to `end` as lines: halved
    /// while a half's middle is too far off the line between its ends.
    fn curve(&mut self, end: (f32, f32), point: impl Fn(f32) -> (f32, f32)) {
        let mut stack = vec![(self.last, 0.0f32, end, 1.0f32)];
        while let Some((a, at, c, ct)) = stack.pop() {
            let bt = (at + ct) * 0.5;
            let b = point(bt);
            let area = (b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1);
            if area.abs() > self.max_area {
                stack.push((a, at, b, bt));
                stack.push((b, bt, c, ct));
            } else {
                self.push(a, c);
            }
        }
        self.last = end;
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = (x, y);
        self.last = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(self.last, (x, y));
        self.last = (x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.last;
        self.curve((x, y), |t| {
            let tm = 1.0 - t;
            let (ka, kb, kc) = (tm * tm, 2.0 * tm * t, t * t);
            (ka * a.0 + kb * x1 + kc * x, ka * a.1 + kb * y1 + kc * y)
        });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.last;
        self.curve((x, y), |t| {
            let tm = 1.0 - t;
            let (ka, kb, kc, kd) = (
                tm * tm * tm,
                3.0 * (tm * tm) * t,
                3.0 * tm * (t * t),
                t * t * t,
            );
            (
                ka * a.0 + kb * x1 + kc * x2 + kd * x,
                ka * a.1 + kb * y1 + kc * y2 + kd * y,
            )
        });
    }

    fn close(&mut self) {
        if self.start != self.last {
            self.push(self.last, self.start);
        }
        self.last = self.start;
    }
}
