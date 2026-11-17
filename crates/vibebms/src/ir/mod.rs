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

pub mod build_id;
pub mod creds;
pub mod store;

use crate::app::AppEvent;
use crate::config::{IrAccount, IrSettings, SubmitPolicy};
use crate::paths::Paths;
use crate::platform::{http, Job};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use vibebms_core::record::{Platform, PlayRecord};
use vibebms_ir::eligibility::{eligible, Refusal};
use vibebms_ir::lamps::RankView;
use vibebms_ir::limiter::Limiter;
use vibebms_ir::outbox::{Item, Outbox, Reply, State};
use vibebms_ir::provider::{ChartId, IrError};
use vibebms_ir::providers::tachi::Tachi;
use vibebms_ir::providers::vibe_json::VibeJson;
use vibebms_ir::{HttpRequest, HttpResponse, Op, Outcome, Provider, ProviderId};

/// What a request was for.
enum Ticket {
    /// An outbox item (its id).
    Item(String),
    /// The account's key is checked.
    Verify,
}

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
    /// Items made before the outbox was read.
    early: Vec<Item>,
    limiters: BTreeMap<ProviderId, Limiter>,
    jobs: Vec<Job<Answer>>,
}

/// The adapter of an account, where this build has one.
fn provider(a: &IrAccount) -> Option<Box<dyn Provider>> {
    match a.provider {
        ProviderId::VibeJson(n) => Some(Box::new(VibeJson::new(n, &a.url)?)),
        ProviderId::Bokutachi => Some(Box::new(Tachi)),
        ProviderId::BmsIr | ProviderId::RianIr => None,
    }
}

impl IrService {
    /// The service over the files in `paths` (`now_ms`: Unix ms).
    pub fn new(paths: &Paths, now_ms: i64) -> IrService {
        let mut history = store::History::open(paths.history.clone(), now_ms);
        history.compact(now_ms);
        let outbox_file = paths.outbox.join("outbox.json");
        let at = outbox_file.clone();
        let loading = Job::start(|tx, _| {
            crate::platform::store::load(&at, move |r| {
                let _ = tx.send(r);
            })
        });
        IrService {
            history,
            keys: creds::Keys::open(paths),
            outbox_file,
            outbox: None,
            loading: Some(loading),
            early: Vec::new(),
            limiters: BTreeMap::new(),
            jobs: Vec::new(),
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
    /// rankings come from (`None`: there is none, or it cannot ask).
    pub fn ranking(
        &self,
        ir: &IrSettings,
        chart: ChartId,
    ) -> Option<Job<Result<RankView, String>>> {
        let a = ir.primary()?.clone();
        fn op(chart: &ChartId) -> Op<'_> {
            Op::Ranking {
                chart,
                view: "",
                limit: 0,
                etag: None,
            }
        }
        let creds = self.keys.get(a.provider);
        let req = provider(&a)?.request(&op(&chart), creds.as_ref()).ok()?;
        let req = http::request(req.method, &req.url, &req.headers, req.body);
        Some(Job::start(move |tx, _| {
            http::exchange(req, move |r| {
                let read = r.map_err(|e| e.to_string()).and_then(|r| {
                    let resp = response(r);
                    // (the adapter is made again here: this runs on the
                    // HTTP thread natively)
                    match provider(&a).map(|p| p.parse(&op(&chart), &resp)) {
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
        if rec.flags.autoplay || rec.flags.replay {
            return;
        }
        if let Err(e) = self.history.add(rec) {
            log::warn!("play history: {e}");
        }
    }

    /// Queue play `rec` (recorded) for every enabled account that sends
    /// plays and that `eligible` lets it go to; `user_intent`: the player
    /// pressed a button for it.
    pub fn submit(&mut self, rec: &PlayRecord, ir: &IrSettings, user_intent: bool, now_ms: i64) {
        if rec.play_id.is_empty() {
            return;
        }
        for a in ir.accounts.iter().filter(|a| a.enabled && a.submit) {
            let (Some(p), Ok(profile)) = (provider(a), eligible(rec, a.provider.kind())) else {
                continue;
            };
            let id = &rec.play_id;
            match Item::freeze(&*p, &a.user_id, rec, id, profile, user_intent, now_ms) {
                Ok(item) => match self.outbox.as_mut() {
                    Some(o) => {
                        for d in o.push(item) {
                            log::warn!("IR outbox full: dropped {} ({})", d.summary.title, d.id);
                        }
                    }
                    None => self.early.push(item),
                },
                Err(e) => log::warn!("IR {}: {e}", a.title()),
            }
        }
        self.save();
    }

    /// Check the key of `p`'s account (it was entered or changed).
    pub fn verify(&mut self, ir: &IrSettings, p: ProviderId) {
        let (Some(prov), Some(creds)) = (ir.account(p).and_then(provider), self.keys.get(p)) else {
            return;
        };
        match prov.request(&Op::Verify, Some(&creds)) {
            Ok(req) => self.send(p, Ticket::Verify, req),
            Err(e) => log::warn!("IR verify: {e}"),
        }
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

    /// Write the outbox (once it was read).
    fn save(&self) {
        let Some(o) = &self.outbox else { return };
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
            let bad = self.outbox_file.with_extension("json.bad");
            let _ = crate::platform::store::rewrite(&bad, &text);
            Outbox::default()
        })
    }

    /// What came back and what is due: answers settle their items, due
    /// items are sent. An account's check changes `ir`.
    pub fn poll(&mut self, now_ms: i64, ir: &mut IrSettings) -> Vec<AppEvent> {
        let mut events = Vec::new();
        let mut changed = false;
        if let Some(r) = self.loading.as_mut().and_then(Job::poll) {
            self.loading = None;
            let mut o = self.loaded(r, now_ms);
            o.prune(now_ms);
            for i in self.early.drain(..) {
                o.push(i);
            }
            self.outbox = Some(o);
            changed = true;
        }
        for a in Job::drain_all(&mut self.jobs) {
            if let (Ticket::Item(_), Some(l)) = (&a.ticket, self.limiters.get_mut(&a.p)) {
                l.finish();
            }
            changed = true;
            match (a.ticket, ir.account_mut(a.p)) {
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
        changed |= self.send_due(ir, now_ms);
        if changed {
            self.save();
        }
        events
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

    fn change(&mut self, f: impl FnOnce(&mut Outbox)) {
        if let Some(o) = self.outbox.as_mut() {
            f(o);
            self.save();
        }
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
        Refusal::OpenGap(gap) => format!("rules not confirmed equal yet ({gap})"),
        Refusal::NotYet => "not in this build yet".into(),
        Refusal::OperatorPending => "waiting for the operator".into(),
        Refusal::RandomChart => "a #RANDOM chart".into(),
        Refusal::MixedDevices => "keyboard and controller mixed".into(),
        Refusal::WallTime => "play time does not fit the chart".into(),
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
        let mut job = again.ranking(&s, chart).unwrap();
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
}
