//! The IR page: when plays are sent, whose rankings show, each account
//! (a pasted key, on / off, send plays), and the outbox.

use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::config::{provider_name, IrAccount, SubmitPolicy};
use crate::ir::creds;
use crate::platform::{store, unix_ms, IS_WEB};
use crate::scene::option_panel::{cycle, onoff};
use vibebms_ir::outbox::State;
use vibebms_ir::ProviderId;

/// The providers with an account of their own, before the vibeBMS IR
/// servers (as many as the player adds).
const PROVIDERS: [ProviderId; 1] = [ProviderId::Bokutachi];

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
    c.config.ir.accounts.push(IrAccount {
        provider: PROVIDERS[n],
        ..Default::default()
    });
}

/// Forget the account's key (a vibeBMS IR server: the server, and its
/// plays waiting).
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
    if matches!(p, ProviderId::VibeJson(_)) {
        c.config.ir.accounts.remove(i);
        c.net.ir.forget(p);
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
        Row::note(format!("API keys are kept {}", creds::KEPT), ""),
    ];
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
    }
    v.push(Row::note(
        "    Bokutachi: vibe-beatoraja PMS plays; 7K/14K once vibe-LR2 is equal",
        "never touch plays",
    ));
    v.push(Row::note(
        "    Its key: boku.tachi.ac, Profile, API Keys",
        "with submit_score",
    ));
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
    v.push(Row::note("BMS-IR, rianIR", "waiting for their operators"));
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
    let forget = if matches!(a.provider, ProviderId::VibeJson(_)) {
        "    Remove this server"
    } else {
        "    Forget the key"
    };
    v.push(Row::new(forget, "×", Act::Run(remove, i)));
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
        _ => None,
    }
    .unwrap_or_default()
}
