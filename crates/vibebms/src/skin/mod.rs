//! Skin support (LR2 `.lr2skin` / CSV and beatoraja JSON / Lua skins) for the
//! play, music select and result screens.
//!
//! The loader ([`lr2`]) turns a skin into a renderer-independent model; this
//! module evaluates the keyframe animations against a [`SkinState`] provided
//! by the scene. Semantics (coordinate system, easing, loop rules, number
//! layout, timer/option/number ids) follow LR2 as documented by beatoraja's
//! open-source LR2 skin loader.

pub mod beatoraja;
pub mod lr2;
mod luapat;
pub mod render;
pub mod script;

use crate::gfx::Rect;
use std::collections::HashMap;
use std::path::PathBuf;
use vibebms_core::judge::Judge;

/// What the user chose for a skin.
#[derive(Clone, Debug, Default)]
pub struct UserSettings {
    /// Option ids on / off (custom options).
    pub options: HashMap<i32, bool>,
    /// Chosen file per custom file name (LR2 `#CUSTOMFILE`, beatoraja
    /// `filepath`).
    pub files: HashMap<String, String>,
    /// Offset values (x, y, w, h, angle, alpha) by offset name.
    pub offsets: HashMap<String, [i32; 6]>,
}

/// The format of a skin file, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinFormat {
    /// LR2 `.lr2skin`, or a `.csv` chosen as the skin itself.
    Lr2,
    /// beatoraja JSON or Lua.
    Beatoraja,
    /// LR2 sound set `.lr2ss`.
    SoundSet,
}

impl SkinFormat {
    pub fn of(path: &std::path::Path) -> Option<SkinFormat> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        Some(match ext.as_str() {
            "lr2skin" | "csv" => SkinFormat::Lr2,
            "json" | "luaskin" => SkinFormat::Beatoraja,
            "lr2ss" => SkinFormat::SoundSet,
            _ => return None,
        })
    }
}

/// Load any supported skin file (LR2 CSV, beatoraja JSON or Lua).
/// `static_option` answers LR2 `#IF`s, `state` what a Lua skin reads
/// while it is built.
pub fn load_file(
    path: &std::path::Path,
    skins_root: &std::path::Path,
    user: &UserSettings,
    static_option: &dyn Fn(i32) -> Option<bool>,
    state: Option<&dyn SkinState>,
    image_size: &dyn Fn(&std::path::Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    if SkinFormat::of(path) == Some(SkinFormat::Lr2) {
        lr2::load_with(path, skins_root, user, static_option, image_size)
    } else {
        beatoraja::load_with(path, user, state, image_size)
    }
}

/// Pixel size of an image file (through the VFS).
pub fn image_size(p: &std::path::Path) -> Option<(u32, u32)> {
    if crate::video::is_video(p) {
        return crate::video::size(p);
    }
    // the start of the file says, but for the odd JPEG with a large
    // preview ahead of its frame header
    const HEAD: usize = 64 << 10;
    let head = crate::platform::vfs::read_head(p, HEAD).ok()?;
    image_size_of(&head, p).or_else(|| {
        let whole = (head.len() == HEAD).then(|| crate::platform::vfs::read(p).ok())??;
        image_size_of(&whole, p)
    })
}

/// A skin's image files while it loads, each measured when something first
/// draws from it (a skin may list images for options not chosen, or
/// videos, which take a process to measure).
struct ImageFiles<'a> {
    paths: Vec<Option<PathBuf>>,
    sizes: Vec<std::cell::OnceCell<Option<(u32, u32)>>>,
    measure: &'a dyn Fn(&std::path::Path) -> Option<(u32, u32)>,
}

impl<'a> ImageFiles<'a> {
    fn new(measure: &'a dyn Fn(&std::path::Path) -> Option<(u32, u32)>) -> Self {
        ImageFiles {
            paths: Vec::new(),
            sizes: Vec::new(),
            measure,
        }
    }

    fn len(&self) -> usize {
        self.paths.len()
    }

    fn push(&mut self, p: Option<PathBuf>) {
        self.paths.push(p);
        self.sizes.push(Default::default());
    }

    /// An image measured already: kept only when it is there.
    fn push_measured(&mut self, p: PathBuf, size: Option<(u32, u32)>) {
        self.paths.push(size.map(|_| p));
        self.sizes.push(size.into());
    }

    fn size(&self, i: usize) -> Option<(u32, u32)> {
        let p = self.paths.get(i)?.as_deref();
        *self.sizes[i].get_or_init(|| p.and_then(self.measure))
    }

    /// The paths, without the images found missing.
    fn finish(self) -> Vec<Option<PathBuf>> {
        self.paths
            .into_iter()
            .zip(self.sizes)
            .map(|(p, s)| p.filter(|_| s.get() != Some(&None)))
            .collect()
    }
}

fn image_size_of(b: &[u8], p: &std::path::Path) -> Option<(u32, u32)> {
    if let Some(s) = crate::formats::dds::size(b).or_else(|| crate::formats::cim::size(b)) {
        return Some(s);
    }
    let r = image::ImageReader::new(std::io::Cursor::new(b))
        .with_guessed_format()
        .ok()?;
    if r.format().is_some() {
        return r.into_dimensions().ok();
    }
    // TGA has no signature: trust the extension
    let tga = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tga"));
    tga.then(|| {
        image::ImageReader::with_format(std::io::Cursor::new(b), image::ImageFormat::Tga)
            .into_dimensions()
            .ok()
    })
    .flatten()
}

