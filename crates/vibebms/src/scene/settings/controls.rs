//! CONTROLS: key and controller bindings (a sub-page per set), the
//! turntable's tuning, and the touch controls.

use super::row::{percent, Act, Row, Rows};
use super::{skins, Screen, Sub};
use crate::config::{
    KeyConfig, LaneKeys, PadId, PadSides, TouchMode, TouchSide, TouchStyle, LENGTH_RANGE,
    PERSPECTIVE_MAX, TOUCH_HEIGHT_RANGE,
};
use crate::input::gamepad::side_name;
use crate::scene::keybind::{key_label, keys_label, Slot};
use crate::scene::option_panel::{cycle, onoff};

/// The bindings, a sub-page each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum KeySet {
    /// Side `.0` (0: 1P, 1: 2P): its 7 keys, scratch and turntable axis.
    Side(usize),
    /// Side `.0`'s second keyboard key of each (KEYBOARD SUB).
    SideSub(usize),
    Nine,
    /// The 4 / 6 / 8 key modes' keys (their key count).
    Few(u8),
    StartSelect,
}

impl KeySet {
    pub(super) fn title(self) -> &'static str {
        match self {
            KeySet::Side(0) => "1P KEYS",
            KeySet::Side(_) => "2P KEYS",
            KeySet::SideSub(0) => "1P KEYBOARD SUB",
            KeySet::SideSub(_) => "2P KEYBOARD SUB",
            KeySet::Nine => "9-BUTTON KEYS",
            KeySet::Few(4) => "4-KEY KEYS",
            KeySet::Few(6) => "6-KEY KEYS",
            KeySet::Few(_) => "8-KEY KEYS",
            KeySet::StartSelect => "START / SELECT",
        }
    }
}

pub(super) fn style_label(s: TouchStyle) -> &'static str {
    match s {
        TouchStyle::Iidx => "BM (turntable + 7)",
        s => s.label(),
    }
}

fn start_select_label(s: TouchSide) -> &'static str {
    match s {
        TouchSide::Auto => "AUTO (scratch side)",
        s => s.label(),
    }
}

/// `v` moved `d` steps of `by` within `range`.
fn step(v: &mut f32, d: i32, by: f32, (lo, hi): (f32, f32)) {
    *v = (*v + d as f32 * by).clamp(lo, hi)
}

