//! ONLINE: when plays are sent, whose rankings and lamps show, each
//! account as a card (its sub-page: a pasted key or a player id, on / off,
//! send plays, BMS-IR's view, tables and rivals), and the outbox.

use super::row::onoff;
use super::row::{Act, Kind, Row, Rows, Tone};
use super::{Sub, TextField};
use crate::config::{provider_name, Beside, IrAccount, PlayerKey, SubmitPolicy};
use crate::ctx::Ctx;
use crate::ir::creds;
use crate::platform::{store, unix_ms, IS_WEB};
use crate::scene::cycle;
use vibebms_ir::outbox::State;
use vibebms_ir::providers::bmsir_read::{self, TABLES, VIEWS};
use vibebms_ir::ProviderId;

/// The providers with an account of their own, before the vibeBMS IR
/// servers (as many as the player adds).
const PROVIDERS: [ProviderId; 2] = [ProviderId::Bokutachi, ProviderId::BmsIr];

/// The outbox items listed at most.
const LISTED: usize = 50;

fn toggle_enabled(c: &mut Ctx, i: usize) {
    if let Some(a) = c.config.ir.accounts.get_mut(i) {
        a.enabled ^= true;
    }
}

fn toggle_submit(c: &mut Ctx, i: usize) {
    if let Some(a) = c.config.ir.accounts.get_mut(i) {
        a.submit ^= true;
    }
}

fn add_provider(c: &mut Ctx, n: usize) {
    let p = PROVIDERS[n];
    let read_only = p == ProviderId::BmsIr;
    c.config.ir.accounts.push(IrAccount {
        provider: p,
        submit: !read_only,
        view: if read_only {
            VIEWS[0].0.into()
        } else {
            String::new()
        },
        tables: if read_only {
            vec![TABLES[0].0.into()]
        } else {
            Vec::new()
        },
        ..Default::default()
    });
}

/// Forget the account's key (a vibeBMS IR server, a BMS-IR player: the
/// account, and its plays waiting).
fn remove(c: &mut Ctx, i: usize) {
    let Some(a) = c.config.ir.accounts.get_mut(i) else {
        return;
    };
    let p = a.provider;
    a.user.clear();
    a.user_id.clear();
    if let Err(e) = c.net.ir.set_key(a, None) {
        c.toast(tr!("Credentials: {e}", e = e));
    }
    if matches!(p, ProviderId::VibeJson(_) | ProviderId::BmsIr) {
        c.config.ir.accounts.remove(i);
        c.net.ir.forget(p);
    }
}

/// The BMS-IR account.
fn bmsir(c: &mut Ctx) -> Option<&mut IrAccount> {
    c.config.ir.account_mut(ProviderId::BmsIr)
}

/// Whose scores BMS-IR counts: the next view.
fn step_view(c: &mut Ctx, d: i32) {
    if let Some(a) = bmsir(c) {
        let keys: Vec<&str> = VIEWS.iter().map(|(k, _)| *k).collect();
        let now = keys
            .iter()
            .copied()
            .find(|&k| k == a.view)
            .unwrap_or(keys[0]);
        a.view = cycle(&keys, now, d).to_string();
    }
}

/// Follow table `t` of [`TABLES`] or no longer.
fn toggle_table(c: &mut Ctx, t: usize) {
    if let Some(a) = bmsir(c) {
        let key = TABLES[t].0;
        match a.tables.iter().position(|k| k == key) {
            Some(n) => {
                a.tables.remove(n);
            }
            None => a.tables.push(key.into()),
        }
    }
}

fn refresh_lamps(c: &mut Ctx, i: usize) {
    let Some(a) = c.config.ir.accounts.get(i) else {
        return;
    };
    let title = a.title().to_string();
    if !c.net.ir.refresh_lamps(a.provider) {
        c.toast(tr!(
            "{title}: lamps read less than 5 minutes ago",
            title = title
        ));
    }
}