/// (type, name) of any supported skin file (cached: reading a Lua skin's
/// header runs it).
pub fn header(path: &std::path::Path) -> Option<(SkinType, String)> {
    type Cache = HashMap<(PathBuf, (i64, i64)), Option<(SkinType, String)>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (path.to_path_buf(), crate::platform::vfs::stamp(path));
    if let Some(h) = CACHE.lock().ok()?.get_or_insert_default().get(&key) {
        return h.clone();
    }
    let h = match SkinFormat::of(path)? {
        SkinFormat::Lr2 => lr2::header(path),
        SkinFormat::Beatoraja => beatoraja::header(path),
        SkinFormat::SoundSet => {
            crate::sysound::lr2ss_header(path).map(|h| (SkinType::SoundSet, h.0))
        }
    };
    CACHE
        .lock()
        .ok()?
        .get_or_insert_default()
        .insert(key, h.clone());
    h
}

/// A source image region set (animation frames).
#[derive(Clone, Debug, Default)]
pub struct Src {
    pub image: usize,
    /// Frames as (x, y, w, h) in image pixels; `w/h < 0` = whole image.
    pub frames: Vec<[f32; 4]>,
    /// Animation cycle in ms (0 = no animation).
    pub cycle: i64,
    pub timer: i32,
}

impl Src {
    /// The frames of a `div_x` × `div_y` grid over (x, y, w, h), row by row.
    pub fn grid(rect: [f32; 4], div_x: i64, div_y: i64) -> Vec<[f32; 4]> {
        // a skin can ask for any grid; real ones stay far below this
        let dx = div_x.clamp(1, 1024) as usize;
        let dy = div_y.clamp(1, 1024).min(65536 / dx as i64) as usize;
        let [x, y, w, h] = rect;
        let (fw, fh) = (w / dx as f32, h / dy as f32);
        (0..dy)
            .flat_map(|j| (0..dx).map(move |i| [x + fw * i as f32, y + fh * j as f32, fw, fh]))
            .collect()
    }

    pub fn frame(&self, now: i64, st: &dyn SkinState) -> Option<[f32; 4]> {
        if self.frames.is_empty() {
            return None;
        }
        if self.cycle <= 0 || self.frames.len() == 1 {
            return Some(self.frames[0]);
        }
        let t = if self.timer > 0 {
            now - st.timer(self.timer)?
        } else {
            now
        };
        let n = self.frames.len() as i64;
        let i = (t.max(0) * n / self.cycle) % n;
        Some(self.frames[i as usize])
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Key {
    pub time: i64,
    pub rect: Rect,
    pub color: [f32; 4],
    pub angle: f32,
}

/// Destination animation (`#DST_*` lines of one object).
#[derive(Clone, Debug, Default)]
pub struct Dst {
    pub keys: Vec<Key>,
    pub acc: i32,
    pub blend: i32,
    pub center: usize,
    pub loop_ms: i64,
    pub timer: i32,
    pub ops: Vec<i32>,
    /// beatoraja offsets added from game state (`SkinState::offset`).
    pub offsets: Vec<i32>,
    /// How an image fits the destination (beatoraja `stretch`, LR2
    /// `#STRETCH`): 0 stretches, 1 fits inside keeping the aspect ratio...
    pub stretch: i32,
    /// Drawn only while the mouse is over this rect (relative to the
    /// destination, y down; LR2 `#SRC_ONMOUSE`, beatoraja `mouseRect`).
    pub hover: Option<Rect>,
    /// vibeBMS `anchor` [x, y, w, h]: when the skin is widened or made
    /// taller (`Skin::reflow`), the share of the added width / height the
    /// object moves right / down by and grows by.
    pub anchor: Option<[f32; 4]>,
}

/// Game image id of the skin select screen's preview (LR2 `#SRC_IMAGE`
/// image 105, beatoraja `skinpreview`).
pub const REF_SKIN_PREVIEW: i32 = 105;

/// beatoraja offset ids.
pub const OFFSET_LIFT: i32 = 3;
pub const OFFSET_LANECOVER: i32 = 4;
pub const OFFSET_HIDDEN_COVER: i32 = 5;

/// Evaluated destination for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub rect: Rect,
    pub color: [f32; 4],
    pub angle: f32,
    pub blend: crate::gfx::Blend,
    /// Rotation centre as a fraction of the rect (y down).
    pub center: (f32, f32),
}

const CENTER_X: [f32; 10] = [0.5, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0, 0.0, 0.5, 1.0];
/// LR2 centres follow the numeric keypad (1 = bottom-left); y grows downward here.
const CENTER_Y: [f32; 10] = [0.5, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0];

impl Dst {
    pub fn add_key(&mut self, k: Key) {
        let i = self.keys.partition_point(|e| e.time <= k.time);
        self.keys.insert(i, k);
    }

