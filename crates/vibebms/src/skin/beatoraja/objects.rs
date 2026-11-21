//! The objects of a skin: definitions found by id, made into the model's
//! kinds by family.

use super::files::{load_bmfont, skin_path, FilePath};
use super::songlist::songlist;
use super::*;

pub(super) struct Ctx<'a> {
    pub sk: &'a Value,
    /// `.fnt` fonts read so far (index in the skin's fonts).
    bitmap_fonts: HashMap<PathBuf, Option<usize>>,
    home: &'a Home,
    fps: &'a [FilePath],
    files: &'a HashMap<String, String>,
    pub h: f32,
    pub images: ImageFiles<'a>,
    source_index: HashMap<Cow<'a, str>, usize>,
    /// Definitions by list name and id (the first of an id counts).
    index: RefCell<HashMap<&'static str, HashMap<Cow<'a, str>, &'a Value>>>,
    pub scripts: RefCell<script::Builder>,
}

impl<'a> Ctx<'a> {
    /// The skin `sk` whose files are at `home`, its sources listed (a
    /// `filepath` customisation: the file chosen for its wildcard).
    pub fn new(
        sk: &'a Value,
        home: &'a Home,
        fps: &'a [FilePath],
        files: &'a HashMap<String, String>,
        image_size: &'a dyn Fn(&Path) -> Option<(u32, u32)>,
        scripts: script::Builder,
    ) -> Ctx<'a> {
        let mut cx = Ctx {
            sk,
            bitmap_fonts: HashMap::new(),
            home,
            fps,
            files,
            h: i(sk, "h", 720) as f32,
            images: ImageFiles::new(image_size),
            source_index: HashMap::new(),
            index: Default::default(),
            scripts: RefCell::new(scripts),
        };
        for src in arr(sk.get("source")) {
            let p = skin_path(home, fps, files, s(src, "path"));
            cx.source_index.insert(id_of(src), cx.images.len());
            cx.images.push(p);
        }
        cx
    }

    /// A field holding an id or a script (Lua function / source): the id
    /// (synthetic for scripts).
    pub(super) fn dyn_id(&self, v: Option<&Value>, kind: ScriptKind) -> Option<i32> {
        let v = v?;
        match num(v) {
            Some(n) => Some(n as i32),
            None => self.scripts.borrow_mut().register(v, kind),
        }
    }

    /// `ref` (an id), else `value` (an id or a script).
    pub(super) fn value_id(&self, def: &Value, kind: ScriptKind) -> i32 {
        match def.get("ref").and_then(num) {
            Some(r) if r != 0 => r as i32,
            _ => self.dyn_id(def.get("value"), kind).unwrap_or(0),
        }
    }

    pub(super) fn src(&self, def: &Value) -> Option<Src> {
        let img = *self.source_index.get(&key(def.get("src")?)?)?;
        let (iw, ih) = self.images.size(img)?;
        let (x, y) = (i(def, "x", 0) as f32, i(def, "y", 0) as f32);
        let w = match i(def, "w", -1) {
            n if n <= 0 => iw as f32,
            n => n as f32,
        };
        let h = match i(def, "h", -1) {
            n if n <= 0 => ih as f32,
            n => n as f32,
        };
        Some(Src {
            image: img,
            frames: (self.images).grid([x, y, w, h], i(def, "divx", 1), i(def, "divy", 1))?,
            cycle: i(def, "cycle", 0),
            timer: self
                .dyn_id(def.get("timer"), ScriptKind::Timer)
                .unwrap_or(0),
        })
    }

    pub(super) fn find(&self, list: &'static str, id: &str) -> Option<&'a Value> {
        let sk = self.sk;
        let mut index = self.index.borrow_mut();
        let defs = index.entry(list).or_insert_with(|| {
            let mut m = HashMap::new();
            for x in arr(sk.get(list)) {
                m.entry(id_of(x)).or_insert(x);
            }
            m
        });
        defs.get(id).copied()
    }

    /// beatoraja destinations are y-up; ours are y-down.
    pub(super) fn dst(&self, d: &Value) -> Dst {
        let mut out = Dst {
            blend: i(d, "blend", 0) as i32,
            center: i(d, "center", 0).clamp(0, 9) as usize,
            loop_ms: i(d, "loop", 0),
            timer: self.dyn_id(d.get("timer"), ScriptKind::Timer).unwrap_or(0),
            // option ids, or conditions given as scripts (also `draw`)
            ops: arr(d.get("op"))
                .iter()
                .chain(d.get("draw"))
                .filter_map(|o| self.dyn_id(Some(o), ScriptKind::Bool))
                .filter(|o| *o != 0)
                .collect(),
            offsets: d
                .get("offset")
                .and_then(num)
                .into_iter()
                .chain(arr(d.get("offsets")).iter().filter_map(num))
                .map(|o| o as i32)
                .filter(|o| *o != 0)
                .collect(),
            anchor: d.get("anchor").map(|a| {
                let v =
                    |k: usize| arr(Some(a)).get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                [v(0), v(1), v(2), v(3)]
            }),
            ..Default::default()
        };
        // values may be fractional (computed by Lua skins)
        let mut prev: Option<[f64; 10]> = None;
        for a in arr(d.get("dst")) {
            let g = |k: &str, idx: usize, def: f64| {
                a.get(k)
                    .and_then(Value::as_f64)
                    .unwrap_or(prev.map(|p| p[idx]).unwrap_or(def))
            };
            let cur = [
                g("time", 0, 0.0),
                g("x", 1, 0.0),
                g("y", 2, 0.0),
                g("w", 3, 0.0),
                g("h", 4, 0.0),
                g("acc", 5, 0.0),
                g("a", 6, 255.0),
                g("r", 7, 255.0),
                g("g", 8, 255.0),
                g("b", 9, 255.0),
            ];
            let angle = a
                .get("angle")
                .and_then(Value::as_f64)
                .unwrap_or(out.keys.last().map(|k| k.angle as f64).unwrap_or(0.0))
                as f32;
            if out.keys.is_empty() {
                out.acc = cur[5] as i32;
            }
            let (x, y, w, h) = (cur[1] as f32, cur[2] as f32, cur[3] as f32, cur[4] as f32);
            let c = |n: f64| n.clamp(0.0, 255.0) as f32 / 255.0;
            out.add_key(Key {
                time: cur[0] as i64,
                rect: Rect::new(x, self.h - (y + h), w, h),
                color: [c(cur[7]), c(cur[8]), c(cur[9]), c(cur[6])],
                angle: -angle,
            });
            prev = Some(cur);
        }
        // shown while the mouse is over this part (y up from the bottom)
        if let (Some(m), Some(k)) = (d.get("mouseRect"), out.keys.first()) {
            let (w, h) = (i(m, "w", 0) as f32, i(m, "h", 0) as f32);
            let y = k.rect.h - i(m, "y", 0) as f32 - h;
            out.hover = Some(Rect::new(i(m, "x", 0) as f32, y, w, h));
        }
        out
    }

    /// The bitmap font (`.fnt`, BMFont text format) a text uses: its index
    /// in `fonts`, its page images added to the skin's images.
    pub(super) fn bitmap_font(&mut self, t: &Value, fonts: &mut Vec<BitmapFont>) -> Option<usize> {
        let f = self.find("font", &key(t.get("font")?)?)?;
        let p = skin_path(self.home, self.fps, self.files, s(f, "path"))?;
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fnt")) {
            return None;
        }
        if let Some(i) = self.bitmap_fonts.get(&p) {
            return *i;
        }
        // types 1 and 2 are distance fields
        let sdf = matches!(i(f, "type", 0), 1 | 2);
        let font = load_bmfont(&p, self.home).map(|(mut font, pages)| {
            font.sdf = sdf;
            let base = self.images.len();
            for page in pages {
                let size = (self.images.measure)(&page);
                self.images.push_measured(page, size);
            }
            for g in font.glyphs.values_mut() {
                g.0 += base;
            }
            fonts.push(font);
            fonts.len() - 1
        });
        self.bitmap_fonts.insert(p, font);
        font
    }

    /// The TrueType / OpenType font file a text uses (`font`, an id in the
    /// skin's `font` list); bitmap fonts are not drawn with.
    pub(super) fn face(&self, t: &Value) -> Option<PathBuf> {
        let f = self.find("font", &key(t.get("font")?)?)?;
        let p = skin_path(self.home, self.fps, self.files, s(f, "path"))?;
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        (matches!(ext.as_str(), "ttf" | "otf" | "ttc") && readable(&p, FONT_MAX)).then_some(p)
    }

    pub(super) fn number_kind(&self, def: &Value) -> Option<Kind> {
        let src = self.src(def)?;
        let n = src.frames.len();
        let digits = if n % 24 == 0 {
            12
        } else if n % 11 == 0 {
            11
        } else if n >= 10 {
            10
        } else {
            return None;
        };
        Some(Kind::Number {
            src,
            digits,
            id: self.value_id(def, ScriptKind::Int),
            align: i(def, "align", 0) as i32,
            keta: i(def, "digit", 1).clamp(1, 32) as usize,
            // beatoraja zeropadding: 0 = none, 1 = '0', 2 = the padding image
            // (the 11th; '0' for fonts without one)
            pad: match i(def, "zeropadding", 0) {
                1 => Some(0),
                2 if digits > 10 => Some(10),
                2 => Some(0),
                _ => None,
            },
        })
    }
}

