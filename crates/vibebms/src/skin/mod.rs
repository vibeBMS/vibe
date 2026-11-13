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
pub mod render;

use crate::gfx::Rect;
use std::path::PathBuf;

/// Load any supported skin file (LR2 CSV, beatoraja JSON or Lua).
pub fn load_file(
    path: &std::path::Path,
    skins_root: &std::path::Path,
    user_options: &std::collections::HashMap<i32, bool>,
    static_option: &dyn Fn(i32) -> Option<bool>,
    image_size: &dyn Fn(&std::path::Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let is_lr2 = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("lr2skin") || e.eq_ignore_ascii_case("csv"))
        .unwrap_or(false);
    if is_lr2 {
        lr2::load(path, skins_root, user_options, static_option, image_size)
    } else {
        beatoraja::load(path, user_options, image_size)
    }
}

/// Pixel size of an image file (through the VFS).
pub fn image_size(p: &std::path::Path) -> Option<(u32, u32)> {
    crate::platform::vfs::read(p).ok().and_then(|b| {
        image::ImageReader::new(std::io::Cursor::new(b))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    })
}

/// (type, name) of any supported skin file.
pub fn header(path: &std::path::Path) -> Option<(SkinType, String)> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "lr2skin" => lr2::header(path),
        "json" | "luaskin" => beatoraja::header(path),
        _ => None,
    }
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
}

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
    pub additive: bool,
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
        } else if end > 0 && t > self.loop_ms {
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
            additive: self.blend == 2,
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
        align: i32,
        /// Text wider than the destination: 0 overflows (LR2, beatoraja
        /// default), 1 shrinks to fit, 2 is cut off.
        overflow: i32,
    },
    Bar {
        src: Src,
        id: i32,
        dir: i32,
    },
    Slider {
        src: Src,
        id: i32,
        dir: i32,
        range: f32,
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
    },
    Bga,
    /// One of several images chosen by a skin number (beatoraja `imageset`).
    ImageSet {
        srcs: Vec<Src>,
        id: i32,
    },
    /// A graph drawn by the scene (result gauge / judge / timing graphs ...).
    Graph(GraphKind),
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
    /// Title text: (destination, align).
    /// Title text: (destination, align, overflow as in `Kind::Text`).
    pub text: Option<(Dst, i32, i32)>,
    /// Level number per difficulty (0..=6).
    pub level: Vec<Option<(NumberFont, Dst)>>,
    /// Clear lamp per clear type (beatoraja order, 0..=10).
    pub lamp: Vec<Option<(Src, Dst)>>,
    /// Lamp / rank distribution on folder bars.
    pub graph: Option<(Dst, Option<Src>, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphKind {
    /// Gauge over the course of the play (result).
    Gauge,
    /// beatoraja `judgegraph` type: 0 note density, 1 judgements, 2 early/late.
    Judge(i32),
    /// BPM over the chart.
    Bpm,
    /// Distribution of timing errors.
    TimingDistribution,
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
    pub note: Option<Src>,
    pub ln_start: Option<Src>,
    pub ln_end: Option<Src>,
    pub ln_body: Option<Src>,
    pub ln_body_active: Option<Src>,
    pub mine: Option<Src>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinType {
    Play7,
    Play5,
    Play14,
    Play10,
    Play9,
    Select,
    Decide,
    Result,
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
            15 => SkinType::CourseResult,
            n => SkinType::Other(n),
        }
    }

    pub fn mode(self) -> Option<vibebms_core::PlayMode> {
        use vibebms_core::PlayMode::*;
        Some(match self {
            SkinType::Play7 => Beat7K,
            SkinType::Play5 => Beat5K,
            SkinType::Play14 => Beat14K,
            SkinType::Play10 => Beat10K,
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
            _ => return None,
        })
    }
}

/// A skin's user option (LR2 `#CUSTOMOPTION`, beatoraja `property`).
#[derive(Clone, Debug)]
pub struct CustomOption {
    pub name: String,
    pub first_id: i32,
    pub labels: Vec<String>,
    /// Option id of each choice.
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
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "lr2skin" | "csv" => lr2::custom_options(path),
        "json" | "luaskin" => beatoraja::custom_options(path),
        _ => Vec::new(),
    }
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
    pub judges: [JudgeSkin; 2],
    /// Bar (measure) lines, one per lane group (both sides in DP).
    pub bar_lines: Vec<(Src, Dst)>,
    pub songlist: Option<SongListSkin>,
    pub custom_options: Vec<CustomOption>,
    pub scene_ms: i64,
    pub input_ms: i64,
    pub fadeout_ms: i64,
    pub finish_margin_ms: i64,
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
    }
}