    /// Evaluate at scene time `now` (ms).
    pub fn eval(&self, now: i64, st: &dyn SkinState) -> Option<Frame> {
        if self.keys.is_empty() || !self.ops.iter().all(|o| st.option(*o)) {
            return None;
        }
        let mut t = now;
        if self.timer > 0 {
            t -= st.timer(self.timer)?;
        }
        let start = self.keys[0].time;
        let end = self.keys[self.keys.len() - 1].time;
        if self.loop_ms == -1 {
            if t > end {
                return None;
            }
        } else if end > 0 && end > start && t > self.loop_ms {
            // (one keyframe: nothing to loop, it stays from its time on)
            t = if end == self.loop_ms {
                self.loop_ms
            } else {
                (t - self.loop_ms) % (end - self.loop_ms) + self.loop_ms
            };
        }
        if t < start {
            return None;
        }
        let i = self.keys.partition_point(|k| k.time <= t).saturating_sub(1);
        let a = &self.keys[i];
        let (rect, color, angle) = match self.keys.get(i + 1) {
            Some(b) if self.acc != 3 && b.time > a.time => {
                let mut r = (t - a.time) as f32 / (b.time - a.time) as f32;
                match self.acc {
                    1 => r *= r,
                    2 => r = 1.0 - (r - 1.0) * (r - 1.0),
                    _ => {}
                }
                let l = |x: f32, y: f32| x + (y - x) * r;
                (
                    Rect::new(
                        l(a.rect.x, b.rect.x),
                        l(a.rect.y, b.rect.y),
                        l(a.rect.w, b.rect.w),
                        l(a.rect.h, b.rect.h),
                    ),
                    [
                        l(a.color[0], b.color[0]),
                        l(a.color[1], b.color[1]),
                        l(a.color[2], b.color[2]),
                        l(a.color[3], b.color[3]),
                    ],
                    l(a.angle, b.angle),
                )
            }
            _ => (a.rect, a.color, a.angle),
        };
        let (mut rect, mut color, mut angle) = (rect, color, angle);
        for id in &self.offsets {
            if let Some([x, y, w, h, r, a]) = st.offset(*id) {
                // offsets are y-up like beatoraja destinations
                rect.x += x;
                rect.y -= y + h;
                rect.w += w;
                rect.h += h;
                angle -= r;
                color[3] = (color[3] + a / 255.0).clamp(0.0, 1.0);
            }
        }
        let c = self.center.min(9);
        Some(Frame {
            rect,
            color,
            angle,
            blend: crate::gfx::Blend::from_skin(self.blend),
            center: (CENTER_X[c], CENTER_Y[c]),
        })
    }
}

#[derive(Clone, Debug)]
pub enum Kind {
    Image(Src),
    /// Image supplied by the game (stagefile 100, backbmp 101, banner 102 ...).
    RefImage(i32),
    Number {
        src: Src,
        digits: usize,
        id: i32,
        align: i32,
        keta: usize,
        /// Glyph drawn in leading positions (`None` = blank).
        pad: Option<usize>,
    },
    Text {
        id: i32,
        /// LR2 bitmap font (index into `Skin::fonts`), else the UI font.
        font: Option<usize>,
        align: i32,
        /// Text wider than the destination: 0 overflows (LR2, beatoraja
        /// default), 1 shrinks to fit, 2 is cut off, 3 (vibeBMS) is cut
        /// off and scrolls while it is the current one (a marquee).
        overflow: i32,
        /// A fixed text (beatoraja `constantText`) instead of text `id`.
        constant: Option<String>,
        /// The skin's own font file (beatoraja TrueType / OpenType fonts).
        face: Option<PathBuf>,
        /// beatoraja outline and shadow (none in LR2).
        fx: TextFx,
        /// beatoraja font size (skin px): `.fnt` glyphs are scaled by it
        /// over the font's own size. `None`: by the destination height.
        size: Option<f32>,
    },
    Bar {
        src: Src,
        id: i32,
        dir: i32,
        /// `id` is a number scaled from this range (beatoraja
        /// `#SRC_BARGRAPH_REFNUMBER`), not a 0..1 rate.
        minmax: Option<(f32, f32)>,
    },
    Slider {
        src: Src,
        id: i32,
        dir: i32,
        range: f32,
        /// As for `Bar` (`#SRC_SLIDER_REFNUMBER`).
        minmax: Option<(f32, f32)>,
    },
    Gauge {
        src: Src,
        part_w: f32,
        part_h: f32,
        step: (f32, f32),
        parts: usize,
        /// beatoraja style: the destination covers the whole gauge and is
        /// split into `parts` cells (LR2 places one cell per step).
        split: bool,
        /// The player side whose gauge it shows (LR2 `#SRC_GROOVEGAUGE`'s
        /// first field; a vibeBMS JSON destination's `side`): 2P is a
        /// battle's opponent.
        side: usize,
    },
    Bga,
    /// One of several images chosen by a skin number (beatoraja `imageset`).
    ImageSet {
        srcs: Vec<Src>,
        id: i32,
    },
    /// A graph drawn by the scene (gauge, notes, BPM, timing ...).
    Graph(Graph),
    /// The music select bar list.
    SongList,
    /// Clear lamp (`rank` false) or DJ rank distribution of a folder as a
    /// horizontal stacked bar (beatoraja graph type -1 / -2); `src` frames
    /// are the segment images per lamp.
    Distribution {
        src: Option<Src>,
        rank: bool,
    },
    /// beatoraja hidden / lift cover: an image clipped above `line` (y down,
    /// skin pixels; the line rises with LIFT when `link_lift`).
    Cover {
        src: Src,
        line: Option<f32>,
        link_lift: bool,
    },
    /// The note field (lanes from `#DST_NOTE`).
    Notes,
    JudgeLine(Src),
    /// `#SRC_NOWJUDGE_*` / `#SRC_NOWCOMBO_*` for one player side.
    Judge {
        side: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub kind: Kind,
    pub dst: Dst,
    /// Clickable: (event id, click mode) as beatoraja `act` / `click`.
    pub act: Option<(i32, i32)>,
}

/// Bar types of the music select list (beatoraja `bar` imageset order).
pub const BAR_SONG: usize = 0;
pub const BAR_FOLDER: usize = 1;
pub const BAR_TABLE: usize = 2;
pub const BAR_GRADE: usize = 3;
pub const BAR_NOGRADE: usize = 4;
pub const BAR_COMMAND: usize = 5;
pub const BAR_SEARCH: usize = 6;
pub const BAR_NOSONG: usize = 7;
/// vibeBMS's bar types after beatoraja's: NONSTOP and CLASS courses (and
/// folders of them), mirrored events, FAVORITE, the folders made of the
/// records. A skin without a body for one draws the bar as the beatoraja
/// type it was before (`BarInfo::base`).
pub const BAR_NONSTOP: usize = 8;
pub const BAR_CLASS: usize = 9;
pub const BAR_EVENT: usize = 10;
pub const BAR_FAVORITE: usize = 11;
pub const BAR_VIRTUAL: usize = 12;

/// A number font: source frames, glyph count (10/11/12), digits, align.
#[derive(Clone, Debug)]
pub struct NumberFont {
    pub src: Src,
    pub digits: usize,
    pub keta: usize,
    pub align: i32,
}

/// Music select bar list (beatoraja `songlist`, LR2 `#SRC_BAR_*`). The
/// title, level and lamp destinations are relative to each bar.
#[derive(Clone, Debug, Default)]
pub struct SongListSkin {
    /// Slot of the selected bar.
    pub center: usize,
    /// Slots that react to clicks (empty = all).
    pub clickable: Vec<usize>,
    /// Bar destination per slot, selected (`on`) and not (`off`).
    pub on: Vec<Option<Dst>>,
    pub off: Vec<Option<Dst>>,
    /// Bar body per bar type (`BAR_*`).
    pub body: Vec<Option<Src>>,
    /// Title: destination, align, overflow (as in `Kind::Text`), bitmap
    /// font.
    pub text: Option<(Dst, i32, i32, Option<usize>)>,
    /// vibeBMS `foldertext`: the title's destination on bars other than
    /// charts (folders, courses, commands), else `text`'s.
    pub folder_text: Option<Dst>,
    /// The title's font file and outline / shadow (beatoraja).
    pub text_face: Option<PathBuf>,
    pub text_fx: TextFx,
    pub text_size: Option<f32>,
    /// Level number per difficulty (0..=6).
    pub level: Vec<Option<(NumberFont, Dst)>>,
    /// Clear lamp per clear type (beatoraja order, 0..=10).
    pub lamp: Vec<Option<(Src, Dst)>>,
    /// The player's and a rival's lamps side by side, shown in place of
    /// `lamp` while a rival is chosen (LR2 `BAR_MY_LAMP` /
    /// `BAR_RIVAL_LAMP`, beatoraja `playerlamp` / `rivallamp`).
    pub my_lamp: Vec<Option<(Src, Dst)>>,
    pub rival_lamp: Vec<Option<(Src, Dst)>>,
    /// Lamp / rank distribution on folder bars.
    pub graph: Option<(Dst, Option<Src>, bool)>,
    /// vibeBMS `count`: the charts of a folder bar, the stages of a course.
    pub count: Option<(NumberFont, Dst)>,
}

/// A graph drawn by the scene, with the look the skin asks for.
#[derive(Clone, Debug)]
pub enum Graph {
    Gauge(GaugeGraph),
    Notes(NotesGraph),
    Bpm(BpmGraph),
    Timing(TimingGraph),
    Visualizer(VisualizerGraph),
}

impl Graph {
    /// Milliseconds the graph takes to appear (0 = the scene's choice).
    pub fn delay(&self) -> i64 {
        match self {
            Graph::Gauge(g) => g.delay,
            Graph::Notes(g) => g.delay,
            Graph::Bpm(g) => g.delay,
            Graph::Timing(_) | Graph::Visualizer(_) => 0,
        }
    }
}

/// The gauge over the play (result).
#[derive(Clone, Debug, Default)]
pub struct GaugeGraph {
    /// Per gauge kind (assist, easy, normal, hard, ex-hard, hazard): line
    /// above the border, background above, line below, background below.
    /// `None`: the built-in skin's look (palette colours, filled, guides).
    pub colors: Option<Vec<[f32; 4]>>,
    pub line_width: f32,
    pub delay: i64,
}

/// beatoraja's notes distribution: per second, one chip per note.
#[derive(Clone, Debug, Default)]
pub struct NotesGraph {
    /// 0 note types, 1 judgements, 2 judgements with FAST / SLOW.
    pub kind: usize,
    pub delay: i64,
    /// Dark background with guides.
    pub back: bool,
    pub order_reverse: bool,
    pub no_gap: bool,
    pub no_gap_x: bool,
    /// The built-in skin's look: with FAST / SLOW, FAST up and SLOW down
    /// per section.
    pub built_in: bool,
}

/// BPM over the chart.
#[derive(Clone, Debug, Default)]
pub struct BpmGraph {
    /// Main, min, max, other BPM, stop line, transition line.
    pub colors: Vec<[f32; 4]>,
    pub line_width: f32,
    pub delay: i64,
}

/// Distribution of timing errors.
#[derive(Clone, Debug, Default)]
pub struct TimingGraph {
    /// Width in 1 ms bins.
    pub bins: i64,
    /// Graph, average, deviation, then the PG .. PR window bands. `None`:
    /// the built-in skin's look (FAST / SLOW coloured, average and spread
    /// as text).
    pub colors: Option<Vec<[f32; 4]>>,
    pub line_width: f32,
    pub average: bool,
    pub deviation: bool,
}

/// Recent timing errors as ticks over the judge windows (play).
#[derive(Clone, Debug, Default)]
pub struct VisualizerGraph {
    /// beatoraja's hit error visualizer rather than the timing visualizer.
    pub hit_error: bool,
    /// Half width in ms.
    pub range_ms: f32,
    /// Line, centre, PG .. PR bands (and the moving average).
    pub colors: Vec<[f32; 4]>,
    pub line_width: f32,
    pub transparent_poor: bool,
    pub decay: bool,
    /// Ticks coloured by judgement.
    pub by_judge: bool,
    /// Moving average line.
    pub ema: bool,
}

/// Per-judgement image + combo number of a `NOWJUDGE` group.
#[derive(Clone, Debug, Default)]
pub struct JudgeSkin {
    pub judge: [Option<(Src, Dst)>; 6],
    pub combo: [Option<(Src, Dst, usize)>; 6],
    pub shift: bool,
}

/// Note sprites for one lane.
#[derive(Clone, Debug, Default)]
pub struct LaneSkin {
    pub rect: Option<Rect>,
    /// Drawn note height (LR2), else the source frame's.
    pub note_h: Option<f32>,
    pub note: Option<Src>,
    pub ln_start: Option<Src>,
    pub ln_end: Option<Src>,
    pub ln_body: Option<Src>,
    pub ln_body_active: Option<Src>,
    pub mine: Option<Src>,
    /// Hell charge notes (beatoraja `#SRC_HCN_*`); LN images when absent.
    pub hcn_start: Option<Src>,
    pub hcn_end: Option<Src>,
    pub hcn_body: Option<Src>,
    pub hcn_body_active: Option<Src>,
    /// Body while released mid-note, and held again after that.
    pub hcn_damage: Option<Src>,
    pub hcn_reactive: Option<Src>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinType {
    Play7,
    Play5,
    Play14,
    Play10,
    Play9,
    /// LR2 types 12 / 13 (beatoraja's "7/5KEYS BATTLE"): a double play
    /// layout for two sides of a battle.
    Battle7,
    Battle5,
    Select,
    Decide,
    Result,
    /// beatoraja / LR2 type 8: the key config screen.
    KeyConfig,
    /// Type 9: the skin select (customize) screen.
    SkinSelect,
    /// Type 10: a system sound set (LR2 `.lr2ss`), chosen like a skin.
    SoundSet,
    CourseResult,
    Other(i32),
}

impl SkinType {
    pub fn from_id(i: i32) -> SkinType {
        match i {
            0 => SkinType::Play7,
            1 => SkinType::Play5,
            2 => SkinType::Play14,
            3 => SkinType::Play10,
            4 => SkinType::Play9,
            5 => SkinType::Select,
            6 => SkinType::Decide,
            7 => SkinType::Result,
            8 => SkinType::KeyConfig,
            9 => SkinType::SkinSelect,
            10 => SkinType::SoundSet,
            12 => SkinType::Battle7,
            13 => SkinType::Battle5,
            15 => SkinType::CourseResult,
            n => SkinType::Other(n),
        }
    }