/// "RRGGBB" / "RRGGBBAA" (beatoraja colours).
pub(crate) fn hex_color(s: &str) -> [f32; 4] {
    let s = s.trim_start_matches('#');
    let b = |k: usize| {
        u8::from_str_radix(s.get(k..k + 2).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0
    };
    [b(0), b(2), b(4), if s.len() >= 8 { b(6) } else { 1.0 }]
}

/// A text's font size (`size`), if it has one.
pub(super) fn text_size(t: &Value) -> Option<f32> {
    let v = t.get("size").and_then(Value::as_f64)? as f32;
    (v > 0.0).then_some(v)
}

/// A text's outline and shadow (`TextFx`), beatoraja's defaults where
/// not given.
pub(super) fn text_fx(t: &Value) -> TextFx {
    let f = |k: &str| t.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    let c = |k: &str| t.get(k).and_then(Value::as_str).map(hex_color);
    let d = TextFx::default();
    TextFx {
        outline_color: c("outlineColor").unwrap_or(d.outline_color),
        outline_width: f("outlineWidth"),
        shadow_color: c("shadowColor").unwrap_or(d.shadow_color),
        shadow_offset: (f("shadowOffsetX"), f("shadowOffsetY")),
        shadow_smoothness: f("shadowSmoothness"),
    }
}

/// beatoraja gaugegraph colours: a `color` list (4 per gauge kind) or the
/// named fields with beatoraja's defaults.
pub(crate) fn gauge_colors(g: &Value) -> Vec<[f32; 4]> {
    let list: Vec<[f32; 4]> = arr(g.get("color"))
        .iter()
        .filter_map(Value::as_str)
        .map(hex_color)
        .collect();
    if list.len() >= 24 {
        return list[..24].to_vec();
    }
    let c = |k: &str, d: &str| hex_color(g.get(k).and_then(Value::as_str).unwrap_or(d));
    let border_line = c("borderlineColor", "ff0000");
    let border_bg = c("borderColor", "440000");
    let line = [
        c("assistClearLineColor", "ff00ff"),
        c("assistAndEasyFailLineColor", "00ffff"),
        c("grooveFailLineColor", "00ff00"),
        c("grooveClearAndHardLineColor", "ff0000"),
        c("exHardLineColor", "ffff00"),
        c("hazardLineColor", "cccccc"),
    ];
    let bg = [
        c("assistClearBGColor", "440044"),
        c("assistAndEasyFailBGColor", "004444"),
        c("grooveFailBGColor", "004400"),
        c("grooveClearAndHardBGColor", "440000"),
        c("exHardBGColor", "444400"),
        c("hazardBGColor", "444444"),
    ];
    (0..6)
        .flat_map(|k| {
            if k < 3 {
                [border_line, border_bg, line[k], bg[k]]
            } else {
                // survival gauges: everything is "above the border"
                [line[k], bg[k], line[k], bg[k]]
            }
        })
        .collect()
}

/// The graph objects (`style: "vibebms"` asks for the built-in look).
/// `isRefNum`: a slider / graph shows number `type` within `min`..`max`
/// (a script `value` is a rate itself).
pub(super) fn ref_num(def: &Value) -> Option<(f32, f32)> {
    let on = def
        .get("isRefNum")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (on && def.get("value").is_none()).then(|| (i(def, "min", 0) as f32, i(def, "max", 0) as f32))
}

fn graph(cx: &Ctx, id: &str) -> Option<Graph> {
    let named = |g: &Value, keys: &[(&str, &str)]| -> Vec<[f32; 4]> {
        keys.iter()
            .map(|(k, d)| hex_color(g.get(*k).and_then(Value::as_str).unwrap_or(d)))
            .collect()
    };
    let built_in = |g: &Value| g.get("style").and_then(Value::as_str) == Some("vibebms");
    if let Some(g) = cx.find("gaugegraph", id) {
        return Some(Graph::Gauge(GaugeGraph {
            colors: (!built_in(g)).then(|| gauge_colors(g)),
            line_width: if built_in(g) {
                0.0
            } else {
                i(g, "lineWidth", 2) as f32
            },
            delay: i(g, "delay", 1500),
        }));
    }
    if let Some(g) = cx.find("judgegraph", id) {
        return Some(Graph::Notes(NotesGraph {
            kind: i(g, "type", 0).clamp(0, 2) as usize,
            delay: i(g, "delay", 500),
            back: i(g, "backTexOff", 0) == 0,
            order_reverse: i(g, "orderReverse", 0) != 0,
            no_gap: i(g, "noGap", 0) != 0,
            no_gap_x: i(g, "noGapX", 0) != 0,
            built_in: built_in(g),
        }));
    }
    if let Some(g) = cx.find("bpmgraph", id) {
        return Some(Graph::Bpm(BpmGraph {
            colors: named(
                g,
                &[
                    ("mainBPMColor", "00ff00"),
                    ("minBPMColor", "0000ff"),
                    ("maxBPMColor", "ff0000"),
                    ("otherBPMColor", "ffff00"),
                    ("stopLineColor", "ff00ff"),
                    ("transitionLineColor", "7f7f7f"),
                ],
            ),
            line_width: i(g, "lineWidth", 2) as f32,
            delay: i(g, "delay", 0),
        }));
    }
    if let Some((g, hit)) = cx
        .find("timingvisualizer", id)
        .map(|g| (g, false))
        .or_else(|| cx.find("hiterrorvisualizer", id).map(|g| (g, true)))
    {
        let d =
            |timing: &'static str, hit_error: &'static str| if hit { hit_error } else { timing };
        let mut keys = vec![
            ("lineColor", d("00FF00FF", "99CCFF80")),
            ("centerColor", "FFFFFFFF"),
            ("PGColor", d("000088FF", "99CCFF80")),
            ("GRColor", d("008800FF", "F2CB3080")),
            ("GDColor", d("888800FF", "14CC8f80")),
            ("BDColor", d("880000FF", "FF1AB380")),
            ("PRColor", d("000000FF", "CC292980")),
        ];
        if hit {
            keys.push(("emaColor", "FF0000FF"));
        }
        return Some(Graph::Visualizer(VisualizerGraph {
            hit_error: hit,
            range_ms: visualizer_range(i(g, "judgeWidthMillis", 150)),
            colors: named(g, &keys),
            line_width: i(g, "lineWidth", 1).clamp(1, 4) as f32,
            transparent_poor: i(g, "transparent", 0) == 1,
            decay: i(g, "drawDecay", 1) == 1,
            by_judge: hit && i(g, "colorMode", 1) == 1,
            ema: hit && i(g, "emaMode", 1) != 0,
        }));
    }
    let g = cx.find("timingdistributiongraph", id)?;
    Some(Graph::Timing(TimingGraph {
        bins: if built_in(g) {
            81
        } else {
            timing_bins(i(g, "width", 301))
        },
        colors: (!built_in(g)).then(|| {
            named(
                g,
                &[
                    ("graphColor", "00FF00FF"),
                    ("averageColor", "FFFFFFFF"),
                    ("devColor", "FFFFFFFF"),
                    ("PGColor", "000088FF"),
                    ("GRColor", "008800FF"),
                    ("GDColor", "888800FF"),
                    ("BDColor", "880000FF"),
                    ("PRColor", "000000FF"),
                ],
            )
        }),
        line_width: i(g, "lineWidth", 1) as f32,
        average: i(g, "drawAverage", 1) != 0,
        deviation: i(g, "drawDev", 1) != 0,
    }))
}

/// Clickable objects: `act` (event id) and `click` (mode) of an image / imageset.
pub(super) fn obj_act(cx: &Ctx, id: &str) -> Option<(i32, i32)> {
    let def = cx.find("image", id).or_else(|| cx.find("imageset", id))?;
    let act = cx.dyn_id(def.get("act"), ScriptKind::Event)?;
    (act != 0).then(|| (act, i(def, "click", 0) as i32))
}

/// What a `destination` draws: each family looks its id up in its own
/// definitions; the first that knows the id makes the object (or nothing,
/// when the definition is unusable).
type Family = fn(&mut Ctx, &mut Skin, &Value, &str, &mut Dst) -> Option<Option<Kind>>;

const FAMILIES: [Family; 15] = [
    image,
    value,
    text,
    slider,
    imageset,
    graph_obj,
    bar_graph,
    note,
    gauge,
    cover,
    game_image,
    skin_preview,
    song_list,
    bga,
    judge,
];

/// The object of destination `d`.
fn object(cx: &mut Ctx, skin: &mut Skin, d: &Value, id: &str, dst: &mut Dst) -> Option<Kind> {
    FAMILIES
        .iter()
        .find_map(|f| f(cx, skin, d, id, dst))
        .flatten()
}

fn image(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let img = cx.find("image", id)?;
    Some(match i(img, "ref", 0) {
        // game-supplied image (100 stage file, 101 back bmp, 102 banner)
        r if r > 0 && img.get("src").is_none() => Some(Kind::RefImage(r as i32)),
        // frames split into `len` groups; the value of `ref` picks one
        r if r > 0 => cx.src(img).map(|src| {
            let len = i(img, "len", 1).max(1) as usize;
            let per = (src.frames.len() / len).max(1);
            let srcs = src
                .frames
                .chunks(per)
                .map(|c| Src {
                    frames: c.to_vec(),
                    ..src.clone()
                })
                .collect();
            Kind::ImageSet { srcs, id: r as i32 }
        }),
        _ => cx.src(img).map(Kind::Image),
    })
}

fn value(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let v = cx.find("value", id)?;
    Some(cx.number_kind(v))
}

fn text(cx: &mut Ctx, skin: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let t = cx.find("text", id)?;
    Some(Some(Kind::Text {
        id: cx.value_id(t, ScriptKind::Str),
        font: cx.bitmap_font(t, &mut skin.fonts),
        align: i(t, "align", 0) as i32,
        overflow: i(t, "overflow", 0) as i32,
        face: cx.face(t),
        fx: text_fx(t),
        size: text_size(t),
        constant: t.get("constantText").and_then(|c| {
            c.as_str()
                .map(String::from)
                .or_else(|| num(c).map(|n| n.to_string()))
        }),
    }))
}

fn slider(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let sl = cx.find("slider", id)?;
    Some(cx.src(sl).map(|src| Kind::Slider {
        minmax: ref_num(sl),
        src,
        id: match sl.get("value") {
            Some(v) => cx.dyn_id(Some(v), ScriptKind::Float).unwrap_or(0),
            None => i(sl, "type", 0) as i32,
        },
        dir: i(sl, "angle", 0) as i32,
        range: i(sl, "range", 0) as f32,
    }))
}

fn imageset(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let set = cx.find("imageset", id)?;
    let srcs: Vec<Src> = arr(set.get("images"))
        .iter()
        .filter_map(|x| cx.find("image", &key(x)?))
        .filter_map(|im| cx.src(im))
        .collect();
    Some((!srcs.is_empty()).then(|| Kind::ImageSet {
        srcs,
        id: cx.value_id(set, ScriptKind::Int),
    }))
}

fn graph_obj(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    graph(cx, id).map(|g| Some(Kind::Graph(g)))
}

/// `graph`: a lamp / rank distribution (types -1 / -2), else a bar.
fn bar_graph(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let g = cx.find("graph", id)?;
    Some(match i(g, "type", 0) {
        t @ (-1 | -2) => Some(Kind::Distribution {
            src: cx.src(g),
            rank: t == -2,
        }),
        _ => cx.src(g).map(|src| Kind::Bar {
            minmax: ref_num(g),
            src,
            id: match g.get("value") {
                Some(v) => cx.dyn_id(Some(v), ScriptKind::Float).unwrap_or(0),
                None => i(g, "type", 0) as i32,
            },
            dir: i(g, "angle", 1) as i32,
        }),
    })
}

/// The skin's one object of `name` (`note`, `gauge`, `bga`), when it is `id`.
fn named<'a>(cx: &Ctx<'a>, name: &str, id: &str) -> Option<&'a Value> {
    cx.sk.get(name).and_then(obj).filter(|n| id_of(n) == id)
}