/// How many of `keys` have a key or button.
fn bound<'a>(keys: impl IntoIterator<Item = &'a LaneKeys>) -> String {
    let (mut n, mut of) = (0, 0);
    for k in keys {
        of += 1;
        n += (!k.keys.is_empty() || !k.pad.is_empty()) as usize;
    }
    format!("{n} of {of} bound")
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let c = &ctx.config;
    let k = &c.keys;
    let t = &c.touch;
    b.header("KEYBOARD AND CONTROLLERS");
    b.extend(skins::open_row(ctx, Screen::KeyConfig));
    for side in 0..2 {
        let sk = [&k.p1, &k.p2][side];
        let keys = sk
            .keys
            .iter()
            .take(7)
            .chain([&sk.scratch_up, &sk.scratch_down]);
        let (label, desc) = if side == 0 {
            (
                "1P keys",
                "Keys 1–7, scratch up / down and the turntable axis of side 1.",
            )
        } else {
            ("2P keys", "The same for side 2 (DP and VS).")
        };
        b.push(
            Row::sub(
                "keys.side",
                label,
                bound(keys),
                Sub::Keys(KeySet::Side(side)),
            )
            .desc(desc)
            .keys("bind controller turntable"),
        );
    }
    for side in 0..2 {
        let sk = [&k.p1, &k.p2][side];
        let n = sk
            .keys
            .iter()
            .take(7)
            .chain([&sk.scratch_up, &sk.scratch_down])
            .filter(|lk| lk.keys.len() > 1)
            .count();
        b.push(
            Row::sub(
                "keys.side_sub",
                format!("{}P keyboard sub", side + 1),
                format!("{n} of 9 bound"),
                Sub::Keys(KeySet::SideSub(side)),
            )
            .desc("A second keyboard key for each key and scratch of the side (KEYBOARD SUB).")
            .keys("keyboard second alternative bind"),
        );
    }
    pads(b);
    b.extend([
        Row::sub(
            "keys.pms",
            "9-button keys",
            bound(k.pms.iter().take(9)),
            Sub::Keys(KeySet::Nine),
        )
        .desc("Keys for 9-button (PMS) charts.")
        .keys("pop'n pms 9k bind"),
        Row::sub(
            "keys.few",
            "4 / 6 / 8-key keys",
            bound(k.k4.iter().chain(&k.k6).chain(&k.k8)),
            Sub::Keys(KeySet::Few(4)),
        )
        .desc(
            "Keys for Qwilight-style 4, 6 and 8 key charts (controllers also play them with their \
             7 key buttons and turntable).",
        )
        .keys("4k 6k 8k qwilight bind"),
        Row::sub(
            "keys.start_select",
            "START / SELECT",
            bound([&k.start, &k.select, &k.p2_start, &k.p2_select]),
            Sub::Keys(KeySet::StartSelect),
        )
        .desc("Option panels, and leaving a play (both sides).")
        .keys("bind"),
        Row::adjust(
            "keys.any_axis_is_scratch",
            "Any gamepad axis is the turntable",
            onoff(k.any_axis_is_scratch),
            |c, _| c.config.keys.any_axis_is_scratch ^= true,
        )
        .desc("Use any controller axis as the turntable, without binding it.")
        .keys("controller analog"),
    ]);
    b.header("TOUCH");
    b.extend([
        Row::adjust(
            "touch.mode",
            "Touch controls",
            format!("{:?}", t.mode).to_uppercase(),
            |c, d| {
                let all = [TouchMode::Auto, TouchMode::On, TouchMode::Off];
                c.config.touch.mode = cycle(&all, c.config.touch.mode, d)
            },
        )
        .desc("AUTO shows them on touch screens; ON / OFF forces them."),
        Row::adjust("touch.style", "Layout", style_label(t.style), |c, d| {
            c.config.touch.style = cycle(&TouchStyle::ALL, c.config.touch.style, d)
        })
        .desc("The touch key layout: turntable + 7, 5 keys, 9 buttons…")
        .keys("touch controller layout"),
        Row::new(
            "touch.layout_editor",
            "Edit the touch layout",
            "edit",
            Act::Open(Screen::TouchLayout),
        )
        .desc("Drag and resize the touch keys.")
        .keys("touch controller layout"),
        Row::adjust(
            "touch.landscape_height",
            "Controls height (landscape)",
            percent(t.landscape_height),
            |c, d| {
                let h = &mut c.config.touch.landscape_height;
                step(h, d, 0.01, crate::touch::CONTROLS_HEIGHT)
            },
        )
        .desc("How much of the screen the touch keys take in landscape."),
        Row::adjust(
            "touch.portrait_height",
            "Controls height (portrait)",
            percent(t.portrait_height),
            |c, d| {
                let h = &mut c.config.touch.portrait_height;
                step(h, d, 0.01, crate::touch::CONTROLS_HEIGHT)
            },
        )
        .desc("How much of the screen the touch keys take in portrait."),
        Row::adjust(
            "touch.screen_inches",
            "Screen size",
            match t.screen_inches {
                0.0 => format!(
                    "AUTO ({:.1}\")",
                    ctx.view.screen.guessed_inches(ctx.view.scale)
                ),
                n => format!("{n:.1}\""),
            },
            |c, d| {
                const SIZES: [f32; 19] = [
                    0.0, 5.4, 6.1, 6.7, 7.9, 8.3, 10.2, 10.9, 11.0, 12.9, 13.0, 13.3, 14.0, 15.6,
                    17.0, 24.0, 27.0, 32.0, 43.0,
                ];
                c.config.touch.screen_inches = cycle(&SIZES, c.config.touch.screen_inches, d);
            },
        )
        .desc("So keys are drawn at a real size; AUTO guesses from the screen.")
        .keys("inches real size"),
        Row::adjust("touch.opacity", "Opacity", percent(t.opacity), |c, d| {
            step(&mut c.config.touch.opacity, d, 0.05, (0.1, 1.0))
        })
        .desc("How see-through the touch keys are."),
        Row::adjust(
            "touch.start_select_side",
            "START / SELECT side",
            start_select_label(t.start_select_side),
            |c, d| {
                let s = &mut c.config.touch.start_select_side;
                *s = cycle(&TouchSide::ALL, *s, d)
            },
        )
        .desc("Which side the touch START and SELECT sit on."),
        Row::adjust(
            "touch.allow_dp",
            "Double play in landscape",
            onoff(t.allow_dp),
            |c, _| c.config.touch.allow_dp ^= true,
        )
        .desc("Allow 14-key and 10-key charts on a touch screen."),
        Row::adjust(
            "touch.scratch_threshold_deg",
            "Scratch sensitivity",
            format!("{:.0}°", t.scratch_threshold_deg),
            |c, d| {
                let v = &mut c.config.touch.scratch_threshold_deg;
                step(v, d, 1.0, (1.0, 45.0))
            },
        )
        .desc("How far a swipe turns the touch turntable, in degrees."),
    ]);
    b.group(
        "controls.turntable",
        "TURNTABLE AND KEYS",
        "Analog dead band and stop time, key chatter filter",
    );
    let tu = &k.tuning;
    b.extend([
        Row::adjust(
            "keys.tuning.turntable_dead_band",
            "Turntable dead band",
            format!("{:.3}", tu.turntable_dead_band as f32 / 1000.0),
            |c, d| {
                let v = &mut c.config.keys.tuning.turntable_dead_band;
                *v = v.saturating_add_signed(d).clamp(1, 50)
            },
        )
        .desc("Smaller analog turntable moves are no turn: higher is less sensitive.")
        .keys("analog controller axis"),
        Row::adjust(
            "keys.tuning.turntable_stop_ms",
            "Turntable stop time",
            format!("{} ms", tu.turntable_stop_ms),
            |c, d| {
                let v = &mut c.config.keys.tuning.turntable_stop_ms;
                *v = v.saturating_add_signed(d * 10).clamp(20, 500)
            },
        )
        .desc("An analog turntable still for this long is let go.")
        .keys("analog controller axis"),
        Row::adjust(
            "keys.tuning.chatter_ms",
            "Key chatter filter",
            match tu.chatter_ms {
                0 => "OFF".into(),
                n => format!("{n} ms"),
            },
            |c, d| {
                let v = &mut c.config.keys.tuning.chatter_ms;
                *v = v.saturating_add_signed(d).min(50)
            },
        )
        .desc("Drops a press this soon after the last of its lane (worn switches).")
        .keys("debounce"),
    ]);
    b.group(
        "controls.perspective",
        "TOUCH PERSPECTIVE",
        "Perspective, lane length, touch area height",
    );
    let persp = &c.perspective;
    b.extend([
        Row::adjust(
            "perspective.strength",
            "Perspective",
            percent(persp.strength),
            |c, d| {
                let v = &mut c.config.perspective.strength;
                step(v, d, 0.05, (0.0, PERSPECTIVE_MAX))
            },
        )
        .desc("How far the lanes lean back, in the touch perspective skin."),
        Row::adjust(
            "perspective.length",
            "Lane length",
            percent(persp.length),
            |c, d| step(&mut c.config.perspective.length, d, 0.05, LENGTH_RANGE),
        )
        .desc("The lanes' length, in the touch perspective skin."),
        Row::adjust(
            "perspective.touch_height",
            "Touch area height",
            percent(persp.touch_height),
            |c, d| {
                let v = &mut c.config.perspective.touch_height;
                step(v, d, 0.01, TOUCH_HEIGHT_RANGE)
            },
        )
        .desc("How far up the screen the lanes are played, in the touch perspective skin (the judge line is at 25 %)."),
    ]);
    b.group(
        "controls.reset",
        "RESET",
        "Every binding back to the defaults",
    );
    b.push(
        Row::new(
            "keys.reset",
            "Reset key config",
            "reset",
            Act::Run(|c, _| c.config.keys = KeyConfig::default(), 0),
        )
        .desc("Every binding back to the defaults."),
    );
}

