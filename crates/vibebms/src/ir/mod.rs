//! Internet ranking: what the app keeps and sends for the IR servers
//! (`vibebms-ir` has the protocols, the eligibility and the outbox's
//! states).
//!
//! A play goes into the history, then, where its account sends plays and
//! `eligible` lets it, into the outbox: one item per account, its body
//! frozen. The outbox is the only thing that sends scores. It is a file,
//! written before the first request and after each change, so a play
//! waits through a restart or a day offline. Each request is one
//! `platform::http::exchange`, at most one in flight per provider, within
//! the provider's `Limiter`; a server's `Retry-After` holds the provider.
//! On the web only the tab holding the outbox lock sends and writes it;
//! another tab reads it and keeps its own plays until it gets the lock.
//!
//! The accounts' lamp sets (`lamps`) are read through the same limiters.

pub mod build_id;
pub mod creds;
pub mod lamps;
pub mod store;

use crate::app::AppEvent;
use crate::config::{Beside, IrAccount, IrSettings, SubmitPolicy};
use crate::paths::Paths;
use crate::platform::{http, Job, IS_WEB};
use lamps::{LampCache, SetKey};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use vibebms_core::record::{Platform, PlayRecord};
use vibebms_ir::eligibility::{eligible, Refusal};
use vibebms_ir::lamps::{RankView, RemoteLamp};
use vibebms_ir::limiter::Limiter;
use vibebms_ir::outbox::{Item, Outbox, Reply, State};
use vibebms_ir::provider::{AuthKind, ChartId, IrError};
use vibebms_ir::providers::bmsir_read::BmsIrRead;
use vibebms_ir::providers::tachi::{self, Tachi};
use vibebms_ir::providers::vibe_json::VibeJson;
use vibebms_ir::{HttpRequest, HttpResponse, Op, Outcome, Provider, ProviderId};

/// What a request was for.
enum Ticket {
    /// An outbox item (its id).
    Item(String),
    /// The account's key is checked.
    Verify,
    /// A lamp set is read.
    Lamps(SetKey),
}

/// Why a ranking was not asked for.
#[derive(Debug, PartialEq)]
pub enum NotAsked {
    /// The account's budget is spent, or its server asked to wait: ask
    /// again later.
    Later,
    /// No account can be asked about the chart.
    Never,
}

/// How often the lamp sets are looked over (ms).
const LAMPS_EVERY_MS: i64 = 1000;
/// How often a tab without the outbox lock reads the outbox again (ms).
const RELOAD_MS: i64 = 10_000;

/// A request's answer, or why none came.
struct Answer {
    p: ProviderId,
    ticket: Ticket,
    r: Result<HttpResponse, String>,
}

/// The IR's state: the history of plays, the accounts' keys, the outbox
/// and the requests out.
pub struct IrService {
    history: store::History,
    pub keys: creds::Keys,
    outbox_file: PathBuf,
    /// `None` until the file is read (at once natively; on the web when the
    /// storage worker answers).
    outbox: Option<Outbox>,
    loading: Option<Job<io::Result<String>>>,
    /// Items made before the outbox was read, or while another tab sends.
    early: Vec<Item>,
    /// This program sends the outbox (on the web: holds its lock).
    sends: bool,
    /// Without the lock: when the outbox is read again (Unix ms).
    reload_ms: i64,
    limiters: BTreeMap<ProviderId, Limiter>,
    jobs: Vec<Job<Answer>>,
    lamps: LampCache,
    /// When the lamp sets are next looked over (Unix ms).
    lamps_ms: i64,
    /// Accounts whose player id was just entered: what its first lamp set
    /// says is told.
    checking: Vec<ProviderId>,
    /// What to tell the player at the next poll.
    notes: Vec<String>,
}

/// The adapter of an account, where this build has one.
fn provider(a: &IrAccount) -> Option<Box<dyn Provider>> {
    match a.provider {
        ProviderId::VibeJson(n) => Some(Box::new(VibeJson::new(n, &a.url)?)),
        ProviderId::Bokutachi => Some(Box::new(Tachi)),
        ProviderId::BmsIr => Some(Box::new(BmsIrRead::at(&a.url))),
        ProviderId::RianIr => None,
    }
}

/// The lamp sets the enabled accounts follow, each with its view: the
/// player's own and the rivals', per BMS-IR table, Tachi playtype, or the
/// one set of a vibeBMS IR.
fn followed(ir: &IrSettings) -> Vec<(SetKey, String)> {
    let mut v = Vec::new();
    for a in ir.accounts.iter().filter(|a| a.enabled) {
        let scopes: Vec<&str> = match a.provider {
            ProviderId::BmsIr => a.tables.iter().map(String::as_str).collect(),
            ProviderId::Bokutachi => tachi::PLAYTYPES.to_vec(),
            ProviderId::VibeJson(_) => vec![""],
            ProviderId::RianIr => continue,
        };
        let players = std::iter::once(&a.user_id).chain(&a.rivals);
        for player in players.filter(|p| !p.is_empty()) {
            for scope in &scopes {
                let key = SetKey {
                    provider: a.provider,
                    player: player.clone(),
                    scope: scope.to_string(),
                };
                v.push((key, a.view.clone()));
            }
        }
    }
    v
}

