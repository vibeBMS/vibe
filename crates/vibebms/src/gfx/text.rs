//! Text rendering with a glyph atlas.
//!
//! Fonts are looked up in `<root>/fonts` and then the system
//! (CJK-capable fonts first, since BMS titles are mostly Japanese/Korean).
//! Missing glyphs fall back font by font. Glyphs are read from the font
//! files as they are drawn (`font`): the memory fonts take follows what is
//! drawn, not how many glyphs they have.

use super::font::{Bytes, Font, Metrics};
use super::{Batch, Color, Gfx, Rect, ATLAS, ATLAS_SIZE};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Bytes of a TrueType / OpenType font a skin names (a CJK font of every
/// weight is some 30 MB).
pub(crate) const FONT_MAX: u64 = 64 << 20;

pub struct Text {
    /// The UI fonts in fallback order: a character is drawn with the first
    /// that has it.
    fonts: Vec<Slot>,
    /// How many of `fonts` were there from the start (those added later
    /// follow).
    built_in: usize,
    /// Where the system fonts in `fonts` are read from.
    db: fontdb::Database,
    /// The system font files read so far: the faces of one file (a CJK
    /// collection's Korean and Japanese ones) share its bytes.
    files: HashMap<PathBuf, Arc<Bytes>>,
    /// Fonts skins bring (by file), drawn with when `face` is set.
    faces: Vec<Font>,
    face_of: HashMap<PathBuf, Option<u16>>,
    /// Whether skin font `face` has a character (asked each time one is
    /// drawn with it).
    face_has: HashMap<(u16, char), bool>,
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

/// A UI font. Most fallbacks are never needed, so a font's file is opened
/// when a character the fonts before it lack is drawn.
enum Slot {
    Ready(Font),
    /// Not opened yet.
    Pending(Source),
    /// Not a font that can be read.
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

/// The system's fonts (not read yet: what fontdb knows of them).
fn system_fonts() -> fontdb::Database {
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
    db
}

impl Text {
    pub fn new(extra_dirs: &[&Path]) -> Text {
        Text::with_fonts(extra_dirs, system_fonts())
    }

    /// The bundled font alone: the same glyphs on every machine and in
    /// every UI language.
    #[cfg(test)]
    pub(crate) fn built_in() -> Text {
        Text::with_fonts(&[], fontdb::Database::new())
    }

    /// The user's fonts in `extra_dirs`, then those of `db`.
    fn with_fonts(extra_dirs: &[&Path], db: fontdb::Database) -> Text {
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
                    match Font::new(Arc::new(Bytes::Owned(data)), 0) {
                        Some(f) => {
                            log::info!("font: {}", p.display());
                            fonts.push(Slot::Ready(f))
                        }
                        None => log::warn!("font {}: not a font", p.display()),
                    }
                }
            }
        }
        let mut used = std::collections::HashSet::new();
        // the UI language's own CJK fonts first: Chinese and Korean text
        // drawn the way they write it, not with Japanese glyph forms
        let own: &[&str] = match crate::i18n::current() {
            crate::i18n::Lang::Ko => &[
                "Noto Sans CJK KR",
                "Source Han Sans K",
                "Noto Sans KR",
                "Malgun Gothic",
                "Apple SD Gothic Neo",
            ],
            crate::i18n::Lang::ZhHans => &[
                "Noto Sans CJK SC",
                "Source Han Sans SC",
                "Noto Sans SC",
                "Microsoft YaHei UI",
                "Microsoft YaHei",
                "PingFang SC",
            ],
            crate::i18n::Lang::ZhHant => &[
                "Noto Sans CJK TC",
                "Source Han Sans TC",
                "Noto Sans TC",
                "Microsoft JhengHei UI",
                "Microsoft JhengHei",
                "PingFang TC",
            ],
            _ => &[],
        };
        for fam in own.iter().chain(PREFERRED_FAMILIES) {
            let q = fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            };
            if let Some(id) = db.query(&q) {
                if !used.insert(id) {
                    continue;
                }
                log::info!("font: {fam}");
                fonts.push(Slot::Pending(Source::System(id)));
            }
            if fonts.len() >= 6 {
                break;
            }
        }
        if fonts.is_empty() {
            // anything at all
            let any = db.faces().next().map(|f| f.id);
            fonts.extend(any.map(|id| Slot::Pending(Source::System(id))));
        }
        // bundled Latin fallback (always available, e.g. in the web build)
        fonts.push(Slot::Pending(Source::Embedded));
        Text::with_slots(fonts, db)
    }

    fn with_slots(fonts: Vec<Slot>, db: fontdb::Database) -> Text {
        Text {
            built_in: fonts.len(),
            fonts,
            db,
            files: HashMap::new(),
            faces: Vec::new(),
            face_of: HashMap::new(),
            face_has: HashMap::new(),
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

    /// Add a fallback font at runtime (e.g. downloaded by the web build):
    /// the last, or with `first` before the others downloaded (the UI
    /// language's own: its characters drawn the way that language writes
    /// them).
    pub fn add_font(&mut self, data: Vec<u8>, first: bool) {
        match Font::new(Arc::new(Bytes::Owned(data)), 0) {
            Some(f) => {
                if first {
                    self.fonts.insert(self.built_in, Slot::Ready(f));
                    // (the fonts after it moved)
                    self.font_of.clear();
                } else {
                    self.fonts.push(Slot::Ready(f));
                    // glyphs drawn with a stand-in so far: draw them again
                    self.font_of.retain(|_, v| v.is_some());
                }
                self.reset_atlas();
            }
            None => log::warn!("font: not a font"),
        }
    }

    /// A skin's font file as a face to draw with (`set_face`); read once,
    /// if it has at most [`FONT_MAX`] bytes.
    pub fn face(&mut self, path: &std::path::Path) -> Option<u16> {
        if let Some(f) = self.face_of.get(path) {
            return *f;
        }
        let font = match crate::platform::vfs::read_capped(path, FONT_MAX) {
            Ok(data) => {
                let f = Font::new(Arc::new(Bytes::Owned(data)), 0);
                if f.is_none() {
                    log::warn!("font {}: not a font", path.display());
                }
                f
            }
            Err(e) => {
                log::warn!("font: {e}");
                None
            }
        };
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

    /// System font `id`, its file read once for all its faces.
    fn system_font(&mut self, id: fontdb::ID) -> Option<Font> {
        let (source, index) = self.db.face_source(id)?;
        let data = match source {
            fontdb::Source::File(path) => match self.files.get(&path) {
                Some(d) => d.clone(),
                None => {
                    let d = Arc::new(Bytes::system_file(&path)?);
                    self.files.insert(path, d.clone());
                    d
                }
            },
            fontdb::Source::SharedFile(_, d) | fontdb::Source::Binary(d) => {
                Arc::new(Bytes::Shared(d))
            }
        };
        Font::new(data, index)
    }

    /// Open UI font `i` if it is pending; whether it can be drawn with.
    fn load(&mut self, i: usize) -> bool {
        if let Slot::Pending(source) = &self.fonts[i] {
            let font = match *source {
                Source::System(id) => self.system_font(id),
                Source::Embedded => Font::new(Arc::new(Bytes::Static(EMBEDDED_FONT)), 0),
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

    fn ready(&self, i: usize) -> Option<&Font> {
        match &self.fonts[i] {
            Slot::Ready(f) => Some(f),
            _ => None,
        }
    }

    fn font_for(&mut self, c: char) -> Option<u16> {
        if let Some(f) = self.font_of.get(&c) {
            return *f;
        }
        let f = (0..self.fonts.len())
            .find(|&i| self.load(i) && self.ready(i).is_some_and(|f| f.has(c)))
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

    /// Whether the skin font in use has `c`.
    fn face_has(&mut self, c: char) -> bool {
        let Some(f) = (self.face as usize).checked_sub(1) else {
            return false;
        };
        let faces = &self.faces;
        *(self.face_has)
            .entry((self.face, c))
            .or_insert_with(|| faces.get(f).is_some_and(|f| f.has(c)))
    }

    /// `c` drawn at `px` with skin font `face` (0: the UI fonts).
    fn rasterize(&mut self, face: u16, c: char, px: u16) -> Option<(Metrics, Vec<u8>)> {
        let font = if face != 0 {
            &self.faces[face as usize - 1]
        } else {
            let fi = self.font_for(c).or_else(|| self.font_for('?'))?;
            self.ready(fi as usize)?
        };
        Some(font.rasterize(c, px as f32))
    }

    fn glyph(&mut self, gfx: &Gfx, c: char, px: u16) -> Option<Glyph> {
        let face = if self.face_has(c) { self.face } else { 0 };
        if let Some(g) = self.cache.get(&(face, c, px)) {
            return *g;
        }
        let (m, bitmap) = self.rasterize(face, c, px)?;
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
        font.map_or(px * 0.8, |f| f.ascent(px))
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

    /// Korean, kana, kanji and Latin: a select screen's worth.
    fn sample_chars() -> Vec<char> {
        let hangul = (0xAC00..0xD7A4).step_by(37);
        let kana = 0x3041..0x30FF;
        let kanji = (0x4E00..0x9FA0).step_by(67);
        let ascii = 0x20..0x7F;
        hangul
            .chain(kana)
            .chain(kanji)
            .chain(ascii)
            .filter_map(char::from_u32)
            .collect()
    }

    /// This process's resident memory in kB: all, anonymous (what
    /// Android's memory limiter counts) and file-backed (mapped files).
    fn rss() -> [u64; 3] {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let kb = |key: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
                .unwrap_or(0)
        };
        [kb("VmRSS:"), kb("RssAnon:"), kb("RssFile:")]
    }

    /// The memory the UI fonts take: start-up and a screen of Korean,
    /// Japanese and Latin text drawn at three sizes. The fonts are
    /// `VIBE_FONT_DIR`'s (else the system's), the UI in `VIBE_LANG`
    /// (`ko`, `ja`, `en`, ...). One test to a process:
    /// `VIBE_FONT_DIR=dir VIBE_LANG=ko cargo test -p vibebms --lib font_memory -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement: run it alone, see its comment"]
    fn font_memory() {
        let lang = std::env::var("VIBE_LANG").unwrap_or_default();
        crate::i18n::set(crate::i18n::Lang::from_locale(&lang));
        let before = rss();
        let t0 = std::time::Instant::now();
        let mut db = fontdb::Database::new();
        match std::env::var_os("VIBE_FONT_DIR") {
            Some(dir) => db.load_fonts_dir(dir),
            None => db = system_fonts(),
        }
        let scanned = t0.elapsed();
        let mut text = Text::with_fonts(&[], db);
        let started = t0.elapsed();
        let after_start = rss();
        let chars = sample_chars();
        let mut drawn = 0;
        for px in [16, 24, 40] {
            for &c in &chars {
                drawn += text.rasterize(0, c, px).is_some() as usize;
            }
        }
        let after_draw = rss();
        let loaded = (text.fonts.iter())
            .filter(|f| matches!(f, Slot::Ready(_)))
            .count();
        println!(
            "  {loaded} of {} UI fonts opened, {} files",
            text.fonts.len(),
            text.files.len()
        );
        let d = |a: [u64; 3]| [0, 1, 2].map(|i| a[i] as i64 - before[i] as i64);
        println!(
            "lang {lang:?}: scan {scanned:?}, start {started:?}, drew {drawn} glyphs in {:?}",
            t0.elapsed() - started
        );
        println!("  [VmRSS, RssAnon, RssFile] kB over the baseline {before:?}");
        println!("  after start: {:?}", d(after_start));
        println!("  after drawing: {:?}", d(after_draw));
    }

    /// How far `font`'s glyphs are from fontdue's (`data`, face `index`)
    /// for `chars` at a few sizes: metrics must be the same, coverage
    /// within rounding. The largest and mean coverage differences.
    fn against_fontdue(font: &Font, data: &[u8], index: u32, chars: &[char]) -> (u8, f64) {
        let settings = fontdue::FontSettings {
            collection_index: index,
            scale: 32.0,
            ..Default::default()
        };
        let old = fontdue::Font::from_bytes(data, settings).unwrap();
        let (mut max, mut sum, mut n) = (0, 0u64, 0u64);
        for px in [12.0, 16.0, 24.0, 40.0, 72.0] {
            assert_eq!(
                font.ascent(px),
                old.horizontal_line_metrics(px).unwrap().ascent
            );
            for &c in chars {
                assert_eq!(font.has(c), old.lookup_glyph_index(c) != 0, "{c:?}");
                let (m, bitmap) = font.rasterize(c, px);
                let (om, old_bitmap) = old.rasterize(c, px);
                let om = Metrics {
                    xmin: om.xmin,
                    ymin: om.ymin,
                    width: om.width,
                    height: om.height,
                    advance_width: om.advance_width,
                };
                assert_eq!(m, om, "{c:?} at {px}");
                for (a, b) in bitmap.iter().zip(&old_bitmap) {
                    let d = a.abs_diff(*b);
                    max = max.max(d);
                    sum += d as u64;
                    n += 1;
                }
            }
        }
        (max, sum as f64 / n.max(1) as f64)
    }

    #[test]
    fn glyphs_are_drawn_as_fontdue_drew_them() {
        let font = Font::new(Arc::new(Bytes::Static(EMBEDDED_FONT)), 0).unwrap();
        let chars: Vec<char> = (' '..='~').chain("éßØ€“”…→".chars()).collect();
        let (max, mean) = against_fontdue(&font, EMBEDDED_FONT, 0, &chars);
        assert!(
            max <= 2 && mean < 0.01,
            "coverage off by up to {max}, {mean} on average"
        );
    }

    /// As `glyphs_are_drawn_as_fontdue_drew_them`, with the CJK fonts of
    /// `VIBE_FONT_DIR` (else the system's): Hangul, kana and kanji. fontdue
    /// reads every glyph of each (hundreds of MB): run it on its own.
    #[test]
    #[ignore = "reads whole CJK fonts with fontdue"]
    fn cjk_glyphs_are_drawn_as_fontdue_drew_them() {
        let mut db = fontdb::Database::new();
        match std::env::var_os("VIBE_FONT_DIR") {
            Some(dir) => db.load_fonts_dir(dir),
            None => db = system_fonts(),
        }
        let chars: Vec<char> = "가나다한국어설정곡選曲設定日本語あいうアイウ漢字譜面ＡＢ１２ABCabc"
            .chars()
            .chain(sample_chars().into_iter().step_by(7))
            .collect();
        let mut compared = 0;
        for fam in ["Noto Sans CJK KR", "Noto Sans CJK JP", "WenQuanYi Zen Hei"] {
            let q = fontdb::Query {
                families: &[fontdb::Family::Name(fam)],
                ..Default::default()
            };
            let Some(id) = db.query(&q) else { continue };
            let mut text = Text::with_slots(vec![Slot::Pending(Source::System(id))], db.clone());
            assert!(text.load(0));
            let font = text.ready(0).unwrap();
            let (max, mean) = db
                .with_face_data(id, |data, index| against_fontdue(font, data, index, &chars))
                .unwrap();
            println!("{fam}: coverage off by up to {max}, {mean:.4} on average");
            assert!(max <= 2 && mean < 0.01);
            compared += 1;
        }
        assert!(compared > 0, "no CJK font");
    }

    /// A font file stands for Inter.
    fn font_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vibebms-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, EMBEDDED_FONT).unwrap();
        path
    }

    #[test]
    fn a_skin_font_file_too_large_is_not_looked_into() {
        let mut text = Text::with_slots(Vec::new(), fontdb::Database::new());
        assert_eq!(text.face(&font_file("skin.otf")), Some(1));
        // the same font, and zeros after it past the limit
        let path = font_file("large.otf");
        let mut data = EMBEDDED_FONT.to_vec();
        data.resize(FONT_MAX as usize + 1, 0);
        std::fs::write(&path, data).unwrap();
        assert_eq!(text.face(&path), None);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn fonts_are_opened_when_a_character_needs_them() {
        let mut db = fontdb::Database::new();
        db.load_font_file(font_file("lazy.otf")).unwrap();
        let id = db.faces().next().unwrap().id;
        let fonts = vec![
            Slot::Pending(Source::Embedded),
            Slot::Pending(Source::System(id)),
        ];
        let mut text = Text::with_slots(fonts, db);
        let opened = |t: &Text| {
            t.fonts
                .iter()
                .map(|f| matches!(f, Slot::Ready(_)))
                .collect::<Vec<_>>()
        };
        assert_eq!(opened(&text), [false, false]);
        assert!(text.files.is_empty());
        // the first font has it: the second is not opened
        assert_eq!(text.font_for('A'), Some(0));
        assert!(text.rasterize(0, 'A', 24).is_some_and(|(m, _)| m.width > 0));
        assert_eq!(opened(&text), [true, false]);
        assert!(text.files.is_empty());
        // one the first lacks: the second is looked at
        assert_eq!(text.font_for('가'), None);
        assert_eq!(opened(&text), [true, true]);
        assert_eq!(text.files.len(), 1);
    }

    #[test]
    fn faces_of_one_file_share_its_bytes() {
        let path = font_file("shared.otf");
        let mut db = fontdb::Database::new();
        // (two faces of one file, as a CJK collection's Korean and Japanese)
        db.load_font_file(&path).unwrap();
        db.load_font_file(&path).unwrap();
        let ids: Vec<_> = db.faces().map(|f| f.id).collect();
        assert_eq!(ids.len(), 2);
        let fonts = ids
            .iter()
            .map(|&id| Slot::Pending(Source::System(id)))
            .collect();
        let mut text = Text::with_slots(fonts, db);
        assert!(text.load(0) && text.load(1));
        let (a, b) = (text.ready(0).unwrap(), text.ready(1).unwrap());
        assert!(Arc::ptr_eq(a.data(), b.data()));
        assert_eq!(text.files.len(), 1);
        assert_eq!(Arc::strong_count(&text.files[&path]), 3);
    }
}
