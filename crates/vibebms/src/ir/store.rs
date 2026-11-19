//! The history of plays: every play of the player's own (not autoplay, not
//! a replay watched) as its `PlayRecord`, one JSON line each, in a file per
//! month (`scores/history/plays-2026-11.jsonl`). Lines are only added, never
//! changed, except that gauge logs older than [`GAUGE_LOG_DAYS`] are dropped
//! from closed months ([`History::compact`]). `scores.json` stays the
//! records of bests; this is what is submitted and what later statistics
//! read.
//!
//! `meta.json` beside them holds the install's random id, which makes play
//! ids unique across installs. It is small and read at start-up, so it is a
//! vfs file like the configuration (read at once on the web too).

use crate::platform::{store, vfs};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::path::{Path, PathBuf};
use vibebms_core::hex;
use vibebms_core::record::PlayRecord;

/// How long a play keeps its gauge log in the history (the rest of the
/// record stays).
pub const GAUGE_LOG_DAYS: i64 = 90;

const DAY_MS: i64 = 24 * 3600 * 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Meta {
    v: u32,
    install_id: String,
    /// When the history began (Unix ms).
    started_ms: i64,
    /// The start of the first month whose gauge logs are still kept
    /// (Unix ms; 0: the month the history began).
    #[serde(default)]
    compact_from_ms: i64,
}

/// A calendar month (UTC): what a play's file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    pub year: i32,
    pub month: u32,
}

impl Month {
    /// The month of `ms` (Unix ms).
    pub fn of(ms: i64) -> Month {
        let d = chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default();
        Month {
            year: d.year(),
            month: d.month(),
        }
    }

    pub fn next(self) -> Month {
        if self.month == 12 {
            Month {
                year: self.year + 1,
                month: 1,
            }
        } else {
            Month {
                month: self.month + 1,
                ..self
            }
        }
    }

    /// Its first moment (Unix ms).
    pub fn start_ms(self) -> i64 {
        NaiveDate::from_ymd_opt(self.year, self.month, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .map_or(0, |t| t.and_utc().timestamp_millis())
    }

    fn file_name(self) -> String {
        format!("plays-{:04}-{:02}.jsonl", self.year, self.month)
    }
}

pub struct History {
    dir: PathBuf,
    meta: Meta,
}

impl History {
    /// The history in `dir`; the first time, its install id is made and
    /// kept (`now_ms`: the time).
    pub fn open(dir: PathBuf, now_ms: i64) -> History {
        let path = dir.join("meta.json");
        let read = vfs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Meta>(&b).ok())
            .filter(|m| !m.install_id.is_empty());
        let meta = read.unwrap_or_else(|| {
            let m = Meta {
                v: 1,
                install_id: random_id(now_ms),
                started_ms: now_ms,
                compact_from_ms: 0,
            };
            save_meta(&path, &m);
            m
        });
        History { dir, meta }
    }

    pub fn install_id(&self) -> &str {
        &self.meta.install_id
    }

    pub fn file(&self, m: Month) -> PathBuf {
        self.dir.join(m.file_name())
    }

    /// Give `rec` its play id and add it to its month's file.
    pub fn add(&self, rec: &mut PlayRecord) -> io::Result<()> {
        rec.play_id = play_id(self.install_id(), rec);
        let line = serde_json::to_string(rec).map_err(io::Error::other)?;
        store::append(&self.file(Month::of(rec.ended_ms)), &line)
    }

    /// The plays of month `m`, and how many lines could not be read.
    pub fn load(
        &self,
        m: Month,
        on_done: impl FnOnce(io::Result<(Vec<PlayRecord>, usize)>) + 'static,
    ) {
        store::load(&self.file(m), move |r| on_done(r.map(|t| parse(&t))))
    }

