//! Text rendering with fontdue and a glyph atlas.
//!
//! Fonts are looked up in `<root>/fonts` and then the system
//! (CJK-capable fonts first, since BMS titles are mostly Japanese/Korean).
//! Missing glyphs fall back font by font.

use super::{Batch, Color, Gfx, Rect, ATLAS, ATLAS_SIZE};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// How a string is drawn.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub size: f32,
    pub color: Color,
    /// Where the string is from `x`.
    pub align: Align,
    /// Wider strings are squeezed horizontally to this (the classic
    /// BMS-select look for long titles).
    pub max_w: f32,
    /// A 1px dark shadow for readability over BGA.
    pub shadow: bool,
}

impl TextStyle {
    pub fn new(size: f32, color: Color, align: Align) -> TextStyle {
        TextStyle {
            size,
            color,
            align,
            max_w: f32::INFINITY,
            shadow: false,
        }
    }

    pub fn fit(self, max_w: f32) -> TextStyle {
        TextStyle { max_w, ..self }
    }

    pub fn shadow(self) -> TextStyle {
        TextStyle {
            shadow: true,
            ..self
        }
    }
}

#[derive(Clone, Copy)]
struct Glyph {
    uv: [f32; 4],
    w: f32,
    h: f32,
    xmin: f32,
    ymin: f32,
    advance: f32,
}

pub struct Text {
    /// The UI fonts in fallback order: a character is drawn with the first
    /// that has it.
    fonts: Vec<Slot>,
    /// Where the system fonts in `fonts` are read from.
    db: fontdb::Database,
    /// Fonts skins bring (by file), drawn with when `face` is set.
    faces: Vec<fontdue::Font>,
    face_of: HashMap<std::path::PathBuf, Option<u16>>,
    /// The skin font drawing uses now (0 = the UI fonts).
    face: u16,
    cache: HashMap<(u16, char, u16), Option<Glyph>>,
    font_of: HashMap<char, Option<u16>>,
    /// Reused by `draw_scaled`.
    line: Vec<Glyph>,
    cx: u32,
    cy: u32,
    shelf_h: u32,
    pub generation: u64,
}

/// A UI font. CJK fonts take long to parse (tens of thousands of glyphs) and
/// most fallbacks are never needed, so they are parsed when first drawn with.
enum Slot {
    Ready(fontdue::Font),
    /// Not parsed yet: what to parse, and the characters it maps (as
    /// fontdue would).
    Pending(Source, Vec<char>),
    /// fontdue could not parse it.
    Failed,
}

enum Source {
    System(fontdb::ID),
    Embedded,
}

/// Inter (SIL OFL 1.1, see assets/fonts/OFL.txt).
const EMBEDDED_FONT: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.otf");

const PREFERRED_FAMILIES: &[&str] = &[
    // Japanese + Korean coverage
    "Noto Sans CJK JP",
    "Noto Sans CJK KR",
    "Source Han Sans",
    "Noto Sans JP",
    "Noto Sans KR",
    "Yu Gothic UI",
    "Yu Gothic",
    "Meiryo UI",
    "Meiryo",
    "Malgun Gothic",
    "MS Gothic",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "Apple SD Gothic Neo",
    "PingFang SC",
    "WenQuanYi Zen Hei",
    "Droid Sans Fallback",
    "Noto Sans",
    "Segoe UI",
    "Roboto",
    "Inter",
    "DejaVu Sans",
    "Liberation Sans",
    "Arial",
    "Unifont",
];

fn settings(collection_index: u32) -> fontdue::FontSettings {
    fontdue::FontSettings {
        collection_index,
        scale: 32.0,
        ..Default::default()
    }
}