    /// The type's number (LR2 `#INFORMATION`, beatoraja `type`).
    pub fn id(self) -> i32 {
        match self {
            SkinType::Play7 => 0,
            SkinType::Play5 => 1,
            SkinType::Play14 => 2,
            SkinType::Play10 => 3,
            SkinType::Play9 => 4,
            SkinType::Select => 5,
            SkinType::Decide => 6,
            SkinType::Result => 7,
            SkinType::KeyConfig => 8,
            SkinType::SkinSelect => 9,
            SkinType::SoundSet => 10,
            SkinType::Battle7 => 12,
            SkinType::Battle5 => 13,
            SkinType::CourseResult => 15,
            SkinType::Other(n) => n,
        }
    }

    pub fn mode(self) -> Option<vibebms_core::PlayMode> {
        use vibebms_core::PlayMode::*;
        Some(match self {
            SkinType::Play7 => Beat7K,
            SkinType::Play5 => Beat5K,
            SkinType::Play14 | SkinType::Battle7 => Beat14K,
            SkinType::Play10 | SkinType::Battle5 => Beat10K,
            SkinType::Play9 => Popn9K,
            _ => return None,
        })
    }

    /// Key of non-play skins in `config.skins` (play skins use the mode label).
    pub fn config_key(self) -> Option<&'static str> {
        Some(match self {
            SkinType::Select => "SELECT",
            SkinType::Decide => "DECIDE",
            SkinType::Result => "RESULT",
            SkinType::CourseResult => "COURSE_RESULT",
            SkinType::KeyConfig => "KEY_CONFIG",
            SkinType::SkinSelect => "SKIN_SELECT",
            SkinType::SoundSet => "SOUND_SET",
            SkinType::Battle7 => "7KEYS BATTLE",
            SkinType::Battle5 => "5KEYS BATTLE",
            _ => return None,
        })
    }