/// Read the player's own scores and take them into the records.
fn download_scores(c: &mut Ctx, i: usize) {
    let Some(p) = c.config.ir.accounts.get(i).map(|a| a.provider) else {
        return;
    };
    let said = match c.net.ir.download_scores(&c.config.ir, p) {
        Ok(s) | Err(s) => s,
    };
    c.toast(said);
}

/// "Download my scores", for an account whose player is known.
fn download_row(p: ProviderId, i: usize) -> Row {
    Row::new(
        "ir.download",
        tr!("Download my scores"),
        "↓",
        Act::Run(download_scores, i),
    )
    .desc(match p {
        ProviderId::BmsIr => {
            tr!("Your lamps on the tables read into your records, where better (LR2's as LN; others where the LN mode does not matter).")
        }
        ProviderId::Bokutachi => {
            tr!("Your PBs into your records, where better: BMS under LR2's rules as LN, PMS under beatoraja's.")
        }
        _ => tr!("Your best scores into your records, where better, each under its rule and LN mode."),
    })
    .keys("import scores pull sync records")
}

/// The choices of whose IR lamps show beside the player's: theirs, each
/// rival, none.
fn beside_choices(ctx: &Ctx) -> Vec<Beside> {
    let mut v = vec![Beside::Mine];
    for a in ctx.config.ir.accounts.iter().filter(|a| a.enabled) {
        v.extend(a.rivals.iter().map(|r| {
            Beside::Player(PlayerKey {
                provider: a.provider,
                player: r.clone(),
            })
        }));
    }
    v.extend(ctx.profiles.rivals().map(|p| Beside::Profile(p.id.clone())));
    v.push(Beside::Off);
    v
}

fn step_beside(c: &mut Ctx, d: i32) {
    let choices = beside_choices(c);
    let n = choices.len() as i32;
    let at = choices.iter().position(|b| *b == c.config.ir.beside);
    c.config.ir.beside = match at {
        Some(i) => choices[(i as i32 + d).rem_euclid(n) as usize].clone(),
        None => Beside::Mine,
    };
}

fn beside_label(ctx: &Ctx) -> String {
    match &ctx.config.ir.beside {
        Beside::Mine => tr!("yours on the IR").into(),
        Beside::Off => tr!("none").into(),
        Beside::Player(k) => player_label(ctx, k.provider, &k.player),
        Beside::Profile(id) => match ctx.profiles.registry.get(id) {
            Some(p) => tr!("{name} (profile)", name = p.name),
            None => tr!("a profile deleted").into(),
        },
    }
}

/// A player as people know them: their name on the IR, else the id.
fn player_label(ctx: &Ctx, p: ProviderId, player: &str) -> String {
    match ctx.net.ir.player_name(p, player) {
        Some(name) => format!("{name} ({player})"),
        None => format!("{} {player}", provider_name(p)),
    }
}

fn step_policy(c: &mut Ctx, d: i32) {
    c.config.ir.submit = cycle(&SubmitPolicy::ALL, c.config.ir.submit, d);
}

/// The next enabled account as the rankings' source.
fn step_primary(c: &mut Ctx, d: i32) {
    let ir = &mut c.config.ir;
    let on: Vec<ProviderId> = (ir.accounts.iter())
        .filter(|a| a.enabled)
        .map(|a| a.provider)
        .collect();
    if let Some(now) = ir.primary().map(|a| a.provider) {
        ir.primary = Some(cycle(&on, now, d));
    }
}

/// The `n`-th item listed.
fn item_id(c: &Ctx, n: usize) -> Option<String> {
    c.net.ir.items().nth(n).map(|i| i.id.clone())
}

fn retry_item(c: &mut Ctx, n: usize) {
    if let Some(id) = item_id(c, n) {
        c.net.ir.retry(&id, unix_ms());
    }
}

fn remove_item(c: &mut Ctx, n: usize) {
    if let Some(id) = item_id(c, n) {
        c.net.ir.remove(&id);
    }
}