    /// Drop the gauge logs of plays more than [`GAUGE_LOG_DAYS`] old, a
    /// whole month at a time, once its last day is that old. (On the web a
    /// month is read and written by the storage worker after this returns:
    /// a page closed meanwhile keeps that month's logs.)
    pub fn compact(&mut self, now_ms: i64) {
        let from = self.meta.compact_from_ms.max(self.meta.started_ms);
        let mut m = Month::of(from);
        let mut done = false;
        while m.next().start_ms() + GAUGE_LOG_DAYS * DAY_MS <= now_ms {
            let p = self.file(m);
            let at = p.clone();
            store::load(&at, move |r| match r {
                Ok(text) => {
                    if let Some(t) = without_gauge_logs(&text) {
                        if let Err(e) = store::rewrite(&p, &t) {
                            log::warn!("history {}: {e}", p.display());
                        }
                    }
                }
                Err(e) => log::warn!("history {}: {e}", p.display()),
            });
            m = m.next();
            done = true;
        }
        if done {
            self.meta.compact_from_ms = m.start_ms();
            save_meta(&self.dir.join("meta.json"), &self.meta);
        }
    }
}

fn save_meta(path: &Path, m: &Meta) {
    let r = serde_json::to_vec(m)
        .map_err(io::Error::other)
        .and_then(|b| vfs::write(path, &b));
    if let Err(e) = r {
        log::warn!("{}: {e}", path.display());
    }
}

/// The plays in a history file's `text`, and how many lines are not one
/// (a line cut short as the program ended, a newer format).
pub fn parse(text: &str) -> (Vec<PlayRecord>, usize) {
    let mut bad = 0;
    let plays = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            let r = serde_json::from_str(l).ok();
            bad += r.is_none() as usize;
            r
        })
        .collect();
    (plays, bad)
}

/// `text` without its plays' gauge logs; `None` when it has none. Lines
/// that are not plays stay as they are.
fn without_gauge_logs(text: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for l in text.lines() {
        match serde_json::from_str::<PlayRecord>(l) {
            Ok(mut r) if !r.result.gauge_log.is_empty() => {
                r.result.gauge_log = Vec::new();
                out.push_str(&serde_json::to_string(&r).ok()?);
                changed = true;
            }
            _ => out.push_str(l),
        }
        out.push('\n');
    }
    changed.then_some(out)
}

/// The id of play `rec` on the install `install_id`: 32 hex digits of
/// the SHA-256 of the install, the chart, the end time, the seed and the
/// EX score.
pub fn play_id(install_id: &str, rec: &PlayRecord) -> String {
    let key = format!(
        "{install_id}|{}|{}|{}|{}",
        rec.chart.sha256,
        rec.ended_ms,
        rec.rules.seed,
        rec.result.score.ex_score()
    );
    hex(&Sha256::digest(key.as_bytes())[..16])
}