impl Slot {
    /// A font to parse when needed; `None` when fontdue could not parse it.
    /// The characters are read as fontdue reads them, without the outlines.
    fn pending(source: Source, data: &[u8], index: u32) -> Option<Slot> {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let glyphs = face.number_of_glyphs();
        // fontdue fails on a glyph id out of range
        let mut in_range = glyphs > 0;
        let mut chars = Vec::new();
        for t in face.tables().cmap.into_iter().flat_map(|c| c.subtables) {
            t.codepoints(|cp| {
                let g = t.glyph_index(cp).filter(|g| g.0 != 0);
                if let (Some(g), Some(c)) = (g, char::from_u32(cp)) {
                    in_range &= g.0 < glyphs;
                    chars.push(c);
                }
            });
        }
        chars.sort_unstable();
        chars.dedup();
        in_range.then_some(Slot::Pending(source, chars))
    }

    fn system(db: &fontdb::Database, id: fontdb::ID) -> Option<Slot> {
        db.with_face_data(id, |data, index| {
            Slot::pending(Source::System(id), data, index)
        })
        .flatten()
    }
}

impl Text {
    pub fn new(extra_dirs: &[&Path]) -> Text {
        let mut fonts = Vec::new();
        // user supplied fonts first
        for dir in extra_dirs {
            let Ok(rd) = crate::platform::vfs::read_dir(dir) else {
                continue;
            };
            let mut files: Vec<_> = rd
                .into_iter()
                .filter(|e| !e.is_dir)
                .map(|e| e.path)
                .collect();
            files.sort();
            for p in files {
                let ext = p
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                    continue;
                }
                if let Ok(data) = crate::platform::vfs::read(&p) {
                    match fontdue::Font::from_bytes(data, settings(0)) {
                        Ok(f) => {
                            log::info!("font: {}", p.display());
                            fonts.push(Slot::Ready(f))
                        }
                        Err(e) => log::warn!("font {}: {e}", p.display()),
                    }
                }
            }
        }
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        // fontdb loads no system fonts on Android
        #[cfg(target_os = "android")]
        db.load_fonts_dir("/system/fonts");
        // the simulator's own iOS file system (fontdb loads the host's)
        #[cfg(target_os = "ios")]
        if let Some(sim) = std::env::var_os("IPHONE_SIMULATOR_ROOT") {
            db.load_fonts_dir(Path::new(&sim).join("System/Library/Fonts"));
        }
        let mut used = std::collections::HashSet::new();
        for fam in PREFERRED_FAMILIES {
            let q = fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            };
            if let Some(id) = db.query(&q) {
                if !used.insert(id) {
                    continue;
                }
                if let Some(f) = Slot::system(&db, id) {
                    log::info!("font: {fam}");
                    fonts.push(f);
                }
            }
            if fonts.len() >= 6 {
                break;
            }
        }
        if fonts.is_empty() {
            // anything at all
            fonts.extend(db.faces().next().and_then(|f| Slot::system(&db, f.id)));
        }
        // bundled Latin fallback (always available, e.g. in the web build)
        fonts.extend(Slot::pending(Source::Embedded, EMBEDDED_FONT, 0));
        if fonts.is_empty() {
            log::error!("no usable font found; text will not be displayed. Put a .ttf into the fonts/ folder.");
        }
        Text {
            fonts,
            db,
            faces: Vec::new(),
            face_of: HashMap::new(),
            face: 0,
            cache: HashMap::new(),
            font_of: HashMap::new(),
            line: Vec::new(),
            cx: 8,
            cy: 0,
            shelf_h: 8,
            generation: 0,
        }
    }

    /// Add a fallback font at runtime (e.g. downloaded by the web build).
    pub fn add_font(&mut self, data: Vec<u8>) {
        match fontdue::Font::from_bytes(data, settings(0)) {
            Ok(f) => {
                self.fonts.push(Slot::Ready(f));
                // glyphs drawn with a stand-in so far: draw them again
                self.font_of.retain(|_, v| v.is_some());
                self.reset_atlas();
            }
            Err(e) => log::warn!("font: {e}"),
        }
    }

    /// A skin's font file as a face to draw with (`set_face`); read once.
    pub fn face(&mut self, path: &std::path::Path) -> Option<u16> {
        if let Some(f) = self.face_of.get(path) {
            return *f;
        }
        let font = crate::platform::vfs::read(path).ok().and_then(|data| {
            fontdue::Font::from_bytes(data, settings(0))
                .map_err(|e| log::warn!("font {}: {e}", path.display()))
                .ok()
        });
        let id = font.map(|f| {
            self.faces.push(f);
            self.faces.len() as u16
        });
        self.face_of.insert(path.to_path_buf(), id);
        id
    }

    /// Draw (and measure) with a skin font from now on (0: the UI fonts).
    pub fn set_face(&mut self, face: u16) {
        self.face = face;
    }

    /// Parse UI font `i` if it is pending; whether it can be drawn with.
    fn load(&mut self, i: usize) -> bool {
        if let Slot::Pending(source, _) = &self.fonts[i] {
            let font = match source {
                Source::System(id) => self
                    .db
                    .with_face_data(*id, |data, index| {
                        fontdue::Font::from_bytes(data, settings(index)).ok()
                    })
                    .flatten(),
                Source::Embedded => fontdue::Font::from_bytes(EMBEDDED_FONT, settings(0)).ok(),
            };
            self.fonts[i] = match font {
                Some(f) => Slot::Ready(f),
                None => {
                    log::warn!("font {i}: could not be read");
                    Slot::Failed
                }
            };
        }
        matches!(self.fonts[i], Slot::Ready(_))
    }

    fn ready(&self, i: usize) -> Option<&fontdue::Font> {
        match &self.fonts[i] {
            Slot::Ready(f) => Some(f),
            _ => None,
        }
    }

    fn has(&self, i: usize, c: char) -> bool {
        match &self.fonts[i] {
            Slot::Ready(f) => f.lookup_glyph_index(c) != 0,
            Slot::Pending(_, chars) => chars.binary_search(&c).is_ok(),
            Slot::Failed => false,
        }
    }

    fn font_for(&mut self, c: char) -> Option<u16> {
        if let Some(f) = self.font_of.get(&c) {
            return *f;
        }
        let f = (0..self.fonts.len())
            .find(|&i| self.has(i, c) && self.load(i))
            .map(|i| i as u16);
        self.font_of.insert(c, f);
        f
    }

    fn reset_atlas(&mut self) {
        self.cache.clear();
        self.cx = 8;
        self.cy = 0;
        self.shelf_h = 8;
        self.generation += 1;
    }

    /// The skin font in use, if it has `c`.
    fn face_font(&self, c: char) -> Option<&fontdue::Font> {
        let f = self.faces.get((self.face as usize).checked_sub(1)?)?;
        (f.lookup_glyph_index(c) != 0).then_some(f)
    }

    fn glyph(&mut self, gfx: &Gfx, c: char, px: u16) -> Option<Glyph> {
        let face = if self.face_font(c).is_some() {
            self.face
        } else {
            0
        };
        if let Some(g) = self.cache.get(&(face, c, px)) {
            return *g;
        }
        let font = if face != 0 {
            &self.faces[face as usize - 1]
        } else {
            let fi = self.font_for(c).or_else(|| self.font_for('?'))?;
            self.ready(fi as usize)?
        };
        let (m, bitmap) = font.rasterize(c, px as f32);
        let (w, h) = (m.width as u32, m.height as u32);
        let mut g = Glyph {
            uv: [0.0; 4],
            w: w as f32,
            h: h as f32,
            xmin: m.xmin as f32,
            ymin: m.ymin as f32,
            advance: m.advance_width,
        };
        if w > 0 && h > 0 {
            if self.cx + w + 1 > ATLAS_SIZE {
                self.cx = 0;
                self.cy += self.shelf_h + 1;
                self.shelf_h = 0;
            }
            if self.cy + h + 1 > ATLAS_SIZE {
                self.reset_atlas();
            }
            let (x, y) = (self.cx, self.cy);
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for a in &bitmap {
                rgba.extend_from_slice(&[255, 255, 255, *a]);
            }
            gfx.update_texture(ATLAS, x, y, w, h, &rgba);
            let s = ATLAS_SIZE as f32;
            g.uv = [
                x as f32 / s,
                y as f32 / s,
                (x + w) as f32 / s,
                (y + h) as f32 / s,
            ];
            self.cx += w + 1;
            self.shelf_h = self.shelf_h.max(h);
        }
        self.cache.insert((face, c, px), Some(g));
        Some(g)
    }

    fn ascent(&mut self, px: f32) -> f32 {
        let face = (self.face as usize)
            .checked_sub(1)
            .and_then(|i| self.faces.get(i));
        let font = match face {
            Some(f) => Some(f),
            None => (0..self.fonts.len())
                .find(|&i| self.load(i))
                .and_then(|i| self.ready(i)),
        };
        font.and_then(|f| f.horizontal_line_metrics(px))
            .map(|m| m.ascent)
            .unwrap_or(px * 0.8)
    }

    pub fn measure(&mut self, gfx: &Gfx, s: &str, size: f32) -> f32 {
        let px = size.round().clamp(4.0, 400.0) as u16;
        s.chars()
            .filter_map(|c| self.glyph(gfx, c, px))
            .map(|g| g.advance)
            .sum()
    }

    /// Draw `s` with its top edge at `y`. Returns the drawn width.
    pub fn draw(
        &mut self,
        gfx: &Gfx,
        batch: &mut Batch,
        s: &str,
        x: f32,
        y: f32,
        style: TextStyle,
    ) -> f32 {
        if style.shadow {
            let sh = (style.size / 16.0).max(1.0);
            let dark = TextStyle {
                color: [0.0, 0.0, 0.0, style.color[3] * 0.8],
                shadow: false,
                ..style
            };
            self.draw(gfx, batch, s, x + sh, y + sh, dark);
        }
        let TextStyle {
            size,
            color,
            align,
            max_w,
            ..
        } = style;
        if s.is_empty() || self.fonts.is_empty() {
            return 0.0;
        }
        let px = size.round().clamp(4.0, 400.0) as u16;
        let mut glyphs = std::mem::take(&mut self.line);
        // glyphs before an atlas reset are gone from it: rasterize them
        // again, once (when the atlas cannot hold the string, the glyphs
        // after the last reset are drawn)
        let mut gone = 0;
        for _ in 0..2 {
            glyphs.clear();
            gone = 0;
            let mut gen = self.generation;
            for c in s.chars() {
                let Some(g) = self.glyph(gfx, c, px) else {
                    continue;
                };
                if self.generation != gen {
                    gen = self.generation;
                    gone = glyphs.len();
                }
                glyphs.push(g);
            }
            if gone == 0 {
                break;
            }
        }
        let width: f32 = glyphs.iter().map(|g| g.advance).sum();
        let sx = if width > max_w && width > 0.0 {
            max_w / width
        } else {
            1.0
        };
        let drawn = width * sx;
        let mut pen = match align {
            Align::Left => x,
            Align::Center => x - drawn * 0.5,
            Align::Right => x - drawn,
        };
        let base = y + self.ascent(px as f32);
        for (i, g) in glyphs.iter().enumerate() {
            if g.w > 0.0 && i >= gone {
                let r = Rect::new(
                    (pen + g.xmin * sx).round(),
                    (base - g.ymin - g.h).round(),
                    g.w * sx,
                    g.h,
                );
                batch.quad(ATLAS, r, g.uv, color);
            }
            pen += g.advance * sx;
        }
        self.line = glyphs;
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_fonts_know_the_characters_fontdue_maps() {
        let Some(Slot::Pending(_, chars)) = Slot::pending(Source::Embedded, EMBEDDED_FONT, 0)
        else {
            panic!("the embedded font is readable");
        };
        let font = fontdue::Font::from_bytes(EMBEDDED_FONT, settings(0)).unwrap();
        let mut mapped: Vec<char> = font.chars().keys().copied().collect();
        mapped.sort_unstable();
        assert_eq!(chars, mapped);
    }
}