/// What the account's key is, as far as is known here.
fn key_state(ctx: &Ctx, a: &IrAccount) -> String {
    match () {
        _ if ctx.net.ir.key_held(a) => {
            tr!("kept, not sent: enter an https:// address or the key here").into()
        }
        _ if ctx.net.ir.key_elsewhere(a) => tr!("kept for another address: paste it again").into(),
        _ if !ctx.net.ir.has_key(a) => tr!("none: paste one").into(),
        _ if !a.user.is_empty() => tr!("connected as {user}", user = a.user),
        _ => tr!("kept").into(),
    }
}

/// The account's two-letter tile.
fn badge(p: ProviderId) -> &'static str {
    match p {
        ProviderId::Bokutachi => "BT",
        ProviderId::BmsIr => "BI",
        ProviderId::VibeJson(_) => "VI",
        ProviderId::RianIr => "RI",
    }
}

/// An account as a card: its state and one line about it.
fn card(ctx: &Ctx, a: &IrAccount) -> Row {
    let p = a.provider;
    let has_key = ctx.net.ir.has_key(a);
    let (state, tone) = match () {
        _ if !a.enabled => (tr!("OFF"), Tone::Dim),
        // (an address from a configuration: one entered here is checked)
        _ if matches!(p, ProviderId::VibeJson(_))
            && crate::platform::http::origin(&a.url).is_none() =>
        {
            (tr!("BAD ADDRESS"), Tone::Red)
        }
        _ if p == ProviderId::BmsIr => (tr!("READ ONLY"), Tone::Amber),
        _ if !has_key => (tr!("NO KEY"), Tone::Red),
        _ if !a.user.is_empty() => (tr!("CONNECTED"), Tone::Accent),
        _ => (tr!("KEY KEPT"), Tone::Accent),
    };
    let mut line: Vec<String> = Vec::new();
    match p {
        ProviderId::BmsIr => {
            line.push(match (a.user_id.as_str(), a.user.as_str()) {
                ("", _) => tr!("no player id yet").into(),
                (id, "") => tr!("player {id}", id = id),
                (id, name) => format!("{name} ({id})"),
            });
            line.push(tr!(
                "{n} of {of} tables read",
                n = a.tables.len(),
                of = TABLES.len()
            ));
            if !a.rivals.is_empty() {
                line.push(tr!("{n} rivals", n = a.rivals.len()));
            }
        }
        _ => {
            if matches!(p, ProviderId::VibeJson(_)) && !a.url.is_empty() {
                line.push(a.url.clone());
            }
            line.push(key_state(ctx, a));
            line.push(
                if a.submit {
                    tr!("sends plays")
                } else {
                    tr!("does not send plays")
                }
                .into(),
            );
        }
    }
    if !a.user_id.is_empty() {
        line.push(tr!("lamps {state}", state = ctx.net.ir.lamps_state(p)));
    }
    Row::sub("ir.account", a.title(), "", Sub::Account(p))
        .desc(line.join(" · "))
        .keys(provider_name(p))
        .kind(Kind::Card {
            badge: badge(p),
            state: state.into(),
            tone,
        })
}

/// The outbox's waiting and failed plays.
fn outbox_counts(ctx: &Ctx) -> (usize, usize) {
    let (mut waiting, mut failed) = (0, 0);
    for i in ctx.net.ir.items() {
        waiting += !i.state.is_final() as usize;
        failed += i.state.failed() as usize;
    }
    (waiting, failed)
}

/// ONLINE's chips on the top level: the accounts on, the plays waiting.
pub(super) fn chips(ctx: &Ctx) -> Vec<String> {
    let mut v: Vec<String> = (ctx.config.ir.accounts.iter())
        .filter(|a| a.enabled)
        .map(|a| a.title().to_string())
        .take(2)
        .collect();
    if v.is_empty() {
        v.push(tr!("no account").into());
    }
    match outbox_counts(ctx) {
        (0, _) => {}
        (n, _) => v.push(tr!("{n} waiting", n = n)),
    }
    v
}

