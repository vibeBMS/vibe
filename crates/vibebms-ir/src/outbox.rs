//! The outbox: every score on its way to an IR, as data the application
//! stores (`platform::store`, rewritten whole) and drives. Nothing here
//! sends or reads a clock: [`Outbox::next`] says what to send now,
//! [`Outbox::settle`] takes what came back.
//!
//! ```text
//! queued ──send──▶ sending ──2xx──────────────────▶ done
//!                     │──202 + import id─▶ polling ⇄ checking ──completed──▶ done
//!                     │                                 └──import error──▶ rejected
//!                     │──408 / 429 / 5xx──▶ retry (next_ms) ──▶ sending
//!                     │──no answer────────▶ retry (attempt not counted; provider offline 1 min)
//!                     │──401 / 403────────▶ auth, and the provider is paused
//!                     └──other 4xx────────▶ rejected
//! retry: the 12th attempt failed ──▶ gave_up
//! ```
//!
//! `auth`, `rejected` and `gave_up` are final until the user retries; the
//! item then goes back with the same id and body. A body is frozen once,
//! with the play's own date in it, and sent byte for byte every time; the
//! id is the idempotency key. An item with an import id is polled, never
//! posted again.

use crate::http::brief;
use crate::provider::{Frozen, IrError, Op, Outcome, Provider, ProviderId};
use crate::rule::Profile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use vibebms_core::record::PlayRecord;
use vibebms_core::score::ClearType;

/// The file format.
pub const VERSION: u32 = 1;
/// Failed attempts before an item gives up (answers without one don't
/// count).
pub const MAX_ATTEMPTS: u32 = 12;
/// Unfinished items kept at most: beyond, the oldest failed ones go first.
pub const MAX_UNFINISHED: usize = 500;

const SEC: i64 = 1000;
const MIN: i64 = 60 * SEC;
const HOUR: i64 = 60 * MIN;
/// Done items stay this long for the UI.
const KEEP_DONE: i64 = 7 * 24 * HOUR;
const BACKOFF_FIRST: i64 = 30 * SEC;
const BACKOFF_MAX: i64 = HOUR;
/// A 429 whose `Retry-After` is missing or unreadable (the web without
/// `Access-Control-Expose-Headers`).
const RETRY_429: i64 = MIN;
/// After a request got no answer, the next try of that provider.
const OFFLINE_PROBE: i64 = MIN;
/// Import polls: these first, then every `POLL_EVERY` for `POLL_FOR`,
/// then every `POLL_LATER`.
const POLL_FIRST: [i64; 4] = [SEC, 2 * SEC, 4 * SEC, 8 * SEC];
const POLL_EVERY: i64 = 15 * SEC;
const POLL_FOR: i64 = 10 * MIN;
const POLL_LATER: i64 = 10 * MIN;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    /// Posted; no answer yet.
    Sending,
    /// Accepted for later (a Tachi import): waiting to poll.
    Polling,
    /// A poll is out.
    Checking,
    /// Waiting to post again.
    Retry,
    Done,
    /// The credentials were refused.
    Auth,
    Rejected,
    GaveUp,
}

impl State {
    /// Nothing more happens to it unless the user retries.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            State::Done | State::Auth | State::Rejected | State::GaveUp
        )
    }

    pub fn failed(self) -> bool {
        self.is_final() && self != State::Done
    }

    fn in_flight(self) -> bool {
        matches!(self, State::Sending | State::Checking)
    }
}

/// The last answer (or its absence).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Last {
    pub at_ms: i64,
    /// `None`: no answer came.
    pub status: Option<u16>,
    pub text: String,
}

/// What the UI shows of the play.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub title: String,
    pub ex: u32,
    pub clear: ClearType,
    pub date_ms: i64,
}

impl Summary {
    pub fn of(rec: &PlayRecord) -> Summary {
        Summary {
            title: rec.chart.title.clone(),
            ex: rec.result.score.ex_score(),
            clear: rec.result.clear,
            date_ms: rec.ended_ms,
        }
    }
}