impl IrService {
    /// The service over the files in `paths` (`now_ms`: Unix ms).
    pub fn new(paths: &Paths, now_ms: i64) -> IrService {
        let mut history = store::History::open(paths.history.clone(), now_ms);
        history.compact(now_ms);
        IrService {
            history,
            keys: creds::Keys::open(paths),
            outbox_file: paths.outbox.join("outbox.json"),
            outbox: None,
            loading: None,
            early: Vec::new(),
            sends: false,
            reload_ms: 0,
            limiters: BTreeMap::new(),
            jobs: Vec::new(),
            lamps: LampCache::new(paths.cache.join("ir")),
            lamps_ms: 0,
            checking: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Read the outbox's file.
    fn load(&mut self) {
        let at = self.outbox_file.clone();
        self.loading = Some(Job::start(|tx, _| {
            crate::platform::store::load(&at, move |r| {
                let _ = tx.send(r);
            })
        }));
    }

    /// Whether this tab sends the outbox now (asked of the lock); it is
    /// read again when the lock comes, and every `RELOAD_MS` without it.
    fn lock(&mut self, now_ms: i64) {
        let Some(held) = crate::platform::store::outbox_lock() else {
            return;
        };
        if held && !self.sends {
            self.sends = true;
            self.outbox = None;
            self.loading = None;
        }
        let stale = !held && now_ms >= self.reload_ms;
        if self.loading.is_none() && (self.outbox.is_none() || stale) {
            self.reload_ms = now_ms + RELOAD_MS;
            self.load();
        }
    }

    /// Keys an older configuration kept go to the credentials store; true
    /// when one did (the configuration is then saved without it).
    pub fn take_old_tokens(&mut self, ir: &mut IrSettings) -> bool {
        let mut moved = false;
        for a in ir.accounts.iter_mut().filter(|a| !a.old_token.0.is_empty()) {
            match self.keys.set(a.provider, Some(a.old_token.0.clone())) {
                Ok(()) => {
                    a.old_token.0.clear();
                    moved = true;
                }
                Err(e) => log::warn!("credentials: {e}"),
            }
        }
        moved
    }

    /// The leaderboard of `chart` at the account the music select's
    /// rankings come from, within its budget.
    pub fn ranking(
        &mut self,
        ir: &IrSettings,
        mut chart: ChartId,
        now_ms: i64,
    ) -> Result<Job<Result<RankView, String>>, NotAsked> {
        let a = ir.primary().ok_or(NotAsked::Never)?.clone();
        let prov = provider(&a).ok_or(NotAsked::Never)?;
        fn op<'a>(chart: &'a ChartId, view: &'a str) -> Op<'a> {
            Op::Ranking {
                chart,
                view,
                limit: 0,
                etag: None,
            }
        }
        // Tachi knows charts by its own id, learned from the PB lists
        chart.remote = (self.lamp(a.provider, &a.user_id, &chart.md5, &chart.sha256))
            .and_then(|l| l.remote.clone());
        let creds = self.keys.get(a.provider);
        let req =
            (prov.request(&op(&chart, &a.view), creds.as_ref())).map_err(|_| NotAsked::Never)?;
        // (its answer is not waited for here: it takes a token only)
        let l = self.limiter(a.provider, &*prov, now_ms);
        l.start(now_ms).map_err(|_| NotAsked::Later)?;
        l.finish();
        let req = http::request(req.method, &req.url, &req.headers, req.body);
        Ok(Job::start(move |tx, _| {
            http::exchange(req, move |r| {
                let read = r.map_err(|e| e.to_string()).and_then(|r| {
                    let resp = response(r);
                    // (the adapter is made again here: this runs on the
                    // HTTP thread natively)
                    let parsed = provider(&a).map(|p| p.parse(&op(&chart, &a.view), &resp));
                    match parsed {
                        Some(Outcome::Ranking(v)) => Ok(v),
                        Some(Outcome::Rejected(why) | Outcome::Retry { why, .. }) => Err(why),
                        o => Err(format!("unexpected answer: {o:?}")),
                    }
                });
                let _ = tx.send(read);
            })
        }))
    }

    /// A play has ended: it gets its play id and goes into the history
    /// (a play of the player's own: not autoplay, not a replay watched).
    pub fn record(&mut self, rec: &mut PlayRecord) {
        if rec.flags.autoplay || rec.flags.replay || rec.flags.practice {
            return;
        }
        if let Err(e) = self.history.add(rec) {
            log::warn!("play history: {e}");
        }
    }

    /// `p`'s limiter, made full at its first use.
    fn limiter(&mut self, p: ProviderId, prov: &dyn Provider, now_ms: i64) -> &mut Limiter {
        let rate = prov.caps(Platform::current()).rate;
        (self.limiters)
            .entry(p)
            .or_insert_with(|| Limiter::new(rate, now_ms))
    }

    /// Queue play `rec` (recorded) for every enabled account that sends
    /// plays and that `eligible` lets it go to; `user_intent`: the player
    /// pressed a button for it.
    pub fn submit(&mut self, rec: &PlayRecord, ir: &IrSettings, user_intent: bool, now_ms: i64) {
        if rec.play_id.is_empty() {
            return;
        }
        let mut dropped = Vec::new();
        for a in ir.accounts.iter().filter(|a| a.enabled && a.submit) {
            let (Some(p), Ok(profile)) = (provider(a), eligible(rec, a.provider.kind())) else {
                continue;
            };
            let id = &rec.play_id;
            match Item::freeze(&*p, &a.user_id, rec, id, profile, user_intent, now_ms) {
                Ok(item) => match self.outbox.as_mut().filter(|_| self.sends) {
                    Some(o) => dropped.extend(o.push(item)),
                    None => self.early.push(item),
                },
                Err(e) => log::warn!("IR {}: {e}", a.title()),
            }
        }
        self.dropped(dropped);
        self.save();
    }

    /// Items the outbox dropped to stay within its bound: logged and told.
    fn dropped(&mut self, items: Vec<Item>) {
        for d in &items {
            log::warn!("IR outbox full: dropped {} ({})", d.summary.title, d.id);
        }
        let max = vibebms_ir::outbox::MAX_UNFINISHED;
        self.notes.extend(match items.as_slice() {
            [] => None,
            [d] => Some(format!(
                "IR outbox full ({max} waiting): dropped {}",
                d.summary.title
            )),
            more => Some(format!(
                "IR outbox full ({max} waiting): dropped the {} oldest",
                more.len()
            )),
        });
    }

    /// Check the account of `p` (its key, or its player id, was entered or
    /// changed).
    pub fn verify(&mut self, ir: &IrSettings, p: ProviderId) {
        let Some(prov) = ir.account(p).and_then(provider) else {
            return;
        };
        if prov.caps(Platform::current()).auth == AuthKind::None {
            // public reads: the player's lamp sets say whether the id holds
            self.lamps.follow(followed(ir));
            self.lamps.refresh(p, crate::platform::unix_ms(), true);
            self.checking.retain(|&q| q != p);
            self.checking.push(p);
            self.lamps_ms = 0;
            return;
        }
        let Some(creds) = self.keys.get(p) else {
            return;
        };
        match prov.request(&Op::Verify, Some(&creds)) {
            Ok(req) => self.send(p, Ticket::Verify, req),
            Err(e) => log::warn!("IR verify: {e}"),
        }
    }

    /// The player asks for `p`'s lamps again; false when they were all
    /// read too lately for that.
    pub fn refresh_lamps(&mut self, p: ProviderId) -> bool {
        self.lamps_ms = 0;
        self.lamps.refresh(p, crate::platform::unix_ms(), false)
    }

    /// `p`'s lamp sets for people.
    pub fn lamps_state(&self, p: ProviderId) -> String {
        self.lamps.state(p, crate::platform::unix_ms())
    }

    /// `player`'s IR lamp of a chart on `p`.
    pub fn lamp(
        &self,
        p: ProviderId,
        player: &str,
        md5: &str,
        sha256: &str,
    ) -> Option<&RemoteLamp> {
        self.lamps.lamp(p, player, md5, sha256)
    }

    /// Whether `player`'s lamps on `p` have been read.
    pub fn has_lamps(&self, p: ProviderId, player: &str) -> bool {
        self.lamps.has(p, player)
    }

    /// Whose IR lamps show beside the player's (`IrSettings::beside`), once
    /// some of theirs were read.
    pub fn beside<'a>(&self, ir: &'a IrSettings) -> Option<(ProviderId, &'a str)> {
        let read = |&(p, player): &(ProviderId, &str)| self.lamps.has(p, player);
        match &ir.beside {
            Beside::Off => None,
            Beside::Player(k) => Some((k.provider, k.player.as_str())).filter(read),
            // the rankings' account first
            Beside::Mine => (ir.primary().into_iter())
                .chain(ir.accounts.iter().filter(|a| a.enabled))
                .map(|a| (a.provider, a.user_id.as_str()))
                .find(read),
        }
    }

    /// The name `p` gives `player`.
    pub fn player_name(&self, p: ProviderId, player: &str) -> Option<&str> {
        self.lamps.name(p, player)
    }

    fn send(&mut self, p: ProviderId, ticket: Ticket, req: HttpRequest) {
        let r = http::request(req.method, &req.url, &req.headers, req.body);
        self.jobs.push(Job::start(move |tx, _| {
            http::exchange(r, move |r| {
                let r = r.map(response).map_err(|e| e.to_string());
                let _ = tx.send(Answer { p, ticket, r });
            })
        }));
    }

    /// Write the outbox (once it was read, and while this tab sends it).
    fn save(&self) {
        let Some(o) = self.outbox.as_ref().filter(|_| self.sends) else {
            return;
        };
        let text = String::from_utf8_lossy(&o.to_json()).into_owned();
        if let Err(e) = crate::platform::store::rewrite(&self.outbox_file, &text) {
            log::warn!("IR outbox: {e}");
        }
    }

    /// The outbox as read from its file.
    fn loaded(&self, r: io::Result<String>, now_ms: i64) -> Outbox {
        let text = match r {
            Ok(t) if t.trim().is_empty() => return Outbox::default(),
            Ok(t) => t,
            Err(e) => {
                log::warn!("IR outbox: {e}");
                return Outbox::default();
            }
        };
        Outbox::load(text.as_bytes(), now_ms).unwrap_or_else(|e| {
            // kept aside, not written over
            log::warn!("{e}");
            if self.sends {
                let bad = self.outbox_file.with_extension("json.bad");
                let _ = crate::platform::store::rewrite(&bad, &text);
            }
            Outbox::default()
        })
    }

    /// What came back and what is due: answers settle their items, due
    /// items are sent. An account's check changes `ir`.
    pub fn poll(&mut self, now_ms: i64, ir: &mut IrSettings) -> Vec<AppEvent> {
        let mut events = Vec::new();
        let mut changed = false;
        self.lock(now_ms);
        if let Some(r) = self.loading.as_mut().and_then(Job::poll) {
            self.loading = None;
            let mut o = self.loaded(r, now_ms);
            if self.sends {
                o.prune(now_ms);
                let mut dropped = Vec::new();
                for i in self.early.drain(..) {
                    dropped.extend(o.push(i));
                }
                self.dropped(dropped);
            }
            self.outbox = Some(o);
            changed = true;
        }
        for a in Job::drain_all(&mut self.jobs) {
            if let (Ticket::Item(_) | Ticket::Lamps(_), Some(l)) =
                (&a.ticket, self.limiters.get_mut(&a.p))
            {
                l.finish();
            }
            changed = true;
            match (a.ticket, ir.account_mut(a.p)) {
                (Ticket::Lamps(k), account) => {
                    events.extend(self.lamps_came(account, &k, a.r, now_ms));
                    self.lamps_ms = 0;
                }
                (Ticket::Verify, Some(account)) => {
                    events.extend(self.verified(account, a.r));
                    events.push(AppEvent::IrChanged);
                }
                (Ticket::Item(id), Some(account)) => {
                    let failed = self.settle(account, &id, a.r, now_ms);
                    events.extend(failed.map(AppEvent::Toast));
                }
                // the account was removed meanwhile, its plays with it
                (Ticket::Item(id), None) => {
                    if let Some(o) = self.outbox.as_mut() {
                        o.items.retain(|i| i.id != id);
                    }
                }
                (Ticket::Verify, None) => {}
            }
        }
        if self.sends {
            changed |= self.send_due(ir, now_ms);
        }
        self.lamps.poll_reads();
        if now_ms >= self.lamps_ms {
            self.lamps_ms = now_ms + LAMPS_EVERY_MS;
            self.lamps.follow(followed(ir));
            // (natively the files are read at once)
            self.lamps.poll_reads();
            if self.sends {
                self.read_lamps(ir, now_ms);
            }
        }
        if changed {
            self.save();
        }
        events.extend(self.notes.drain(..).map(AppEvent::Toast));
        events
    }

    /// Ask for the lamp sets due, one at a time per server, within its
    /// budget. Natively with the `ETag` of the set kept; not on the web,
    /// where the header cannot be read and a header of the page's own
    /// would cost a preflight (the browser's cache revalidates instead).
    fn read_lamps(&mut self, ir: &IrSettings, now_ms: i64) {
        for k in self.lamps.due(now_ms) {
            let p = k.provider;
            let Some(prov) = ir.account(p).and_then(provider) else {
                continue;
            };
            if self.lamps.asking(p) || self.limiter(p, &*prov, now_ms).start(now_ms).is_err() {
                continue;
            }
            let Some(c) = self.lamps.get(&k) else {
                continue;
            };
            let op = Op::PlayerLamps {
                player: &k.player,
                scope: &k.scope,
                view: c.want_view(),
                etag: c.etag().filter(|_| !IS_WEB),
            };
            match prov.request(&op, self.keys.get(p).as_ref()) {
                Ok(req) => {
                    self.lamps.asked(&k);
                    self.send(p, Ticket::Lamps(k), req);
                }
                Err(e) => {
                    if let Some(l) = self.limiters.get_mut(&p) {
                        l.finish();
                    }
                    self.lamps
                        .settle(&k, Ok(Outcome::Rejected(e.to_string())), now_ms);
                }
            }
        }
    }

    /// A lamp set's answer. The player's own name comes with their lamps;
    /// after the player id was entered, a refusal of it is told.
    fn lamps_came(
        &mut self,
        a: Option<&mut IrAccount>,
        k: &SetKey,
        r: Result<HttpResponse, String>,
        now_ms: i64,
    ) -> Option<AppEvent> {
        let parsed = match (r, a.as_deref().and_then(provider)) {
            (Ok(resp), Some(prov)) => {
                let op = Op::PlayerLamps {
                    player: &k.player,
                    scope: &k.scope,
                    view: "",
                    etag: None,
                };
                Ok(prov.parse(&op, &resp))
            }
            (Ok(_), None) => Err("the account was removed".into()),
            (Err(why), _) => Err(why),
        };
        if let (
            Ok(Outcome::Retry {
                after: Some(at), ..
            }),
            Some(l),
        ) = (&parsed, self.limiters.get_mut(&k.provider))
        {
            l.hold(now_ms + at.delay_ms(now_ms) as i64);
        }
        let said = match &parsed {
            Ok(Outcome::Lamps(set)) => Ok(set.player_name.clone()),
            Ok(Outcome::Rejected(why)) => Err(why.clone()),
            _ => Err(String::new()),
        };
        self.lamps.settle(k, parsed, now_ms);
        let own = a.filter(|a| a.user_id == k.player)?;
        let checking = self.checking.contains(&own.provider);
        match said {
            Ok(name) => {
                self.checking.retain(|&p| p != own.provider);
                (!name.is_empty() && name != own.user).then(|| {
                    own.user = name;
                    AppEvent::IrChanged
                })
            }
            Err(why) if checking && !why.is_empty() => {
                self.checking.retain(|&p| p != own.provider);
                own.user.clear();
                let player = &own.user_id;
                Some(AppEvent::Toast(format!(
                    "{}: player {player}: {why}",
                    own.title()
                )))
            }
            _ => None,
        }
    }

    /// An answer for item `id`; what to tell the player when it failed for
    /// good.
    fn settle(
        &mut self,
        a: &IrAccount,
        id: &str,
        r: Result<HttpResponse, String>,
        now_ms: i64,
    ) -> Option<String> {
        let o = self.outbox.as_mut()?;
        let reply = match r {
            Err(why) => Reply::NoAnswer(why),
            Ok(resp) => {
                let outcome = match (provider(a), o.item(id)) {
                    (Some(p), Some(i)) => p.parse(&i.op(), &resp),
                    _ => return None,
                };
                if let (
                    Outcome::Retry {
                        after: Some(at), ..
                    },
                    Some(l),
                ) = (&outcome, self.limiters.get_mut(&a.provider))
                {
                    l.hold(now_ms + at.delay_ms(now_ms) as i64);
                }
                Reply::Answer {
                    status: resp.status,
                    outcome,
                }
            }
        };
        o.settle(id, reply, now_ms);
        let i = o.item(id).filter(|i| i.state.failed())?;
        let why = i.last.as_ref().map_or("", |l| l.text.as_str());
        Some(match i.state {
            State::Auth => format!(
                "{}: the key was refused; enter it again in Settings, IR",
                a.title()
            ),
            _ => format!("{}: {} not sent ({why})", a.title(), i.summary.title),
        })
    }

    /// The account's check came back: who it is, or why not.
    fn verified(&mut self, a: &mut IrAccount, r: Result<HttpResponse, String>) -> Option<AppEvent> {
        let p = provider(a)?;
        let said = match r.map(|resp| p.parse(&Op::Verify, &resp)) {
            Ok(Outcome::Account(info)) => {
                a.user = info.name;
                a.user_id = info.id;
                self.resume(a.provider);
                if info.can_submit {
                    return None;
                }
                "this key cannot send scores".to_string()
            }
            // a server without the check (vibeBMS IR protocol 1)
            Ok(Outcome::Unsupported) => {
                self.resume(a.provider);
                return None;
            }
            Ok(Outcome::Auth(why)) => {
                a.user.clear();
                format!("the key was refused ({why})")
            }
            Ok(Outcome::Rejected(why) | Outcome::Retry { why, .. }) => {
                format!("the key could not be checked ({why})")
            }
            Ok(_) => "the key could not be checked".into(),
            Err(why) => format!("no answer ({why})"),
        };
        Some(AppEvent::Toast(format!("{}: {said}", a.title())))
    }

    fn resume(&mut self, p: ProviderId) {
        if let Some(o) = self.outbox.as_mut() {
            o.resume(p);
        }
    }

    /// Send what is due; whether anything was.
    fn send_due(&mut self, ir: &IrSettings, now_ms: i64) -> bool {
        let Some(o) = self.outbox.as_mut() else {
            return false;
        };
        let (limiters, keys) = (&mut self.limiters, &self.keys);
        let ready = |p| {
            let prov = ir.account(p).filter(|a| a.enabled).and_then(provider)?;
            keys.has(p).then_some(prov)
        };
        let ids = o.next(now_ms, |p| {
            ready(p).is_some_and(|prov| {
                let rate = prov.caps(Platform::current()).rate;
                let l = limiters
                    .entry(p)
                    .or_insert_with(|| Limiter::new(rate, now_ms));
                l.start(now_ms).is_ok()
            })
        });
        let mut out = Vec::new();
        for id in &ids {
            let Some(i) = o.item(id) else { continue };
            let p = i.provider;
            let req = ready(p)
                .ok_or(IrError::NoCredentials)
                .and_then(|prov| prov.request(&i.op(), keys.get(p).as_ref()));
            match req {
                Ok(req) => out.push((p, id.clone(), req)),
                Err(e) => {
                    if let Some(l) = limiters.get_mut(&p) {
                        l.finish();
                    }
                    let outcome = Outcome::Rejected(e.to_string());
                    o.settle(id, Reply::Answer { status: 0, outcome }, now_ms);
                }
            }
        }
        for (p, id, req) in out {
            self.send(p, Ticket::Item(id), req);
        }
        !ids.is_empty()
    }

    /// Every item, the newest first.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        let saved = self.outbox.iter().flat_map(|o| o.items.iter());
        saved.chain(&self.early).rev()
    }

