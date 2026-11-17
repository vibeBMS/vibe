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
use std::borrow::Cow;
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
/// An id given as a string or a number.
fn key(v: &Value) -> Option<Cow<'_, str>> {
    match v {
        Value::String(s) => Some(Cow::Borrowed(s)),
        Value::Number(n) => Some(Cow::Owned(n.to_string())),
        _ => None,
    }
}

fn id_of(v: &Value) -> Cow<'_, str> {
    v.get("id").and_then(key).unwrap_or_default()
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
                            // the values are items of this array: branches
                            // among them are resolved the same way
                            if let Value::Array(vs) =
                                preprocess(Value::Array(vs.clone()), en, dir, depth)
                            {
                                out.extend(vs);
                            }
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

/// Read a BMFont text font (`.fnt`): the font and its page image files
/// (glyph image indices are page ids).
fn load_bmfont(path: &Path) -> Option<(BitmapFont, Vec<PathBuf>)> {
    let bytes = vfs::read(path).ok()?;
    let text = vibebms_core::encoding::decode_text(&bytes);
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut font = BitmapFont::default();
    let mut pages: Vec<PathBuf> = Vec::new();
    let (mut size, mut line_h) = (0.0f32, 0.0f32);
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}');
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        // key=value pairs; values may be quoted
        let mut kv: HashMap<&str, String> = HashMap::new();
        let mut rest = rest.trim();
        while let Some((k, v)) = rest.split_once('=') {
            let (val, after) = match v.strip_prefix('"') {
                Some(q) => q.split_once('"').unwrap_or((q, "")),
                None => v.split_once(' ').unwrap_or((v, "")),
            };
            kv.insert(k.trim(), val.to_string());
            rest = after.trim();
        }
        let n = |k: &str| kv.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match tag {
            "info" => size = n("size").abs(),
            "common" => line_h = n("lineHeight"),
            "page" => {
                let id = n("id") as usize;
                if id < 64 {
                    if pages.len() <= id {
                        pages.resize(id + 1, PathBuf::new());
                    }
                    let file = kv.get("file").cloned().unwrap_or_default();
                    pages[id] = vfs::resolve_case(&dir.join(file.replace('\\', "/")));
                }
            }
            "char" => {
                let Some(c) = char::from_u32(n("id") as u32) else {
                    continue;
                };
                let page = n("page") as usize;
                if page < pages.len() {
                    font.glyphs
                        .insert(c, (page, [n("x"), n("y"), n("width"), n("height")]));
                    font.metrics
                        .insert(c, [n("xoffset"), n("yoffset"), n("xadvance")]);
                }
            }
            _ => {}
        }
    }
    font.size = if line_h > 0.0 { line_h } else { size };
    // beatoraja falls back to the line height when there is no size
    font.em = if size > 0.0 { size } else { font.size };
    (!font.glyphs.is_empty() && font.size > 0.0).then_some((font, pages))
}

/// A `filepath` customisation: name, path (with a `*`), default file.
struct FilePath {
    name: String,
    path: String,
    def: String,
}

fn filepaths(sk: &Value) -> Vec<FilePath> {
    arr(sk.get("filepath"))
        .iter()
        .map(|f| FilePath {
            name: s(f, "name").to_string(),
            path: s(f, "path").replace('\\', "/"),
            def: s(f, "def").to_string(),
        })
        .filter(|f| f.path.contains('*'))
        .collect()
}

/// The files a `filepath` offers (names, sorted): `*|n|` marks one of
/// several choices over the same folder.
fn filepath_files(dir: &Path, path: &str) -> Vec<String> {
    let pattern = match (path.find('|'), path.rfind('|')) {
        (Some(a), Some(b)) => format!("{}{}", &path[..a], &path[b + 1..]),
        _ => path.to_string(),
    };
    super::lr2::glob_paths(&vfs::resolve_case(&dir.join(pattern)))
        .iter()
        .filter(|p| !p.to_string_lossy().contains('*'))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect()
}

/// The file chosen for a `filepath`: the user's, else the default (a name
/// with or without its extension), else the first.
fn filepath_chosen(dir: &Path, f: &FilePath, files: &HashMap<String, String>) -> Option<String> {
    let all = filepath_files(dir, &f.path);
    let stem = |n: &str| {
        Path::new(n)
            .file_stem()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let want = files.get(&f.name).unwrap_or(&f.def).to_lowercase();
    all.iter()
        .find(|n| n.to_lowercase() == want)
        .or_else(|| all.iter().find(|n| !want.is_empty() && stem(n) == want))
        .or(all.first())
        .cloned()
}

/// The file a skin names: a `filepath` whose path starts it puts the
/// chosen file in place of its `*`; another `*` picks one of the matching
/// files at random; other paths are matched to the files' letter case.
///
/// Written from a behaviour specification, not from another program's
/// code.
fn skin_path(dir: &Path, fps: &[FilePath], files: &HashMap<String, String>, path: &str) -> PathBuf {
    let path = path.replace('\\', "/");
    for f in fps {
        let Some(rest) = path.strip_prefix(f.path.as_str()) else {
            continue;
        };
        // no file to choose: try the next entry
        let Some(chosen) = filepath_chosen(dir, f, files) else {
            continue;
        };
        let before_star = &f.path[..f.path.find('*').unwrap_or(f.path.len())];
        let folder = &before_star[..before_star.rfind('/').map_or(0, |i| i + 1)];
        return vfs::resolve_case(&dir.join(format!("{folder}{chosen}{rest}")));
    }
    let Some(star) = path.rfind('*') else {
        return vfs::resolve_case(&dir.join(&path));
    };
    let after = &path[star + 1..];
    let suffix = match (after.find('|'), after.rfind('|')) {
        (Some(a), Some(b)) => format!("{}{}", &after[..a], &after[b + 1..]),
        _ => after.to_string(),
    }
    .to_lowercase();
    let folder = match path[..star].rfind('/') {
        Some(i) => vfs::resolve_case(&dir.join(&path[..i])),
        None => dir.to_path_buf(),
    };
    let mut found: Vec<PathBuf> = vfs::read_dir(&folder)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.path)
        .filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix))
        .collect();
    if found.is_empty() {
        return dir.join(&path);
    }
    found.sort();
    let nanos = crate::platform::SystemTime::now()
        .duration_since(crate::platform::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos()) as usize;
    found.swap_remove(nanos % found.len())
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