    /// The `config.skins` key of every type (play skins by mode label).
    pub fn key(self) -> Option<&'static str> {
        if self.is_battle() {
            return self.config_key();
        }
        self.mode().map(|m| m.label()).or_else(|| self.config_key())
    }

    /// A battle play skin (types 12 / 13).
    pub fn is_battle(self) -> bool {
        matches!(self, SkinType::Battle7 | SkinType::Battle5)
    }

    /// The battle type for a double play type and back (what a battle
    /// slot accepts besides its own type).
    pub fn battle_pair(self) -> Option<SkinType> {
        Some(match self {
            SkinType::Battle7 => SkinType::Play14,
            SkinType::Battle5 => SkinType::Play10,
            SkinType::Play14 => SkinType::Battle7,
            SkinType::Play10 => SkinType::Battle5,
            _ => return None,
        })
    }
}

/// beatoraja skin select settings (`skinSelect` of a JSON / Lua skin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkinSelectInfo {
    /// The skin type shown first (`defaultCategory`).
    pub default_category: i32,
    /// Option rows the skin shows (`customPropertyCount`; 0: as many as
    /// it has buttons 220-229 for).
    pub custom_property_count: usize,
}

/// A position adjustment a skin offers (beatoraja `offset`): objects
/// referring to `id` move by the user's values.
#[derive(Clone, Debug)]
pub struct CustomOffset {
    pub name: String,
    pub id: i32,
    /// Which of x, y, w, h, angle, alpha the user may change.
    pub fields: [bool; 6],
}

