//! beatoraja skins: JSON (`.json`) and Lua (`.luaskin`) play skins.
//!
//! A Lua skin is a script returning the same table a JSON skin contains, so
//! both are turned into a `serde_json::Value` and converted by one builder.
//! Values given as Lua functions (or, in JSON skins, Lua source strings) —
//! draw conditions, timers, numbers, texts, image indices, rates, click
//! events, `customTimers` / `customEvents` — run through [`script`].

mod files;
mod json;
mod lua;
mod objects;
mod songlist;

pub(crate) use objects::{gauge_colors, hex_color};

use super::script::{self, ScriptKind};
use super::*;
use crate::platform::vfs;
use files::{filepath_chosen, filepath_files, filepaths, FilePath};
use json::{add_missing_commas, merge_double_braces, parse};
use objects::Ctx;
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

/// What `preprocess` reads includes with: the skin's `Home`, and what
/// its load may still read through them.
struct Files<'a> {
    home: &'a Home,
    includes: Includes,
}

/// Resolve `include` objects (files named from `dir`, within what
/// `files` lets the skin read) and `{"if": .., "value"/"values": ..}`
/// branches.
fn preprocess(v: Value, en: &HashSet<i64>, files: &mut Files, dir: &Path, depth: usize) -> Value {
    match v {
        Value::Object(m) if m.contains_key("include") && depth < 8 => {
            let p = (files.home).file_from(dir, m["include"].as_str().unwrap_or(""));
            let inc = p.as_ref().and_then(|p| {
                let text = String::from_utf8(files.includes.read(p)?).ok()?;
                parse(&text).ok()
            });
            match (inc, p.as_deref().and_then(Path::parent)) {
                (Some(inc), Some(dir)) => preprocess(inc, en, files, dir, depth + 1),
                _ => Value::Null,
            }
        }
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k, preprocess(v, en, files, dir, depth)))
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
                                preprocess(Value::Array(vs.clone()), en, files, dir, depth)
                            {
                                out.extend(vs);
                            }
                        } else if let Some(v) = item.get("value") {
                            out.push(preprocess(v.clone(), en, files, dir, depth));
                        }
                        if branches {
                            break; // object-valued conditional: first matching clause wins
                        }
                    }
                } else {
                    out.push(preprocess(item, en, files, dir, depth));
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

/// What a skin says of itself before it is built, from its table as read
/// without the user's settings.
struct Header {
    kind: Option<i64>,
    name: String,
    size: (f32, f32),
    props: Vec<Property>,
    /// Every `offset`'s name, and the ones the user may adjust.
    offset_names: Vec<String>,
    offsets: Vec<CustomOffset>,
    filepaths: Vec<FilePath>,
}

impl Header {
    fn of(v: &Value) -> Header {
        Header {
            kind: v.get("type").and_then(Value::as_i64),
            name: s(v, "name").to_string(),
            size: (i(v, "w", 1280) as f32, i(v, "h", 720) as f32),
            props: properties(v),
            offset_names: arr(v.get("offset"))
                .iter()
                .map(|o| s(o, "name").to_string())
                .collect(),
            offsets: offsets_of(v),
            filepaths: filepaths(v),
        }
    }
}

/// A skin file's header, read once per version of the file: a Lua skin
/// runs for it.
fn header_of(path: &Path, home: &Home) -> Result<std::sync::Arc<Header>, String> {
    type Cache = HashMap<(PathBuf, (i64, i64)), std::sync::Arc<Header>>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let key = (path.to_path_buf(), vfs::stamp(path));
    let cached = |c: &mut Option<Cache>| c.get_or_insert_default().get(&key).cloned();
    if let Some(h) = CACHE.lock().ok().as_deref_mut().and_then(cached) {
        return Ok(h);
    }
    let h = std::sync::Arc::new(Header::of(&read_skin(path, home, None, None)?.0));
    if let Ok(mut c) = CACHE.lock() {
        c.get_or_insert_default().insert(key, h.clone());
    }
    Ok(h)
}

/// The `offset` list of a skin: positions the user may adjust.
pub fn custom_offsets(path: &Path, skins_root: &Path) -> Vec<CustomOffset> {
    header_of(path, &Home::of(path, skins_root))
        .map(|h| h.offsets.clone())
        .unwrap_or_default()
}

/// The `property` list of a skin as custom options, then its `filepath`
/// choices.
pub fn custom_options(path: &Path, skins_root: &Path) -> Vec<CustomOption> {
    let home = Home::of(path, skins_root);
    let Ok(h) = header_of(path, &home) else {
        return Vec::new();
    };
    let mut out: Vec<CustomOption> = h.props.iter().filter_map(to_custom).collect();
    for f in &h.filepaths {
        let labels = filepath_files(&home, &f.path);
        let chosen = filepath_chosen(&home, f, &HashMap::new());
        if labels.len() > 1 {
            out.push(CustomOption {
                default: labels
                    .iter()
                    .position(|l| Some(l) == chosen.as_ref())
                    .unwrap_or(0),
                name: f.name.clone(),
                first_id: 0,
                labels,
                ops: Vec::new(),
            });
        }
    }
    out
}

/// What a Lua skin's `skin_config` is made from: chosen option per
/// property name, the skin's header, the user's offsets by name, the
/// user's files by `filepath` name.
type SkinConfig<'a> = (
    &'a HashMap<String, i64>,
    &'a Header,
    &'a HashMap<String, [i32; 6]>,
    &'a HashMap<String, String>,
);