/// One play on its way to one account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// The idempotency key: [`item_id`].
    pub id: String,
    pub play_id: String,
    pub provider: ProviderId,
    pub account: String,
    pub created_ms: i64,
    pub state: State,
    /// Failed attempts so far.
    pub attempts: u32,
    /// When it is next due (`Queued`, `Retry`, `Polling`).
    pub next_ms: i64,
    /// The service's id of the deferred submission (Tachi's import id).
    pub remote: Option<String>,
    /// Polls so far, and since when.
    #[serde(default)]
    pub polls: u32,
    #[serde(default)]
    pub poll_from_ms: i64,
    pub last: Option<Last>,
    /// A remark of the service's on a done item.
    #[serde(default)]
    pub note: Option<String>,
    /// The user pressed a button to send it.
    #[serde(default)]
    pub user_intent: bool,
    #[serde(flatten)]
    pub frozen: Frozen,
    pub summary: Summary,
}

/// The id of play `play_id` sent to `account` on `provider`: the same
/// every time, so the same play is never two items, and a server that
/// sees the key twice knows it is one play.
pub fn item_id(provider: ProviderId, account: &str, play_id: &str) -> String {
    let h = Sha256::digest(format!("{provider}\n{account}\n{play_id}"));
    vibebms_core::hex(&h[..16])
}

impl Item {
    /// The item of play `rec` (id `play_id`) that `eligibility` let go to
    /// `provider`'s account `account` as `profile`: its body frozen now.
    #[allow(clippy::too_many_arguments)]
    pub fn freeze(
        provider: &dyn Provider,
        account: &str,
        rec: &PlayRecord,
        play_id: &str,
        profile: Profile,
        user_intent: bool,
        now_ms: i64,
    ) -> Result<Item, IrError> {
        let id = provider.id();
        Ok(Item {
            id: item_id(id, account, play_id),
            play_id: play_id.into(),
            provider: id,
            account: account.into(),
            created_ms: now_ms,
            state: State::Queued,
            attempts: 0,
            next_ms: now_ms,
            remote: None,
            polls: 0,
            poll_from_ms: 0,
            last: None,
            note: None,
            user_intent,
            frozen: provider.freeze(rec, play_id, profile)?,
            summary: Summary::of(rec),
        })
    }

    /// The request to make for it: a poll once the service deferred it,
    /// else the frozen body.
    pub fn op(&self) -> Op<'_> {
        match &self.remote {
            Some(remote) => Op::Poll { remote },
            None => Op::Submit {
                body: &self.frozen,
                key: &self.id,
                user_intent: self.user_intent,
            },
        }
    }

    fn waiting(&self) -> State {
        if self.remote.is_some() {
            State::Polling
        } else {
            State::Retry
        }
    }

    /// The delay before the next poll.
    fn poll_delay(&self, now_ms: i64) -> i64 {
        if now_ms - self.poll_from_ms >= POLL_FOR {
            POLL_LATER
        } else {
            POLL_FIRST
                .get(self.polls as usize)
                .copied()
                .unwrap_or(POLL_EVERY)
        }
    }

    /// The delay after failed attempt `attempts`: 30 s doubling up to an
    /// hour, plus up to 25 % taken from the id (the same every time, and
    /// different between items).
    fn backoff(&self) -> i64 {
        let n = self.attempts.saturating_sub(1).min(16);
        let base = (BACKOFF_FIRST << n).min(BACKOFF_MAX);
        let h = Sha256::digest(format!("{}:{}", self.id, self.attempts));
        base + base * (h[0] % 26) as i64 / 100
    }
}

/// What came back for a request [`Outbox::next`] gave out.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// An answer: its status and what the provider read in it.
    Answer { status: u16, outcome: Outcome },
    /// None came (no connection, a timeout); why.
    NoAnswer(String),
}