/// The plays the outbox has not sent yet (the rail's and the card's
/// number).
pub(super) fn waiting(ctx: &Ctx) -> usize {
    outbox_counts(ctx).0
}

/// What needs the player: failed plays, accounts without their key.
pub(super) fn attention(ctx: &Ctx, v: &mut Vec<String>) {
    match outbox_counts(ctx) {
        (0, 0) => {}
        (1, 0) => v.push(tr!("1 play waiting to send").into()),
        (w, 0) => v.push(tr!("{n} plays waiting to send", n = w)),
        (_, 1) => v.push(tr!("1 play failed to send").into()),
        (_, f) => v.push(tr!("{n} plays failed to send", n = f)),
    }
    for a in ctx.config.ir.accounts.iter().filter(|a| a.enabled) {
        if a.provider != ProviderId::BmsIr && !ctx.net.ir.has_key(a) {
            v.push(tr!("{account}: no API key", account = a.title()));
        }
    }
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let ir = &ctx.config.ir;
    b.header(tr!("SCORES AND RANKINGS"));
    b.extend([
        Row::adjust(
            "ir.submit",
            tr!("Send scores"),
            ir.submit.label(),
            step_policy,
        )
        .desc(tr!(
            "AUTO sends each play to every account that takes them; ASK asks; OFF never."
        ))
        .keys("submit upload"),
        Row::adjust(
            "ir.primary",
            tr!("Rankings from"),
            ir.primary().map_or("—", IrAccount::title),
            step_primary,
        )
        .desc(tr!(
            "The IR whose ranking and clear rate the song select shows."
        )),
        Row::adjust(
            "ir.beside",
            tr!("Rival lamps beside yours"),
            beside_label(ctx),
            step_beside,
        )
        .desc(tr!(
            "A second lamp on each song bar: yours on the IR, an IR rival's, or another profile's."
        ))
        .keys("rival lamp local profile"),
    ]);
    if ctx.net.ir.reads_only() {
        b.push(Row::note(
            "ir.reads_only",
            tr!("Another tab sends the IR plays: this one only shows them"),
            "",
        ));
    }
    if store::limited() {
        b.push(Row::note(
            "ir.store_limited",
            tr!("This browser keeps only the newest plays of the history"),
            "",
        ));
    }
    b.header(tr!("ACCOUNTS"));
    for (n, &p) in PROVIDERS.iter().enumerate() {
        match ir.account(p) {
            Some(a) => b.push(card(ctx, a)),
            None => b.push(
                Row::new(
                    "ir.add_provider",
                    provider_name(p),
                    "",
                    Act::Run(add_provider, n),
                )
                .desc(match p {
                    ProviderId::Bokutachi => tr!("Enter: add your account (an API key)"),
                    _ => tr!("Enter: add your player id (rankings and lamps, read only)"),
                })
                .kind(Kind::Card {
                    badge: badge(p),
                    state: tr!("ADD +").into(),
                    tone: Tone::Accent,
                }),
            ),
        }
    }
    for a in ir.accounts.iter() {
        if matches!(a.provider, ProviderId::VibeJson(_)) {
            b.push(card(ctx, a));
        }
    }
    b.push(
        Row::new(
            "ir.add_server",
            tr!("Add a vibeBMS IR server…"),
            "+",
            Act::Text(TextField::AddIrServer),
        )
        .desc(tr!("Paste its URL; its API key goes on its page."))
        .keys("vibe json server"),
    );
    b.header(tr!("OUTBOX"));
    let (waiting, failed) = outbox_counts(ctx);
    let n = ctx.net.ir.items().count();
    b.extend([
        Row::new(
            "ir.outbox",
            tr!(
                "{waiting} waiting, {failed} failed",
                waiting = waiting,
                failed = failed
            ),
            tr!("send now ↻"),
            Act::Run(|c, _| c.net.ir.send_now(unix_ms()), 0),
        )
        .desc(tr!(
            "Plays not sent yet are kept and sent when the server answers."
        ))
        .keys("outbox send retry"),
        Row::sub(
            "ir.outbox.show",
            tr!("Show the outbox"),
            if n == 1 {
                tr!("1 play").into()
            } else {
                tr!("{n} plays", n = n)
            },
            Sub::Outbox,
        )
        .desc(tr!("Each play, where it goes, and Retry / Discard.")),
    ]);
}

