//! SYSTEM › STREAMING (desktop): Discord Rich Presence and OBS WebSocket.

use super::row::onoff;
use super::row::{Act, Row, Rows};
use super::TextField;
use crate::app::Ctx;
use crate::config::ObsScenes;
use crate::integrations::ObsPassword;
use crate::ir::creds;

/// A field's value as shown: `—` when empty.
fn shown(s: &str) -> String {
    if s.trim().is_empty() {
        "—".into()
    } else {
        s.to_string()
    }
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let d = &ctx.config.discord;
    let o = &ctx.config.obs;
    b.header(tr!("STREAMING"));
    b.extend([
        Row::adjust(
            "discord.enabled",
            tr!("Discord Rich Presence"),
            onoff(d.enabled),
            |c, _| c.config.discord.enabled ^= true,
        )
        .desc(tr!(
            "Your Discord status shows the screen and the chart (song select, playing title \
             [difficulty], result), through the Discord app on this computer."
        ))
        .keys("discord rich presence status activity"),
        Row::new(
            "discord.application_id",
            tr!("Discord application id"),
            shown(&d.application_id),
            Act::Text(TextField::DiscordAppId),
        )
        .desc(tr!(
            "The id of a Discord application (Discord's developer portal; its name is what \
             you are shown playing). None is built in: without one it stays off."
        ))
        .keys("discord client id"),
        Row::note(
            "info.discord",
            "Discord",
            ctx.streaming.discord_status(&ctx.config).label(),
        )
        .keys("discord"),
    ]);
    b.group(
        "system.obs",
        "OBS",
        tr!("Scene per screen and recording, through obs-websocket 5"),
    );
    b.extend([
        Row::adjust(
            "obs.enabled",
            tr!("OBS WebSocket"),
            onoff(o.enabled),
            |c, _| c.config.obs.enabled ^= true,
        )
        .desc(tr!(
            "Switch OBS scenes on each screen and record plays (OBS 28 or later: Tools › \
             WebSocket Server Settings)."
        ))
        .keys("obs websocket stream record scene"),
        Row::new(
            "obs.host",
            tr!("Host"),
            shown(&o.host),
            Act::Text(TextField::ObsHost),
        )
        .desc(if creds::loopback(&o.host) {
            tr!("The computer OBS runs on (127.0.0.1: this one).")
        } else {
            tr!(
                "OBS on another computer: the connection is not encrypted, and whoever \
                 answers at this address can try passwords against what is sent. Use \
                 127.0.0.1 unless you trust the network."
            )
        })
        .keys("obs address server"),
        Row::new(
            "obs.port",
            tr!("Port"),
            o.port.to_string(),
            Act::Text(TextField::ObsPort),
        )
        .desc(tr!("The WebSocket server's port (OBS: 4455)."))
        .keys("obs"),
        Row::new(
            "obs.password",
            tr!("Password"),
            match ctx.streaming.obs_password(o) {
                ObsPassword::None => "—",
                ObsPassword::Kept => "••••••",
                ObsPassword::Elsewhere => tr!("kept for another address: paste it again"),
            },
            Act::Text(TextField::ObsPassword),
        )
        .desc(tr!(
            "The WebSocket server's password (empty: none). Kept {where}, for this host \
             and port only.",
            where = creds::kept()
        ))
        .keys("obs authentication"),
    ]);
    for (i, label) in ObsScenes::LABELS.iter().enumerate() {
        b.push(
            Row::new(
                "obs.scenes",
                tr!("Scene: {label}", label = label),
                shown(o.scenes.get(i)),
                Act::Text(TextField::ObsScene(i)),
            )
            .desc(tr!(
                "The OBS scene to switch to on this screen (empty: no switch)."
            ))
            .keys("obs scene switch"),
        );
    }
    b.extend([
        Row::adjust(
            "obs.record_play",
            tr!("Record plays"),
            onoff(o.record_play),
            |c, _| c.config.obs.record_play ^= true,
        )
        .desc(tr!(
            "OBS starts recording as a play starts and stops once its result is left (a file \
             per play; a retry starts the next)."
        ))
        .keys("obs record recording video"),
        Row::note(
            "info.obs",
            "OBS",
            ctx.streaming.obs_status(&ctx.config).label(),
        )
        .keys("obs"),
    ]);
}

/// What a text field shows as its editing starts (never the password).
pub(super) fn current(ctx: &Ctx, tf: TextField) -> String {
    let c = &ctx.config;
    match tf {
        TextField::DiscordAppId => c.discord.application_id.clone(),
        TextField::ObsHost => c.obs.host.clone(),
        TextField::ObsPort => c.obs.port.to_string(),
        TextField::ObsScene(i) => c.obs.scenes.get(i).to_string(),
        _ => String::new(),
    }
}

/// A text field typed: `false` (with a message) when it does not read.
pub(super) fn commit(ctx: &mut Ctx, tf: TextField, s: String) -> bool {
    let c = &mut ctx.config;
    match tf {
        TextField::DiscordAppId => {
            if !s.is_empty() && !s.chars().all(|ch| ch.is_ascii_digit()) {
                ctx.toast(tr!("A Discord application id is a number"));
                return false;
            }
            c.discord.application_id = s;
        }
        TextField::ObsHost => c.obs.host = if s.is_empty() { "127.0.0.1".into() } else { s },
        TextField::ObsPort => match s.parse::<u16>() {
            Ok(p) if p > 0 => c.obs.port = p,
            _ => {
                ctx.toast(tr!("Not a port: {s}", s = s));
                return false;
            }
        },
        TextField::ObsPassword => {
            if let Err(e) = ctx.streaming.set_obs_password(&ctx.config.obs, s) {
                ctx.toast(tr!("Credentials: {e}", e = e));
            }
        }
        TextField::ObsScene(i) => *c.obs.scenes.get_mut(i) = s,
        _ => {}
    }
    true
}