/// Every item, and the providers held back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outbox {
    pub v: u32,
    /// Providers whose credentials were refused: nothing is sent to them
    /// until the account is checked again ([`Outbox::resume`]).
    #[serde(default)]
    pub paused: BTreeSet<ProviderId>,
    pub items: Vec<Item>,
    /// Providers that gave no answer: next tried then (Unix ms). Not kept:
    /// a start-up tries them at once.
    #[serde(skip)]
    offline: Vec<(ProviderId, i64)>,
}

impl Default for Outbox {
    fn default() -> Outbox {
        Outbox {
            v: VERSION,
            paused: BTreeSet::new(),
            items: Vec::new(),
            offline: Vec::new(),
        }
    }
}

impl Outbox {
    /// The outbox stored as `bytes`, at start-up: what was out when the
    /// program stopped is due again now (safe: the same key and body, and
    /// a deferred submission is only polled).
    pub fn load(bytes: &[u8], now_ms: i64) -> Result<Outbox, String> {
        let mut o: Outbox = serde_json::from_slice(bytes).map_err(|e| format!("outbox: {e}"))?;
        for i in o.items.iter_mut().filter(|i| i.state.in_flight()) {
            i.state = i.waiting();
            i.next_ms = now_ms;
        }
        Ok(o)
    }