/// ONLINE › an account: on / off, its key or player id, what it sends and
/// reads; BMS-IR's tables as a grid.
pub(super) fn account(b: &mut Rows, p: ProviderId) {
    let ctx = b.ctx;
    let Some(i) = ctx.config.ir.accounts.iter().position(|a| a.provider == p) else {
        return;
    };
    let a = &ctx.config.ir.accounts[i];
    b.push(
        Row::new(
            "ir.enabled",
            tr!("Use {account}", account = a.title()),
            onoff(a.enabled),
            Act::Toggle(toggle_enabled, i),
        )
        .desc(match p {
            ProviderId::Bokutachi => {
                tr!("Rankings and PBs; sending waits for Bokutachi to accept vibeBMS.")
            }
            ProviderId::BmsIr => {
                tr!("Reads rankings and lamps of players who publish them. Sending: not yet.")
            }
            _ => tr!("Rankings, and the plays you send."),
        }),
    );
    if p == ProviderId::BmsIr {
        return bmsir_rows(b, i);
    }
    if matches!(p, ProviderId::VibeJson(_)) {
        b.extend([
            Row::new(
                "ir.name",
                tr!("Name"),
                a.name.clone(),
                Act::Text(TextField::IrName(i)),
            )
            .desc(tr!("What this server is called here.")),
            Row::new(
                "ir.url",
                "URL",
                a.url.clone(),
                Act::Text(TextField::IrUrl(i)),
            )
            .desc(tr!(
                "The server's address (https://). Its API key is kept for this address only."
            )),
        ]);
    }
    let key_desc = match p {
        ProviderId::Bokutachi => tr!(
            "From boku.tachi.ac, Profile, API Keys (any key reads). Kept {where}.",
            where = creds::kept()
        ),
        _ => tr!("Paste it. Kept {where}, not config.toml.", where = creds::kept()),
    };
    b.push(
        Row::new(
            "ir.key",
            tr!("API key"),
            key_state(ctx, a),
            Act::Text(TextField::IrKey(i)),
        )
        .desc(key_desc)
        .keys("token credentials"),
    );
    b.push(
        Row::new(
            "ir.submit_plays",
            tr!("Send plays"),
            onoff(a.submit),
            Act::Toggle(toggle_submit, i),
        )
        .desc(if p == ProviderId::Bokutachi && IS_WEB {
            tr!("Plays in the browser are not sent to Bokutachi.")
        } else {
            tr!("Send your plays to this account.")
        }),
    );
    if !a.user_id.is_empty() {
        b.push(lamps_row(ctx, a, i));
        b.push(download_row(p, i));
    }
    let (label, desc) = if matches!(p, ProviderId::VibeJson(_)) {
        (
            tr!("Remove this server"),
            tr!("Its key and its plays waiting go too."),
        )
    } else {
        (
            tr!("Forget the key"),
            tr!("The key is deleted from this device."),
        )
    };
    b.push(Row::new("ir.remove", label, "×", Act::Run(remove, i)).desc(desc));
}

/// Read the lamps again; how they are.
fn lamps_row(ctx: &Ctx, a: &IrAccount, i: usize) -> Row {
    Row::new(
        "ir.lamps",
        tr!("IR lamps"),
        format!("{} ↻", ctx.net.ir.lamps_state(a.provider)),
        Act::Run(refresh_lamps, i),
    )
    .desc(tr!("Read again now (they are kept for a day)."))
}

/// The tables whose lamps BMS-IR reads: all or none.
fn all_tables(c: &mut Ctx, all: usize) {
    if let Some(a) = bmsir(c) {
        a.tables = if all == 1 {
            TABLES.iter().map(|(k, _)| k.to_string()).collect()
        } else {
            Vec::new()
        };
    }
}

