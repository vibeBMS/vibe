//! The IR lamps of the players the accounts follow (the player's own,
//! rivals'): one lamp set per player and scope (a BMS-IR table, a Tachi
//! playtype), kept in `cache/ir/` so they show offline. A set is read
//! again when it is older than [`STALE_MS`], or when the player asks and
//! it is older than [`FLOOR_MS`] (the servers cache their answers that
//! long).

use crate::platform::{store, Job};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;
use vibebms_ir::lamps::{LampSet, RemoteLamp};
use vibebms_ir::{Outcome, ProviderId};

const MINUTE: i64 = 60_000;
pub const STALE_MS: i64 = 360 * MINUTE;
pub const FLOOR_MS: i64 = 5 * MINUTE;
/// The first wait after a failed read (no answer, a busy server); it
/// doubles with each failure.
const RETRY_MS: i64 = MINUTE;

/// A player's lamps in one scope on one service.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetKey {
    pub provider: ProviderId,
    pub player: String,
    pub scope: String,
}

/// A lamp set and how it came.
#[derive(Debug, Default)]
pub struct Cached {
    pub set: LampSet,
    /// The view it was read in, and the one wanted now (BMS-IR's
    /// `client_view`).
    pub view: String,
    want_view: String,
    /// When it was read (Unix ms; 0: never).
    pub fetched_ms: i64,
    /// Why the last read failed.
    pub error: Option<String>,
    state: Load,
    /// Not asked for before this (a failure's backoff).
    next_ms: i64,
    failures: u32,
    /// The player asked for it again.
    forced: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Load {
    /// Its file is being read.
    #[default]
    Reading,
    Ready,
    /// A request for it is out.
    Asking,
}

impl Cached {
    fn due(&self, now_ms: i64) -> bool {
        self.state == Load::Ready
            && now_ms >= self.next_ms
            && (self.forced || self.view != self.want_view || now_ms - self.fetched_ms >= STALE_MS)
    }

    /// The `If-None-Match` for the next read: the set's, when it is of the
    /// view wanted.
    pub fn etag(&self) -> Option<&str> {
        (self.view == self.want_view)
            .then_some(self.set.etag.as_deref())
            .flatten()
    }

    pub fn want_view(&self) -> &str {
        &self.want_view
    }

    /// Being read (its file, or from the server), or asked for again.
    fn busy(&self) -> bool {
        self.state != Load::Ready || self.forced
    }
}

/// A set's file.
#[derive(Serialize, Deserialize)]
struct File {
    view: String,
    fetched_ms: i64,
    set: LampSet,
}

pub struct LampCache {
    dir: PathBuf,
    sets: BTreeMap<SetKey, Cached>,
    reading: Vec<Job<(SetKey, io::Result<String>)>>,
}

impl LampCache {
    pub fn new(dir: PathBuf) -> LampCache {
        LampCache {
            dir,
            sets: BTreeMap::new(),
            reading: Vec::new(),
        }
    }

    /// `<dir>/<provider>/<player>/<scope>.json`, each part kept to letters,
    /// digits, `-` and `_`.
    fn file(&self, k: &SetKey) -> PathBuf {
        let part = |s: &str| -> String {
            let keep = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
            let s: String = s.chars().map(|c| if keep(c) { c } else { '_' }).collect();
            if s.is_empty() {
                "_".into()
            } else {
                s
            }
        };
        let (p, player) = (part(&k.provider.to_string()), part(&k.player));
        self.dir.join(p).join(player).join(part(&k.scope) + ".json")
    }

    /// Follow the sets `want` (each with the view wanted): a new one is
    /// read from its file, one no longer wanted is forgotten (unless a
    /// request for it is out).
    pub fn follow(&mut self, want: Vec<(SetKey, String)>) {
        let keys: BTreeSet<&SetKey> = want.iter().map(|(k, _)| k).collect();
        self.sets
            .retain(|k, c| c.state == Load::Asking || keys.contains(k));
        for (k, view) in want {
            if let Some(c) = self.sets.get_mut(&k) {
                c.want_view = view;
                continue;
            }
            let (path, key) = (self.file(&k), k.clone());
            self.reading.push(Job::start(|tx, _| {
                store::load(&path, move |r| {
                    let _ = tx.send((key, r));
                })
            }));
            let c = Cached {
                want_view: view,
                ..Default::default()
            };
            self.sets.insert(k, c);
        }
    }

    /// Take the files read since the last call.
    pub fn poll_reads(&mut self) {
        for (k, r) in Job::drain_all(&mut self.reading) {
            let Some(c) = self.sets.get_mut(&k) else {
                continue;
            };
            c.state = Load::Ready;
            let text = r.unwrap_or_else(|e| {
                log::warn!("IR lamps: {e}");
                String::new()
            });
            if text.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<File>(&text) {
                Ok(f) => {
                    (c.set, c.view, c.fetched_ms) = (f.set, f.view, f.fetched_ms);
                }
                // read again from the server
                Err(e) => log::warn!("IR lamps {k:?}: {e}"),
            }
        }
    }