    /// The bytes to store.
    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an outbox is JSON")
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    fn item_mut(&mut self, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    /// Add `item` unless it is there already; the items dropped to stay
    /// within `MAX_UNFINISHED` (to tell the user).
    pub fn push(&mut self, item: Item) -> Vec<Item> {
        if self.item(&item.id).is_some() {
            return Vec::new();
        }
        self.items.push(item);
        let mut dropped = Vec::new();
        while self.items.iter().filter(|i| i.state != State::Done).count() > MAX_UNFINISHED {
            // the oldest failed one, else the oldest waiting one
            let oldest = |pick: fn(State) -> bool| {
                (self.items.iter().enumerate())
                    .filter(|(_, i)| pick(i.state))
                    .min_by_key(|&(n, i)| (i.created_ms, n))
                    .map(|(n, _)| n)
            };
            let Some(n) =
                oldest(State::failed).or_else(|| oldest(|s| !s.is_final() && !s.in_flight()))
            else {
                break;
            };
            dropped.push(self.items.remove(n));
        }
        dropped
    }

    /// What to send now: at most one request per provider in flight, its
    /// oldest unsent play first (a server sees plays in the order they
    /// were played), polls when due. `allow` is asked before each (the
    /// provider's `Limiter`); the ids given are out until settled.
    pub fn next(&mut self, now_ms: i64, mut allow: impl FnMut(ProviderId) -> bool) -> Vec<String> {
        let providers: BTreeSet<ProviderId> = self.items.iter().map(|i| i.provider).collect();
        let mut out = Vec::new();
        for p in providers {
            let held = self
                .offline
                .iter()
                .any(|&(q, until)| q == p && until > now_ms);
            let mine = || {
                self.items
                    .iter()
                    .enumerate()
                    .filter(move |(_, i)| i.provider == p)
            };
            if held || self.paused.contains(&p) || mine().any(|(_, i)| i.state.in_flight()) {
                continue;
            }
            let unsent = mine()
                .filter(|(_, i)| matches!(i.state, State::Queued | State::Retry))
                .min_by_key(|&(n, i)| (i.created_ms, n));
            let due = unsent
                .into_iter()
                .chain(mine().filter(|(_, i)| i.state == State::Polling))
                .filter(|(_, i)| i.next_ms <= now_ms)
                .min_by_key(|&(n, i)| (i.next_ms, n))
                .map(|(n, _)| n);
            if let Some(n) = due.filter(|_| allow(p)) {
                let i = &mut self.items[n];
                i.state = if i.remote.is_some() {
                    State::Checking
                } else {
                    State::Sending
                };
                out.push(i.id.clone());
            }
        }
        out
    }

    /// What came back for item `id` at `now_ms`. An item not out (removed
    /// or retried meanwhile) ignores it.
    pub fn settle(&mut self, id: &str, reply: Reply, now_ms: i64) {
        let Some(i) = self.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        if !i.state.in_flight() {
            return;
        }
        let p = i.provider;
        self.offline.retain(|&(q, _)| q != p);
        let (status, outcome) = match reply {
            Reply::NoAnswer(why) => {
                i.last = Some(Last {
                    at_ms: now_ms,
                    status: None,
                    text: brief(&why),
                });
                i.state = i.waiting();
                i.next_ms = now_ms + OFFLINE_PROBE;
                self.offline.push((p, now_ms + OFFLINE_PROBE));
                return;
            }
            Reply::Answer { status, outcome } => (status, outcome),
        };
        // (a server's words, kept in the stored file: a line of them)
        let last = |text: String| {
            Some(Last {
                at_ms: now_ms,
                status: Some(status),
                text: brief(&text),
            })
        };
        match outcome {
            Outcome::Sent { note, .. } => {
                let note = note.map(|n| brief(&n));
                i.last = last(note.clone().unwrap_or_default());
                i.note = note;
                i.state = State::Done;
            }
            Outcome::Deferred { remote } => {
                i.last = last(String::new());
                i.remote = Some(remote);
                i.polls = 0;
                i.poll_from_ms = now_ms;
                i.state = State::Polling;
                i.next_ms = now_ms + i.poll_delay(now_ms);
            }
            Outcome::Pending if i.remote.is_some() => {
                i.last = last(String::new());
                i.polls += 1;
                i.state = State::Polling;
                i.next_ms = now_ms + i.poll_delay(now_ms);
            }
            Outcome::Retry { after, why } => {
                i.last = last(why);
                i.attempts += 1;
                if i.attempts >= MAX_ATTEMPTS {
                    i.state = State::GaveUp;
                } else {
                    let asked = match after {
                        Some(a) => a.delay_ms(now_ms) as i64,
                        None if status == 429 => RETRY_429,
                        None => 0,
                    };
                    i.state = i.waiting();
                    i.next_ms = now_ms + i.backoff().max(asked);
                }
            }
            Outcome::Auth(why) => {
                i.last = last(why);
                i.state = State::Auth;
                self.paused.insert(p);
            }
            Outcome::Rejected(why) => {
                i.last = last(why);
                i.state = State::Rejected;
            }
            o => {
                i.last = last(format!("unexpected answer: {o:?}"));
                i.state = State::Rejected;
            }
        }
    }

    /// The user's Retry on a final item: back in line, same id and body.
    pub fn retry(&mut self, id: &str, now_ms: i64) {
        if let Some(i) = self.item_mut(id).filter(|i| i.state.failed()) {
            i.state = if i.remote.is_some() {
                State::Polling
            } else {
                State::Queued
            };
            i.attempts = 0;
            i.next_ms = now_ms;
        }
    }

    /// The user's Remove (an item out is removed when it comes back).
    pub fn remove(&mut self, id: &str) {
        self.items.retain(|i| i.id != id || i.state.in_flight());
    }

    /// The user's Send now: everything waiting is due, offline providers
    /// are tried again.
    pub fn send_now(&mut self, now_ms: i64) {
        self.offline.clear();
        for i in &mut self.items {
            if matches!(i.state, State::Retry | State::Polling) {
                i.next_ms = i.next_ms.min(now_ms);
            }
        }
    }

    /// The account of `p` was checked again: send to it again.
    pub fn resume(&mut self, p: ProviderId) {
        self.paused.remove(&p);
    }

    /// Drop done items older than a week.
    pub fn prune(&mut self, now_ms: i64) {
        self.items.retain(|i| {
            let at = i.last.as_ref().map_or(i.created_ms, |l| l.at_ms);
            i.state != State::Done || now_ms - at < KEEP_DONE
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpResponse;
    use crate::providers::vibe_json::VibeJson;
    use vibebms_core::PlayMode;

    const T0: i64 = 1_767_225_600_000;

    fn ir() -> VibeJson {
        VibeJson::new(0, "https://ir.example").unwrap()
    }

    fn play(n: i64) -> (PlayRecord, String) {
        let mut r = PlayRecord {
            ended_ms: T0 + n,
            ..Default::default()
        };
        r.chart.title = format!("chart {n}");
        r.chart.sha256 = format!("{n:064x}");
        r.chart.mode = PlayMode::Beat7K;
        r.result.clear = ClearType::Hard;
        (r, format!("play{n}"))
    }

    fn item(p: &dyn Provider, n: i64, now: i64) -> Item {
        let (rec, id) = play(n);
        Item::freeze(p, "acct", &rec, &id, Profile::VibeBeatoraja, false, now).unwrap()
    }

    /// A canned answer, read by the vibe-json adapter.
    fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Reply {
        let resp = HttpResponse {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.as_bytes().to_vec(),
        };
        let b = Frozen {
            content_type: String::new(),
            body: String::new(),
        };
        let op = Op::Submit {
            body: &b,
            key: "",
            user_intent: false,
        };
        Reply::Answer {
            status,
            outcome: ir().parse(&op, &resp),
        }
    }

    fn outcome(status: u16, outcome: Outcome) -> Reply {
        Reply::Answer { status, outcome }
    }

    /// What `next` gives out at `now`, everything allowed.
    fn due(o: &mut Outbox, now: i64) -> Vec<String> {
        o.next(now, |_| true)
    }

    fn one(o: &mut Outbox, n: i64) -> String {
        let p = ir();
        o.push(item(&p, n, T0));
        o.items.last().unwrap().id.clone()
    }

    #[test]
    fn ids_are_stable_and_items_unique() {
        let a = item_id(ProviderId::Bokutachi, "1", "p");
        assert_eq!(a.len(), 32);
        assert_eq!(a, item_id(ProviderId::Bokutachi, "1", "p"));
        assert_ne!(a, item_id(ProviderId::Bokutachi, "2", "p"));
        assert_ne!(a, item_id(ProviderId::BmsIr, "1", "p"));
        let mut o = Outbox::default();
        let p = ir();
        assert!(o.push(item(&p, 1, T0)).is_empty());
        o.push(item(&p, 1, T0 + 5));
        assert_eq!(o.items.len(), 1);
    }

    #[test]
    fn sent_and_done_with_the_body_frozen() {
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        let body = o.items[0].frozen.clone();
        assert!(body.body.contains(&format!("\"date_ms\":{}", T0 + 1)));
        assert!(matches!(o.items[0].op(), Op::Submit { key, .. } if key == id));
        assert_eq!(due(&mut o, T0), [id.as_str()]);
        assert_eq!(o.items[0].state, State::Sending);
        // out: not given again
        assert!(due(&mut o, T0 + 1).is_empty());
        o.settle(&id, answer(503, &[], "busy"), T0 + 10);
        assert_eq!(o.items[0].state, State::Retry);
        let at = o.items[0].next_ms;
        assert!(due(&mut o, at - 1).is_empty());
        assert_eq!(due(&mut o, at), [id.as_str()]);
        // the same bytes on every attempt
        assert_eq!(o.items[0].frozen, body);
        o.settle(&id, answer(200, &[], ""), at + 5);
        let i = &o.items[0];
        assert_eq!((i.state, i.attempts), (State::Done, 1));
        assert_eq!(i.last.as_ref().unwrap().status, Some(200));
        assert!(due(&mut o, at + HOUR).is_empty());
    }

    #[test]
    fn a_429_waits_as_asked_or_a_minute() {
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        due(&mut o, T0);
        o.settle(&id, answer(429, &[("Retry-After", "600")], ""), T0);
        assert_eq!(o.items[0].next_ms, T0 + 600 * SEC);
        due(&mut o, T0 + 600 * SEC);
        // no header (the web without Expose-Headers): a minute at least
        o.settle(&id, answer(429, &[], ""), T0 + 600 * SEC);
        assert!(o.items[0].next_ms >= T0 + 600 * SEC + RETRY_429);
        let at = o.items[0].next_ms;
        due(&mut o, at);
        // an HTTP date, and a day at most
        let now = T0 + HOUR;
        o.settle(
            &id,
            answer(429, &[("Retry-After", "Sun, 01 Feb 2026 00:00:00 GMT")], ""),
            now,
        );
        assert_eq!(o.items[0].next_ms, now + 24 * HOUR);
        assert_eq!(o.items[0].attempts, 3);
    }

    #[test]
    fn backoff_grows_then_gives_up() {
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        let mut now = T0;
        let mut waits = Vec::new();
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(due(&mut o, now), [id.as_str()]);
            o.settle(&id, answer(500, &[], ""), now);
            if o.items[0].state == State::GaveUp {
                break;
            }
            waits.push(o.items[0].next_ms - now);
            now = o.items[0].next_ms;
        }
        assert_eq!(o.items[0].state, State::GaveUp);
        assert_eq!(o.items[0].attempts, MAX_ATTEMPTS);
        assert_eq!(waits.len() as u32, MAX_ATTEMPTS - 1);
        for (n, w) in waits.iter().enumerate() {
            let base = (BACKOFF_FIRST << n.min(16)).min(BACKOFF_MAX);
            assert!((base..=base * 5 / 4).contains(w), "attempt {n}: {w}");
        }
        assert!(due(&mut o, now + 10 * HOUR).is_empty());
        // the user's Retry: back in line, same id and body
        let body = o.items[0].frozen.clone();
        o.retry(&id, now);
        assert_eq!((o.items[0].state, o.items[0].attempts), (State::Queued, 0));
        assert_eq!(due(&mut o, now), [id]);
        assert_eq!(o.items[0].frozen, body);
    }

    #[test]
    fn a_refused_key_pauses_the_provider() {
        let mut o = Outbox::default();
        let a = one(&mut o, 1);
        let b = one(&mut o, 2);
        assert_eq!(due(&mut o, T0), [a.as_str()]);
        o.settle(&a, answer(401, &[], r#"{"error":"bad token"}"#), T0);
        assert_eq!(o.items[0].state, State::Auth);
        assert_eq!(o.items[0].last.as_ref().unwrap().text, "bad token");
        assert!(o.paused.contains(&ProviderId::VibeJson(0)));
        // b waits, untouched
        assert!(due(&mut o, T0 + HOUR).is_empty());
        assert_eq!((o.items[1].state, o.items[1].attempts), (State::Queued, 0));
        o.resume(ProviderId::VibeJson(0));
        assert_eq!(due(&mut o, T0 + HOUR), [b]);
        // 403 the same
        let c = o.items[1].id.clone();
        o.settle(&c, answer(403, &[], ""), T0 + HOUR);
        assert_eq!(o.items[1].state, State::Auth);
    }

    #[test]
    fn other_refusals_are_final() {
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        due(&mut o, T0);
        o.settle(&id, answer(422, &[], "no such chart"), T0);
        let i = &o.items[0];
        assert_eq!(i.state, State::Rejected);
        assert_eq!(i.last.as_ref().unwrap().text, "no such chart");
        assert!(due(&mut o, T0 + 100 * HOUR).is_empty());
        // an answer for an item no longer out changes nothing
        o.settle(&id, answer(200, &[], ""), T0);
        assert_eq!(o.items[0].state, State::Rejected);
    }

    /// What a server says is stored as a line, however much it says.
    #[test]
    fn a_servers_words_are_stored_cut() {
        let long = "x".repeat(1 << 20);
        let kept = crate::http::TEXT_MAX + 1;
        let stored = |o: &Outbox| {
            let i = &o.items[0];
            let text = i.last.as_ref().unwrap().text.chars().count();
            (text, i.note.as_ref().map(|n| n.chars().count()))
        };
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        due(&mut o, T0);
        o.settle(&id, Reply::NoAnswer(long.clone()), T0);
        assert_eq!(stored(&o), (kept, None));
        for (status, made) in [
            (422, Outcome::Rejected(long.clone())),
            (401, Outcome::Auth(long.clone())),
            (
                500,
                Outcome::Retry {
                    after: None,
                    why: long.clone(),
                },
            ),
            (
                200,
                Outcome::Sent {
                    remote: None,
                    note: Some(long.clone()),
                },
            ),
        ] {
            let mut o = Outbox::default();
            let id = one(&mut o, 1);
            due(&mut o, T0);
            let sent = matches!(made, Outcome::Sent { .. });
            o.settle(&id, outcome(status, made), T0);
            assert_eq!(stored(&o), (kept, sent.then_some(kept)), "{status}");
            assert!(o.to_json().len() < long.len() / 4, "{status}");
        }
    }

    #[test]
    fn a_deferred_import_is_polled_never_posted_again() {
        let mut o = Outbox::default();
        let id = one(&mut o, 1);
        due(&mut o, T0);
        let deferred = Outcome::Deferred {
            remote: "imp1".into(),
        };
        o.settle(&id, outcome(202, deferred), T0);
        assert_eq!(o.items[0].state, State::Polling);
        assert!(matches!(o.items[0].op(), Op::Poll { remote: "imp1" }));
        // after 1, 2, 4, 8 s, then every 15 s
        let mut now = T0;
        let mut gaps = Vec::new();
        for _ in 0..6 {
            let at = o.items[0].next_ms;
            gaps.push(at - now);
            assert!(due(&mut o, at - 1).is_empty());
            assert_eq!(due(&mut o, at), [id.as_str()]);
            assert_eq!(o.items[0].state, State::Checking);
            o.settle(&id, outcome(200, Outcome::Pending), at);
            now = at;
        }
        assert_eq!(gaps, [SEC, 2 * SEC, 4 * SEC, 8 * SEC, 15 * SEC, 15 * SEC]);
        // after 10 minutes, every 10 minutes
        now = T0 + POLL_FOR;
        o.items[0].next_ms = now;
        due(&mut o, now);
        o.settle(&id, outcome(200, Outcome::Pending), now);
        assert_eq!(o.items[0].next_ms, now + POLL_LATER);
        // a failing poll keeps polling
        due(&mut o, now + POLL_LATER);
        o.settle(&id, answer(502, &[], ""), now + POLL_LATER);
        assert_eq!(o.items[0].state, State::Polling);
        assert!(matches!(o.items[0].op(), Op::Poll { .. }));
        let at = o.items[0].next_ms;
        due(&mut o, at);
        let note = Some("chart not on Bokutachi".to_string());
        let sent = Outcome::Sent {
            remote: None,
            note: note.clone(),
        };
        o.settle(&id, outcome(200, sent), at);
        assert_eq!((o.items[0].state, &o.items[0].note), (State::Done, &note));
        // an import that failed
        let id = one(&mut o, 2);
        due(&mut o, T0);
        o.settle(
            &id,
            outcome(
                202,
                Outcome::Deferred {
                    remote: "imp2".into(),
                },
            ),
            T0,
        );
        due(&mut o, T0 + SEC);
        o.settle(
            &id,
            outcome(200, Outcome::Rejected("bad score".into())),
            T0 + SEC,
        );
        assert_eq!(o.items[1].state, State::Rejected);
        // the user's Retry polls it again
        o.retry(&id, T0 + 2 * SEC);
        assert_eq!(o.items[1].state, State::Polling);
    }

    #[test]
    fn no_answer_is_not_an_attempt() {
        let mut o = Outbox::default();
        let a = one(&mut o, 1);
        due(&mut o, T0);
        o.settle(&a, Reply::NoAnswer("connection refused".into()), T0);
        let i = &o.items[0];
        assert_eq!(
            (i.state, i.attempts, i.next_ms),
            (State::Retry, 0, T0 + MIN)
        );
        assert_eq!(i.last.as_ref().unwrap().status, None);
        // the provider is offline for a minute, then the item is the probe
        assert!(due(&mut o, T0 + MIN - 1).is_empty());
        assert_eq!(due(&mut o, T0 + MIN), [a.as_str()]);
        o.settle(&a, Reply::NoAnswer("again".into()), T0 + MIN);
        o.send_now(T0 + MIN + 1);
        assert_eq!(due(&mut o, T0 + MIN + 1), [a.as_str()]);
        o.settle(&a, answer(204, &[], ""), T0 + MIN + 2);
        assert_eq!(o.items[0].state, State::Done);
    }

    #[test]
    fn plays_go_in_order_one_at_a_time_per_provider() {
        let mut o = Outbox::default();
        let p = ir();
        // pushed out of order: the older play goes first
        o.push(item(&p, 2, T0 + 20));
        o.push(item(&p, 1, T0 + 10));
        let other = VibeJson::new(1, "https://other.example").unwrap();
        o.push(item(&other, 3, T0 + 30));
        let (newer, older, third) = (
            o.items[0].id.clone(),
            o.items[1].id.clone(),
            o.items[2].id.clone(),
        );
        assert_eq!(due(&mut o, T0 + 30), [older.clone(), third]);
        o.settle(&older, answer(500, &[], ""), T0 + 30);
        // the older one waits to retry: the newer one waits behind it
        assert!(due(&mut o, T0 + 31).is_empty());
        let at = o.items[1].next_ms;
        assert_eq!(due(&mut o, at), [older.as_str()]);
        o.settle(&older, answer(200, &[], ""), at);
        // the limiter says no: nothing goes, nothing is marked
        assert!(o.next(at, |_| false).is_empty());
        assert_eq!(o.items[0].state, State::Queued);
        assert_eq!(due(&mut o, at), [newer]);
    }

    #[test]
    fn a_restart_resumes_what_was_out() {
        let mut o = Outbox::default();
        let a = one(&mut o, 1);
        let b = one(&mut o, 2);
        let c = one(&mut o, 3);
        o.items[1].provider = ProviderId::Bokutachi;
        o.items[2].provider = ProviderId::BmsIr;
        due(&mut o, T0);
        o.settle(
            &b,
            outcome(202, Outcome::Deferred { remote: "i".into() }),
            T0,
        );
        due(&mut o, T0 + SEC);
        o.settle(&c, answer(401, &[], ""), T0);
        assert_eq!(o.items[0].state, State::Sending);
        assert_eq!(o.items[1].state, State::Checking);
        let bytes = o.to_json();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains(r#""paused":["bmsir"]"#), "{text}");
        assert!(text.contains(r#""provider":"vibe-json:0""#), "{text}");
        let later = T0 + HOUR;
        let mut back = Outbox::load(&bytes, later).unwrap();
        let s: Vec<(State, i64)> = back.items.iter().map(|i| (i.state, i.next_ms)).collect();
        assert_eq!(
            s,
            [
                (State::Retry, later),
                (State::Polling, later),
                (State::Auth, o.items[2].next_ms)
            ]
        );
        assert_eq!(back.items[0].frozen, o.items[0].frozen);
        assert_eq!(due(&mut back, later), [a, b]);
        assert!(Outbox::load(b"{", later).is_err());
    }

    #[test]
    fn the_outbox_stays_bounded() {
        let mut o = Outbox::default();
        let p = ir();
        for n in 0..MAX_UNFINISHED as i64 {
            o.push(item(&p, n, T0 + n));
        }
        // a failed one goes first, then the oldest waiting one
        o.items[7].state = State::GaveUp;
        let dropped = o.push(item(&p, 1000, T0 + 1000));
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].summary.title, "chart 7");
        let dropped = o.push(item(&p, 1001, T0 + 1001));
        assert_eq!(dropped[0].summary.title, "chart 0");
        assert_eq!(o.items.len(), MAX_UNFINISHED);
        // done items stay a week
        let id = o.items[0].id.clone();
        due(&mut o, T0 + 2000);
        o.settle(&id, answer(200, &[], ""), T0 + 2000);
        o.prune(T0 + 2000 + KEEP_DONE - 1);
        assert!(o.item(&id).is_some());
        o.prune(T0 + 2000 + KEEP_DONE);
        assert!(o.item(&id).is_none());
        // the user's Remove
        let id = o.items[0].id.clone();
        o.remove(&id);
        assert!(o.item(&id).is_none());
    }
}