/// A BMS-IR account: the player id (no key: its reads are public), whose
/// scores count, rivals, the tables followed.
fn bmsir_rows(b: &mut Rows, i: usize) {
    let ctx = b.ctx;
    let a = &ctx.config.ir.accounts[i];
    let player = match (a.user_id.as_str(), a.user.as_str()) {
        ("", _) => tr!("enter yours").to_string(),
        (id, "") => format!("{id}: {}", ctx.net.ir.lamps_state(a.provider)),
        (id, name) => format!("{name} ({id})"),
    };
    let view = (VIEWS.iter())
        .find(|(k, _)| *k == a.view)
        .map_or(a.view.as_str(), |(_, name)| name);
    let rivals: Vec<String> = (a.rivals.iter())
        .map(|r| player_label(ctx, a.provider, r))
        .collect();
    b.extend([
        Row::new(
            "ir.player",
            tr!("Player id"),
            player,
            Act::Text(TextField::IrPlayer(i)),
        )
        .desc(tr!(
            "Your number on BMS-IR, or paste your player page's address."
        )),
        Row::adjust("ir.view", tr!("Scores of"), view, step_view).desc(tr!(
            "Whose plays count: all clients, LR2, beatoraja, LR2oraja…"
        )),
        Row::new(
            "ir.rivals",
            tr!("Rivals"),
            match rivals.len() {
                0 => tr!("none").into(),
                1 | 2 => rivals.join(", "),
                n => tr!("{n} players", n = n),
            },
            Act::Text(TextField::IrRivals(i)),
        )
        .desc(tr!(
            "Players whose lamps you can show beside yours (their player ids)."
        )),
    ]);
    if !a.user_id.is_empty() {
        b.push(lamps_row(ctx, a, i));
        b.push(download_row(a.provider, i));
    }
    let on = TABLES
        .iter()
        .filter(|(k, _)| a.tables.iter().any(|t| t == k))
        .count();
    b.header_with(
        tr!(
            "TABLES WHOSE LAMPS ARE READ · {on} OF {n}",
            on = on,
            n = TABLES.len()
        ),
        vec![
            (tr!("all"), Act::Run(all_tables, 1)),
            (tr!("none"), Act::Run(all_tables, 0)),
        ],
    );
    for (t, (key, name)) in TABLES.iter().enumerate() {
        let on = a.tables.iter().any(|k| k == key);
        b.push(
            Row::new("ir.table", *name, "", Act::Toggle(toggle_table, t)).kind(Kind::Cell { on }),
        );
    }
    b.header(tr!("ACCOUNT"));
    b.push(
        Row::new("ir.remove", tr!("Remove"), "×", Act::Run(remove, i)).desc(tr!(
            "Forget the player id, its lamps and its plays waiting."
        )),
    );
}

/// ONLINE › OUTBOX: each play, newest first, with Retry / Discard.
pub(super) fn outbox(b: &mut Rows) {
    let ctx = b.ctx;
    let ir = &ctx.net.ir;
    b.header(tr!("PLAYS"));
    b.push(
        Row::new(
            "ir.outbox",
            tr!("Send now"),
            tr!("send now ↻"),
            Act::Run(|c, _| c.net.ir.send_now(unix_ms()), 0),
        )
        .desc(tr!("Try every waiting play again now.")),
    );
    for (n, i) in ir.items().take(LISTED).enumerate() {
        let to =
            (ctx.config.ir.account(i.provider)).map_or(provider_name(i.provider), IrAccount::title);
        b.push(
            Row::note(
                "ir.outbox.item",
                i.summary.title.clone(),
                ir.item_words(i, &ctx.config.ir),
            )
            .desc(tr!("→ {to}", to = to)),
        );
        if i.state.failed() {
            b.push(
                Row::new(
                    "ir.outbox.retry",
                    tr!("Retry"),
                    "↻",
                    Act::Run(retry_item, n),
                )
                .desc(tr!("Send this play again now.")),
            );
        }
        if i.state != State::Done {
            b.push(
                Row::new(
                    "ir.outbox.discard",
                    tr!("Discard"),
                    "×",
                    Act::Run(remove_item, n),
                )
                .desc(tr!("Never send this play.")),
            );
        }
    }
}