/// Names of the offset fields, as in `CustomOffset::fields`.
pub const OFFSET_FIELDS: [&str; 6] = ["X", "Y", "W", "H", "ANGLE", "ALPHA"];

/// The adjustable offsets of a skin file.
pub fn custom_offsets(path: &std::path::Path) -> Vec<CustomOffset> {
    match SkinFormat::of(path) {
        Some(SkinFormat::Lr2) => lr2::custom_offsets(path),
        Some(SkinFormat::Beatoraja) => beatoraja::custom_offsets(path),
        _ => Vec::new(),
    }
}

/// A skin's user option (LR2 `#CUSTOMOPTION`, beatoraja `property`), or
/// a choice of files (LR2 `#CUSTOMFILE`, beatoraja `filepath`).
#[derive(Clone, Debug)]
pub struct CustomOption {
    pub name: String,
    pub first_id: i32,
    pub labels: Vec<String>,
    /// Option id of each choice (none for a choice of files).
    pub ops: Vec<i32>,
    /// Choice used when none is set.
    pub default: usize,
}

impl CustomOption {
    /// Index of the chosen label (else the default).
    pub fn chosen(&self, label: Option<&str>) -> usize {
        label
            .and_then(|l| self.labels.iter().position(|x| x == l))
            .unwrap_or(self.default)
    }
}

/// The custom options of a skin file, without building the skin.
pub fn custom_options(path: &std::path::Path) -> Vec<CustomOption> {
    match SkinFormat::of(path) {
        Some(SkinFormat::Lr2) => lr2::custom_options(path),
        Some(SkinFormat::Beatoraja) => beatoraja::custom_options(path),
        _ => Vec::new(),
    }
}

/// Decide draw conditions on the skin's own (custom) options when it
/// loads: they cannot change while a screen is up, and the screens do not
/// know them. Objects whose such conditions fail are dropped.
pub fn resolve_static_ops(skin: &mut Skin, known: &HashMap<i32, bool>) {
    let holds = |op: i32| known.get(&op.abs()).map(|v| *v == (op > 0));
    skin.objects.retain_mut(|o| {
        let mut ok = true;
        o.dst.ops.retain(|op| match holds(*op) {
            Some(v) => {
                ok &= v;
                false
            }
            None => true,
        });
        ok
    });
}

/// The files chosen for a skin's custom files (by name; defaults left out).
pub fn user_files(
    opts: &[CustomOption],
    chosen: Option<&std::collections::BTreeMap<String, String>>,
) -> HashMap<String, String> {
    opts.iter()
        .filter(|o| o.ops.is_empty())
        .filter_map(|o| {
            let l = chosen?.get(&o.name)?;
            o.labels.contains(l).then(|| (o.name.clone(), l.clone()))
        })
        .collect()
}