fn is_lua(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("luaskin") || e.eq_ignore_ascii_case("lua"))
}

/// A skin's table, with the Lua state it ran in (Lua skins).
fn read_skin(
    path: &Path,
    home: &Home,
    skin_config: Option<SkinConfig<'_>>,
    state: Option<&dyn SkinState>,
) -> Result<(Value, Option<script::Script>), String> {
    if is_lua(path) {
        return lua::eval(path, home, skin_config, state).map(|(v, s)| (v, Some(s)));
    }
    let text = read_text(path)
        .map(|b| vibebms_core::encoding::decode_text(&b))
        .map_err(|e| e.to_string())?;
    let text = add_missing_commas(&text);
    parse(&text)
        .or_else(|e| parse(&merge_double_braces(&text)).map_err(|_| e))
        .map(|v| (v, None))
        .map_err(|e| format!("{}: {e}", path.display()))
}
/// A skin's size (w, h), without building it.
pub fn size(path: &Path, skins_root: &Path) -> Option<(f32, f32)> {
    let read = || header_of(path, &Home::of(path, skins_root)).map(|h| h.size);
    no_panic(path, read).ok()
}

/// (type, name) without building the skin.
pub fn header(path: &Path, skins_root: &Path) -> Option<(SkinType, String)> {
    let h = header_of(path, &Home::of(path, skins_root)).ok()?;
    Some((SkinType::from_id(h.kind? as i32), h.name.clone()))
}

/// The options in effect: of each property, the user's choice, else its
/// default; as a set of ops and by property name.
fn chosen_options(
    props: &[Property],
    user: &HashMap<i32, bool>,
) -> (HashSet<i64>, HashMap<String, i64>) {
    let mut enabled = HashSet::new();
    let mut by_name = HashMap::new();
    for (name, items, def) in props {
        // a user choice overrides the default
        let chosen = items
            .iter()
            .position(|(_, op)| user.get(&(*op as i32)).copied().unwrap_or(false))
            .unwrap_or(*def);
        if let Some((_, op)) = items.get(chosen) {
            enabled.insert(*op);
            by_name.insert(name.clone(), *op);
        }
    }
    (enabled, by_name)
}

/// The timers and events scripts drive (`customTimers`, `customEvents`).
fn custom_scripts(sk: &Value, b: &mut script::Builder) {
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

/// Build a skin from a beatoraja JSON/Lua skin with the user's settings;
/// `state` holds the scene's values a Lua skin reads while it is built.
pub fn load_with(
    path: &Path,
    skins_root: &Path,
    user: &UserSettings,
    state: Option<&dyn SkinState>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let home = Home::of(path, skins_root);
    // pass 1: header + defaults (a Lua skin's run without settings is kept)
    let (header, json) = if is_lua(path) {
        (header_of(path, &home)?, None)
    } else {
        let v = read_skin(path, &home, None, None)?.0;
        (std::sync::Arc::new(Header::of(&v)), Some(v))
    };
    let props = &header.props;
    let (enabled, by_name) = chosen_options(props, &user.options);
    // pass 2 (Lua): run again with skin_config
    let (raw, lua_state) = match json {
        Some(v) => (v, None),
        None => {
            let config = (&by_name, &*header, &user.offsets, &user.files);
            read_skin(path, &home, Some(config), state)?
        }
    };
    let mut files = Files {
        home: &home,
        includes: Includes::new(),
    };
    let sk = preprocess(raw, &enabled, &mut files, &home.dir, 0);
    let fps = filepaths(&sk);
    let scripts = script::Builder::new(lua_state, &home);
    let mut cx = Ctx::new(&sk, &home, &fps, &user.files, image_size, scripts);
    let kind = SkinType::from_id(i(&sk, "type", -1) as i32);
    let mut skin = Skin {
        name: s(&sk, "name").to_string(),
        author: s(&sk, "author").to_string(),
        custom_options: props.iter().filter_map(to_custom).collect(),
        scene_ms: i(&sk, "scene", 0),
        input_ms: i(&sk, "input", 0),
        fadeout_ms: i(&sk, "fadeout", 0),
        finish_margin_ms: i(&sk, "finishmargin", 0),
        perspective: sk.get("perspective").and_then(Value::as_bool) == Some(true),
        // (the sources are the first images, in order)
        lazy_images: (arr(sk.get("source")).iter().enumerate())
            .filter(|(_, src)| src.get("lazy").and_then(Value::as_bool) == Some(true))
            .map(|(i, _)| i)
            .collect(),
        options_on: enabled.iter().map(|op| *op as i32).collect(),
        ..Skin::new(path, kind, i(&sk, "w", 1280) as f32, cx.h)
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
    for d in arr(sk.get("destination")) {
        objects::add_object(&mut cx, &mut skin, d);
    }
    within_limits(path, &files.includes, &cx.images)?;
    custom_scripts(&sk, cx.scripts.get_mut());
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
        let v = user.offsets.get(&o.name).copied().unwrap_or_default();
        skin.offsets.insert(o.id, v.map(|x| x as f32));
    }
    skin.images = cx.images.finish();
    // beatoraja's own key config skin is empty: its screen draws the rows
    if skin.objects.is_empty() && skin.kind != SkinType::KeyConfig {
        return Err(format!("{}: no drawable objects", path.display()));
    }
    Ok(skin)
}

#[cfg(test)]
mod tests;
