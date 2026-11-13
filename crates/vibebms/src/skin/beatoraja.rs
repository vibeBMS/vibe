//! beatoraja skins: JSON (`.json`) and Lua (`.luaskin`) play skins.
//!
//! A Lua skin is a script returning the same table a JSON skin contains, so
//! both are turned into a `serde_json::Value` and converted by one builder.
//! Values given as Lua functions (or, in JSON skins, Lua source strings) —
//! draw conditions, timers, numbers, texts, image indices, rates, click
//! events, `customTimers` / `customEvents` — run through [`script`].

use super::script::{self, ScriptKind};
use super::*;
use crate::platform::vfs;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn i(v: &Value, k: &str, d: i64) -> i64 {
    v.get(k)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .unwrap_or(d)
}
/// A number, integral or not (Lua skins compute fractional values).
fn num(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}
/// Treat a field as an object even if a conditional branch left an array.
fn obj(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(_) => Some(v),
        Value::Array(a) => a.iter().find(|x| x.is_object()),
        _ => None,
    }
}
fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}
/// Id fields may be strings or numbers.
fn id_of(v: &Value) -> String {
    match v.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// `if` test: 901 / [901, 911] (and) / [[901, 902], 911] (or inside and) / -901 (not).
fn test(ops: Option<&Value>, en: &HashSet<i64>) -> bool {
    let num = |n: i64| {
        if n >= 0 {
            en.contains(&n)
        } else {
            !en.contains(&-n)
        }
    };
    match ops {
        None => true,
        Some(Value::Number(n)) => num(n.as_i64().unwrap_or(0)),
        Some(Value::Array(a)) => a.iter().all(|x| match x {
            Value::Number(n) => num(n.as_i64().unwrap_or(0)),
            Value::Array(b) => b.iter().any(|y| y.as_i64().map(num).unwrap_or(false)),
            _ => false,
        }),
        _ => false,
    }
}

/// Resolve `include` objects and `{"if": .., "value"/"values": ..}` branches.
fn preprocess(v: Value, en: &HashSet<i64>, dir: &Path, depth: usize) -> Value {
    match v {
        Value::Object(m) if m.contains_key("include") && depth < 8 => {
            let p = dir.join(m["include"].as_str().unwrap_or(""));
            match vfs::read_to_string(&p)
                .ok()
                .and_then(|t| json5::from_str::<Value>(&t).ok())
            {
                Some(inc) => preprocess(inc, en, p.parent().unwrap_or(dir), depth + 1),
                None => Value::Null,
            }
        }
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, preprocess(v, en, dir, depth)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(a) => {
            let mut out = Vec::new();
            let branches = !a.is_empty()
                && a.iter().all(|x| {
                    x.get("if").is_some() && x.get("value").is_some() && x.get("values").is_none()
                });
            for item in a {
                let is_branch = item.get("if").is_some()
                    && (item.get("value").is_some() || item.get("values").is_some());
                if is_branch {
                    if test(item.get("if"), en) {
                        if let Some(vs) = item.get("values").and_then(Value::as_array) {
                            out.extend(vs.iter().cloned().map(|x| preprocess(x, en, dir, depth)));
                        } else if let Some(v) = item.get("value") {
                            out.push(preprocess(v.clone(), en, dir, depth));
                        }
                        if branches {
                            break; // object-valued conditional: first matching clause wins
                        }
                    }
                } else {
                    out.push(preprocess(item, en, dir, depth));
                }
            }
            Value::Array(out)
        }
        v => v,
    }
}

/// Custom options declared by the skin (`property`): (name, ops, default index).
/// (name, [(item name, op)], default index)
type Property = (String, Vec<(String, i64)>, usize);

fn properties(sk: &Value) -> Vec<Property> {
    arr(sk.get("property"))
        .iter()
        .map(|p| {
            let items: Vec<(String, i64)> = arr(p.get("item"))
                .iter()
                .map(|it| (s(it, "name").to_string(), i(it, "op", 0)))
                .collect();
            let def = items
                .iter()
                .position(|(n, _)| n == s(p, "def"))
                .unwrap_or(0);
            (s(p, "name").to_string(), items, def)
        })
        .collect()
}

fn to_custom((name, items, def): &Property) -> Option<CustomOption> {
    items.first().map(|f| CustomOption {
        name: name.clone(),
        first_id: f.1 as i32,
        labels: items.iter().map(|x| x.0.clone()).collect(),
        ops: items.iter().map(|x| x.1 as i32).collect(),
        default: *def,
    })
}

/// The file a `filepath` customisation (`path` with a `*`) picks: the
/// one named `def` (without extension), else the first match.
fn filepath_choice(dir: &Path, path: &str, def: &str) -> Option<PathBuf> {
    let all = super::lr2::glob_paths(&dir.join(path.replace('\\', "/")));
    let stem = |p: &PathBuf| {
        p.file_stem()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    all.iter()
        .find(|p| !def.is_empty() && stem(p) == def.to_lowercase())
        .or(all.first())
        .cloned()
}

/// The source path of an image: a wildcard is resolved like the skin's
/// `filepath` customisation with the same path would (its default).
fn source_path(sk: &Value, dir: &Path, path: &str) -> PathBuf {
    let def = arr(sk.get("filepath"))
        .iter()
        .find(|f| s(f, "path") == path)
        .map(|f| s(f, "def"))
        .unwrap_or("");
    filepath_choice(dir, path, def).unwrap_or_else(|| dir.join(path.replace('\\', "/")))
}

fn offsets_of(sk: &Value) -> Vec<CustomOffset> {
    let b = |o: &Value, k: &str| o.get(k).and_then(Value::as_bool).unwrap_or(false);
    arr(sk.get("offset"))
        .iter()
        .map(|o| CustomOffset {
            name: s(o, "name").to_string(),
            id: i(o, "id", 0) as i32,
            fields: ["x", "y", "w", "h", "r", "a"].map(|k| b(o, k)),
        })
        .filter(|o| o.id != 0 && o.fields.iter().any(|f| *f))
        .collect()
}

/// The `offset` list of a skin: positions the user may adjust.
pub fn custom_offsets(path: &Path) -> Vec<CustomOffset> {
    read_value(path).map(|v| offsets_of(&v)).unwrap_or_default()
}

/// The `property` list of a skin as custom options.
pub fn custom_options(path: &Path) -> Vec<CustomOption> {
    read_value(path)
        .map(|v| properties(&v).iter().filter_map(to_custom).collect())
        .unwrap_or_default()
}

pub fn read_value(path: &Path) -> Result<Value, String> {
    read_skin(path, None).map(|v| v.0)
}

/// A skin's table, with the Lua state it ran in (Lua skins).
fn read_skin(
    path: &Path,
    skin_config: Option<(&HashMap<String, i64>, &Value, &HashMap<String, [i32; 6]>)>,
) -> Result<(Value, Option<script::Script>), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "luaskin" || ext == "lua" {
        return lua::eval(path, skin_config).map(|(v, s)| (v, Some(s)));
    }
    let text = vfs::read(path)
        .map(|b| vibebms_core::encoding::decode_text(&b))
        .map_err(|e| e.to_string())?;
    json5::from_str::<Value>(&add_missing_commas(&text))
        .map(|v| (v, None))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// beatoraja reads skins with libGDX's lenient JSON reader, which accepts
/// values separated only by a line break; insert the missing commas so
/// json5 accepts them too (strings and comments are left alone).
fn add_missing_commas(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 64);
    // last significant char outside strings / comments
    let mut last: Option<char> = None;
    let mut i = 0;
    let ends_value = |c: char| matches!(c, '}' | ']' | '"' | '\'') || c.is_alphanumeric();
    let starts_value =
        |c: char| matches!(c, '{' | '[' | '"' | '\'' | '-' | '+' | '.') || c.is_alphanumeric();
    let mut newline_since_last = false;
    while i < b.len() {
        let c = b[i];
        if c == '"' || c == '\'' {
            if newline_since_last && last.map(ends_value).unwrap_or(false) {
                out.push(',');
            }
            newline_since_last = false;
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() {
                out.push(b[i]);
                i += 1;
            }
            last = Some(c);
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                out.push(b[i]);
                i += 1;
            }
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'*') {
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                out.push(b[i]);
                i += 1;
            }
            out.push_str("*/");
            i += 2;
            continue;
        }
        if c == '\n' {
            newline_since_last = true;
        } else if !c.is_whitespace() {
            if newline_since_last && starts_value(c) && last.map(ends_value).unwrap_or(false) {
                out.push(',');
            }
            newline_since_last = false;
            last = Some(c);
        }
        out.push(c);
        i += 1;
    }
    out
}