/// The note field: the lanes' images and rects, and the bar lines.
fn note(cx: &mut Ctx, skin: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let n = named(cx, "note", id)?;
    let lanes = arr(n.get("dst"));
    let count = skin
        .kind
        .mode()
        .map(|m| m.lane_count())
        .unwrap_or(lanes.len())
        .min(lanes.len());
    skin.lanes = vec![LaneSkin::default(); count];
    let pick = |list: &str, li: usize| {
        arr(n.get(list))
            .get(li)
            .and_then(key)
            .and_then(|iid| cx.find("image", &iid))
            .and_then(|img| cx.src(img))
    };
    let h = cx.h;
    for (li, (ls, a)) in skin.lanes.iter_mut().zip(lanes).enumerate() {
        let (x, y, lw, lh) = (
            i(a, "x", 0) as f32,
            i(a, "y", 0) as f32,
            i(a, "w", 0) as f32,
            i(a, "h", 0) as f32,
        );
        ls.rect = Some(Rect::new(x, h - (y + lh), lw, lh));
        ls.note = pick("note", li);
        ls.ln_start = pick("lnstart", li);
        ls.ln_end = pick("lnend", li);
        ls.ln_body = pick("lnbody", li);
        ls.ln_body_active = pick("lnbodyActive", li).or_else(|| pick("lnactive", li));
        ls.mine = pick("mine", li);
    }
    for g in arr(n.get("group")) {
        let gd = cx.dst(g);
        if let Some(img) = cx.find("image", &id_of(g)).and_then(|i| cx.src(i)) {
            skin.bar_lines.push((img, gd));
        }
    }
    Some(Some(Kind::Notes))
}

