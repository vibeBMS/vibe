use super::objects::{text_fx, text_size, Ctx};
use super::*;

/// Parse a beatoraja `songlist`. Sub-destinations are relative to the bar
/// (y-up offsets from its bottom); convert them to y-down offsets.
pub(super) fn songlist(cx: &Ctx, sl: &Value, h: f32) -> SongListSkin {
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