    /// The sets to ask for now.
    pub fn due(&self, now_ms: i64) -> Vec<SetKey> {
        (self.sets.iter())
            .filter(|(_, c)| c.due(now_ms))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// A request for one of `p`'s sets is out.
    pub fn asking(&self, p: ProviderId) -> bool {
        (self.sets.iter()).any(|(k, c)| k.provider == p && c.state == Load::Asking)
    }

    pub fn get(&self, k: &SetKey) -> Option<&Cached> {
        self.sets.get(k)
    }

    /// The request for `k` went out.
    pub fn asked(&mut self, k: &SetKey) {
        if let Some(c) = self.sets.get_mut(k) {
            c.state = Load::Asking;
        }
    }

    /// What the server said about set `k` (`Err`: no answer).
    pub fn settle(&mut self, k: &SetKey, r: Result<Outcome, String>, now_ms: i64) {
        let Some(c) = self.sets.get_mut(k) else {
            return;
        };
        c.state = Load::Ready;
        c.forced = false;
        let backoff = RETRY_MS << c.failures.min(8);
        let (why, wait) = match r {
            Ok(Outcome::Lamps(set)) => {
                c.set = set;
                self.read(k, now_ms);
                return;
            }
            Ok(Outcome::NotModified) => {
                self.read(k, now_ms);
                return;
            }
            Ok(Outcome::Retry { after, why }) => {
                let asked = after.map_or(0, |a| a.delay_ms(now_ms) as i64);
                (why, asked.max(backoff))
            }
            Ok(Outcome::Unsupported) => ("this server has no player lamps".into(), STALE_MS),
            Ok(Outcome::Rejected(why) | Outcome::Auth(why)) => (why, STALE_MS),
            Ok(o) => (format!("unexpected answer: {o:?}"), STALE_MS),
            Err(why) => (format!("no answer ({why})"), backoff),
        };
        c.error = Some(why);
        c.failures += 1;
        c.next_ms = now_ms + wait.min(STALE_MS);
    }

    /// Set `k` was read now: kept in its file.
    fn read(&mut self, k: &SetKey, now_ms: i64) {
        let Some(c) = self.sets.get_mut(k) else {
            return;
        };
        c.fetched_ms = now_ms;
        c.view = c.want_view.clone();
        (c.error, c.failures, c.next_ms) = (None, 0, 0);
        let f = File {
            view: c.view.clone(),
            fetched_ms: now_ms,
            set: std::mem::take(&mut c.set),
        };
        let text = serde_json::to_string(&f).unwrap_or_default();
        c.set = f.set;
        if let Err(e) = store::rewrite(&self.file(k), &text) {
            log::warn!("IR lamps: {e}");
        }
    }

    /// The player asks for `p`'s sets again: those older than
    /// [`FLOOR_MS`] (all of them when `all`) are asked for as soon as the
    /// server's budget allows. Whether any is.
    pub fn refresh(&mut self, p: ProviderId, now_ms: i64, all: bool) -> bool {
        let mut any = false;
        for (_, c) in self.sets.iter_mut().filter(|(k, _)| k.provider == p) {
            if all || c.error.is_some() || now_ms - c.fetched_ms >= FLOOR_MS {
                (c.forced, c.next_ms) = (true, 0);
                any = true;
            }
        }
        any
    }

    /// The sets of `player` on `p`.
    fn of<'s: 'a, 'a>(
        &'s self,
        p: ProviderId,
        player: &'a str,
    ) -> impl Iterator<Item = &'s Cached> + 'a {
        (self.sets.iter())
            .filter(move |(k, _)| k.provider == p && k.player == player)
            .map(|(_, c)| c)
    }

    /// `player`'s lamp on the chart with these hashes: the best in its sets
    /// (a chart can be in several tables).
    pub fn lamp(
        &self,
        p: ProviderId,
        player: &str,
        md5: &str,
        sha256: &str,
    ) -> Option<&RemoteLamp> {
        (self.of(p, player))
            .filter_map(|c| c.set.get(md5, sha256))
            .max_by_key(|l| l.clear)
    }

    /// `player`'s sets on `p` with their keys, once none is being read
    /// (`None` while one is).
    pub fn settled_sets(&self, p: ProviderId, player: &str) -> Option<Vec<(&SetKey, &Cached)>> {
        let sets: Vec<(&SetKey, &Cached)> = (self.sets.iter())
            .filter(|(k, _)| k.provider == p && k.player == player)
            .collect();
        (!sets.iter().any(|(_, c)| c.busy())).then_some(sets)
    }

    /// Whether any of `player`'s sets on `p` has been read.
    pub fn has(&self, p: ProviderId, player: &str) -> bool {
        self.of(p, player).any(|c| c.fetched_ms > 0)
    }

    /// The name the server gives `player`.
    pub fn name(&self, p: ProviderId, player: &str) -> Option<&str> {
        (self.of(p, player))
            .map(|c| c.set.player_name.as_str())
            .find(|n| !n.is_empty())
    }

