use super::{Act, Row, TextField};
use crate::app::Ctx;
use crate::config::{IrAccount, Provider, SubmitPolicy};
use crate::ir::creds;
use crate::platform::{store, IS_WEB};
use crate::scene::option_panel::{cycle, onoff};

/// The providers with an account of their own (vibeBMS IRs are listed
/// after them, as many as the player adds).
const PROVIDERS: [Provider; 1] = [Provider::Bokutachi];

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

fn turn_on(c: &mut Ctx, p: usize) {
    c.config.ir.account_mut(PROVIDERS[p]).enabled = true;
}

/// Forget the account's key (a vibeBMS IR: the account too).
fn remove(c: &mut Ctx, i: usize) {
    let Some(a) = c.config.ir.accounts.get(i) else {
        return;
    };
    let (key, server) = (a.key(), a.provider == Provider::VibeJson);
    if let Err(e) = c.net.ir.creds.set(&key, None) {
        c.toast(format!("Credentials: {e}"));
    }
    if server {
        c.config.ir.accounts.remove(i);
    } else {
        c.config.ir.accounts[i].user.clear();
    }
}

fn step_policy(c: &mut Ctx, d: i32) {
    c.config.ir.submit = cycle(&SubmitPolicy::ALL, c.config.ir.submit, d);
}

/// The next enabled account as the rankings' source.
fn step_primary(c: &mut Ctx, d: i32) {
    let ir = &mut c.config.ir;
    let on: Vec<usize> = (0..ir.accounts.len())
        .filter(|&i| ir.accounts[i].enabled)
        .collect();
    let now = ir
        .primary()
        .and_then(|p| ir.accounts.iter().position(|a| std::ptr::eq(a, p)));
    if let Some(now) = now {
        ir.primary = ir.accounts[cycle(&on, now, d)].key();
    }
}

/// What the account is, as far as is known here.
fn state(ctx: &Ctx, a: &IrAccount) -> String {
    let key = ctx.net.ir.creds.get(&a.key()).is_some();
    match () {
        _ if !a.user.is_empty() => format!("connected as {}", a.user),
        _ if key => "key kept".into(),
        _ => "no API key".into(),
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
    for (p, &provider) in PROVIDERS.iter().enumerate() {
        let Some(i) = ir.accounts.iter().position(|a| a.provider == provider) else {
            v.push(Row::new(
                provider.name(),
                onoff(false),
                Act::Run(turn_on, p),
            ));
            continue;
        };
        account_rows(ctx, &mut v, i);
    }
    v.push(Row::note(
        "    Bokutachi takes plays under beatoraja rules (PMS) for now, LR2 rules (7K/14K) later; never touch plays",
        "",
    ));
    for (i, a) in ir.accounts.iter().enumerate() {
        if a.provider == Provider::VibeJson {
            account_rows(ctx, &mut v, i);
        }
    }
    v.push(Row::new(
        "Add a vibeBMS IR server…",
        "+",
        Act::Text(TextField::AddIrServer),
    ));
    v.extend([
        Row::note("BMS-IR, rianIR", "waiting for their operators"),
        Row::note("vibeBMS IR protocol: docs/IR_PROTOCOL.md", ""),
    ]);
    v
}

fn account_rows(ctx: &Ctx, v: &mut Vec<Row>, i: usize) {
    let a = &ctx.config.ir.accounts[i];
    v.push(Row::new(
        a.title(),
        onoff(a.enabled),
        Act::Run(toggle_enabled, i),
    ));
    if a.provider == Provider::VibeJson {
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
        "    API key (paste)",
        state(ctx, a),
        Act::Text(TextField::IrKey(i)),
    ));
    if a.provider == Provider::Bokutachi && IS_WEB {
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
    let forget = if a.provider == Provider::VibeJson {
        "    Remove this server"
    } else {
        "    Forget the key"
    };
    v.push(Row::new(forget, "×", Act::Run(remove, i)));
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
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            // the key goes with the server
            let old = a.key();
            a.url = s;
            let new = a.key();
            let creds = &mut ctx.net.ir.creds;
            if let Some(k) = creds.get(&old).filter(|_| old != new).map(str::to_string) {
                let r = creds.set(&new, Some(k)).and_then(|_| creds.set(&old, None));
                if let Err(e) = r {
                    ctx.toast(format!("Credentials: {e}"));
                }
            }
        }
        TextField::IrKey(i) => {
            let Some(a) = ir.accounts.get_mut(i) else {
                return;
            };
            a.user.clear();
            let key = a.key();
            if let Err(e) = ctx
                .net
                .ir
                .creds
                .set(&key, Some(s).filter(|s| !s.is_empty()))
            {
                ctx.toast(format!("Credentials: {e}"));
            }
        }
        TextField::AddIrServer if !s.is_empty() => ir.accounts.push(IrAccount {
            url: s,
            ..Default::default()
        }),
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