/// A controller choice as shown: the name, `(not connected)` when it is
/// not.
fn pad_choice(ctx: &crate::app::Ctx, p: &Option<PadId>) -> String {
    match p {
        None => "AUTO (connection order)".into(),
        Some(p) if ctx.input.pads.iter().any(|c| c.pad == *p) => p.name.clone(),
        Some(p) => format!("{} (not connected)", p.name),
    }
}

/// Side `side`'s controller: AUTO, then the connected controllers (and the
/// one chosen if it is not connected), `d` steps on. A controller the
/// other side has changes sides with this side's.
fn step_pad(c: &mut crate::app::Ctx, side: usize, d: i32) {
    let mut all: Vec<Option<PadId>> = vec![None];
    let cur = c.config.keys.controllers.get(side).clone();
    for p in c
        .input
        .pads
        .iter()
        .map(|p| Some(p.pad.clone()))
        .chain([cur.clone()])
    {
        if !all.contains(&p) {
            all.push(p);
        }
    }
    let i = all.iter().position(|p| *p == cur).unwrap_or(0) as i32;
    let new = all[(i + d).rem_euclid(all.len() as i32) as usize].clone();
    let sides = &mut c.config.keys.controllers;
    if new.is_some() && *sides.get(1 - side) == new {
        *sides.get_mut(1 - side) = cur;
    }
    *sides.get_mut(side) = new;
}