fn gauge(cx: &mut Ctx, _: &mut Skin, d: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let g = named(cx, "gauge", id)?;
    let mut frames = Vec::new();
    let mut image = 0;
    for nid in arr(g.get("nodes")) {
        let nid = key(nid).unwrap_or_default();
        if let Some(src) = cx.find("image", &nid).and_then(|i| cx.src(i)) {
            image = src.image;
            frames.push(src.frames[0]);
        }
    }
    let parts = gauge_parts(i(g, "parts", 50));
    Some((!frames.is_empty()).then(|| {
        let (pw, ph) = (frames[0][2], frames[0][3]);
        Kind::Gauge {
            src: Src {
                image,
                frames,
                cycle: 0,
                timer: 0,
            },
            part_w: pw,
            part_h: ph,
            step: (0.0, 0.0),
            parts,
            split: true,
            side: i(d, "side", 0).clamp(0, 1) as usize,
        }
    }))
}

/// The hidden and LIFT covers (they move with their offsets).
fn cover(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, dst: &mut Dst) -> Option<Option<Kind>> {
    let (def, off) = cx
        .find("hiddenCover", id)
        .map(|c| (c, OFFSET_HIDDEN_COVER))
        .or_else(|| cx.find("liftCover", id).map(|c| (c, OFFSET_LIFT)))?;
    dst.offsets.push(off);
    Some(cx.src(def).map(|src| {
        Kind::Cover {
            src,
            line: match i(def, "disapearLine", -1) {
                l if l >= 0 => Some(cx.h - l as f32),
                _ => None,
            },
            link_lift: def
                .get("isDisapearLineLinkLift")
                .and_then(Value::as_bool)
                .unwrap_or(true),
        }
    }))
}

