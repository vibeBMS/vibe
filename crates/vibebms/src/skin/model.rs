//! The model a skin is loaded into, apart from how it was written (LR2,
//! beatoraja) and from how it is drawn.

use super::graph::Graph;
use super::options::CustomOption;
use super::types::{Acc, Align, Dir, Overflow, SkinSelectInfo, SkinType, Stretch};
use super::{guard, script};
use crate::gfx::Rect;
use std::collections::HashMap;
use std::path::PathBuf;
use vibebms_core::judge::Judge;

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
    pub acc: Acc,
    pub blend: crate::gfx::Blend,
    pub center: usize,
    pub loop_ms: i64,
    pub timer: i32,
    pub ops: Vec<i32>,
    /// beatoraja offsets added from game state (`SkinState::offset`).
    pub offsets: Vec<i32>,
    /// How an image fits the destination.
    pub stretch: Stretch,
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

#[derive(Clone, Debug)]
pub enum Kind {
    Image(Src),
    /// Image supplied by the game (stagefile 100, backbmp 101, banner 102 ...).
    RefImage(i32),
    Number {
        id: i32,
        font: NumberFont,
    },
    Text {
        id: i32,
        /// A fixed text (beatoraja `constantText`) instead of text `id`.
        constant: Option<String>,
        style: TextStyle,
    },
    Bar {
        src: Src,
        id: i32,
        dir: Dir,
        /// `id` is a number scaled from this range (beatoraja
        /// `#SRC_BARGRAPH_REFNUMBER`), not a 0..1 rate.
        minmax: Option<(f32, f32)>,
    },
    Slider {
        src: Src,
        id: i32,
        dir: Dir,
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
    Distribution(CountBar),
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

/// How a number is set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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

/// A number font: its digit images and how they are set.
#[derive(Clone, Debug)]
pub struct NumberFont {
    pub src: Src,
    pub style: NumberStyle,
}

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

/// A folder's clear lamp (`rank` false) or DJ rank counts as a stacked
/// bar; `src` frames are the segment images per lamp.
#[derive(Clone, Debug, Default)]
pub struct CountBar {
    pub src: Option<Src>,
    pub rank: bool,
}

/// `SongListSkin::label`'s kinds (LR2 `BAR_LABEL` indices).
pub const BAR_LABEL_LN: usize = 0;
pub const BAR_LABEL_RANDOM: usize = 1;
pub const BAR_LABEL_MINE: usize = 2;
pub const BAR_LABEL_CN: usize = 3;
pub const BAR_LABEL_HCN: usize = 4;
pub const BAR_LABELS: usize = 5;

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
    /// Title.
    pub text: Option<SongTitle>,
    /// vibeBMS `foldertext`: the title's destination on bars other than
    /// charts (folders, courses, commands), else `text`'s.
    pub folder_text: Option<Dst>,
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
    pub graph: Option<FolderGraph>,
    /// Labels on chart and course bars (LR2 `BAR_LABEL`), by `BAR_LABEL_*`:
    /// long notes, `#RANDOM`, mines, CN, HCN.
    pub label: Vec<Option<(Src, Dst)>>,
    /// A class course's trophy achieved (LR2 `BAR_TROPHY`): bronze,
    /// silver, gold.
    pub trophy: Vec<Option<(Src, Dst)>>,
    /// vibeBMS `count`: the charts of a folder bar, the stages of a course.
    pub count: Option<(NumberFont, Dst)>,
}

/// The title of a bar of the select list.
#[derive(Clone, Debug, Default)]
pub struct SongTitle {
    pub dst: Dst,
    pub style: TextStyle,
}

/// The lamp / rank counts on a folder bar.
#[derive(Clone, Debug, Default)]
pub struct FolderGraph {
    pub dst: Dst,
    pub bar: CountBar,
}

/// Per-judgement image + combo number of a `NOWJUDGE` group.
#[derive(Clone, Debug, Default)]
pub struct JudgeSkin {
    pub judge: [Option<(Src, Dst)>; 6],
    pub combo: [Option<ComboSkin>; 6],
    pub shift: bool,
}

/// A combo number beside a judgement (its position relative to it).
#[derive(Clone, Debug)]
pub struct ComboSkin {
    pub font: NumberFont,
    pub dst: Dst,
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

#[derive(Clone, Debug)]
pub struct Skin {
    pub path: PathBuf,
    pub name: String,
    pub author: String,
    pub kind: SkinType,
    pub width: f32,
    pub height: f32,
    pub images: Vec<Option<PathBuf>>,
    /// Images loaded when first drawn (vibeBMS's `lazy` sources: the
    /// built-in select skin's panels).
    pub lazy_images: Vec<usize>,
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
    /// LR2 `#HORIZONTAL`: the note field scrolls right to left. Each
    /// lane's rect runs from the judge position (its left edge) to the
    /// right edge of the canvas; `LaneSkin::note_h` is then the notes'
    /// width.
    pub horizontal: bool,
    /// The ops of the skin's own options in effect (each property's
    /// chosen item), for what the game draws itself.
    pub options_on: std::collections::HashSet<i32>,
    /// The file's load as [`guard`] notes it: past once the skin is drawn
    /// ready, or let go.
    pub note: Option<std::sync::Arc<guard::Note>>,
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
            lazy_images: Vec::new(),
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
            horizontal: false,
            options_on: Default::default(),
            note: None,
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
                | Kind::Bar { src: s, .. }
                | Kind::Slider { src: s, .. }
                | Kind::Gauge { src: s, .. }
                | Kind::Cover { src: s, .. }
                | Kind::Distribution(CountBar { src: Some(s), .. }) => mark(s),
                Kind::Number { font, .. } => mark(&font.src),
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
            j.combo.iter().flatten().for_each(|c| mark(&c.font.src));
        }
        self.bar_lines.iter().for_each(|(s, _)| mark(s));
        if let Some(sl) = &self.songlist {
            sl.body.iter().flatten().for_each(&mut mark);
            sl.level.iter().flatten().for_each(|(f, _)| mark(&f.src));
            (sl.lamp.iter().chain(&sl.my_lamp).chain(&sl.rival_lamp))
                .flatten()
                .for_each(|(s, _)| mark(s));
            if let Some(s) = sl.graph.as_ref().and_then(|g| g.bar.src.as_ref()) {
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