/// Which controller is 1P and which 2P.
fn pads(b: &mut Rows) {
    let ctx = b.ctx;
    let ch = &ctx.config.keys.controllers;
    let connected = if ctx.input.pads.is_empty() {
        "none".to_string()
    } else {
        ctx.input
            .pads
            .iter()
            .map(|p| format!("{}: {}", side_name(p.number), p.pad.name))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    b.extend([
        Row::adjust(
            "keys.controllers.p1",
            "1P controller",
            pad_choice(ctx, &ch.p1),
            |c, d| step_pad(c, 0, d),
        )
        .desc("The controller for the 1P bindings (Pad0), kept by name and id; AUTO: connection order.")
        .keys("gamepad controller assign player side order"),
        Row::adjust(
            "keys.controllers.p2",
            "2P controller",
            pad_choice(ctx, &ch.p2),
            |c, d| step_pad(c, 1, d),
        )
        .desc(
            "The controller for the 2P bindings (Pad1); AUTO: the next one in connection order.",
        )
        .keys("gamepad controller assign player side order"),
        Row::new(
            "keys.controllers.swap",
            "Swap 1P and 2P controllers",
            "swap",
            Act::Run(
                |c, _| {
                    let pad = |n| {
                        c.input
                            .pads
                            .iter()
                            .find(|p| p.number == n)
                            .map(|p| p.pad.clone())
                    };
                    let (p1, p2) = (pad(1), pad(0));
                    c.config.keys.controllers = PadSides { p1, p2 };
                },
                0,
            ),
        )
        .desc("The 2P controller becomes 1P and the 1P one 2P (both then chosen by name).")
        .keys("gamepad controller assign player side"),
        Row::note("info.controllers", "Connected controllers", connected)
            .desc("Each connected controller and the side its buttons are.")
            .keys("gamepad controller"),
    ]);
}

/// CONTROLS › one set of bindings: Enter waits for a key or button.
pub(super) fn keys(b: &mut Rows, set: KeySet) {
    let k = &b.ctx.config.keys;
    let bind = |id, label: String, keys: &LaneKeys, s: Slot| {
        Row::new(id, label, keys_label(keys), Act::Bind(s))
            .desc("Enter, then press a key or button. Delete clears it.")
    };
    b.header(set.title());
    match set {
        KeySet::Side(side) => {
            let sk = [&k.p1, &k.p2][side];
            let n = side + 1;
            for (idx, lk) in sk.keys.iter().enumerate().take(7) {
                b.push(bind(
                    "keys.key",
                    format!("{n}P key {}", idx + 1),
                    lk,
                    Slot::Key { side, idx },
                ));
            }
            b.push(bind(
                "keys.scratch_up",
                format!("{n}P scratch ↑"),
                &sk.scratch_up,
                Slot::ScratchUp(side),
            ));
            b.push(bind(
                "keys.scratch_down",
                format!("{n}P scratch ↓"),
                &sk.scratch_down,
                Slot::ScratchDown(side),
            ));
            let axes: Vec<String> = sk
                .scratch_axis
                .iter()
                .map(|a| format!("Pad{}#{}", a / 1_000_000, a % 1_000_000))
                .collect();
            b.push(
                Row::new(
                    "keys.scratch_axis",
                    format!("{n}P turntable axis"),
                    if axes.is_empty() {
                        "—".into()
                    } else {
                        axes.join(", ")
                    },
                    Act::Bind(Slot::ScratchAxis(side)),
                )
                .desc("Enter, then spin the turntable. Delete clears it."),
            );
        }
        KeySet::SideSub(side) => {
            let sk = [&k.p1, &k.p2][side];
            let n = side + 1;
            let sub = |id, label: String, lk: &LaneKeys, s: Slot| {
                let v = lk.keys.get(1).map_or("—".into(), |c| key_label(*c));
                Row::new(id, label, v, Act::Bind(s))
                    .desc("Enter, then press a key. Delete clears it.")
            };
            for (idx, lk) in sk.keys.iter().enumerate().take(7) {
                b.push(sub(
                    "keys.key_sub",
                    format!("{n}P key {} (sub)", idx + 1),
                    lk,
                    Slot::Key { side, idx },
                ));
            }
            b.push(sub(
                "keys.scratch_up_sub",
                format!("{n}P scratch ↑ (sub)"),
                &sk.scratch_up,
                Slot::ScratchUp(side),
            ));
            b.push(sub(
                "keys.scratch_down_sub",
                format!("{n}P scratch ↓ (sub)"),
                &sk.scratch_down,
                Slot::ScratchDown(side),
            ));
        }
        KeySet::Nine => {
            for (i, lk) in k.pms.iter().enumerate().take(9) {
                b.push(bind(
                    "keys.pms",
                    format!("Button {}", i + 1),
                    lk,
                    Slot::Pms(i),
                ));
            }
        }
        KeySet::Few(_) => {
            for (n, keys) in [(4u8, &k.k4), (6, &k.k6), (8, &k.k8)] {
                for (i, lk) in keys.iter().enumerate() {
                    b.push(bind(
                        "keys.few",
                        format!("{n}K key {}", i + 1),
                        lk,
                        Slot::Few(n, i),
                    ));
                }
            }
        }
        KeySet::StartSelect => b.extend([
            bind("keys.start", "START".into(), &k.start, Slot::Start),
            bind("keys.select", "SELECT".into(), &k.select, Slot::Select),
            bind(
                "keys.p2_start",
                "2P START (VS)".into(),
                &k.p2_start,
                Slot::Start2,
            ),
            bind(
                "keys.p2_select",
                "2P SELECT (VS)".into(),
                &k.p2_select,
                Slot::Select2,
            ),
        ]),
    }
}