/// An address no request goes to was entered for a server.
fn not_an_address(ctx: &mut Ctx) {
    ctx.toast(tr!(
        "An IR server's address starts with https:// (http:// only on this computer)"
    ));
}

/// A text the player entered for the IR page.
pub(super) fn commit(ctx: &mut Ctx, tf: TextField, s: String) {
    let ir = &mut ctx.config.ir;
    match tf {
        TextField::IrName(i) => {
            if let Some(a) = ir.accounts.get_mut(i) {
                a.name = s;
            }
        }
        TextField::IrUrl(i) => {
            let Some(origin) = crate::platform::http::origin(&s) else {
                return not_an_address(ctx);
            };
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            // another server: who the account is there is not known, and
            // the key (kept for the old one) is asked for again
            if crate::platform::http::origin(&a.url).as_deref() != Some(origin.as_str()) {
                a.user.clear();
                a.user_id.clear();
                a.chart_page.clear();
            }
            a.url = s;
            // a key held for want of such an address is for the one the
            // player entered
            if let Err(e) = ctx.net.ir.entered_address(a) {
                ctx.toast(tr!("Credentials: {e}", e = e));
            }
        }
        TextField::IrKey(i) => {
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            a.user.clear();
            a.user_id.clear();
            let p = a.provider;
            if let Err(e) = ctx.net.ir.set_key(a, Some(s).filter(|s| !s.is_empty())) {
                ctx.toast(tr!("Credentials: {e}", e = e));
            }
            ctx.net.ir.verify(&ctx.config.ir, p);
        }
        TextField::IrPlayer(i) => {
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            match bmsir_read::player_id(&s) {
                Some(id) => a.user_id = id.into(),
                None if s.is_empty() => a.user_id.clear(),
                None => {
                    ctx.toast(tr!("Not a BMS-IR player id: {s}", s = s));
                    return;
                }
            }
            a.user.clear();
            let p = a.provider;
            ctx.net.ir.verify(&ctx.config.ir, p);
        }
        TextField::IrRivals(i) => {
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            let mut bad = Vec::new();
            a.rivals.clear();
            for w in s.split(|c: char| c == ',' || c.is_whitespace()) {
                match bmsir_read::player_id(w) {
                    Some(id) if !a.rivals.iter().any(|r| r == id) => a.rivals.push(id.into()),
                    Some(_) => {}
                    None if w.is_empty() => {}
                    None => bad.push(w),
                }
            }
            if !bad.is_empty() {
                ctx.toast(tr!("Not BMS-IR player ids: {ids}", ids = bad.join(", ")));
            }
        }
        TextField::AddIrServer if !s.is_empty() => {
            if crate::platform::http::origin(&s).is_none() {
                return not_an_address(ctx);
            }
            let Some(n) = ir.free_slot() else { return };
            ir.accounts.push(IrAccount {
                provider: ProviderId::VibeJson(n),
                url: s,
                ..Default::default()
            });
        }
        _ => {}
    }
}

/// What a text field shows as its editing starts (never a key).
pub(super) fn current(ctx: &Ctx, tf: TextField) -> String {
    let a = |i: usize| ctx.config.ir.accounts.get(i);
    match tf {
        TextField::IrName(i) => a(i).map(|a| a.name.clone()),
        TextField::IrUrl(i) => a(i).map(|a| a.url.clone()),
        TextField::IrPlayer(i) => a(i).map(|a| a.user_id.clone()),
        TextField::IrRivals(i) => a(i).map(|a| a.rivals.join(", ")),
        _ => None,
    }
    .unwrap_or_default()
}