    /// The player's Retry, Remove and Send now in the outbox list.
    pub fn retry(&mut self, id: &str, now_ms: i64) {
        self.change(|o| o.retry(id, now_ms));
    }

    pub fn remove(&mut self, id: &str) {
        self.change(|o| o.remove(id));
    }

    pub fn send_now(&mut self, now_ms: i64) {
        self.change(|o| o.send_now(now_ms));
    }

    /// The account of `p` is gone: so are its plays waiting (a server
    /// added later in its slot is another).
    pub fn forget(&mut self, p: ProviderId) {
        self.early.retain(|i| i.provider != p);
        self.change(|o| {
            let ids: Vec<String> = (o.items.iter())
                .filter(|i| i.provider == p)
                .map(|i| i.id.clone())
                .collect();
            for id in ids {
                o.remove(&id);
            }
        });
    }

    /// Change the outbox (only the tab that sends it does).
    fn change(&mut self, f: impl FnOnce(&mut Outbox)) {
        if let Some(o) = self.outbox.as_mut().filter(|_| self.sends) {
            f(o);
            self.save();
        }
    }

    /// Whether another tab sends the outbox (this one only reads it).
    pub fn reads_only(&self) -> bool {
        crate::platform::store::outbox_lock() == Some(false)
    }

    /// One line per enabled account on what became of play `rec`: sent,
    /// waiting, refused, or why it is not sent.
    pub fn status(&self, rec: &PlayRecord, ir: &IrSettings) -> Vec<String> {
        let mut lines = Vec::new();
        for a in ir.accounts.iter().filter(|a| a.enabled) {
            let item = self
                .items()
                .find(|i| i.play_id == rec.play_id && i.provider == a.provider);
            let words = match (item, eligible(rec, a.provider.kind())) {
                (Some(i), _) => self.item_words(i),
                (None, Err(Refusal::OperatorPending)) => {
                    format!("not sent: waiting for {} to accept vibeBMS", a.title())
                }
                (None, Err(r)) => format!("not sent: {}", refusal_words(r)),
                _ if provider(a).is_none() => "not in this build yet".into(),
                _ if ir.submit == SubmitPolicy::Off || !a.submit => "not sending".into(),
                _ if ir.submit == SubmitPolicy::Ask => "press I to send".into(),
                _ => "not sent".into(),
            };
            lines.push(format!("{}: {words}", a.title()));
        }
        lines
    }