/// (type, name) without building the skin.
pub fn header(path: &Path) -> Option<(SkinType, String)> {
    let v = read_value(path).ok()?;
    let t = v.get("type")?.as_i64()?;
    Some((SkinType::from_id(t as i32), s(&v, "name").to_string()))
}

struct Ctx<'a> {
    sk: &'a Value,
    h: f32,
    images: Vec<Option<PathBuf>>,
    sizes: Vec<Option<(u32, u32)>>,
    source_index: HashMap<String, usize>,
    scripts: RefCell<script::Builder>,
}

impl Ctx<'_> {
    /// A field holding an id or a script (Lua function / source): the id
    /// (synthetic for scripts).
    fn dyn_id(&self, v: Option<&Value>, kind: ScriptKind) -> Option<i32> {
        let v = v?;
        match num(v) {
            Some(n) => Some(n as i32),
            None => self.scripts.borrow_mut().register(v, kind),
        }
    }

    /// `ref` (an id), else `value` (an id or a script).
    fn value_id(&self, def: &Value, kind: ScriptKind) -> i32 {
        match def.get("ref").and_then(num) {
            Some(r) if r != 0 => r as i32,
            _ => self.dyn_id(def.get("value"), kind).unwrap_or(0),
        }
    }

    fn src(&self, def: &Value) -> Option<Src> {
        let img = *self.source_index.get(&match def.get("src") {
            Some(Value::String(x)) => x.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return None,
        })?;
        let (iw, ih) = self.sizes.get(img).copied().flatten()?;
        let (x, y) = (i(def, "x", 0) as f32, i(def, "y", 0) as f32);
        let w = match i(def, "w", -1) {
            n if n <= 0 => iw as f32,
            n => n as f32,
        };
        let h = match i(def, "h", -1) {
            n if n <= 0 => ih as f32,
            n => n as f32,
        };
        let dx = i(def, "divx", 1).max(1) as usize;
        let dy = i(def, "divy", 1).max(1) as usize;
        let (fw, fh) = (w / dx as f32, h / dy as f32);
        let mut frames = Vec::new();
        for j in 0..dy {
            for k in 0..dx {
                frames.push([x + fw * k as f32, y + fh * j as f32, fw, fh]);
            }
        }
        Some(Src {
            image: img,
            frames,
            cycle: i(def, "cycle", 0),
            timer: self
                .dyn_id(def.get("timer"), ScriptKind::Timer)
                .unwrap_or(0),
        })
    }

    fn find<'b>(&'b self, list: &str, id: &str) -> Option<&'b Value> {
        arr(self.sk.get(list)).iter().find(|x| id_of(x) == id)
    }

    /// beatoraja destinations are y-up; ours are y-down.
    fn dst(&self, d: &Value) -> Dst {
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
        out
    }

    fn number_kind(&self, def: &Value) -> Option<Kind> {
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
            keta: i(def, "digit", 1).max(1) as usize,
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

/// beatoraja gaugegraph colours: a `color` list (4 per gauge kind) or the
/// named fields with beatoraja's defaults.
pub(crate) fn gauge_style(g: &Value) -> GraphStyle {
    let list: Vec<[f32; 4]> = arr(g.get("color"))
        .iter()
        .filter_map(Value::as_str)
        .map(hex_color)
        .collect();
    let colors = if list.len() >= 24 {
        list[..24].to_vec()
    } else {
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
    };
    GraphStyle {
        colors,
        line_width: i(g, "lineWidth", 2) as f32,
        delay: i(g, "delay", 1500),
        ..Default::default()
    }
}

/// Clickable objects: `act` (event id) and `click` (mode) of an image / imageset.
fn obj_act(cx: &Ctx, id: &str) -> Option<(i32, i32)> {
    let def = cx.find("image", id).or_else(|| cx.find("imageset", id))?;
    let act = cx.dyn_id(def.get("act"), ScriptKind::Event)?;
    (act != 0).then(|| (act, i(def, "click", 0) as i32))
}

/// Parse a beatoraja `songlist`. Sub-destinations are relative to the bar
/// (y-up offsets from its bottom); convert them to y-down offsets.
fn songlist(cx: &Ctx, sl: &Value, h: f32) -> SongListSkin {
    let mut out = SongListSkin {
        center: i(sl, "center", 0).max(0) as usize,
        clickable: arr(sl.get("clickable"))
            .iter()
            .filter_map(Value::as_i64)
            .map(|n| n.max(0) as usize)
            .collect(),
        ..Default::default()
    };
    let slots = |key: &str| -> Vec<Option<Dst>> {
        arr(sl.get(key))
            .iter()
            .map(|d| {
                let d = cx.dst(d);
                (!d.keys.is_empty()).then_some(d)
            })
            .collect()
    };
    out.on = slots("liston");
    out.off = slots("listoff");
    let bar_h = out
        .off
        .iter()
        .chain(out.on.iter())
        .flatten()
        .next()
        .and_then(|d| d.keys.first())
        .map(|k| k.rect.h)
        .unwrap_or(0.0);
    let rel = |d: &Value| {
        let mut d = cx.dst(d);
        for k in &mut d.keys {
            k.rect.y = k.rect.y - h + bar_h;
        }
        d
    };
    // bar bodies: an imageset (one image per bar type) or a single image
    if let Some(first) = arr(sl.get("listoff"))
        .first()
        .or(arr(sl.get("liston")).first())
    {
        let bid = id_of(first);
        if let Some(set) = cx.find("imageset", &bid) {
            out.body = arr(set.get("images"))
                .iter()
                .filter_map(|x| x.as_str())
                .map(|iid| cx.find("image", iid).and_then(|im| cx.src(im)))
                .collect();
        } else if let Some(src) = cx.find("image", &bid).and_then(|im| cx.src(im)) {
            out.body = vec![Some(src); 8];
        }
    }
    if let Some(t) = arr(sl.get("text")).first() {
        let def = cx.find("text", &id_of(t));
        let get = |k: &str| def.map(|d| i(d, k, 0) as i32).unwrap_or(0);
        let mut d = rel(t);
        let k = get("align").clamp(0, 2) as f32 * 0.5;
        for key in &mut d.keys {
            key.rect.x -= key.rect.w * k; // anchored like other beatoraja text
        }
        out.text = Some((d, get("align"), get("overflow"), None));
    }
    out.level = arr(sl.get("level"))
        .iter()
        .map(|d| {
            let v = cx.find("value", &id_of(d))?;
            match cx.number_kind(v)? {
                Kind::Number {
                    src,
                    digits,
                    keta,
                    align,
                    ..
                } => Some((
                    NumberFont {
                        src,
                        digits,
                        keta,
                        align,
                    },
                    rel(d),
                )),
                _ => None,
            }
        })
        .collect();
    if let Some(g) = sl.get("graph").filter(|g| g.is_object()) {
        if let Some(def) = cx.find("graph", &id_of(g)) {
            out.graph = Some((rel(g), cx.src(def), i(def, "type", -1) == -2));
        }
    }
    out.lamp = arr(sl.get("lamp"))
        .iter()
        .map(|d| {
            let src = cx.find("image", &id_of(d)).and_then(|im| cx.src(im))?;
            Some((src, rel(d)))
        })
        .collect();
    out
}

/// Build a skin from a beatoraja JSON/Lua skin (no offsets adjusted).
pub fn load(
    path: &Path,
    user_options: &HashMap<i32, bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    load_with(path, user_options, &HashMap::new(), image_size)
}

/// Build a skin from a beatoraja JSON/Lua skin; `offsets` are the user's
/// values (x, y, w, h, angle, alpha) by offset name.
pub fn load_with(
    path: &Path,
    user_options: &HashMap<i32, bool>,
    offsets: &HashMap<String, [i32; 6]>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    // pass 1: header + defaults
    let (raw, mut lua_state) = read_skin(path, None)?;
    let props = properties(&raw);
    let mut enabled: HashSet<i64> = HashSet::new();
    let mut by_name = HashMap::new();
    for (name, items, def) in &props {
        // a user choice overrides the default
        let chosen = items
            .iter()
            .position(|(_, op)| user_options.get(&(*op as i32)).copied().unwrap_or(false))
            .unwrap_or(*def);
        if let Some((_, op)) = items.get(chosen) {
            enabled.insert(*op);
            by_name.insert(name.clone(), *op);
        }
    }
    // pass 2 (Lua): run again with skin_config
    let raw = if path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("luaskin"))
        .unwrap_or(false)
    {
        let (v, s) = read_skin(path, Some((&by_name, &raw, offsets)))?;
        lua_state = s;
        v
    } else {
        raw
    };
    let sk = preprocess(raw, &enabled, &dir, 0);
    // debugging aid: the skin table as evaluated
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(out) = std::env::var("VIBEBMS_DUMP_SKIN") {
        let _ = std::fs::write(out, serde_json::to_string_pretty(&sk).unwrap_or_default());
    }
    let w = i(&sk, "w", 1280) as f32;
    let h = i(&sk, "h", 720) as f32;
    let kind = SkinType::from_id(i(&sk, "type", -1) as i32);

    // sources (filepath customisation: first file matching the wildcard)
    let mut cx = Ctx {
        sk: &sk,
        h,
        images: Vec::new(),
        sizes: Vec::new(),
        source_index: HashMap::new(),
        scripts: RefCell::new(script::Builder::new(lua_state, &dir)),
    };
    for src in arr(sk.get("source")) {
        let p = source_path(&sk, &dir, s(src, "path"));
        let size = image_size(&p);
        cx.source_index.insert(id_of(src), cx.images.len());
        cx.sizes.push(size);
        cx.images.push(size.map(|_| p));
    }

    let mut skin = Skin {
        path: path.to_path_buf(),
        name: s(&sk, "name").to_string(),
        author: s(&sk, "author").to_string(),
        kind,
        width: w,
        height: h,
        images: Vec::new(),
        objects: Vec::new(),
        lanes: Vec::new(),
        judges: Default::default(),
        bar_lines: Vec::new(),
        songlist: None,
        custom_options: props.iter().filter_map(to_custom).collect(),
        scene_ms: i(&sk, "scene", 0),
        input_ms: i(&sk, "input", 0),
        fadeout_ms: i(&sk, "fadeout", 0),
        finish_margin_ms: i(&sk, "finishmargin", 0),
        script: None,
        fonts: Vec::new(),
        trans_color: None,
        offsets: HashMap::new(),
    };
    let note = sk.get("note").and_then(obj);
    let gauge = sk.get("gauge").and_then(obj);
    let bga = sk.get("bga").and_then(obj);
    let mode = kind.mode();

    for d in arr(sk.get("destination")) {
        let id = id_of(d);
        let mut dst = cx.dst(d);
        let kind = if let Some(img) = cx.find("image", &id) {
            match i(img, "ref", 0) {
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
            }
        } else if let Some(v) = cx.find("value", &id) {
            cx.number_kind(v)
        } else if let Some(t) = cx.find("text", &id) {
            Some(Kind::Text {
                id: cx.value_id(t, ScriptKind::Str),
                font: None,
                align: i(t, "align", 0) as i32,
                overflow: i(t, "overflow", 0) as i32,
            })
        } else if let Some(sl) = cx.find("slider", &id) {
            cx.src(sl).map(|src| Kind::Slider {
                src,
                id: match sl.get("value") {
                    Some(v) => cx.dyn_id(Some(v), ScriptKind::Float).unwrap_or(0),
                    None => i(sl, "type", 0) as i32,
                },
                dir: i(sl, "angle", 0) as i32,
                range: i(sl, "range", 0) as f32,
            })
        } else if let Some(set) = cx.find("imageset", &id) {
            let srcs: Vec<Src> = arr(set.get("images"))
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| x.to_string())
                })
                .filter_map(|iid| cx.find("image", &iid).and_then(|im| cx.src(im)))
                .collect();
            (!srcs.is_empty()).then(|| Kind::ImageSet {
                srcs,
                id: cx.value_id(set, ScriptKind::Int),
            })
        } else if let Some(g) = cx
            .find("gaugegraph", &id)
            .or_else(|| cx.find("judgegraph", &id))
            .or_else(|| cx.find("bpmgraph", &id))
            .or_else(|| cx.find("timingdistributiongraph", &id))
            .filter(|g| g.get("style").and_then(Value::as_str) == Some("vibebms"))
        {
            // vibeBMS extension: the player's own graph look
            let kind = if cx.find("gaugegraph", &id).is_some() {
                GraphKind::Gauge
            } else if cx.find("judgegraph", &id).is_some() {
                GraphKind::Judge(i(g, "type", 0) as i32)
            } else if cx.find("bpmgraph", &id).is_some() {
                GraphKind::Bpm
            } else {
                GraphKind::TimingDistribution
            };
            Some(Kind::Graph(kind, GraphStyle::default()))
        } else if let Some(g) = cx.find("gaugegraph", &id) {
            Some(Kind::Graph(GraphKind::Gauge, gauge_style(g)))
        } else if let Some(g) = cx.find("judgegraph", &id) {
            Some(Kind::Graph(
                GraphKind::Judge(i(g, "type", 0) as i32),
                GraphStyle {
                    flags: [
                        i(g, "noGap", 0) != 0,
                        i(g, "orderReverse", 0) != 0,
                        i(g, "backTexOff", 0) == 0,
                        i(g, "noGapX", 0) != 0,
                    ],
                    chips: true,
                    delay: i(g, "delay", 500),
                    ..Default::default()
                },
            ))
        } else if let Some(g) = cx.find("bpmgraph", &id) {
            let c = |k: &str, d: &str| hex_color(g.get(k).and_then(Value::as_str).unwrap_or(d));
            Some(Kind::Graph(
                GraphKind::Bpm,
                GraphStyle {
                    colors: vec![
                        c("mainBPMColor", "00ff00"),
                        c("minBPMColor", "0000ff"),
                        c("maxBPMColor", "ff0000"),
                        c("otherBPMColor", "ffff00"),
                        c("stopLineColor", "ff00ff"),
                        c("transitionLineColor", "7f7f7f"),
                    ],
                    line_width: i(g, "lineWidth", 2) as f32,
                    delay: i(g, "delay", 0),
                    ..Default::default()
                },
            ))
        } else if let Some((g, hit)) = cx
            .find("timingvisualizer", &id)
            .map(|g| (g, false))
            .or_else(|| cx.find("hiterrorvisualizer", &id).map(|g| (g, true)))
        {
            let c = |k: &str, d: &str| hex_color(g.get(k).and_then(Value::as_str).unwrap_or(d));
            let mut colors = vec![
                c("lineColor", if hit { "99CCFF80" } else { "00FF00FF" }),
                c("centerColor", "FFFFFFFF"),
                c("PGColor", if hit { "99CCFF80" } else { "000088FF" }),
                c("GRColor", if hit { "F2CB3080" } else { "008800FF" }),
                c("GDColor", if hit { "14CC8f80" } else { "888800FF" }),
                c("BDColor", if hit { "FF1AB380" } else { "880000FF" }),
                c("PRColor", if hit { "CC292980" } else { "000000FF" }),
            ];
            if hit {
                colors.push(c("emaColor", "FF0000FF"));
            }
            Some(Kind::Graph(
                GraphKind::TimingVisualizer(hit),
                GraphStyle {
                    colors,
                    line_width: i(g, "lineWidth", 1).clamp(1, 4) as f32,
                    // transparent poor band, decaying lines, colour by
                    // judge, moving average
                    flags: [
                        i(g, "transparent", 0) == 1,
                        i(g, "drawDecay", 1) == 1,
                        hit && i(g, "colorMode", 1) == 1,
                        hit && i(g, "emaMode", 1) != 0,
                    ],
                    width: i(g, "judgeWidthMillis", 150).max(1) as i32,
                    delay: i(g, "windowLength", 30),
                    ..Default::default()
                },
            ))
        } else if let Some(g) = cx.find("timingdistributiongraph", &id) {
            let c = |k: &str, d: &str| hex_color(g.get(k).and_then(Value::as_str).unwrap_or(d));
            Some(Kind::Graph(
                GraphKind::TimingDistribution,
                GraphStyle {
                    colors: vec![
                        c("graphColor", "00FF00FF"),
                        c("averageColor", "FFFFFFFF"),
                        c("devColor", "FFFFFFFF"),
                        c("PGColor", "000088FF"),
                        c("GRColor", "008800FF"),
                        c("GDColor", "888800FF"),
                        c("BDColor", "880000FF"),
                        c("PRColor", "000000FF"),
                    ],
                    line_width: i(g, "lineWidth", 1) as f32,
                    flags: [
                        i(g, "drawAverage", 1) != 0,
                        i(g, "drawDev", 1) != 0,
                        false,
                        false,
                    ],
                    width: i(g, "width", 301) as i32,
                    ..Default::default()
                },
            ))
        } else if let Some(g) = cx
            .find("graph", &id)
            .filter(|g| matches!(i(g, "type", 0), -1 | -2))
        {
            Some(Kind::Distribution {
                src: cx.src(g),
                rank: i(g, "type", 0) == -2,
            })
        } else if let Some(g) = cx.find("graph", &id) {
            cx.src(g).map(|src| Kind::Bar {
                src,
                id: match g.get("value") {
                    Some(v) => cx.dyn_id(Some(v), ScriptKind::Float).unwrap_or(0),
                    None => i(g, "type", 0) as i32,
                },
                dir: i(g, "angle", 1) as i32,
            })
        } else if note.map(|n| id_of(n) == id).unwrap_or(false) {
            let n = note.unwrap();
            let lanes = arr(n.get("dst"));
            let count = mode
                .map(|m| m.lane_count())
                .unwrap_or(lanes.len())
                .min(lanes.len());
            skin.lanes = vec![LaneSkin::default(); count];
            let pick = |key: &str, li: usize| {
                arr(n.get(key))
                    .get(li)
                    .and_then(|x| {
                        x.as_str()
                            .map(String::from)
                            .or_else(|| x.as_i64().map(|n| n.to_string()))
                    })
                    .and_then(|iid| cx.find("image", &iid))
                    .and_then(|img| cx.src(img))
            };
            for (li, ls) in skin.lanes.iter_mut().enumerate() {
                let a = &lanes[li];
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
            Some(Kind::Notes)
        } else if gauge.map(|g| id_of(g) == id).unwrap_or(false) {
            let g = gauge.unwrap();
            let mut frames = Vec::new();
            let mut image = 0;
            for nid in arr(g.get("nodes")) {
                let nid = nid
                    .as_str()
                    .map(String::from)
                    .or_else(|| nid.as_i64().map(|n| n.to_string()))
                    .unwrap_or_default();
                if let Some(src) = cx.find("image", &nid).and_then(|i| cx.src(i)) {
                    image = src.image;
                    frames.push(src.frames[0]);
                }
            }
            let parts = i(g, "parts", 50).max(1) as usize;
            (!frames.is_empty()).then(|| {
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
                }
            })
        } else if let Some(c) = cx
            .find("hiddenCover", &id)
            .map(|c| (c, OFFSET_HIDDEN_COVER))
            .or_else(|| cx.find("liftCover", &id).map(|c| (c, OFFSET_LIFT)))
        {
            let (def, off) = c;
            dst.offsets.push(off);
            cx.src(def).map(|src| Kind::Cover {
                src,
                line: match i(def, "disapearLine", -1) {
                    l if l >= 0 => Some(h - l as f32),
                    _ => None,
                },
                link_lift: def
                    .get("isDisapearLineLinkLift")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        } else if let Some(r) = id.parse::<i32>().ok().filter(|n| *n < 0) {
            // game-supplied images: -100 stage file, -101 back bmp,
            // -102 banner, -110 black, -111 white
            Some(Kind::RefImage(-r))
        } else if let Some(sl) = sk.get("songlist").filter(|n| id_of(n) == id) {
            skin.songlist = Some(songlist(&cx, sl, h));
            Some(Kind::SongList)
        } else if bga.map(|b| id_of(b) == id).unwrap_or(false) {
            Some(Kind::Bga)
        } else if let Some(j) = arr(sk.get("judge")).iter().find(|x| id_of(x) == id) {
            let side = i(j, "index", 0).clamp(0, 1) as usize;
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
                        let y_up = h - (key.rect.y + key.rect.h);
                        key.rect.y = jh - y_up - key.rect.h;
                    }
                    js.combo[k] = Some((src, nd, keta));
                }
            }
            Some(Kind::Judge { side })
        } else {
            None
        };
        if kind.is_none() {
            log::debug!("{}: unsupported object {id}", path.display());
        }
        // beatoraja anchors centred text at x and right-aligned text at x + 0
        // (the box is x - w/2 .. x + w/2, x - w .. x)
        if let Some(Kind::Text { align, .. }) = &kind {
            let k = (*align).clamp(0, 2) as f32 * 0.5;
            for key in &mut dst.keys {
                key.rect.x -= key.rect.w * k;
            }
        }
        if let Some(kind) = kind {
            let act = obj_act(&cx, &id);
            skin.objects.push(Obj { kind, dst, act });
        }
    }
    // script-driven timers and events
    {
        let mut b = cx.scripts.borrow_mut();
        for t in arr(sk.get("customTimers")) {
            if let (Some(id), Some(v)) = (t.get("id").and_then(Value::as_i64), t.get("timer")) {
                b.custom_timer(id as i32, v);
            }
        }
        for ev in arr(sk.get("customEvents")) {
            if let Some(a) = ev.get("action") {
                b.custom_event(a, ev.get("condition"), i(ev, "minInterval", 0));
            }
        }
    }
    // conditions on the skin's own properties
    let known: HashMap<i32, bool> = props
        .iter()
        .flat_map(|(_, items, _)| {
            items
                .iter()
                .map(|(_, op)| (*op as i32, enabled.contains(op)))
        })
        .collect();
    resolve_static_ops(&mut skin, &known);
    skin.script = cx.scripts.into_inner().finish();
    for o in offsets_of(&sk) {
        let v = offsets.get(&o.name).copied().unwrap_or_default();
        skin.offsets.insert(o.id, v.map(|x| x as f32));
    }
    skin.images = cx.images;
    if skin.objects.is_empty() {
        return Err(format!("{}: no drawable objects", path.display()));
    }
    Ok(skin)
}