    /// `p`'s sets for people: being read, why one failed, or when the
    /// oldest was read.
    pub fn state(&self, p: ProviderId, now_ms: i64) -> String {
        let sets: Vec<&Cached> = (self.sets.iter())
            .filter(|(k, _)| k.provider == p)
            .map(|(_, c)| c)
            .collect();
        if sets.is_empty() {
            return "nothing to read".into();
        }
        if sets.iter().any(|c| c.state != Load::Ready || c.forced) {
            return "reading…".into();
        }
        if let Some(e) = sets.iter().find_map(|c| c.error.as_deref()) {
            return e.to_string();
        }
        match sets.iter().map(|c| c.fetched_ms).min() {
            Some(t) if t > 0 => format!("read {}", ago(now_ms - t)),
            _ => "not read yet".into(),
        }
    }
}

/// `ms` ago, for people.
fn ago(ms: i64) -> String {
    match ms / MINUTE {
        0 => "just now".into(),
        m @ 1..=119 => format!("{m} min ago"),
        m => format!("{} h ago", m / 60),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use vibebms_core::score::ClearType;
    use vibebms_ir::http::RetryAfter;

    fn key(scope: &str) -> SetKey {
        SetKey {
            provider: ProviderId::BmsIr,
            player: "100002".into(),
            scope: scope.into(),
        }
    }

    fn set(md5: &str, clear: ClearType) -> LampSet {
        let mut by_chart = std::collections::HashMap::new();
        let lamp = RemoteLamp {
            clear,
            ..Default::default()
        };
        by_chart.insert(md5.to_string(), lamp);
        LampSet {
            player_name: "P".into(),
            by_chart,
            etag: Some("\"e\"".into()),
        }
    }

    /// Read once, kept, not asked again within six hours unless the
    /// player asks (and not within five minutes even then); a busy server
    /// is asked again after its wait; the file outlasts a restart.
    #[test]
    fn sets_age_and_refresh() {
        let dir = std::env::temp_dir().join(format!("vibebms-lamps-{}", std::process::id()));
        let mut c = LampCache::new(dir.clone());
        let want = || {
            vec![
                (key("official"), "all_clients".to_string()),
                (key("satellite"), "all_clients".to_string()),
            ]
        };
        c.follow(want());
        assert!(c.due(0).is_empty(), "files first");
        c.poll_reads();
        let t0 = 1_800_000_000_000;
        assert_eq!(c.due(t0), [key("official"), key("satellite")]);
        let md5 = "0123456789abcdef0123456789abcdef";
        c.asked(&key("official"));
        assert!(c.asking(ProviderId::BmsIr));
        c.settle(
            &key("official"),
            Ok(Outcome::Lamps(set(md5, ClearType::Hard))),
            t0,
        );
        let busy = Outcome::Retry {
            after: Some(RetryAfter::Secs(10)),
            why: "server busy".into(),
        };
        c.asked(&key("satellite"));
        c.settle(&key("satellite"), Ok(busy), t0);
        assert_eq!(c.state(ProviderId::BmsIr, t0), "server busy");
        // the backoff's minute is longer than the server's 10 s
        assert!(c.due(t0 + 30_000).is_empty());
        assert_eq!(c.due(t0 + RETRY_MS), [key("satellite")]);
        c.settle(&key("satellite"), Ok(Outcome::NotModified), t0 + RETRY_MS);
        assert_eq!(
            c.lamp(ProviderId::BmsIr, "100002", md5, "").unwrap().clear,
            ClearType::Hard
        );
        assert_eq!(c.name(ProviderId::BmsIr, "100002"), Some("P"));
        assert_eq!(c.get(&key("official")).unwrap().etag(), Some("\"e\""));
        // fresh: not due; the player's refresh waits five minutes
        let t1 = t0 + 2 * MINUTE;
        assert!(c.due(t1).is_empty());
        assert!(!c.refresh(ProviderId::BmsIr, t1, false));
        let t2 = t0 + FLOOR_MS + RETRY_MS;
        assert!(c.refresh(ProviderId::BmsIr, t2, false));
        assert_eq!(c.due(t2).len(), 2);
        // a refusal waits for the next six hours
        c.settle(
            &key("official"),
            Ok(Outcome::Rejected("player not public".into())),
            t2,
        );
        assert!(!c.due(t2 + STALE_MS - 1).contains(&key("official")));
        assert!(c.due(t2 + STALE_MS).contains(&key("official")));
        // another view is read again at once
        let mut again = LampCache::new(dir.clone());
        again.follow(want());
        again.poll_reads();
        assert_eq!(again.get(&key("official")).unwrap().fetched_ms, t0);
        assert!(again.due(t0 + MINUTE).is_empty());
        again.follow(vec![(key("official"), "lr2".into())]);
        assert_eq!(again.due(t0 + MINUTE), [key("official")]);
        assert_eq!(again.get(&key("official")).unwrap().etag(), None);
        assert!(again.get(&key("satellite")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