    /// An item's state for people.
    pub fn item_words(&self, i: &Item) -> String {
        let last = i.last.as_ref();
        let why = last.map_or("", |l| l.text.as_str());
        let offline = last.is_some_and(|l| l.status.is_none());
        match i.state {
            State::Queued | State::Retry if !self.keys.has(i.provider) => {
                "waiting for an API key".into()
            }
            State::Queued | State::Retry | State::Polling if offline => "waiting (offline)".into(),
            State::Queued => "waiting".into(),
            State::Sending => "sending…".into(),
            State::Polling | State::Checking => "processing".into(),
            State::Retry => format!("will retry at {}", clock(i.next_ms)),
            State::Done => match &i.note {
                Some(n) => format!("saved ({n})"),
                None => "saved".into(),
            },
            State::Auth => "sign in again".into(),
            State::Rejected => format!("refused: {why}"),
            State::GaveUp => format!("gave up: {why}"),
        }
    }
}

fn response(r: http::Response) -> HttpResponse {
    HttpResponse {
        status: r.status,
        headers: r.headers.headers,
        body: r.body,
    }
}

/// `ms` (Unix) as the local time of day.
fn clock(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map_or(String::new(), |t| {
        t.with_timezone(&chrono::Local).format("%H:%M").to_string()
    })
}