#[cfg(not(target_arch = "wasm32"))]
mod lua {
    use super::*;

    /// Run a `.luaskin` and return its table as JSON (functions become
    /// script markers) with the Lua state. `skin_config` (option name ->
    /// selected op) is exported like beatoraja does.
    pub fn eval(
        path: &Path,
        skin_config: Option<(&HashMap<String, i64>, &Value, &HashMap<String, [i32; 6]>)>,
    ) -> Result<(Value, script::Script), String> {
        let src = vfs::read(path).map_err(|e| e.to_string())?;
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let e = |e: mlua::Error| format!("{}: {e}", path.display());
        let mut sc = script::Script::new(&dir, false).map_err(e)?;
        let lua = sc.lua().clone();
        let g = lua.globals();
        if let Some((opts, header, user_offsets)) = skin_config {
            let cfg = lua.create_table().map_err(e)?;
            let option = lua.create_table().map_err(e)?;
            let enabled = lua.create_table().map_err(e)?;
            for (k, v) in opts {
                option.set(k.as_str(), *v).map_err(e)?;
                enabled.push(*v).map_err(e)?;
            }
            cfg.set("option", option).map_err(e)?;
            cfg.set("enabled_options", enabled).map_err(e)?;
            // custom offsets (the user's values) and the files `filepath`
            // customisations pick
            let offsets = lua.create_table().map_err(e)?;
            for o in arr(header.get("offset")) {
                let t = lua.create_table().map_err(e)?;
                let v = user_offsets.get(s(o, "name")).copied().unwrap_or_default();
                for (k, x) in ["x", "y", "w", "h", "r", "a"].iter().zip(v) {
                    t.set(*k, x).map_err(e)?;
                }
                offsets.set(s(o, "name"), t).map_err(e)?;
            }
            cfg.set("offset", offsets).map_err(e)?;
            let files = lua.create_table().map_err(e)?;
            for f in arr(header.get("filepath")) {
                if let Some(p) = filepath_choice(&dir, s(f, "path"), s(f, "def")) {
                    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                    files.set(s(f, "name"), name).map_err(e)?;
                }
            }
            cfg.set("file_path", files).map_err(e)?;
            let d2 = dir.clone();
            cfg.set(
                "get_path",
                lua.create_function(move |_, p: String| {
                    Ok(super::super::lr2::glob_path(&d2.join(p.replace('\\', "/")))
                        .to_string_lossy()
                        .into_owned())
                })
                .map_err(e)?,
            )
            .map_err(e)?;
            g.set("skin_config", cfg).map_err(e)?;
        }
        let v: mlua::Value = lua
            .load(&src[..])
            .set_name(path.display().to_string())
            .eval()
            .map_err(e)?;
        let json = sc.to_json(v).map_err(e)?;
        Ok((json, sc))
    }
}

