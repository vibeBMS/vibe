//! beatoraja skins: JSON (`.json`) and Lua (`.luaskin`) play skins.
//!
//! A Lua skin is a script returning the same table a JSON skin contains, so
//! both are turned into a `serde_json::Value` and converted by one builder.
//! Values given as Lua functions (dynamic draw conditions / numbers) are not
//! evaluated yet; those destinations fall back to their static settings.

use super::*;
use crate::platform::vfs;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

fn i(v: &Value, k: &str, d: i64) -> i64 {
    v.get(k)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .unwrap_or(d)
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

pub fn read_value(
    path: &Path,
    skin_config: Option<&HashMap<String, i64>>,
) -> Result<Value, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "luaskin" || ext == "lua" {
        return lua::eval(path, skin_config);
    }
    let text = vfs::read(path)
        .map(|b| vibebms_core::encoding::decode_text(&b))
        .map_err(|e| e.to_string())?;
    json5::from_str::<Value>(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// (type, name) without building the skin.
pub fn header(path: &Path) -> Option<(SkinType, String)> {
    let v = read_value(path, None).ok()?;
    let t = v.get("type")?.as_i64()?;
    Some((SkinType::from_id(t as i32), s(&v, "name").to_string()))
}

struct Ctx<'a> {
    sk: &'a Value,
    h: f32,
    images: Vec<Option<PathBuf>>,
    sizes: Vec<Option<(u32, u32)>>,
    source_index: HashMap<String, usize>,
}

impl Ctx<'_> {
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
            timer: i(def, "timer", 0) as i32,
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
            timer: i(d, "timer", 0) as i32,
            ops: arr(d.get("op"))
                .iter()
                .filter_map(Value::as_i64)
                .map(|o| o as i32)
                .collect(),
            ..Default::default()
        };
        let mut prev: Option<[i64; 10]> = None;
        for a in arr(d.get("dst")) {
            let g = |k: &str, idx: usize, def: i64| {
                a.get(k)
                    .and_then(Value::as_i64)
                    .unwrap_or(prev.map(|p| p[idx]).unwrap_or(def))
            };
            let cur = [
                g("time", 0, 0),
                g("x", 1, 0),
                g("y", 2, 0),
                g("w", 3, 0),
                g("h", 4, 0),
                g("acc", 5, 0),
                g("a", 6, 255),
                g("r", 7, 255),
                g("g", 8, 255),
                g("b", 9, 255),
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
            let c = |n: i64| n.clamp(0, 255) as f32 / 255.0;
            out.add_key(Key {
                time: cur[0],
                rect: Rect::new(x, self.h - (y + h), w, h),
                color: [c(cur[7]), c(cur[8]), c(cur[9]), c(cur[6])],
                angle: -angle,
            });
            prev = Some(cur);
        }
        out
    }

    fn number_kind(&self, def: &Value) -> Option<Kind> {
        let mut src = self.src(def)?;
        let n = src.frames.len();
        let digits = if n % 24 == 0 {
            src.frames.truncate(12);
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
            id: i(def, "ref", 0) as i32,
            align: i(def, "align", 0) as i32,
            keta: i(def, "digit", 1).max(1) as usize,
            zero_pad: digits > 10 || i(def, "zeropadding", 0) > 0,
        })
    }
}