/// Why a play is not sent, for people.
pub fn refusal_words(r: Refusal) -> String {
    match r {
        Refusal::OldRecord => "a record of an older version".into(),
        Refusal::Autoplay => "autoplay".into(),
        Refusal::Practice => "a practice play".into(),
        Refusal::Replay => "a replay".into(),
        Refusal::Versus => "a VS play".into(),
        Refusal::GhostBattle => "G-BATTLE".into(),
        Refusal::Battle => "BATTLE".into(),
        Refusal::CourseStage => "a course stage".into(),
        Refusal::Aborted => "quit".into(),
        Refusal::Assist => "assist options".into(),
        Refusal::LocalRules => "not under vibe-LR2 or vibe-beatoraja rules".into(),
        Refusal::Touch => "touch".into(),
        Refusal::WebPlay => "played in the browser".into(),
        Refusal::NotTaken(p, m) => format!("{} {} not taken", p.label(), m.label()),
        Refusal::OpenGap(id) => match vibebms_ir::rule::gap(id).map(|g| g.reason) {
            Some(reason) if !reason.is_empty() => reason.into(),
            _ => format!("rules not confirmed equal yet ({id})"),
        },
        Refusal::NotYet => "not in this build yet".into(),
        Refusal::OperatorPending => "waiting for the operator".into(),
        Refusal::RandomChart => "a #RANDOM chart".into(),
        Refusal::MixedDevices => "keyboard and controller mixed".into(),
        Refusal::WallTime => "play time does not fit the chart".into(),
        Refusal::Gauge(g) => format!("no {} gauge there", g.label()),
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::config::Config;

    /// An `[ir_server]` of before: its token leaves config.toml for the
    /// credentials store the first time the app starts.
    #[test]
    fn the_old_token_leaves_the_config() {
        let root = std::env::temp_dir().join(format!("vibebms-ir-old-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let file = paths.config_file();
        std::fs::write(
            &file,
            "[ir_server]\nenabled = true\nurl = \"https://ir.example\"\ntoken = \"s3cr3t\"\n",
        )
        .unwrap();
        let mut config = Config::load(&file);
        let mut ir = IrService::new(&paths, 0);
        assert!(ir.take_old_tokens(&mut config.ir));
        config.save(&file);
        assert!(!std::fs::read_to_string(&file).unwrap().contains("s3cr3t"));
        let a = &Config::load(&file).ir.accounts[0];
        let again = IrService::new(&paths, 0);
        assert_eq!(again.keys.get(a.provider).unwrap().token.expose(), "s3cr3t");
        std::fs::remove_dir_all(&root).ok();
    }

    /// A PMS keyboard play under beatoraja rules, as the play scene makes it.
    fn play(ended_ms: i64) -> PlayRecord {
        use vibebms_core::record::VERSION;
        let mut r = PlayRecord {
            v: VERSION,
            client: build_id::client(),
            ended_ms,
            wall_ms: 120_000,
            ..Default::default()
        };
        r.chart.title = "SONG".into();
        r.chart.sha256 = "ab".repeat(32);
        r.chart.mode = vibebms_core::PlayMode::Popn9K;
        r.chart.length_us = 120_000_000;
        r.chart.ln.undefined = true;
        r.device.keyboard = 900;
        r.device.platform = Platform::Linux;
        r.result.clear = vibebms_core::score::ClearType::Normal;
        r
    }

    /// Poll at `now` until no request is out.
    fn settle(ir: &mut IrService, s: &mut IrSettings, now: i64) {
        for _ in 0..500 {
            ir.poll(now, s);
            if ir.jobs.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no answer");
    }

    /// A play goes to a vibeBMS IR through the outbox: a 503 is tried
    /// again when its Retry-After says, with the same key and body, a 200 is saved; the outbox
    /// outlasts a restart and never holds the key. A play under rules of
    /// no profile is not sent, and says why.
    #[test]
    fn a_play_reaches_a_server_through_the_outbox() {
        let (url, seen) = crate::platform::http::tests::serve(vec![
            "HTTP/1.1 503 Busy\r\nRetry-After: 3600\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            "HTTP/1.1 200 OK\r\nContent-Length: 125\r\nConnection: close\r\n\r\n[{\"rank\":1,\"player\":\"P\",\"player_id\":\"9\",\"clear\":\"Hard\",\"ex_score\":7,\"pgreat\":3,\"great\":1,\"max_combo\":4,\"min_bp\":0,\"notes\":4}]",
        ]);
        let root = std::env::temp_dir().join(format!("vibebms-ir-send-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let mut s = IrSettings {
            accounts: vec![IrAccount {
                name: "Mine".into(),
                url,
                ..Default::default()
            }],
            ..Default::default()
        };
        let p = ProviderId::VibeJson(0);
        let t0 = 1_794_916_800_000;
        let mut ir = IrService::new(&paths, t0);
        ir.keys.set(p, Some("s3cr3t".into())).unwrap();
        let mut rec = play(t0);
        ir.record(&mut rec);
        ir.submit(&rec, &s, false, t0);
        assert_eq!(ir.status(&rec, &s), ["Mine: waiting"]);
        settle(&mut ir, &mut s, t0);
        let item = ir.items().next().unwrap().clone();
        assert_eq!((item.state, item.attempts), (State::Retry, 1));
        assert!(ir.status(&rec, &s)[0].starts_with("Mine: will retry at "));
        // the server's Retry-After over the backoff's 30 s
        assert!(item.next_ms >= t0 + 3_600_000);
        let first = seen.recv().unwrap();
        assert!(first.contains("Bearer s3cr3t"));
        let key = format!("idempotency-key: {}", item.id);
        assert!(first.to_lowercase().contains(&key));
        assert!(first.contains(&rec.play_id));
        // not due yet; then due
        settle(&mut ir, &mut s, t0 + 1000);
        assert_eq!(ir.items().next().unwrap().state, State::Retry);
        settle(&mut ir, &mut s, item.next_ms);
        assert_eq!(ir.status(&rec, &s), ["Mine: saved"]);
        let second = seen.recv().unwrap();
        let body = |r: &str| r.split("\r\n\r\n").nth(1).unwrap().to_string();
        assert_eq!(body(&first), body(&second));
        // kept, without the key
        let file = std::fs::read_to_string(paths.outbox.join("outbox.json")).unwrap();
        assert!(file.contains(&rec.play_id) && !file.contains("s3cr3t"));
        let mut again = IrService::new(&paths, t0);
        again.poll(t0, &mut s);
        assert_eq!(again.items().next().unwrap().state, State::Done);
        // rules of no profile (IIDX judge): nothing queued
        let mut local = play(t0 + 5);
        local.rules.judge = vibebms_core::judge::JudgePreset::Iidx;
        again.record(&mut local);
        again.submit(&local, &s, false, t0);
        assert_eq!(again.items().count(), 1);
        assert_eq!(
            again.status(&local, &s),
            ["Mine: not sent: not under vibe-LR2 or vibe-beatoraja rules"]
        );
        // the send policy ASK: the player is asked
        s.submit = SubmitPolicy::Ask;
        let mut asked = play(t0 + 9);
        again.record(&mut asked);
        assert_eq!(again.status(&asked, &s), ["Mine: press I to send"]);
        // the music select's ranking, from the primary account
        let chart = ChartId {
            sha256: rec.chart.sha256.clone(),
            ..Default::default()
        };
        let mut job = again.ranking(&s, chart, t0).unwrap();
        let got = (0..500)
            .find_map(|_| {
                std::thread::sleep(std::time::Duration::from_millis(10));
                job.poll()
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            (got.entries[0].player.as_str(), got.entries[0].ex),
            ("P", 7)
        );
        assert!(seen
            .recv()
            .unwrap()
            .contains(&format!("sha256={}", rec.chart.sha256)));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Poll at `now` until no request is out; what the polls said.
    fn events(ir: &mut IrService, s: &mut IrSettings, now: i64) -> Vec<AppEvent> {
        let mut got = Vec::new();
        for _ in 0..500 {
            got.extend(ir.poll(now, s));
            if ir.jobs.is_empty() {
                return got;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("no answer");
    }

    fn answer(status: &str, headers: &str, body: &str) -> &'static str {
        let len = body.len();
        let text = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {len}\r\nConnection: close\r\n\r\n{body}"
        );
        Box::leak(text.into_boxed_str())
    }

    /// A BMS-IR player id is checked through its lamps: one not public is
    /// told and not kept as the player's; a public one's lamps are kept in
    /// the cache with their ETag, asked again with it once stale (a 304
    /// keeps them), and not before.
    #[test]
    fn a_bmsir_player_is_checked_and_its_lamps_kept() {
        let lamps = include_str!("../../../vibebms-ir/tests/fixtures/bmsir/player_lamps.json");
        let not_public = r#"{"schema_version":1,"ok":false,"error":"player_not_public"}"#;
        let (url, seen) = crate::platform::http::tests::serve(vec![
            answer("404 Not Found", "", not_public),
            answer("200 OK", "ETag: \"a1\"\r\n", lamps),
            answer("304 Not Modified", "ETag: \"a1\"\r\n", ""),
        ]);
        let root = std::env::temp_dir().join(format!("vibebms-ir-lamps-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let mut s = IrSettings {
            accounts: vec![IrAccount {
                provider: ProviderId::BmsIr,
                url: format!("{url}/api"),
                user_id: "1".into(),
                view: "all_clients".into(),
                tables: vec!["satellite".into()],
                submit: false,
                ..Default::default()
            }],
            ..Default::default()
        };
        let p = ProviderId::BmsIr;
        let t0 = crate::platform::unix_ms();
        let mut ir = IrService::new(&paths, t0);
        ir.verify(&s, p);
        let said = events(&mut ir, &mut s, t0);
        assert_eq!(
            said,
            [AppEvent::Toast(
                "BMS-IR: player 1: player not public".into()
            )]
        );
        assert!(seen.recv().unwrap().starts_with(
            "GET /api/player_lamps.json?id=1&table=satellite&client_view=all_clients "
        ));
        assert_eq!(ir.lamps_state(p), "player not public");
        // another id: its name comes with its lamps
        s.accounts[0].user_id = "100002".into();
        ir.verify(&s, p);
        assert_eq!(events(&mut ir, &mut s, t0), [AppEvent::IrChanged]);
        assert_eq!(s.accounts[0].user, "プレイヤー");
        let md5 = "02c4d238c7ab99ae088bae519d92dc06";
        let lamp = ir.lamp(p, "100002", md5, "").unwrap();
        assert_eq!(lamp.clear, vibebms_core::score::ClearType::Perfect);
        assert_eq!(ir.beside(&s), Some((p, "100002")));
        assert!(!seen
            .recv()
            .unwrap()
            .to_lowercase()
            .contains("if-none-match"));
        // fresh: neither asked again by itself nor at the player's word
        assert!(!ir.refresh_lamps(p));
        assert!(events(&mut ir, &mut s, t0 + 60_000).is_empty());
        // stale: asked with the ETag; unchanged
        let later = t0 + lamps::STALE_MS;
        assert!(events(&mut ir, &mut s, later).is_empty());
        let asked = seen.recv().unwrap().to_lowercase();
        assert!(asked.contains("if-none-match: \"a1\""), "{asked}");
        let lamp = ir.lamp(p, "100002", md5, "").unwrap();
        assert_eq!(lamp.clear, vibebms_core::score::ClearType::Perfect);
        // kept: another start has them before asking
        let mut again = IrService::new(&paths, later);
        again.poll(later, &mut s);
        assert!(again.has_lamps(p, "100002"));
        assert_eq!(again.player_name(p, "100002"), Some("プレイヤー"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Rankings take a token of the account's budget: once it is spent,
    /// later; a chart the IR cannot address (a bmson on BMS-IR), never.
    #[test]
    fn rankings_keep_to_the_budget() {
        let root = std::env::temp_dir().join(format!("vibebms-ir-rank-{}", std::process::id()));
        let s = IrSettings {
            accounts: vec![IrAccount {
                provider: ProviderId::BmsIr,
                url: "http://127.0.0.1:9".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut ir = IrService::new(&Paths::at(root.clone()), 0);
        let chart = |md5: &str| ChartId {
            md5: md5.into(),
            ..Default::default()
        };
        let t = 1_800_000_000_000;
        assert_eq!(ir.ranking(&s, chart(""), t).err(), Some(NotAsked::Never));
        let rate = BmsIrRead::at("").caps(Platform::current()).rate;
        for _ in 0..rate.burst {
            assert!(ir.ranking(&s, chart("ab"), t).is_ok());
        }
        assert_eq!(ir.ranking(&s, chart("ab"), t).err(), Some(NotAsked::Later));
        assert!(ir.ranking(&s, chart("ab"), t + 60_000).is_ok());
        std::fs::remove_dir_all(&root).ok();
    }

    /// The outbox at its bound drops its oldest waiting play for a new one,
    /// and says so.
    #[test]
    fn a_full_outbox_tells_what_it_dropped() {
        let root = std::env::temp_dir().join(format!("vibebms-ir-full-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let mut s = IrSettings {
            accounts: vec![IrAccount {
                url: "http://127.0.0.1:9".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let t0 = 1_794_916_800_000;
        let mut ir = IrService::new(&paths, t0);
        // no key: nothing is sent
        ir.poll(t0, &mut s);
        let vj = VibeJson::new(0, "http://127.0.0.1:9").unwrap();
        let profile = vibebms_ir::rule::Profile::VibeBeatoraja;
        let o = ir.outbox.as_mut().unwrap();
        for n in 0..vibebms_ir::outbox::MAX_UNFINISHED {
            let rec = play(t0 + n as i64);
            let id = format!("p{n}");
            o.push(Item::freeze(&vj, "", &rec, &id, profile, false, t0 + n as i64).unwrap());
        }
        let mut rec = play(t0 + 10_000);
        rec.chart.title = "NEWEST".into();
        ir.record(&mut rec);
        ir.submit(&rec, &s, false, t0 + 10_000);
        let said = ir.poll(t0 + 10_000, &mut s);
        assert_eq!(
            said,
            [AppEvent::Toast(
                "IR outbox full (500 waiting): dropped SONG".into()
            )]
        );
        assert!(ir.items().any(|i| i.play_id == rec.play_id));
        assert!(!ir.items().any(|i| i.play_id == "p0"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// An open gap is told by its reason; one without, by its id.
    #[test]
    fn open_gaps_in_words() {
        let a4 = vibebms_ir::rule::gap("A4").unwrap().reason;
        assert!(!a4.is_empty());
        assert_eq!(refusal_words(Refusal::OpenGap("A4")), a4);
        assert_eq!(
            refusal_words(Refusal::OpenGap("Z9")),
            "rules not confirmed equal yet (Z9)"
        );
    }
}
