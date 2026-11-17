//! The IR page: when plays are sent, whose rankings and lamps show, each
//! account (a pasted key or a player id, on / off, send plays, BMS-IR's
//! view, tables and rivals), and the outbox.

use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::config::{provider_name, Beside, IrAccount, PlayerKey, SubmitPolicy};
use crate::ir::creds;
use crate::platform::{store, unix_ms, IS_WEB};
use crate::scene::option_panel::{cycle, onoff};
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
    if let Err(e) = c.net.ir.keys.set(p, None) {
        c.toast(format!("Credentials: {e}"));
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
        c.toast(format!("{title}: lamps read less than 5 minutes ago"));
    }
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
        Beside::Mine => "yours on the IR".into(),
        Beside::Off => "none".into(),
        Beside::Player(k) => player_label(ctx, k.provider, &k.player),
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
        _ if !ctx.net.ir.keys.has(a.provider) => "none: paste one".into(),
        _ if !a.user.is_empty() => format!("connected as {}", a.user),
        _ => "kept".into(),
    }
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let ir = &ctx.config.ir;
    let mut v = vec![
        Row::adjust("Send scores", ir.submit.label(), step_policy),
        Row::adjust(
            "Rankings from",
            ir.primary().map_or("—", IrAccount::title),
            step_primary,
        ),
        Row::adjust("IR lamps beside yours", beside_label(ctx), step_beside),
        Row::note(format!("API keys are kept {}", creds::KEPT), ""),
    ];
    if ctx.net.ir.reads_only() {
        v.push(Row::note(
            "Another tab sends the IR plays: this one only shows them",
            "",
        ));
    }
    if store::limited() {
        v.push(Row::note(
            "This browser keeps only the newest plays of the history",
            "",
        ));
    }
    for (n, &p) in PROVIDERS.iter().enumerate() {
        match ir.accounts.iter().position(|a| a.provider == p) {
            Some(i) => account_rows(ctx, &mut v, i),
            None => v.push(Row::new(
                provider_name(p),
                "add +",
                Act::Run(add_provider, n),
            )),
        }
        match p {
            ProviderId::Bokutachi => {
                v.push(Row::note(
                    "    Sending waits for Bokutachi to accept vibeBMS as a client",
                    "PBs, rankings: read",
                ));
                v.push(Row::note(
                    "    Its key: boku.tachi.ac, Profile, API Keys",
                    "any key reads",
                ));
            }
            ProviderId::BmsIr => v.push(Row::note(
                "    BMS-IR: rankings and lamps of players who publish them",
                "sending: not yet",
            )),
            _ => {}
        }
    }
    for (i, a) in ir.accounts.iter().enumerate() {
        if matches!(a.provider, ProviderId::VibeJson(_)) {
            account_rows(ctx, &mut v, i);
        }
    }
    v.push(Row::new(
        "Add a vibeBMS IR server…",
        "+",
        Act::Text(TextField::AddIrServer),
    ));
    v.push(Row::note("rianIR", "waiting for its operator"));
    outbox_rows(ctx, &mut v);
    v
}

fn account_rows(ctx: &Ctx, v: &mut Vec<Row>, i: usize) {
    let a = &ctx.config.ir.accounts[i];
    v.push(Row::new(
        a.title(),
        onoff(a.enabled),
        Act::Run(toggle_enabled, i),
    ));
    if a.provider == ProviderId::BmsIr {
        return bmsir_rows(ctx, v, i);
    }
    if matches!(a.provider, ProviderId::VibeJson(_)) {
        v.push(Row::new(
            "    Name",
            a.name.clone(),
            Act::Text(TextField::IrName(i)),
        ));
        v.push(Row::new(
            "    URL",
            a.url.clone(),
            Act::Text(TextField::IrUrl(i)),
        ));
    }
    v.push(Row::new(
        "    API key",
        key_state(ctx, a),
        Act::Text(TextField::IrKey(i)),
    ));
    if a.provider == ProviderId::Bokutachi && IS_WEB {
        v.push(Row::note(
            "    Plays in the browser are not sent to Bokutachi",
            "",
        ));
    }
    v.push(Row::new(
        "    Send plays",
        onoff(a.submit),
        Act::Run(toggle_submit, i),
    ));
    if !a.user_id.is_empty() {
        v.push(lamps_row(ctx, a, i));
    }
    let forget = if matches!(a.provider, ProviderId::VibeJson(_)) {
        "    Remove this server"
    } else {
        "    Forget the key"
    };
    v.push(Row::new(forget, "×", Act::Run(remove, i)));
}