#[cfg(target_arch = "wasm32")]
mod lua {
    use super::*;
    pub fn eval(
        path: &Path,
        _: Option<(&HashMap<String, i64>, &Value, &HashMap<String, [i32; 6]>)>,
    ) -> Result<(Value, script::Script), String> {
        Err(format!(
            "{}: Lua skins are not supported in the web build",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_json_commas() {
        let t = "{\"a\": [ {\"x\":1}\n {\"y\":\"s\"}\n ],\n \"b\": 2 // c\n , d: true\n e: 'q'\n}";
        let v: Value = json5::from_str(&add_missing_commas(t)).unwrap();
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
        assert_eq!(v["d"], Value::Bool(true));
        assert_eq!(v["e"], Value::String("q".into()));
        // already valid JSON is unchanged
        let ok = "{\"a\": 1,\n \"b\": [1,\n 2]}";
        assert_eq!(add_missing_commas(ok), ok);
    }

    #[test]
    fn builtin_result_skin_loads() {
        let path = crate::scene::skinned::default_skin(SkinType::Result, false).unwrap();
        let sk = load(&path, &HashMap::new(), &crate::skin::image_size).unwrap();
        assert_eq!(sk.kind, SkinType::Result);
        assert!(sk.images.iter().all(Option::is_some));
        let graphs = sk
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, Kind::Graph(_, s) if s.colors.is_empty()))
            .count();
        // our own skin asks for the player's graph look
        assert_eq!(graphs, 3);
        assert!(sk
            .objects
            .iter()
            .any(|o| matches!(&o.kind, Kind::ImageSet { srcs, id: 370 } if srcs.len() == 11)));
    }

    #[test]
    fn graph_colors() {
        let g: Value =
            json5::from_str(r#"{"id": "g", "lineWidth": 3, "exHardBGColor": "102030"}"#).unwrap();
        let s = gauge_style(&g);
        assert_eq!(s.colors.len(), 24);
        assert_eq!(s.line_width, 3.0);
        // normal gauge: line above the border red, below green (defaults)
        assert_eq!(s.colors[8], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(s.colors[10], [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(s.colors[4 * 4 + 1], hex_color("102030"));
        let list: Vec<String> = (0..24).map(|k| format!("{:02x}000080", k)).collect();
        let g = serde_json::json!({ "color": list });
        let s = gauge_style(&g);
        assert_eq!(s.colors[5], [5.0 / 255.0, 0.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn builtin_select_skins_load() {
        for portrait in [false, true] {
            let path = crate::scene::skinned::default_skin(SkinType::Select, portrait).unwrap();
            let sk = load(&path, &HashMap::new(), &crate::skin::image_size).unwrap();
            assert_eq!(sk.kind, SkinType::Select);
            let sl = sk.songlist.as_ref().expect("songlist");
            assert!(sl.off.len() > 10 && sl.off.len() == sl.on.len());
            assert_eq!(sl.body.iter().flatten().count(), 8);
            assert!(sl.text.is_some() && sl.level.len() == 7 && sl.lamp.len() == 11);
            // option buttons are clickable
            // option buttons and the folder back button are clickable
            assert_eq!(sk.objects.iter().filter(|o| o.act.is_some()).count(), 11);
            let path = crate::scene::skinned::default_skin(SkinType::Result, portrait).unwrap();
            assert!(load(&path, &HashMap::new(), &crate::skin::image_size).is_ok());
        }
    }

    #[test]
    fn builtin_default_skins_load() {
        use crate::scene::play_skin::default_skin;
        use vibebms_core::PlayMode;
        let size = |p: &Path| {
            let b = vfs::read(p).ok()?;
            image::ImageReader::new(std::io::Cursor::new(b))
                .with_guessed_format()
                .ok()?
                .into_dimensions()
                .ok()
        };
        for mode in PlayMode::ALL {
            for (p2, portrait) in [(false, false), (true, false), (false, true), (true, true)] {
                let path = default_skin(mode, p2, portrait);
                let sk = load(&path, &HashMap::new(), &size)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(sk.kind.mode(), Some(mode), "{}", path.display());
                assert_eq!(sk.lanes.len(), mode.lane_count());
                assert!(sk
                    .lanes
                    .iter()
                    .all(|l| l.rect.is_some() && l.note.is_some()));
                assert!(sk.images.iter().all(Option::is_some), "{}", path.display());
                assert!(sk.judges[0].judge.iter().all(Option::is_some));
                assert_eq!(sk.bar_lines.len(), mode.players(), "{}", path.display());
                if mode.is_double() {
                    assert!(sk.judges[1].judge.iter().all(Option::is_some));
                }
            }
        }
    }

    const JSON: &str = r#"{
      // comments and trailing commas are accepted like beatoraja's parser
      type: 0, name: "JS", w: 1280, h: 720,
      property: [{ name: "Lane", item: [{name: "A", op: 900}, {name: "B", op: 901}], def: "B" }],
      source: [{ id: "s", path: "p*.png" }],
      image: [
        { id: "bg", src: "s", x: 0, y: 0, w: 10, h: 10 },
        { id: "n1", src: "s", x: 0, y: 0, w: 4, h: 2 },
        { id: "j0", src: "s", x: 0, y: 0, w: 8, h: 4 },
      ],
      value: [{ id: "v", src: "s", x: 0, y: 0, w: 100, h: 10, divx: 10, digit: 4, ref: 101 }],
      note: { id: "notes", note: ["n1","n1","n1","n1","n1","n1","n1","n1"], dst: [
        {x:10,y:100,w:20,h:500},{x:30,y:100,w:20,h:500},{x:50,y:100,w:20,h:500},{x:70,y:100,w:20,h:500},
        {x:90,y:100,w:20,h:500},{x:110,y:100,w:20,h:500},{x:130,y:100,w:20,h:500},{x:150,y:100,w:20,h:500}] },
      judge: [{ id: "judge", index: 0, images: [{ id: "j0", loop: -1, timer: 46, dst: [{x: 0, y: 300, w: 80, h: 40}] }] }],
      destination: [
        { if: [900], value: { id: "bg", dst: [{ x: 0, y: 0, w: 1280, h: 720 }] } },
        { if: [901], value: { id: "bg", dst: [{ time: 0, x: 0, y: 0, w: 640, h: 360 }, { time: 500, x: 100 }] } },
        { id: "notes" },
        { id: "v", op: [32], dst: [{ x: 20, y: 20, w: 16, h: 24 }] },
        { id: "judge" },
      ],
    }"#;

    #[test]
    fn loads_json_skin() {
        let dir = std::env::temp_dir().join(format!("vibebms-bskin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("play.json"), JSON).unwrap();
        std::fs::write(dir.join("parts.png"), b"x").unwrap();
        let sz = |_: &Path| Some((256, 256));
        let s = load(&dir.join("play.json"), &HashMap::new(), &sz).unwrap();
        assert_eq!(s.kind, SkinType::Play7);
        assert!(s.images[0].as_ref().unwrap().ends_with("parts.png"));
        // def "B" (901) selected: animated bg, inherits y/w/h in the 2nd key, y flipped
        let bg = &s.objects[0];
        assert_eq!(bg.dst.keys.len(), 2);
        assert_eq!(bg.dst.keys[1].rect, Rect::new(100.0, 360.0, 640.0, 360.0));
        assert!(matches!(s.objects[1].kind, Kind::Notes));
        assert_eq!(
            s.lanes[0].rect.unwrap(),
            Rect::new(10.0, 120.0, 20.0, 500.0)
        );
        assert!(matches!(
            s.objects[2].kind,
            Kind::Number {
                id: 101,
                keta: 4,
                digits: 10,
                ..
            }
        ));
        assert_eq!(s.objects[2].dst.ops, vec![32]);
        assert!(s.judges[0].judge[0].is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    struct ScriptSt {
        flag: bool,
    }
    impl SkinState for ScriptSt {
        fn timer(&self, id: i32) -> Option<i64> {
            (id == 41).then_some(500)
        }
        fn option(&self, id: i32) -> bool {
            id == 5 && self.flag
        }
        fn number(&self, id: i32) -> Option<i64> {
            (id == 71).then_some(21)
        }
        fn text(&self, _: i32) -> Option<String> {
            None
        }
        fn rate(&self, _: i32) -> f32 {
            0.0
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lua_skin_scripts() {
        use crate::skin::script::Scripted;
        let dir = std::env::temp_dir().join(format!("vibebms-lscript-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lua = r#"
            local main_state = require("main_state")
            local timer_util = require("timer_util")
            local s = { type = 0, name = "S", w = 100, h = 100 }
            if not skin_config then return s end
            s.source = { { id = 1, path = "a.png" } }
            s.image = { { id = "img", src = 1, x = 0, y = 0, w = 10, h = 10, act = function() main_state.event_exec(15) end } }
            -- a stray positional entry must not turn the table into an array
            s.image[2] = { id = "img2", src = 1, x = 0, y = 0, w = 10, h = 10, 12345 }
            s.value = { { id = "num", src = 1, x = 0, y = 0, w = 100, h = 10, divx = 10, digit = 3,
                          value = function() return main_state.number(71) * 2 end } }
            s.text = { { id = "txt", font = 0, size = 10, value = function() return "ex " .. main_state.number(71) end } }
            local on = timer_util.timer_observe_boolean(function() return main_state.option(5) end)
            s.destination = {
              { id = "img", timer = on, dst = { { x = 0, y = 0, w = 10, h = 10 } } },
              { id = "num", draw = function() return main_state.timer_is_on(41) end, dst = { { x = 0, y = 0, w = 10, h = 10 } } },
              { id = "txt", dst = { { x = 0, y = 0, w = 10, h = 10 } } },
            }
            return s
        "#;
        std::fs::write(dir.join("play.luaskin"), lua).unwrap();
        let sz = |_: &Path| Some((100, 20));
        let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
        let raw = read_skin(
            &dir.join("play.luaskin"),
            Some((&HashMap::new(), &Value::Null, &HashMap::new())),
        )
        .unwrap()
        .0;
        assert_eq!(raw["image"][1]["id"], "img2");
        // no way out of the sandbox
        let escape = "return (os == nil and io == nil and package.loadlib == nil and #package.searchers == 2)";
        std::fs::write(dir.join("esc.luaskin"), escape).unwrap();
        let (v, _) = read_skin(&dir.join("esc.luaskin"), None).unwrap();
        assert_eq!(v, Value::Bool(true));
        std::fs::remove_dir_all(&dir).ok();
        let rt = s.script.clone().expect("scripts");
        let (img, num, txt) = (&s.objects[0], &s.objects[1], &s.objects[2]);
        let Kind::Number { id: num_id, .. } = num.kind else {
            panic!("number")
        };
        let Kind::Text { id: txt_id, .. } = txt.kind else {
            panic!("text")
        };
        let st = ScriptSt { flag: false };
        rt.update(&st, 100);
        let sc = Scripted { st: &st, rt: &rt };
        assert_eq!(sc.number(num_id), Some(42));
        assert_eq!(sc.text(txt_id).as_deref(), Some("ex 21"));
        assert!(num.dst.ops.iter().all(|o| sc.option(*o)));
        assert_eq!(sc.timer(img.dst.timer), None);
        // the observed condition turns on at 300 ms: the timer starts there
        let st = ScriptSt { flag: true };
        rt.update(&st, 300);
        rt.update(&st, 400);
        let sc = Scripted { st: &st, rt: &rt };
        assert_eq!(sc.timer(img.dst.timer), Some(300));
        // click: the script runs on the next frame and asks for event 15
        let (act, _) = img.act.unwrap();
        assert!(rt.click(act));
        rt.update(&st, 500);
        assert_eq!(rt.take_events(), vec![15]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lua_skin_config_files() {
        let root = std::env::temp_dir().join(format!("vibebms-lcfg-{}", std::process::id()));
        let dir = root.join("play");
        std::fs::create_dir_all(dir.join("judge")).unwrap();
        for f in ["a.png", "b.png"] {
            std::fs::write(dir.join("judge").join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("judge/b.lua"), "return { w = 77 }").unwrap();
        std::fs::write(root.join("outside.txt"), "return 1").unwrap();
        let lua = r#"
            local header = { type = 0, name = "C", w = 100, h = 100,
              filepath = { { name = "Judge", path = "judge/*.png", def = "b" } },
              offset = { { name = "Lift", id = 40, x = false, y = true } } }
            if not skin_config then return header end
            local o = skin_config.offset["Lift"]
            local p = string.match(skin_config.get_path("judge/" .. skin_config.file_path["Judge"]), "(.+)%.png$") .. ".lua"
            local ok, setting = pcall(dofile, p)
            local escaped = pcall(dofile, "../outside.txt")
            header.source = { { id = 1, path = "judge/*.png" } }
            header.image = { { id = "i", src = 1, x = 0, y = 0, w = -1, h = -1 } }
            header.destination = { { id = "i", dst = { { x = o.y, y = 0, w = setting.w, h = escaped and 1 or 2 } } } }
            return header
        "#;
        std::fs::write(dir.join("play.luaskin"), lua).unwrap();
        let sz = |_: &Path| Some((10, 10));
        let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
        let r = s.objects[0].dst.keys[0].rect;
        assert_eq!((r.x, r.w, r.h), (0.0, 77.0, 2.0));
        // the user's offset values reach the script and the skin
        let mut offs = HashMap::new();
        offs.insert("Lift".to_string(), [0, 7, 0, 0, 0, 0]);
        let s2 = load_with(&dir.join("play.luaskin"), &HashMap::new(), &offs, &sz).unwrap();
        assert_eq!(s2.objects[0].dst.keys[0].rect.x, 7.0);
        assert_eq!(s2.offsets[&40], [0.0, 7.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(custom_offsets(&dir.join("play.luaskin")).len(), 1);
        std::fs::remove_dir_all(&root).ok();
        // the wildcard source follows the filepath default too
        assert!(s.images[0].as_ref().unwrap().ends_with("judge/b.png"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn json_skin_scripts() {
        use crate::skin::script::Scripted;
        let dir = std::env::temp_dir().join(format!("vibebms-jscript-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = r#"{
            "type": 0, "name": "J", "w": 100, "h": 100,
            "source": [{"id": 1, "path": "a.png"}],
            "value": [{"id": "n", "src": 1, "x": 0, "y": 0, "w": 100, "h": 10, "divx": 10, "digit": 3,
                       "value": "number(71) + 1"}],
            "customTimers": [{"id": 10001, "timer": "timer_observe_boolean(function() return option(5) end)"}],
            "destination": [{"id": "n", "op": [32, "option(5)"], "timer": 10001,
                             "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]
        }"#;
        std::fs::write(dir.join("play.json"), json).unwrap();
        let sz = |_: &Path| Some((100, 20));
        let s = load(&dir.join("play.json"), &HashMap::new(), &sz).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let rt = s.script.clone().expect("scripts");
        let o = &s.objects[0];
        let Kind::Number { id, .. } = o.kind else {
            panic!("number")
        };
        assert_eq!(o.dst.ops.len(), 2);
        assert_eq!(o.dst.ops[0], 32);
        let st = ScriptSt { flag: true };
        rt.update(&st, 250);
        let sc = Scripted { st: &st, rt: &rt };
        assert_eq!(sc.number(id), Some(22));
        assert!(sc.option(o.dst.ops[1]));
        assert_eq!(sc.timer(10001), Some(250));
        let st = ScriptSt { flag: false };
        rt.update(&st, 300);
        let sc = Scripted { st: &st, rt: &rt };
        assert!(!sc.option(o.dst.ops[1]));
        assert_eq!(sc.timer(10001), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn loads_lua_skin() {
        let dir = std::env::temp_dir().join(format!("vibebms-lskin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lua = r#"
            local main_state = require("main_state")
            local header = { type = 0, name = "LuaSkin", w = 1280, h = 720,
              property = { { name = "Mode", item = { { name = "X", op = 910 }, { name = "Y", op = 911 } } } } }
            if not skin_config then return header end
            header.source = { { id = 1, path = "a.png" } }
            header.image = { { id = "bg", src = 1, x = 0, y = 0, w = -1, h = -1 } }
            local dst = { { x = 0, y = 0, w = 1280, h = 720 } }
            if skin_config.option["Mode"] == 911 then dst[1].w = 10 end
            header.destination = { { id = "bg", dst = dst, draw = function() return main_state.option(1) end } }
            return header
        "#;
        std::fs::write(dir.join("play.luaskin"), lua).unwrap();
        let sz = |_: &Path| Some((64, 32));
        assert_eq!(header(&dir.join("play.luaskin")).unwrap().1, "LuaSkin");
        let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
        assert_eq!(s.objects.len(), 1);
        assert_eq!(s.objects[0].dst.keys[0].rect.w, 1280.0);
        let mut opt = HashMap::new();
        opt.insert(911, true);
        let s = load(&dir.join("play.luaskin"), &opt, &sz).unwrap();
        assert_eq!(s.objects[0].dst.keys[0].rect.w, 10.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
