//! The page tree (every page, every sub-page its rows open, every group
//! open) and search over it.

use super::per_play::MOVED;
use super::row::{Folds, Kind, Row};
use super::{Armed, Confirm, Page, View};
use crate::ctx::Ctx;

/// Results shown at most.
const SHOWN: usize = 60;

/// Every row of every page and sub-page, with the views down to it (a
/// sub-page is walked once, from the first row that opens it).
pub(super) fn walk(ctx: &Ctx, armed: &Armed<Confirm>) -> Vec<(Vec<View>, Row)> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for p in Page::ALL {
        visit(ctx, armed, &mut vec![View::Page(p)], &mut seen, &mut out);
    }
    out
}

fn visit(
    ctx: &Ctx,
    armed: &Armed<Confirm>,
    path: &mut Vec<View>,
    seen: &mut Vec<View>,
    out: &mut Vec<(Vec<View>, Row)>,
) {
    let Some(view) = path.last() else {
        return;
    };
    for row in view.rows(ctx, armed, Folds::All) {
        let sub = row.open.clone().map(View::Sub);
        out.push((path.clone(), row));
        if let Some(sub) = sub.filter(|s| !seen.contains(s)) {
            seen.push(sub.clone());
            path.push(sub);
            visit(ctx, armed, path, seen, out);
            path.pop();
        }
    }
}

/// "ONLINE › BMS-IR › TABLES …": where a row is.
fn crumb(ctx: &Ctx, path: &[View], row: &Row) -> String {
    let mut parts: Vec<String> = path.iter().map(|v| v.title(ctx)).collect();
    if !row.section.is_empty() {
        parts.push(row.section.clone());
    }
    parts.join(" › ")
}

/// The rows matching every word of `query`, each with its place in its
/// description, then pointers to the song select's options it names.
pub(super) fn find(ctx: &Ctx, armed: &Armed<Confirm>, query: &str) -> (Vec<Row>, Vec<Vec<View>>) {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let hit = |hay: &str| words.iter().all(|w| hay.contains(w.as_str()));
    let (mut rows, mut paths) = (Vec::new(), Vec::new());
    for m in MOVED {
        let label = crate::i18n::get(m.label);
        // (the English label too: search finds it in either)
        if hit(&format!("{} {} {}", label, m.label, m.keys).to_lowercase()) {
            rows.push(
                Row::note("moved", tr!("{option} → song select", option = label), "").desc(tr!(
                    "SONG SELECT › {place}",
                    place = crate::i18n::get(m.place)
                )),
            );
            paths.push(Vec::new());
        }
    }
    for (path, mut row) in walk(ctx, armed) {
        if !row.focusable() || matches!(row.kind, Kind::Group { .. }) || !hit(&row.haystack()) {
            continue;
        }
        let at = crumb(ctx, &path, &row);
        row.desc = if row.desc.is_empty() {
            at
        } else {
            format!("{at} · {}", row.desc)
        };
        row.open = None;
        row.kind = Kind::Item;
        rows.push(row);
        paths.push(path);
        if rows.len() >= SHOWN {
            break;
        }
    }
    (rows, paths)
}