/// Build a play skin from a beatoraja JSON/Lua skin.
pub fn load(
    path: &Path,
    user_options: &HashMap<i32, bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
    // pass 1: header + defaults
    let raw = read_value(path, None)?;
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
        read_value(path, Some(&by_name))?
    } else {
        raw
    };
    let sk = preprocess(raw, &enabled, &dir, 0);
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
    };
    for src in arr(sk.get("source")) {
        let p = super::lr2::glob_path(&dir.join(s(src, "path").replace('\\', "/")));
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
        bar_line: None,
        custom_options: props
            .iter()
            .filter_map(|(n, items, _)| {
                items.first().map(|f| CustomOption {
                    name: n.clone(),
                    first_id: f.1 as i32,
                    labels: items.iter().map(|x| x.0.clone()).collect(),
                })
            })
            .collect(),
        scene_ms: i(&sk, "scene", 0),
        input_ms: i(&sk, "input", 0),
        fadeout_ms: i(&sk, "fadeout", 0),
        finish_margin_ms: i(&sk, "finishmargin", 0),
    };
    let note = sk.get("note").and_then(obj);
    let gauge = sk.get("gauge").and_then(obj);
    let bga = sk.get("bga").and_then(obj);
    let mode = kind.mode();

    for d in arr(sk.get("destination")) {
        let id = id_of(d);
        let dst = cx.dst(d);
        let kind = if let Some(img) = cx.find("image", &id) {
            cx.src(img).map(Kind::Image)
        } else if let Some(v) = cx.find("value", &id) {
            cx.number_kind(v)
        } else if let Some(t) = cx.find("text", &id) {
            Some(Kind::Text {
                id: i(t, "ref", 0) as i32,
                align: i(t, "align", 0) as i32,
            })
        } else if let Some(sl) = cx.find("slider", &id) {
            cx.src(sl).map(|src| Kind::Slider {
                src,
                id: i(sl, "type", 0) as i32,
                dir: i(sl, "angle", 0) as i32,
                range: i(sl, "range", 0) as f32,
            })
        } else if let Some(g) = cx.find("graph", &id) {
            cx.src(g).map(|src| Kind::Bar {
                src,
                id: i(g, "type", 0) as i32,
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
            if let Some(g) = arr(n.get("group")).first() {
                let gd = cx.dst(g);
                if let Some(img) = cx.find("image", &id_of(g)).and_then(|i| cx.src(i)) {
                    skin.bar_line = Some((img, gd));
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
                }
            })
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
        if let Some(kind) = kind {
            skin.objects.push(Obj { kind, dst });
        }
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
    use mlua::{Lua, LuaSerdeExt};

    /// Run a `.luaskin` and return its table as JSON. `skin_config`
    /// (option name -> selected op) is exported like beatoraja does.
    pub fn eval(path: &Path, skin_config: Option<&HashMap<String, i64>>) -> Result<Value, String> {
        let src = vfs::read(path).map_err(|e| e.to_string())?;
        let dir = path.parent().unwrap_or(Path::new("")).to_path_buf();
        let lua = Lua::new();
        let e = |e: mlua::Error| e.to_string();
        let g = lua.globals();
        // modules: stubs for load-time evaluation + skin-relative require
        let preload: mlua::Table = lua
            .globals()
            .get::<mlua::Table>("package")
            .map_err(e)?
            .get("preload")
            .map_err(e)?;
        for m in ["main_state", "timer_util", "event_util"] {
            let t = lua.create_table().map_err(e)?;
            let stub = lua
                .create_function(|_, _: mlua::MultiValue| Ok(0))
                .map_err(e)?;
            let meta = lua.create_table().map_err(e)?;
            meta.set(
                "__index",
                lua.create_function(move |_, (_, _): (mlua::Value, mlua::Value)| Ok(stub.clone()))
                    .map_err(e)?,
            )
            .map_err(e)?;
            t.set_metatable(Some(meta)).map_err(e)?;
            preload
                .set(
                    m,
                    lua.create_function(move |_, _: mlua::MultiValue| Ok(t.clone()))
                        .map_err(e)?,
                )
                .map_err(e)?;
        }
        let pkg: mlua::Table = g.get("package").map_err(e)?;
        pkg.set(
            "path",
            format!("{}/?.lua;{}/?.luaskin", dir.display(), dir.display()),
        )
        .map_err(e)?;
        if let Some(opts) = skin_config {
            let cfg = lua.create_table().map_err(e)?;
            let option = lua.create_table().map_err(e)?;
            let enabled = lua.create_table().map_err(e)?;
            for (k, v) in opts {
                option.set(k.as_str(), *v).map_err(e)?;
                enabled.push(*v).map_err(e)?;
            }
            cfg.set("option", option).map_err(e)?;
            cfg.set("enabled_options", enabled).map_err(e)?;
            cfg.set("offset", lua.create_table().map_err(e)?)
                .map_err(e)?;
            cfg.set("file_path", lua.create_table().map_err(e)?)
                .map_err(e)?;
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
        let opts = mlua::serde::DeserializeOptions::new()
            .deny_unsupported_types(false)
            .deny_recursive_tables(false);
        lua.from_value_with::<Value>(v, opts).map_err(e)
    }
}

#[cfg(target_arch = "wasm32")]
mod lua {
    use super::*;
    pub fn eval(path: &Path, _: Option<&HashMap<String, i64>>) -> Result<Value, String> {
        Err(format!(
            "{}: Lua skins are not supported in the web build",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