/// Option ids to enable / disable for the chosen labels (by option name).
pub fn user_options(
    opts: &[CustomOption],
    chosen: Option<&std::collections::BTreeMap<String, String>>,
) -> std::collections::HashMap<i32, bool> {
    let mut out = std::collections::HashMap::new();
    for o in opts {
        let k = o.chosen(chosen.and_then(|c| c.get(&o.name)).map(String::as_str));
        for (i, op) in o.ops.iter().enumerate() {
            out.insert(*op, i == k);
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct Skin {
    pub path: PathBuf,
    pub name: String,
    pub author: String,
    pub kind: SkinType,
    pub width: f32,
    pub height: f32,
    pub images: Vec<Option<PathBuf>>,
    pub objects: Vec<Obj>,
    /// Lane skins indexed by our lane numbering.
    pub lanes: Vec<LaneSkin>,
    pub judges: [JudgeSkin; 3],
    /// Bar (measure) lines, one per lane group (both sides in DP).
    pub bar_lines: Vec<(Src, Dst)>,
    pub songlist: Option<SongListSkin>,
    pub custom_options: Vec<CustomOption>,
    pub scene_ms: i64,
    pub input_ms: i64,
    pub fadeout_ms: i64,
    /// beatoraja `#DST_NOTE_EXPANSION_RATE` (PMS): notes grow to w, h % on
    /// each beat and shrink back.
    pub note_expansion: Option<(f32, f32)>,
    /// beatoraja `#DST_NOTE2` (PMS): missed notes fall below the judge line
    /// and vanish at this y.
    pub note_fall_y: Option<f32>,
    /// beatoraja `#JUDGETIMER`: the worst judge that starts the bomb timers
    /// (else GREAT).
    pub judge_timer: Option<Judge>,
    /// LR2 `#PLAYSTART`: ms from ready to the chart's start (else 1500).
    pub play_start_ms: Option<i64>,
    /// LR2 `#CLOSE`: ms the failed scene runs (else 1500).
    pub close_ms: Option<i64>,
    pub finish_margin_ms: i64,
    /// Script values (beatoraja Lua functions / scripts).
    pub script: Option<std::rc::Rc<script::Runtime>>,
    /// LR2 bitmap fonts (`#LR2FONT`).
    pub fonts: Vec<BitmapFont>,
    /// LR2 `#TRANSCOLOR`: this colour is transparent in images without
    /// an alpha channel.
    pub trans_color: Option<[u8; 3]>,
    /// User values of the skin's offsets (x, y, w, h, angle, alpha) by id.
    pub offsets: HashMap<i32, [f32; 6]>,
    /// Skin select screens: their settings.
    pub skin_select: Option<SkinSelectInfo>,
    /// vibeBMS `perspective`: the note field is drawn as lanes in
    /// perspective (`scene::play_persp`), filling the canvas.
    pub perspective: bool,
    /// The ops of the skin's own options in effect (each property's
    /// chosen item), for what the game draws itself.
    pub options_on: std::collections::HashSet<i32>,
}

impl Skin {
    /// An empty skin of `kind`, `width` x `height`.
    pub fn new(path: &std::path::Path, kind: SkinType, width: f32, height: f32) -> Skin {
        Skin {
            path: path.to_path_buf(),
            name: String::new(),
            author: String::new(),
            kind,
            width,
            height,
            images: Vec::new(),
            objects: Vec::new(),
            lanes: Vec::new(),
            judges: Default::default(),
            bar_lines: Vec::new(),
            songlist: None,
            custom_options: Vec::new(),
            scene_ms: 0,
            input_ms: 0,
            fadeout_ms: 0,
            note_expansion: None,
            note_fall_y: None,
            judge_timer: None,
            play_start_ms: None,
            close_ms: None,
            finish_margin_ms: 0,
            script: None,
            fonts: Vec::new(),
            trans_color: None,
            offsets: HashMap::new(),
            skin_select: None,
            perspective: false,
            options_on: Default::default(),
        }
    }

    /// Widen or heighten the canvas to `w` x `h`, moving the objects with
    /// an `anchor` (the others stay where they are).
    pub fn reflow(&mut self, w: f32, h: f32) {
        let (dw, dh) = (w - self.width, h - self.height);
        let shift = |d: &mut Dst| {
            if let Some([ax, ay, sw, sh]) = d.anchor {
                for k in &mut d.keys {
                    k.rect.x += ax * dw;
                    k.rect.y += ay * dh;
                    k.rect.w += sw * dw;
                    k.rect.h += sh * dh;
                }
            }
        };
        for o in &mut self.objects {
            shift(&mut o.dst);
        }
        for j in &mut self.judges {
            for (_, d) in j.judge.iter_mut().flatten() {
                shift(d);
            }
        }
        self.width = w;
        self.height = h;
    }

    /// Which images something of the skin draws from (a skin may list
    /// more: for options not chosen, or never used).
    pub fn used_images(&self) -> Vec<bool> {
        let mut used = vec![false; self.images.len()];
        let mut mark = |s: &Src| {
            if let Some(u) = used.get_mut(s.image) {
                *u = true;
            }
        };
        for o in &self.objects {
            match &o.kind {
                Kind::Image(s)
                | Kind::JudgeLine(s)
                | Kind::Number { src: s, .. }
                | Kind::Bar { src: s, .. }
                | Kind::Slider { src: s, .. }
                | Kind::Gauge { src: s, .. }
                | Kind::Cover { src: s, .. }
                | Kind::Distribution { src: Some(s), .. } => mark(s),
                Kind::ImageSet { srcs, .. } => srcs.iter().for_each(&mut mark),
                _ => {}
            }
        }
        for l in &self.lanes {
            [
                &l.note,
                &l.ln_start,
                &l.ln_end,
                &l.ln_body,
                &l.ln_body_active,
                &l.mine,
                &l.hcn_start,
                &l.hcn_end,
                &l.hcn_body,
                &l.hcn_body_active,
                &l.hcn_damage,
                &l.hcn_reactive,
            ]
            .into_iter()
            .flatten()
            .for_each(&mut mark);
        }
        for j in &self.judges {
            j.judge.iter().flatten().for_each(|(s, _)| mark(s));
            j.combo.iter().flatten().for_each(|(s, ..)| mark(s));
        }
        self.bar_lines.iter().for_each(|(s, _)| mark(s));
        if let Some(sl) = &self.songlist {
            sl.body.iter().flatten().for_each(&mut mark);
            sl.level.iter().flatten().for_each(|(f, _)| mark(&f.src));
            (sl.lamp.iter().chain(&sl.my_lamp).chain(&sl.rival_lamp))
                .flatten()
                .for_each(|(s, _)| mark(s));
            if let Some((_, Some(s), _)) = &sl.graph {
                mark(s);
            }
        }
        for f in &self.fonts {
            for (img, _) in f.glyphs.values() {
                if let Some(u) = used.get_mut(*img) {
                    *u = true;
                }
            }
        }
        used
    }
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

/// Game state queried by the skin each frame.
pub trait SkinState {
    /// Start time (scene ms) of a timer, `None` if off. Timer 0 = scene start.
    fn timer(&self, id: i32) -> Option<i64>;
    /// LR2 option (negative id = negated).
    fn option(&self, id: i32) -> bool;
    fn number(&self, id: i32) -> Option<i64>;
    fn text(&self, id: i32) -> Option<String>;
    /// 0..1 for bar graphs and sliders.
    fn rate(&self, id: i32) -> f32;
    /// Index of the image an image set / button shows (button values are
    /// a namespace of their own; defaults to the number with that id).
    fn button(&self, id: i32) -> Option<i64> {
        self.number(id)
    }
    /// Clear lamp counts (by clear type, 11) or DJ rank counts (AAA..F,
    /// then no play) of the selected folder.
    fn distribution(&self, rank: bool) -> Option<Vec<u32>> {
        let _ = rank;
        None
    }
    /// beatoraja offset (x, y, w, h, angle, alpha) in skin pixels, y up.
    fn offset(&self, id: i32) -> Option<[f32; 6]> {
        let _ = id;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct St;
    impl SkinState for St {
        fn timer(&self, id: i32) -> Option<i64> {
            (id == 41).then_some(1000)
        }
        fn option(&self, id: i32) -> bool {
            id >= 0
        }
        fn number(&self, _: i32) -> Option<i64> {
            None
        }
        fn text(&self, _: i32) -> Option<String> {
            None
        }
        fn rate(&self, _: i32) -> f32 {
            0.0
        }
    }

    fn key(t: i64, x: f32) -> Key {
        Key {
            time: t,
            rect: Rect::new(x, 0.0, 10.0, 10.0),
            color: [1.0; 4],
            angle: 0.0,
        }
    }

    #[test]
    fn a_csv_skin_is_an_lr2_skin_throughout() {
        let dir = std::env::temp_dir().join(format!("vibebms-csv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.csv");
        std::fs::write(
            &p,
            "#INFORMATION,7,CSV skin,me\n#CUSTOMOPTION,Lane,900,Dark,Light\n\
             #CUSTOMOFFSET,Judge,10,1,1,0,0,0,1\n",
        )
        .unwrap();
        assert_eq!(header(&p), Some((SkinType::Result, "CSV skin".into())));
        assert_eq!(custom_options(&p).len(), 1);
        assert_eq!(custom_offsets(&p).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn image_sizes_from_the_header_or_the_whole_file() {
        let dir = std::env::temp_dir().join(format!("vibebms-imgsize-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbImage::new(300, 200);
        let (png, jpg) = (dir.join("a.png"), dir.join("a.jpg"));
        img.save(&png).unwrap();
        // a JPEG whose frame header is past the first 64 KiB
        let mut b = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Jpeg)
            .unwrap();
        let mut big = b[..2].to_vec();
        for _ in 0..2 {
            big.extend([0xFF, 0xE1, 0xFF, 0xFF]);
            big.resize(big.len() + 0xFFFD, 0);
        }
        big.extend(&b[2..]);
        std::fs::write(&jpg, big).unwrap();
        assert_eq!(image_size(&png), Some((300, 200)));
        assert_eq!(image_size(&jpg), Some((300, 200)));
        assert_eq!(image_size(&dir.join("none.png")), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn custom_options_map_to_op_ids() {
        let dir = std::env::temp_dir().join(format!("vibebms-opt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.lr2skin");
        std::fs::write(
            &p,
            "#INFORMATION,0,S,me\n#CUSTOMOPTION,Lane,900,Dark,Light,Glow\n#CUSTOMOPTION,Judge,910,A,B\n",
        )
        .unwrap();
        let opts = custom_options(&p);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].ops, vec![900, 901, 902]);
        let mut chosen = std::collections::BTreeMap::new();
        chosen.insert("Lane".to_string(), "Glow".to_string());
        let m = user_options(&opts, Some(&chosen));
        assert_eq!((m[&900], m[&901], m[&902]), (false, false, true));
        // unset options use their default (the first choice for LR2)
        assert_eq!((m[&910], m[&911]), (true, false));
    }

    #[test]
    fn custom_files_are_options() {
        let dir = std::env::temp_dir().join(format!("vibebms-cfile-{}", std::process::id()));
        let theme = dir.join("MyTheme");
        std::fs::create_dir_all(theme.join("play/bg")).unwrap();
        for f in ["a.png", "b.png", "c.png"] {
            std::fs::write(theme.join("play/bg").join(f), b"").unwrap();
        }
        // the theme folder renamed: found by the path's folders
        let p = theme.join("play/s.lr2skin");
        std::fs::write(
            &p,
            "#INFORMATION,0,S,me\n#CUSTOMFILE,Back,LR2files\\Theme\\Other\\play\\bg\\*.png,b\n",
        )
        .unwrap();
        let opts = custom_options(&p);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].labels, vec!["a", "b", "c"]);
        assert_eq!(opts[0].default, 1);
        let mut chosen = std::collections::BTreeMap::new();
        chosen.insert("Back".to_string(), "c".to_string());
        assert_eq!(user_files(&opts, Some(&chosen))["Back"], "c");
    }

    #[test]
    fn keyframes_loop_and_timer() {
        let mut d = Dst {
            loop_ms: 0,
            ..Default::default()
        };
        d.add_key(key(0, 0.0));
        d.add_key(key(1000, 100.0));
        assert_eq!(d.eval(500, &St).unwrap().rect.x, 50.0);
        assert_eq!(d.eval(1500, &St).unwrap().rect.x, 50.0); // looped
        d.loop_ms = -1;
        assert!(d.eval(1500, &St).is_none());
        d.timer = 41;
        d.loop_ms = 0;
        assert_eq!(d.eval(1250, &St).unwrap().rect.x, 25.0);
        d.timer = 46; // off
        assert!(d.eval(1250, &St).is_none());
        d.timer = 0;
        d.ops = vec![-5];
        assert!(d.eval(10, &St).is_none());
        d.ops.clear();
        d.acc = 1;
        assert_eq!(d.eval(500, &St).unwrap().rect.x, 25.0);
        // a single keyframe at 30 ms, loop 0: shown from 30 ms on (LR2)
        let mut one = Dst {
            loop_ms: 0,
            ..Default::default()
        };
        one.add_key(key(30, 7.0));
        assert!(one.eval(10, &St).is_none());
        for t in [30, 59, 60, 1000, 123_457] {
            assert_eq!(one.eval(t, &St).unwrap().rect.x, 7.0, "{t}");
        }
    }
}