/// Game-supplied images: -100 stage file, -101 back bmp, -102 banner,
/// -110 black, -111 white.
fn game_image(_: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let r = id.parse::<i32>().ok().filter(|n| *n < 0)?;
    Some(Some(Kind::RefImage(-r)))
}

/// The skin select screen's preview of the chosen skin (as LR2's image
/// 105).
fn skin_preview(
    cx: &mut Ctx,
    _: &mut Skin,
    _: &Value,
    id: &str,
    _: &mut Dst,
) -> Option<Option<Kind>> {
    cx.sk
        .get("skinpreview")
        .filter(|n| id_of(n) == id)
        .map(|_| Some(Kind::RefImage(REF_SKIN_PREVIEW)))
}

fn song_list(
    cx: &mut Ctx,
    skin: &mut Skin,
    _: &Value,
    id: &str,
    _: &mut Dst,
) -> Option<Option<Kind>> {
    let sl = cx.sk.get("songlist").filter(|n| id_of(n) == id)?;
    skin.songlist = Some(songlist(cx, sl, cx.h));
    Some(Some(Kind::SongList))
}

fn bga(cx: &mut Ctx, _: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    named(cx, "bga", id).map(|_| Some(Kind::Bga))
}

/// A judgement display: its images and combo numbers (placed relative to
/// the judge image).
fn judge(cx: &mut Ctx, skin: &mut Skin, _: &Value, id: &str, _: &mut Dst) -> Option<Option<Kind>> {
    let j = cx.find("judge", id)?;
    let side = i(j, "index", 0).clamp(0, 2) as usize;
    let js = &mut skin.judges[side];
    js.shift = j.get("shift").and_then(Value::as_bool).unwrap_or(true);
    for (k, im) in arr(j.get("images")).iter().enumerate().take(6) {
        if let Some(src) = cx.find("image", &id_of(im)).and_then(|x| cx.src(x)) {
            js.judge[k] = Some((src, cx.dst(im)));
        }
    }
    let jh = js
        .judge
        .iter()
        .flatten()
        .next()
        .and_then(|(_, d)| d.keys.first())
        .map(|k| k.rect.h)
        .unwrap_or(0.0);
    for (k, nm) in arr(j.get("numbers")).iter().enumerate().take(6) {
        if let Some(Kind::Number { src, keta, .. }) =
            cx.find("value", &id_of(nm)).and_then(|v| cx.number_kind(v))
        {
            let mut nd = cx.dst(nm);
            // relative position: convert y-up offset from the judge image
            for key in &mut nd.keys {
                let y_up = cx.h - (key.rect.y + key.rect.h);
                key.rect.y = jh - y_up - key.rect.h;
            }
            js.combo[k] = Some((src, nd, keta));
        }
    }
    Some(Some(Kind::Judge { side }))
}

/// Add what destination `d` draws to the skin.
pub(super) fn add_object(cx: &mut Ctx, skin: &mut Skin, d: &Value) {
    let id = id_of(d);
    let mut dst = cx.dst(d);
    let Some(kind) = object(cx, skin, d, &id, &mut dst) else {
        log::debug!("{}: unsupported object {id}", skin.path.display());
        return;
    };
    // beatoraja anchors centred text at x and right-aligned text at x + 0
    // (the box is x - w/2 .. x + w/2, x - w .. x)
    if let Kind::Text { align, .. } = &kind {
        let k = (*align).clamp(0, 2) as f32 * 0.5;
        for key in &mut dst.keys {
            key.rect.x -= key.rect.w * k;
        }
    }
    let act = obj_act(cx, &id);
    skin.objects.push(Obj { kind, dst, act });
}
