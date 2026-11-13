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
    fonts: Vec<fontdue::Font>,
    cache: HashMap<(char, u16), Option<Glyph>>,
    font_of: HashMap<char, Option<u16>>,
    cx: u32,
    cy: u32,
    shelf_h: u32,
    pub generation: u64,
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

impl Text {
    pub fn new(extra_dirs: &[&Path]) -> Text {
        let mut fonts = Vec::new();
        let settings = |i: u32| fontdue::FontSettings {
            collection_index: i,
            scale: 32.0,
            ..Default::default()
        };
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
                            fonts.push(f)
                        }
                        Err(e) => log::warn!("font {}: {e}", p.display()),
                    }
                }
            }
        }
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
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
                let f = db
                    .with_face_data(id, |data, index| {
                        fontdue::Font::from_bytes(data, settings(index)).ok()
                    })
                    .flatten();
                if let Some(f) = f {
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
            if let Some(face) = db.faces().next() {
                if let Some(f) = db
                    .with_face_data(face.id, |d, i| {
                        fontdue::Font::from_bytes(d, settings(i)).ok()
                    })
                    .flatten()
                {
                    fonts.push(f);
                }
            }
        }
        // bundled Latin fallback (always available, e.g. in the web build)
        if let Ok(f) = fontdue::Font::from_bytes(EMBEDDED_FONT, settings(0)) {
            fonts.push(f);
        }
        if fonts.is_empty() {
            log::error!("no usable font found; text will not be displayed. Put a .ttf into the fonts/ folder.");
        }
        Text {
            fonts,
            cache: HashMap::new(),
            font_of: HashMap::new(),
            cx: 8,
            cy: 0,
            shelf_h: 8,
            generation: 0,
        }
    }

    /// Add a fallback font at runtime (e.g. downloaded by the web build).
    pub fn add_font(&mut self, data: Vec<u8>) {
        match fontdue::Font::from_bytes(
            data,
            fontdue::FontSettings {
                scale: 32.0,
                ..Default::default()
            },
        ) {
            Ok(f) => {
                self.fonts.push(f);
                self.font_of.retain(|_, v| v.is_some()); // retry glyphs that were missing
            }
            Err(e) => log::warn!("font: {e}"),
        }
    }

    fn font_for(&mut self, c: char) -> Option<u16> {
        if let Some(f) = self.font_of.get(&c) {
            return *f;
        }
        let f = self
            .fonts
            .iter()
            .position(|f| f.lookup_glyph_index(c) != 0)
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

    fn glyph(&mut self, gfx: &Gfx, c: char, px: u16) -> Option<Glyph> {
        if let Some(g) = self.cache.get(&(c, px)) {
            return *g;
        }
        let fi = self.font_for(c).or_else(|| self.font_for('?'))?;
        let font = &self.fonts[fi as usize];
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
        self.cache.insert((c, px), Some(g));
        Some(g)
    }

    fn ascent(&self, px: f32) -> f32 {
        self.fonts
            .first()
            .and_then(|f| f.horizontal_line_metrics(px))
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
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gfx: &Gfx,
        batch: &mut Batch,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
    ) -> f32 {
        self.draw_scaled(gfx, batch, s, x, y, size, color, align, f32::INFINITY)
    }

    /// Like [`Text::draw`] but squeezes horizontally to fit `max_w`
    /// (the classic BMS-select look for long titles).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scaled(
        &mut self,
        gfx: &Gfx,
        batch: &mut Batch,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
        max_w: f32,
    ) -> f32 {
        if s.is_empty() || self.fonts.is_empty() {
            return 0.0;
        }
        let px = size.round().clamp(4.0, 400.0) as u16;
        let gen = self.generation;
        let glyphs: Vec<Glyph> = s.chars().filter_map(|c| self.glyph(gfx, c, px)).collect();
        if self.generation != gen {
            // atlas was reset mid-string; rasterize again
            return self.draw_scaled(gfx, batch, s, x, y, size, color, align, max_w);
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
        for g in glyphs {
            if g.w > 0.0 {
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
        drawn
    }

    /// Draw with a 1px dark shadow for readability over BGA.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shadow(
        &mut self,
        gfx: &Gfx,
        batch: &mut Batch,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        align: Align,
    ) -> f32 {
        let sh = (size / 16.0).max(1.0);
        self.draw(
            gfx,
            batch,
            s,
            x + sh,
            y + sh,
            size,
            [0.0, 0.0, 0.0, color[3] * 0.8],
            align,
        );
        self.draw(gfx, batch, s, x, y, size, color, align)
    }
}