/// The `property` list of a skin as custom options, then its `filepath`
/// choices.
pub fn custom_options(path: &Path) -> Vec<CustomOption> {
    let Ok(v) = read_value(path) else {
        return Vec::new();
    };
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut out: Vec<CustomOption> = properties(&v).iter().filter_map(to_custom).collect();
    for f in filepaths(&v) {
        let labels = filepath_files(dir, &f.path);
        let chosen = filepath_chosen(dir, &f, &HashMap::new());
        if labels.len() > 1 {
            out.push(CustomOption {
                default: labels
                    .iter()
                    .position(|l| Some(l) == chosen.as_ref())
                    .unwrap_or(0),
                name: f.name,
                first_id: 0,
                labels,
                ops: Vec::new(),
            });
        }
    }
    out
}

pub fn read_value(path: &Path) -> Result<Value, String> {
    read_skin(path, None, None).map(|v| v.0)
}

/// What a Lua skin's `skin_config` is made from: chosen option per
/// property name, the skin's header, the user's offsets by name, the
/// user's files by `filepath` name.
type SkinConfig<'a> = (
    &'a HashMap<String, i64>,
    &'a Value,
    &'a HashMap<String, [i32; 6]>,
    &'a HashMap<String, String>,
);

/// A skin's table, with the Lua state it ran in (Lua skins).
fn read_skin(
    path: &Path,
    skin_config: Option<SkinConfig<'_>>,
    state: Option<&dyn SkinState>,
) -> Result<(Value, Option<script::Script>), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "luaskin" || ext == "lua" {
        return lua::eval(path, skin_config, state).map(|(v, s)| (v, Some(s)));
    }
    let text = vfs::read(path)
        .map(|b| vibebms_core::encoding::decode_text(&b))
        .map_err(|e| e.to_string())?;
    let text = add_missing_commas(&text);
    json5::from_str::<Value>(&text)
        .or_else(|e| json5::from_str::<Value>(&merge_double_braces(&text)).map_err(|_| e))
        .map(|v| (v, None))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// beatoraja's own play5.json has `"value": {{...}},`, one brace too many
/// that libGDX's lenient reader gets past: read `{{` as `{`.
fn merge_double_braces(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut quote = None;
    while i < b.len() {
        let c = b[i];
        out.push(c);
        i += 1;
        match quote {
            Some(_) if c == '\\' => {
                if let Some(n) = b.get(i) {
                    out.push(*n);
                    i += 1;
                }
            }
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '{' => {
                let mut j = i;
                while j < b.len() && b[j].is_whitespace() {
                    j += 1;
                }
                if b.get(j) == Some(&'{') {
                    i = j + 1;
                }
            }
            None => {}
        }
    }
    out
}

/// beatoraja reads skins with libGDX's lenient JSON reader, which accepts
/// values separated only by a line break and numbers with leading zeros;
/// insert the missing commas and drop the zeros so json5 accepts them too
/// (strings and comments are left alone).
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
        // numbers with leading zeros (`"y":00`)
        if c == '0'
            && b.get(i + 1).is_some_and(char::is_ascii_digit)
            && !out
                .chars()
                .next_back()
                .is_some_and(|p| p.is_alphanumeric() || p == '.' || p == '_')
        {
            i += 1;
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
/// A JSON skin's size (w, h), without loading it.
pub fn size(path: &Path) -> Option<(f32, f32)> {
    let v = read_value(path).ok()?;
    Some((i(&v, "w", 1280) as f32, i(&v, "h", 720) as f32))
}

pub fn header(path: &Path) -> Option<(SkinType, String)> {
    let v = read_value(path).ok()?;
    let t = v.get("type")?.as_i64()?;
    Some((SkinType::from_id(t as i32), s(&v, "name").to_string()))
}

struct Ctx<'a> {
    sk: &'a Value,
    /// `.fnt` fonts read so far (index in the skin's fonts).
    bitmap_fonts: HashMap<PathBuf, Option<usize>>,
    dir: &'a Path,
    fps: &'a [FilePath],
    files: &'a HashMap<String, String>,
    h: f32,
    images: ImageFiles<'a>,
    source_index: HashMap<Cow<'a, str>, usize>,
    /// Definitions by list name and id (the first of an id counts).
    index: RefCell<HashMap<&'static str, HashMap<Cow<'a, str>, &'a Value>>>,
    scripts: RefCell<script::Builder>,
}