/// Read the lamps again; how they are.
fn lamps_row(ctx: &Ctx, a: &IrAccount, i: usize) -> Row {
    let state = ctx.net.ir.lamps_state(a.provider);
    Row::new(
        "    IR lamps (read again)",
        state,
        Act::Run(refresh_lamps, i),
    )
}

/// A BMS-IR account: the player id (no key: its reads are public), whose
/// scores count, rivals, the tables followed.
fn bmsir_rows(ctx: &Ctx, v: &mut Vec<Row>, i: usize) {
    let a = &ctx.config.ir.accounts[i];
    let player = match (a.user_id.as_str(), a.user.as_str()) {
        ("", _) => "enter yours".to_string(),
        (id, "") => format!("{id}: {}", ctx.net.ir.lamps_state(a.provider)),
        (id, name) => format!("{name} ({id})"),
    };
    v.push(Row::new(
        "    Player id",
        player,
        Act::Text(TextField::IrPlayer(i)),
    ));
    let view = (VIEWS.iter())
        .find(|(k, _)| *k == a.view)
        .map_or(a.view.as_str(), |(_, name)| name);
    v.push(Row::adjust("    Scores of", view, step_view));
    let rivals: Vec<String> = (a.rivals.iter())
        .map(|r| player_label(ctx, a.provider, r))
        .collect();
    v.push(Row::new(
        "    Rivals (player ids)",
        if rivals.is_empty() {
            "none".into()
        } else {
            rivals.join(", ")
        },
        Act::Text(TextField::IrRivals(i)),
    ));
    if !a.user_id.is_empty() {
        v.push(lamps_row(ctx, a, i));
    }
    v.push(Row::note("    Tables whose lamps are read", ""));
    for (t, (key, name)) in TABLES.iter().enumerate() {
        let on = a.tables.iter().any(|k| k == key);
        v.push(Row::new(
            format!("        {name}"),
            onoff(on),
            Act::Run(toggle_table, t),
        ));
    }
    v.push(Row::new("    Remove", "×", Act::Run(remove, i)));
}

/// "Outbox: 3 waiting, 1 failed", Send now, and the items, newest first.
fn outbox_rows(ctx: &Ctx, v: &mut Vec<Row>) {
    let ir = &ctx.net.ir;
    let (mut waiting, mut failed) = (0, 0);
    for i in ir.items() {
        waiting += !i.state.is_final() as usize;
        failed += i.state.failed() as usize;
    }
    v.push(Row::new(
        format!("Outbox: {waiting} waiting, {failed} failed"),
        "send now ↻",
        Act::Run(|c, _| c.net.ir.send_now(unix_ms()), 0),
    ));
    for (n, i) in ir.items().take(LISTED).enumerate() {
        let to =
            (ctx.config.ir.account(i.provider)).map_or(provider_name(i.provider), IrAccount::title);
        v.push(Row::note(
            format!("    {} → {to}", i.summary.title),
            ir.item_words(i),
        ));
        if i.state.failed() {
            v.push(Row::new("        Retry", "↻", Act::Run(retry_item, n)));
        }
        if i.state != State::Done {
            v.push(Row::new("        Discard", "×", Act::Run(remove_item, n)));
        }
    }
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
            if let Some(a) = ir.accounts.get_mut(i) {
                a.url = s;
            }
        }
        TextField::IrKey(i) => {
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            a.user.clear();
            a.user_id.clear();
            let p = a.provider;
            if let Err(e) = ctx.net.ir.keys.set(p, Some(s).filter(|s| !s.is_empty())) {
                ctx.toast(format!("Credentials: {e}"));
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
                    ctx.toast(format!("Not a BMS-IR player id: {s}"));
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
                ctx.toast(format!("Not BMS-IR player ids: {}", bad.join(", ")));
            }
        }
        TextField::AddIrServer if !s.is_empty() => {
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