/// 32 random hex digits (the OS's randomness natively, the browser's on
/// the web, mixed with the time).
fn random_id(now_ms: i64) -> String {
    use std::hash::BuildHasher;
    let mut h = Sha256::new();
    h.update(now_ms.to_le_bytes());
    // (the web has no process id: `process::id` panics there)
    #[cfg(not(target_arch = "wasm32"))]
    h.update(std::process::id().to_le_bytes());
    // seeded from the OS natively
    let r = std::collections::hash_map::RandomState::new().hash_one(now_ms);
    h.update(r.to_le_bytes());
    #[cfg(target_arch = "wasm32")]
    for _ in 0..4 {
        h.update(js_sys::Math::random().to_le_bytes());
    }
    hex(&h.finalize()[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 2026-11-17 12:00 UTC.
    const NOV: i64 = 1_794_916_800_000;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vibebms-history-{name}-{}", std::process::id()))
    }

    fn play(ended_ms: i64, ex: u32) -> PlayRecord {
        let mut r = PlayRecord {
            v: vibebms_core::record::VERSION,
            ended_ms,
            ..Default::default()
        };
        r.chart.sha256 = "ab".repeat(32);
        r.result.score.max_combo = ex;
        r.result.score.counts[0][0] = ex / 2;
        r.result.gauge_log = vec![0.0, 50.5, 100.0];
        r
    }

    fn loaded(h: &History, m: Month) -> (Vec<PlayRecord>, usize) {
        let got = Rc::new(RefCell::new(None));
        let g = got.clone();
        h.load(m, move |r| *g.borrow_mut() = Some(r.unwrap()));
        got.take().unwrap()
    }

    #[test]
    fn months() {
        let m = Month::of(NOV);
        assert_eq!((m.year, m.month), (2026, 11));
        assert_eq!(
            m.next().next(),
            Month {
                year: 2027,
                month: 1
            }
        );
        assert_eq!(Month::of(m.start_ms()), m);
        assert_eq!(
            Month::of(m.start_ms() - 1),
            Month {
                year: 2026,
                month: 10
            }
        );
        assert_eq!(m.file_name(), "plays-2026-11.jsonl");
    }

    #[test]
    fn plays_are_added_and_read_back() {
        let dir = temp("add");
        let h = History::open(dir.clone(), NOV);
        let id = h.install_id().to_string();
        assert_eq!(id.len(), 32);
        // the id is kept
        assert_eq!(History::open(dir.clone(), NOV + 5).install_id(), id);
        let (mut a, mut b) = (play(NOV, 10), play(NOV + 1000, 12));
        h.add(&mut a).unwrap();
        h.add(&mut b).unwrap();
        assert_eq!(a.play_id, play_id(&id, &a));
        assert_eq!(a.play_id.len(), 32);
        assert_ne!(a.play_id, b.play_id);
        let (plays, bad) = loaded(&h, Month::of(NOV));
        assert_eq!((plays, bad), (vec![a.clone(), b.clone()], 0));
        // another install's play of the same chart at the same moment
        assert_ne!(play_id("0".repeat(32).as_str(), &a), a.play_id);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bad_line_is_skipped_and_counted() {
        let dir = temp("bad");
        let h = History::open(dir.clone(), NOV);
        let mut a = play(NOV, 10);
        h.add(&mut a).unwrap();
        // the program ended while writing a line; the next play is whole
        let p = h.file(Month::of(NOV));
        let mut text = std::fs::read_to_string(&p).unwrap();
        text.push_str("{\"v\":1,\"play_id\":\"cut");
        std::fs::write(&p, text).unwrap();
        let mut b = play(NOV + 1, 11);
        h.add(&mut b).unwrap();
        assert_eq!(loaded(&h, Month::of(NOV)), (vec![a, b], 1));
        assert_eq!(parse("\n\nnot json\n"), (vec![], 1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_month_is_a_new_file() {
        let dir = temp("month");
        let h = History::open(dir.clone(), NOV);
        let dec = Month::of(NOV).next();
        let (mut a, mut b) = (play(dec.start_ms() - 1, 1), play(dec.start_ms(), 2));
        h.add(&mut a).unwrap();
        h.add(&mut b).unwrap();
        assert_eq!(loaded(&h, Month::of(NOV)).0, [a]);
        assert_eq!(loaded(&h, dec).0, [b]);
        assert!(h.file(dec).ends_with("plays-2026-12.jsonl"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn old_months_lose_their_gauge_logs() {
        let dir = temp("compact");
        let nov = Month::of(NOV);
        let mut h = History::open(dir.clone(), NOV);
        let (mut a, mut b) = (play(NOV, 1), play(nov.next().start_ms() + 1, 2));
        h.add(&mut a).unwrap();
        h.add(&mut b).unwrap();
        std::fs::write(
            h.file(nov),
            std::fs::read_to_string(h.file(nov)).unwrap() + "cut\n",
        )
        .unwrap();
        // November ends on Dec 1st: 90 days later it goes, not before
        let end = nov.next().start_ms();
        h.compact(end + GAUGE_LOG_DAYS * DAY_MS - 1);
        assert_eq!(loaded(&h, nov).0[0].result.gauge_log.len(), 3);
        h.compact(end + GAUGE_LOG_DAYS * DAY_MS);
        let (plays, bad) = loaded(&h, nov);
        a.result.gauge_log.clear();
        assert_eq!((plays, bad), (vec![a], 1));
        assert_eq!(loaded(&h, nov.next()).0, [b]);
        // remembered: November is not read again
        let again = History::open(dir.clone(), NOV);
        assert_eq!(again.meta.compact_from_ms, end);
        std::fs::remove_dir_all(&dir).ok();
    }
}