impl<'a> Ctx<'a> {
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
            frames: Src::grid([x, y, w, h], i(def, "divx", 1), i(def, "divy", 1)),
            cycle: i(def, "cycle", 0),
            timer: self
                .dyn_id(def.get("timer"), ScriptKind::Timer)
                .unwrap_or(0),
        })
    }

    fn find(&self, list: &'static str, id: &str) -> Option<&'a Value> {
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
    fn bitmap_font(&mut self, t: &Value, fonts: &mut Vec<BitmapFont>) -> Option<usize> {
        let f = self.find("font", &key(t.get("font")?)?)?;
        let p = skin_path(self.dir, self.fps, self.files, s(f, "path"));
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fnt")) {
            return None;
        }
        if let Some(i) = self.bitmap_fonts.get(&p) {
            return *i;
        }
        // types 1 and 2 are distance fields
        let sdf = matches!(i(f, "type", 0), 1 | 2);
        let font = load_bmfont(&p).map(|(mut font, pages)| {
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
    fn face(&self, t: &Value) -> Option<PathBuf> {
        let f = self.find("font", &key(t.get("font")?)?)?;
        let p = skin_path(self.dir, self.fps, self.files, s(f, "path"));
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        matches!(ext.as_str(), "ttf" | "otf" | "ttc").then_some(p)
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
fn text_size(t: &Value) -> Option<f32> {
    let v = t.get("size").and_then(Value::as_f64)? as f32;
    (v > 0.0).then_some(v)
}

/// A text's outline and shadow (`TextFx`), beatoraja's defaults where
/// not given.
fn text_fx(t: &Value) -> TextFx {
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
fn ref_num(def: &Value) -> Option<(f32, f32)> {
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
            range_ms: i(g, "judgeWidthMillis", 150).max(1) as f32,
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
        bins: if built_in(g) { 81 } else { i(g, "width", 301) },
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
        out.text_face = def.and_then(|t| cx.face(t));
        out.text_fx = def.map(text_fx).unwrap_or_default();
        out.text_size = def.and_then(text_size);
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
    let user = UserSettings {
        options: user_options.clone(),
        ..Default::default()
    };
    load_with(path, &user, None, image_size)
}

/// Build a skin from a beatoraja JSON/Lua skin with the user's settings;
/// `state` holds the scene's values a Lua skin reads while it is built.
pub fn load_with(
    path: &Path,
    user: &UserSettings,
    state: Option<&dyn SkinState>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let (user_options, offsets) = (&user.options, &user.offsets);
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    // pass 1: header + defaults
    let (raw, mut lua_state) = read_skin(path, None, None)?;
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
        let (v, s) = read_skin(path, Some((&by_name, &raw, offsets, &user.files)), state)?;
        lua_state = s;
        v
    } else {
        raw
    };
    let sk = preprocess(raw, &enabled, &dir, 0);
    let fps = filepaths(&sk);
    let w = i(&sk, "w", 1280) as f32;
    let h = i(&sk, "h", 720) as f32;
    let kind = SkinType::from_id(i(&sk, "type", -1) as i32);

    // sources (filepath customisation: first file matching the wildcard)
    let mut cx = Ctx {
        sk: &sk,
        bitmap_fonts: HashMap::new(),
        dir: &dir,
        fps: &fps,
        files: &user.files,
        h,
        images: ImageFiles::new(image_size),
        source_index: HashMap::new(),
        index: Default::default(),
        scripts: RefCell::new(script::Builder::new(lua_state, &dir)),
    };
    for src in arr(sk.get("source")) {
        let p = skin_path(&dir, &fps, &user.files, s(src, "path"));
        cx.source_index.insert(id_of(src), cx.images.len());
        cx.images.push(Some(p));
    }

    let mut skin = Skin {
        name: s(&sk, "name").to_string(),
        author: s(&sk, "author").to_string(),
        custom_options: props.iter().filter_map(to_custom).collect(),
        scene_ms: i(&sk, "scene", 0),
        input_ms: i(&sk, "input", 0),
        fadeout_ms: i(&sk, "fadeout", 0),
        finish_margin_ms: i(&sk, "finishmargin", 0),
        perspective: sk.get("perspective").and_then(Value::as_bool) == Some(true),
        ..Skin::new(path, kind, w, h)
    };
    if kind == SkinType::SkinSelect {
        let ss = sk.get("skinSelect");
        skin.skin_select = Some(crate::skin::SkinSelectInfo {
            default_category: ss.map(|v| i(v, "defaultCategory", 0)).unwrap_or(0) as i32,
            custom_property_count: ss
                .map(|v| i(v, "customPropertyCount", -1))
                .unwrap_or(-1)
                .max(0) as usize,
        });
    }
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
            })
        } else if let Some(sl) = cx.find("slider", &id) {
            cx.src(sl).map(|src| Kind::Slider {
                minmax: ref_num(sl),
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
                .filter_map(|x| cx.find("image", &key(x)?))
                .filter_map(|im| cx.src(im))
                .collect();
            (!srcs.is_empty()).then(|| Kind::ImageSet {
                srcs,
                id: cx.value_id(set, ScriptKind::Int),
            })
        } else if let Some(g) = graph(&cx, &id) {
            Some(Kind::Graph(g))
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
                minmax: ref_num(g),
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
            let pick = |list: &str, li: usize| {
                arr(n.get(list))
                    .get(li)
                    .and_then(key)
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
                let nid = key(nid).unwrap_or_default();
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
                    side: i(d, "side", 0).clamp(0, 1) as usize,
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
        } else if sk.get("skinpreview").is_some_and(|n| id_of(n) == id) {
            // the skin select screen's preview of the chosen skin (as
            // LR2's image 105)
            Some(Kind::RefImage(REF_SKIN_PREVIEW))
        } else if let Some(sl) = sk.get("songlist").filter(|n| id_of(n) == id) {
            skin.songlist = Some(songlist(&cx, sl, h));
            Some(Kind::SongList)
        } else if bga.map(|b| id_of(b) == id).unwrap_or(false) {
            Some(Kind::Bga)
        } else if let Some(j) = cx.find("judge", &id) {
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
    skin.images = cx.images.finish();
    // beatoraja's own key config skin is empty: its screen draws the rows
    if skin.objects.is_empty() && skin.kind != SkinType::KeyConfig {
        return Err(format!("{}: no drawable objects", path.display()));
    }
    Ok(skin)
}

mod lua {
    use super::*;

    /// Run a `.luaskin` and return its table as JSON (functions become
    /// script markers) with the Lua state. `skin_config` (option name ->
    /// selected op) is exported like beatoraja does.
    pub fn eval(
        path: &Path,
        skin_config: Option<SkinConfig<'_>>,
        state: Option<&dyn SkinState>,
    ) -> Result<(Value, script::Script), String> {
        let src = vfs::read(path).map_err(|e| e.to_string())?;
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let e = |e: mlua::Error| format!("{}: {e}", path.display());
        let mut sc = script::Script::new(&dir, false).map_err(e)?;
        let lua = sc.lua().clone();
        let g = lua.globals();
        if let Some((opts, header, user_offsets, user_files)) = skin_config {
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
            let fps = filepaths(header);
            let files = lua.create_table().map_err(e)?;
            for f in &fps {
                if let Some(name) = filepath_chosen(&dir, f, user_files) {
                    files.set(f.name.as_str(), name).map_err(e)?;
                }
            }
            cfg.set("file_path", files).map_err(e)?;
            let (d2, chosen) = (dir.clone(), user_files.clone());
            cfg.set(
                "get_path",
                lua.create_function(move |_, p: String| {
                    Ok(skin_path(&d2, &fps, &chosen, &p)
                        .to_string_lossy()
                        .into_owned())
                })
                .map_err(e)?,
            )
            .map_err(e)?;
            g.set("skin_config", cfg).map_err(e)?;
        }
        let v: mlua::Value = sc
            .with_state(state, || script::text_chunk(&lua, &src, path).eval())
            .map_err(e)?;
        let json = sc.to_json(v).map_err(e)?;
        Ok((json, sc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_runs_like_beatoraja() {
        // skins written for LuaJ (Lua 5.2) and beatoraja's restricted io / os
        let dir = std::env::temp_dir().join(format!("vibebms-lcompat-{}", std::process::id()));
        let skin = dir.join("theme").join("play");
        std::fs::create_dir_all(skin.join("parts")).unwrap();
        std::fs::write(skin.join("parts").join("list.txt"), "a\nb\n").unwrap();
        std::fs::write(skin.join("parts").join("p.lua"), "return 7").unwrap();
        let lua = r#"
            local main_state = require("main_state")
            local n = 0
            for l in io.lines("skin/othername/play/parts/list.txt") do n = n + 1 end
            local f = io.open("parts/list.txt", "r")
            local first = f:read("*l")
            f:close()
            local w = io.open("log.txt", "w")
            w:write("x")
            w:close()
            return {
                lines = n, first = first, written = io.open("log.txt") == nil,
                part = dofile("skin/othername/play/parts/p.lua"),
                band = bit32.band(0xff, 0x0f), shift = bit32.lshift(1, 4),
                random = math.random(1.0, 1.5), fmt = string.format("%d", 2.7),
                date = type(os.date("%Y")), time = type(os.time()),
                opt = main_state.option(150), num = main_state.number(71),
            }"#;
        std::fs::write(skin.join("t.luaskin"), lua).unwrap();
        struct St;
        impl SkinState for St {
            fn timer(&self, _: i32) -> Option<i64> {
                None
            }
            fn option(&self, id: i32) -> bool {
                id == 150
            }
            fn number(&self, id: i32) -> Option<i64> {
                (id == 71).then_some(1234)
            }
            fn text(&self, _: i32) -> Option<String> {
                None
            }
            fn rate(&self, _: i32) -> f32 {
                0.0
            }
        }
        let (v, _) = read_skin(&skin.join("t.luaskin"), None, Some(&St)).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let want = serde_json::json!({
            "lines": 2, "first": "a", "written": true, "part": 7, "band": 15, "shift": 16,
            "random": 1, "fmt": "2", "date": "string", "time": "number", "opt": true,
            "num": 1234,
        });
        assert_eq!(v, want);
    }

    /// Every example of the path resolution's specification (§5.2).
    #[test]
    fn skin_path_rules() {
        let dir = std::env::temp_dir().join(format!("vibebms-skinpath-{}", std::process::id()));
        for d in ["bomb/a", "bomb/b", "bg", "lane", "parts", "img", "plain"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in [
            "bomb/a/x.png",
            "bomb/b/x.png",
            "bg/one.png",
            "bg/two.png",
            "lane/lane_blue.png",
            "lane/lane_red.png",
            "parts/a.png",
            "parts/b.PNG",
            "parts/c.jpg",
            "img/f1.png",
            "img/f2.txt",
            "plain/file.png",
        ] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let v = serde_json::json!({"filepath": [
            {"name": "Gone", "path": "gone/*.png", "def": "x"},
            {"name": "Bomb", "path": "bomb\\*|1|", "def": "b"},
            {"name": "Back", "path": "bg/*.png", "def": "two"},
            {"name": "Lane", "path": "lane/lane_*.png", "def": "lane_blue.png"},
            {"name": "Plain", "path": "plain/file.png", "def": ""},
        ]});
        let fps = filepaths(&v);
        // an entry without a `*` is no entry
        assert_eq!(fps.len(), 4);
        let none = HashMap::new();
        let with = |k: &str, val: &str| HashMap::from([(k.to_string(), val.to_string())]);
        let p = |files: &HashMap<String, String>, path: &str| skin_path(&dir, &fps, files, path);
        assert_eq!(p(&none, "bomb/*|1|/x.png"), dir.join("bomb/b/x.png"));
        assert_eq!(p(&none, "bomb\\*|1|\\x.png"), dir.join("bomb/b/x.png"));
        assert_eq!(
            p(&with("Bomb", "A"), "bomb/*|1|/x.png"),
            dir.join("bomb/a/x.png")
        );
        assert_eq!(p(&none, "bg/*.png"), dir.join("bg/two.png"));
        assert_eq!(
            p(&with("Back", "three"), "bg/*.png"),
            dir.join("bg/one.png")
        );
        assert_eq!(
            p(&with("Back", "ONE.png"), "bg/*.png"),
            dir.join("bg/one.png")
        );
        assert_eq!(
            p(&with("Lane", "LANE_RED.PNG"), "lane/lane_*.png"),
            dir.join("lane/lane_red.png")
        );
        assert_eq!(p(&none, "lane/lane_*.png"), dir.join("lane/lane_blue.png"));
        // no candidates: the entry is skipped, then the wildcard rule finds
        // nothing either: the path as given
        assert_eq!(p(&none, "gone/*.png"), dir.join("gone/*.png"));
        // a wildcard without an entry: one of the matching files, at random
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let got = p(&none, "parts/*.png");
            assert!(
                got == dir.join("parts/a.png") || got == dir.join("parts/b.PNG"),
                "{got:?}"
            );
            seen.insert(got);
        }
        assert_eq!(p(&none, "img/*|frame|.png"), dir.join("img/f1.png"));
        assert_eq!(p(&none, "parts/none_*.gif"), dir.join("parts/none_*.gif"));
        // no wildcard: the path with its letter case as on disk
        assert_eq!(p(&none, "Plain\\File.PNG"), dir.join("plain/file.png"));
        assert_eq!(p(&none, "plain/missing.png"), dir.join("plain/missing.png"));
        std::fs::remove_dir_all(&dir).ok();
        assert!(!seen.is_empty());
    }

    #[test]
    fn filepath_choices() {
        let dir = std::env::temp_dir().join(format!("vibebms-fpath-{}", std::process::id()));
        for d in ["bomb/a", "bomb/b", "bg"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["bomb/a/x.png", "bomb/b/x.png", "bg/one.png", "bg/two.png"] {
            std::fs::write(dir.join(f), b"").unwrap();
        }
        let json = r#"{"type": 0, "name": "F",
            "filepath": [
                {"name": "Bomb", "path": "bomb/*|1|", "def": "b"},
                {"name": "Back", "path": "bg/*.png", "def": "two"}
            ]}"#;
        std::fs::write(dir.join("s.json"), json).unwrap();
        let v = read_value(&dir.join("s.json")).unwrap();
        let fps = filepaths(&v);
        let mut user = HashMap::new();
        // defaults, with or without the extension
        let p = skin_path(&dir, &fps, &user, "bomb/*|1|/x.png");
        assert_eq!(p, dir.join("bomb/b/x.png"));
        assert_eq!(
            skin_path(&dir, &fps, &user, "bg/*.png"),
            dir.join("bg/two.png")
        );
        user.insert("Bomb".to_string(), "a".to_string());
        assert_eq!(
            skin_path(&dir, &fps, &user, "bomb/*|1|/x.png"),
            dir.join("bomb/a/x.png")
        );
        let opts = custom_options(&dir.join("s.json"));
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(opts.len(), 2);
        assert_eq!(
            (opts[0].labels.clone(), opts[0].default),
            (vec!["a".into(), "b".into()], 1)
        );
        assert_eq!(
            opts[1].labels,
            vec!["one.png".to_string(), "two.png".into()]
        );
        assert!(opts[1].ops.is_empty());
    }

    #[test]
    fn nested_branches() {
        let v = serde_json::json!([
            {"if": [1], "values": [
                {"id": "a"},
                {"if": [2], "values": [{"id": "b"}, {"if": [-3], "values": [{"id": "c"}]}]},
                {"if": [3], "values": [{"id": "d"}]}
            ]}
        ]);
        let en: HashSet<i64> = [1, 2].into();
        let out = preprocess(v, &en, Path::new(""), 0);
        let ids: Vec<&str> = out.as_array().unwrap().iter().map(|x| s(x, "id")).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn double_braces_read_as_one() {
        let t =
            "{\"a\": [{\"if\": [1], \"value\": {{\"x\": 2, \"s\": \"{{\"}}], \"b\": {\"c\": 1}}";
        let v: Value = json5::from_str(&merge_double_braces(t)).unwrap();
        assert_eq!(v["a"][0]["value"]["x"], 2);
        assert_eq!(v["a"][0]["value"]["s"], "{{");
        assert_eq!(v["b"]["c"], 1);
    }

    #[test]
    fn lenient_json_commas() {
        let t = "{\"a\": [ {\"x\":1}\n {\"y\":\"s\"}\n ],\n \"b\": 2 // c\n , d: true\n e: 'q'\n f: 00, g: -007, h: 0.05, i: \"007\"\n}";
        let v: Value = json5::from_str(&add_missing_commas(t)).unwrap();
        assert_eq!(v["a"].as_array().unwrap().len(), 2);
        assert_eq!(v["d"], Value::Bool(true));
        assert_eq!(v["e"], Value::String("q".into()));
        assert_eq!((v["f"].as_i64(), v["g"].as_i64()), (Some(0), Some(-7)));
        assert_eq!(
            (v["h"].as_f64(), &v["i"]),
            (Some(0.05), &Value::String("007".into()))
        );
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
            .filter(|o| {
                matches!(
                    &o.kind,
                    Kind::Graph(Graph::Gauge(GaugeGraph { colors: None, .. }))
                        | Kind::Graph(Graph::Timing(TimingGraph { colors: None, .. }))
                )
            })
            .count();
        // our own skin asks for the built-in graph look
        assert_eq!(graphs, 2);
        assert!(sk
            .objects
            .iter()
            .any(|o| matches!(&o.kind, Kind::ImageSet { srcs, id: 370 } if srcs.len() == 11)));
    }

    #[test]
    fn graph_colors() {
        let g: Value = json5::from_str(r#"{"id": "g", "exHardBGColor": "102030"}"#).unwrap();
        let c = gauge_colors(&g);
        assert_eq!(c.len(), 24);
        // normal gauge: line above the border red, below green (defaults)
        assert_eq!(c[8], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(c[10], [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(c[4 * 4 + 1], hex_color("102030"));
        let list: Vec<String> = (0..24).map(|k| format!("{:02x}000080", k)).collect();
        let g = serde_json::json!({ "color": list });
        assert_eq!(gauge_colors(&g)[5], [5.0 / 255.0, 0.0, 0.0, 128.0 / 255.0]);
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
            // the 11 option buttons and the folder back button are clickable
            assert_eq!(sk.objects.iter().filter(|o| o.act.is_some()).count(), 12);
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
            for (p2, portrait, touch) in [
                (false, false, false),
                (true, false, false),
                (false, true, false),
                (true, true, false),
                (false, false, true),
                (true, false, true),
            ] {
                let path = default_skin(mode, p2, portrait, touch);
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

    #[test]
    fn builtin_battle_skins_load() {
        use crate::scene::play_skin::battle_skin;
        use vibebms_core::PlayMode;
        for (mode, kind) in [
            (PlayMode::Beat14K, SkinType::Battle7),
            (PlayMode::Beat10K, SkinType::Battle5),
        ] {
            for touch in [false, true] {
                let path = battle_skin(mode, touch);
                let sk = load(&path, &HashMap::new(), &crate::skin::image_size)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(sk.kind, kind);
                assert_eq!(sk.kind.battle_pair().and_then(|t| t.mode()), Some(mode));
                assert_eq!(sk.lanes.len(), mode.lane_count());
                // a gauge for each side, a judge region for each
                let sides: Vec<usize> = sk
                    .objects
                    .iter()
                    .filter_map(|o| match o.kind {
                        Kind::Gauge { side, .. } => Some(side),
                        _ => None,
                    })
                    .collect();
                assert_eq!(sides, vec![0, 1], "{}", path.display());
                assert!(sk.judges[1].judge.iter().all(Option::is_some));
            }
        }
    }

    #[test]
    fn builtin_perspective_skins_load_and_reflow() {
        use crate::scene::play_skin::perspective_skin;
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
            for portrait in [false, true] {
                let path = perspective_skin(mode, portrait);
                let mut sk = load(&path, &HashMap::new(), &size)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(sk.perspective, "{}", path.display());
                assert_eq!(sk.kind.mode(), Some(mode));
                assert_eq!(sk.lanes.len(), mode.lane_count());
                assert_eq!(sk.width > sk.height, !portrait);
                assert!(sk.images.iter().all(Option::is_some), "{}", path.display());
                assert!(sk.objects.iter().any(|o| matches!(o.kind, Kind::Notes)));
                assert!(sk.objects.iter().all(|o| o.dst.anchor.is_some()));
                // START / SELECT's corners are left free
                let (w, h) = (sk.width, sk.height);
                for right in [false, true] {
                    for [x, y, bw, bh] in crate::perspective::system_buttons(w, h, right) {
                        let b = Rect::new(x, y, bw, bh);
                        for o in &sk.objects {
                            let Some(k) = o.dst.keys.first() else {
                                continue;
                            };
                            let r = k.rect;
                            let full = r.w >= w && r.h >= h;
                            let apart = r.right() <= b.x
                                || r.x >= b.right()
                                || r.bottom() <= b.y
                                || r.y >= b.bottom();
                            assert!(full || apart, "{}: {r:?}", path.display());
                        }
                    }
                }
                // a taller screen: the background stretches, the bottom
                // half's objects come down with the lanes
                let bg = sk.objects[0].dst.keys[0].rect;
                let judge = sk.judges[0].judge[0].as_ref().unwrap().1.keys[2].rect;
                sk.reflow(w, h + 200.0);
                assert_eq!(sk.height, h + 200.0);
                let bg2 = sk.objects[0].dst.keys[0].rect;
                assert_eq!((bg2.y, bg2.h), (bg.y, bg.h + 200.0));
                let judge2 = sk.judges[0].judge[0].as_ref().unwrap().1.keys[2].rect;
                assert_eq!(judge2.y, judge.y + 100.0);
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

    #[test]
    fn bmfont_texts() {
        let dir = std::env::temp_dir().join(format!("vibebms-bmf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fnt = "info face=\"My Font\" size=-32 bold=0\ncommon lineHeight=36 base=29 pages=1\n\
page id=0 file=\"f_0.png\"\nchars count=1\n\
char id=65 x=2 y=3 width=20 height=24 xoffset=1 yoffset=5 xadvance=22 page=0 chnl=15\n";
        std::fs::write(dir.join("f.fnt"), fnt).unwrap();
        let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
            "font": [{"id": 0, "path": "f.fnt"}],
            "text": [{"id": "t", "ref": 10, "font": 0, "size": 24}],
            "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
        std::fs::write(dir.join("r.json"), json).unwrap();
        let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((64, 64))).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(matches!(
            &s.objects[0].kind,
            Kind::Text {
                font: Some(0),
                size: Some(24.0),
                ..
            }
        ));
        let f = &s.fonts[0];
        assert_eq!((f.size, f.em), (36.0, 32.0));
        // glyphs scale by the text's size over the font's (beatoraja), not
        // by the destination height over the line height
        use crate::skin::render::bmfont_scale;
        assert_eq!(bmfont_scale(f, 10.0, Some(24.0)), 0.75);
        assert_eq!(bmfont_scale(f, 18.0, None), 0.5);
        let lr2 = BitmapFont {
            size: 20.0,
            ..Default::default()
        };
        assert_eq!(bmfont_scale(&lr2, 10.0, Some(24.0)), 0.5);
        let (img, rect) = f.glyphs[&'A'];
        assert!(s.images[img].as_ref().unwrap().ends_with("f_0.png"));
        assert_eq!(
            (rect, f.metrics[&'A']),
            ([2.0, 3.0, 20.0, 24.0], [1.0, 5.0, 22.0])
        );
    }

    #[test]
    fn text_outline_and_shadow() {
        let dir = std::env::temp_dir().join(format!("vibebms-tfx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fnt = "info face=\"F\" size=32\ncommon lineHeight=32 base=29 pages=1\n\
page id=0 file=\"f_0.png\"\nchar id=65 x=0 y=0 width=20 height=24 xoffset=0 yoffset=0 xadvance=22 page=0\n";
        std::fs::write(dir.join("d.fnt"), fnt).unwrap();
        let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
            "font": [{"id": 0, "path": "f.ttf"}, {"id": 1, "path": "d.fnt", "type": 1}],
            "text": [{"id": "t", "ref": 10, "font": 0, "outlineColor": "000000ff",
                      "outlineWidth": 0.4, "shadowColor": "ff0000", "shadowOffsetX": 4,
                      "shadowOffsetY": -2.5, "shadowSmoothness": 0.5},
                     {"id": "u", "ref": 10, "font": 1}],
            "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]},
                            {"id": "u", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
        std::fs::write(dir.join("r.json"), json).unwrap();
        let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((64, 64))).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let Kind::Text { fx, .. } = &s.objects[0].kind else {
            panic!()
        };
        assert_eq!(
            *fx,
            TextFx {
                outline_color: [0.0, 0.0, 0.0, 1.0],
                outline_width: 0.4,
                shadow_color: [1.0, 0.0, 0.0, 1.0],
                shadow_offset: (4.0, -2.5),
                shadow_smoothness: 0.5,
            }
        );
        assert!(fx.has_shadow());
        let st = fx.sdf();
        assert!((st.outline_distance - 0.3).abs() < 1e-6);
        assert_eq!(st.shadow_smoothing, 0.25);
        // nothing given: beatoraja's defaults (transparent white, no shadow)
        let Kind::Text { fx, font, .. } = &s.objects[1].kind else {
            panic!()
        };
        assert_eq!(*fx, TextFx::default());
        assert!(!fx.has_shadow());
        assert_eq!(fx.sdf().outline_distance, 0.5);
        // a font of type 1 is a distance field
        assert!(s.fonts[font.unwrap()].sdf);
    }

    #[test]
    fn constant_texts() {
        let dir = std::env::temp_dir().join(format!("vibebms-ctext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
            "font": [{"id": 0, "path": "f.TTF"}, {"id": 1, "path": "b.fnt"}],
            "text": [{"id": "t", "constantText": "A\nB", "ref": 10, "font": 0},
                     {"id": "u", "ref": 10, "font": 1}],
            "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]},
                            {"id": "u", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
        std::fs::write(dir.join("r.json"), json).unwrap();
        let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((1, 1))).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        let Kind::Text { constant, face, .. } = &s.objects[0].kind else {
            panic!()
        };
        assert_eq!(constant.as_deref(), Some("A\nB"));
        // TrueType fonts are drawn with, bitmap ones are not
        assert!(face.as_ref().is_some_and(|p| p.ends_with("f.TTF")));
        assert!(matches!(&s.objects[1].kind, Kind::Text { face: None, .. }));
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

    #[test]
    fn lua_skin_scripts() {
        use crate::skin::script::Overlay;
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
            Some((
                &HashMap::new(),
                &Value::Null,
                &HashMap::new(),
                &HashMap::new(),
            )),
            None,
        )
        .unwrap()
        .0;
        assert_eq!(raw["image"][1]["id"], "img2");
        // what the sandbox keeps out
        let escape = r#"
            local ok = os.execute == nil and os.remove == nil and os.getenv == nil
                and io.popen("ls") == nil and io.open("/etc/passwd") == nil
                and io.open("../../x.txt") == nil and debug.sethook == nil and string.dump == nil
                and package.loadlib == nil and package.searchpath == nil
                and load("\27Lua", "b", "b") == nil
            package.path = "/etc/?"
            return ok and not pcall(require, "passwd") and not pcall(dofile, "../../x.lua")
                and loadfile("/etc/passwd") == nil and loadfile("../../x.lua") == nil
                and loadfile("play.luaskin") ~= nil"#;
        std::fs::write(dir.join("esc.luaskin"), escape).unwrap();
        let (v, _) = read_skin(&dir.join("esc.luaskin"), None, None).unwrap();
        assert_eq!(v, Value::Bool(true));
        // endless loops are stopped
        std::fs::write(dir.join("loop.luaskin"), "while true do end").unwrap();
        assert!(read_skin(&dir.join("loop.luaskin"), None, None).is_err());
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
        let sc = Overlay {
            st: &st,
            rt: Some(&rt),
            offsets: &HashMap::new(),
        };
        assert_eq!(sc.number(num_id), Some(42));
        assert_eq!(sc.text(txt_id).as_deref(), Some("ex 21"));
        assert!(num.dst.ops.iter().all(|o| sc.option(*o)));
        assert_eq!(sc.timer(img.dst.timer), None);
        // the observed condition turns on at 300 ms: the timer starts there
        let st = ScriptSt { flag: true };
        rt.update(&st, 300);
        rt.update(&st, 400);
        let sc = Overlay {
            st: &st,
            rt: Some(&rt),
            offsets: &HashMap::new(),
        };
        assert_eq!(sc.timer(img.dst.timer), Some(300));
        // click: the script runs on the next frame and asks for event 15
        let (act, _) = img.act.unwrap();
        assert!(rt.click(act));
        rt.update(&st, 500);
        assert_eq!(rt.take_events(), vec![15]);
    }

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
        let user = UserSettings {
            offsets: offs,
            ..Default::default()
        };
        let s2 = load_with(&dir.join("play.luaskin"), &user, None, &sz).unwrap();
        assert_eq!(s2.objects[0].dst.keys[0].rect.x, 7.0);
        assert_eq!(s2.offsets[&40], [0.0, 7.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(custom_offsets(&dir.join("play.luaskin")).len(), 1);
        std::fs::remove_dir_all(&root).ok();
        // the wildcard source follows the filepath default too
        assert!(s.images[0].as_ref().unwrap().ends_with("judge/b.png"));
    }

    #[test]
    fn json_skin_scripts() {
        use crate::skin::script::Overlay;
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
        let sc = Overlay {
            st: &st,
            rt: Some(&rt),
            offsets: &HashMap::new(),
        };
        assert_eq!(sc.number(id), Some(22));
        assert!(sc.option(o.dst.ops[1]));
        assert_eq!(sc.timer(10001), Some(250));
        let st = ScriptSt { flag: false };
        rt.update(&st, 300);
        let sc = Overlay {
            st: &st,
            rt: Some(&rt),
            offsets: &HashMap::new(),
        };
        assert!(!sc.option(o.dst.ops[1]));
        assert_eq!(sc.timer(10001), None);
    }

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
