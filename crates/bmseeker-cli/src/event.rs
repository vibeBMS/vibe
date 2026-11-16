//! `event works`, `event check`: a BMS event's works and how their links
//! fare, as records (JSON Lines) a script keeps however it likes.
//!
//! ```text
//! work   {"num","title","artist","team","kind","size_kb","page",
//!         "links":[{"label","url"}],"main":[url],"extra":[[url]],"review"?, ...}
//! check  {"url","state":"archive|chart|page|gone|error","kind"?,"format"?,
//!         "size","charts","title"?,"error"?,"at"}
//! ```

use crate::io::{read_text, records, s};
use crate::Run;
use bmseeker::hosts::Want;
use bmseeker::manbow;
use bmseeker::net::{Client, Http, Request};
use clap::Subcommand;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Event {
    /// An event's works (manbow's venue number), each with its links and
    /// which to take: the main archive, add-ons, mirrors.
    Works {
        venue: u32,
        /// Records (by "num") whose fields replace the guess: choices made
        /// by hand.
        #[arg(long, value_name = "FILE|-")]
        overrides: Option<PathBuf>,
    },
    /// Each work's links downloaded and looked into: whether they still
    /// work and what they hold (the song, an add-on, media).
    Check {
        /// Work records (`event works`'). Default: stdin.
        #[arg(default_value = "-")]
        works: PathBuf,
        /// Earlier checks: those fresh enough are kept, not checked again.
        #[arg(long, value_name = "FILE")]
        known: Option<PathBuf>,
        /// How long a check stays fresh (days; a failure that may pass: one).
        #[arg(long, default_value_t = 30)]
        days: u64,
        /// Stop checking after this long (the rest keep their earlier checks).
        #[arg(long)]
        minutes: Option<u64>,
    },
}

pub fn event(r: &Run, c: Event) -> Result<(), String> {
    match c {
        Event::Works { venue, overrides } => works(r, venue, overrides),
        Event::Check {
            works,
            known,
            days,
            minutes,
        } => check(r, works, known, days, minutes),
    }
}

fn works(r: &Run, venue: u32, overrides: Option<PathBuf>) -> Result<(), String> {
    let http = Client::new(r.config());
    let base = &r.config().manbow;
    let page = |u: String| http.send(&Request::Get(u)).map(|b| manbow::decode(&b));
    let works = manbow::works(
        base,
        venue,
        &page(manbow::url_list_url(base, venue))?,
        &page(manbow::work_list_url(base, venue))?,
    );
    let by_hand = match overrides {
        Some(p) => records(&read_text(&p)?, "num")?,
        None => Vec::new(),
    };
    let (mut review, mut disq) = (0, 0);
    for w in &works {
        let mut o = Map::new();
        o.insert("num".into(), json!(w.num));
        o.insert("title".into(), json!(w.title));
        o.insert("artist".into(), json!(w.artist));
        o.insert("team".into(), json!(w.team));
        o.insert("kind".into(), json!(w.kind));
        o.insert("size_kb".into(), json!(w.size_kb));
        o.insert("page".into(), json!(w.page));
        let links: Vec<Value> = w
            .links
            .iter()
            .map(|(l, u)| json!({"label": l, "url": u}))
            .collect();
        o.insert("links".into(), Value::Array(links));
        o.insert("main".into(), json!(w.pick.main));
        o.insert("extra".into(), json!(w.pick.extra));
        if !w.pick.review.is_empty() {
            o.insert("review".into(), json!(w.pick.review));
        }
        if let Some(h) = by_hand
            .iter()
            .find(|h| h.get("num").and_then(|n| n.as_u64()) == Some(w.num.into()))
        {
            for (k, v) in h {
                o.insert(k.clone(), v.clone());
            }
            o.shift_remove("review");
        }
        review += o.contains_key("review") as usize;
        disq += (w.kind == "Disq") as usize;
        r.out.json(&Value::Object(o));
    }
    r.out.note(format!(
        "{} works, {review} to review, {disq} disqualified",
        works.len()
    ));
    Ok(())
}

fn check(
    r: &Run,
    works: PathBuf,
    known: Option<PathBuf>,
    days: u64,
    minutes: Option<u64>,
) -> Result<(), String> {
    let works = records(&read_text(&works)?, "num")?;
    let known = match known {
        Some(p) if p.exists() => records(&read_text(&p)?, "url")?,
        _ => Vec::new(),
    };
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    let start = now();
    let tmp = std::env::temp_dir().join(format!("bmseeker-check-{}", std::process::id()));
    let (mut checked, mut kept) = (0, 0);
    let mut seen: Vec<String> = Vec::new();
    for w in &works {
        let want = Want {
            title: s(w, "title"),
            artist: s(w, "artist"),
        };
        let links = w
            .get("links")
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default();
        for l in links {
            let url = l
                .as_str()
                .or_else(|| l.get("url").and_then(|u| u.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            if !url.starts_with("http") || seen.contains(&url) {
                continue;
            }
            seen.push(url.clone());
            let before = known.iter().find(|k| s(k, "url") == url);
            let fresh = before.is_some_and(|k| {
                let at = k.get("at").and_then(|a| a.as_u64()).unwrap_or(0);
                let keep = if s(k, "state") == "error" { 1 } else { days };
                now().saturating_sub(at) < keep * 86_400
            });
            let time_up = minutes.is_some_and(|m| now() - start > m * 60);
            if fresh || (time_up && before.is_some()) {
                kept += 1;
                r.out
                    .json(&Value::Object(before.cloned().unwrap_or_default()));
                continue;
            }
            if time_up {
                continue;
            }
            let c = bmseeker::probe::probe(&url, Some(want.clone()), &tmp, &r.cx);
            r.out.note(format!(
                "{} | {:?} {:?} {} MB {} charts {} | {url}",
                want.title,
                c.state,
                c.kind,
                c.size >> 20,
                c.charts,
                c.error
            ));
            let mut v = serde_json::to_value(&c).map_err(|e| e.to_string())?;
            if let Value::Object(o) = &mut v {
                let mut rec = Map::new();
                rec.insert("url".into(), json!(url));
                rec.extend(std::mem::take(o));
                rec.insert("at".into(), json!(now()));
                v = Value::Object(rec);
            }
            r.out.json(&v);
            checked += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    r.out.note(format!(
        "{checked} checked, {kept} kept{}",
        if minutes.is_some_and(|m| now() - start > m * 60) {
            "; time is up, run again to go on"
        } else {
            ""
        }
    ));
    Ok(())
}
