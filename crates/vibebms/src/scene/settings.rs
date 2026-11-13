//! In-game settings: every option lives here (no external launcher).
//!
//! Keyboard: ↑↓ select, ←→ change, Enter activate, Esc back.
//! Mouse/touch: tap a category, tap the left/right half of a value to change it.

use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{
    HiSpeedMode, KeyConfig, LaneKeys, PresentMode, Side, TableConfig, TouchMode, TouchStyle,
};
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Gfx, Rect};
use crate::input::{InputEv, Phase};
use crate::platform::Instant;
use crate::theme::Theme;
use crate::ui;
use vibebms_audio::Bus;
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::LnType;
use winit::keyboard::KeyCode;

const CATEGORIES: [&str; 9] = [
    "PLAY",
    "JUDGE & GAUGE",
    "KEY CONFIG",
    "TOUCH",
    "DISPLAY",
    "AUDIO",
    "LIBRARY",
    "TABLES",
    "IR",
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextField {
    PlayerName,
    AddFolder,
    AddTable,
    IrUrl,
    IrName,
    IrToken,
}

/// Which binding a key-config row edits.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Key { side: usize, idx: usize },
    ScratchUp(usize),
    ScratchDown(usize),
    ScratchAxis(usize),
    Pms(usize),
    Start,
    Select,
}

#[derive(Clone, Copy)]
enum Act {
    None,
    Adjust(fn(&mut Ctx, i32)),
    Text(TextField),
    Bind(Slot),
    RemoveFolder(usize),
    ToggleTable(usize),
    RemoveTable(usize),
    Rescan,
    RefreshTables,
    ResetKeys,
    RestoreDefaultTables,
    BrowseFolder,
    /// Cycle custom option `.1` of the skin chosen for `SKIN_KEYS[.0]`.
    SkinOption(usize, usize),
    /// Adjust field `.2` (x, y, w, h, angle, alpha) of offset `.1` of that skin.
    SkinOffset(usize, usize, usize),
}

/// `config.skins` keys, in menu order.
const SKIN_KEYS: [&str; 7] = [
    "7KEYS", "5KEYS", "14KEYS", "10KEYS", "9KEYS", "SELECT", "RESULT",
];

/// Path and custom options of the skin configured for `SKIN_KEYS[slot]`.
fn skin_options_of(ctx: &Ctx, slot: usize) -> Option<(String, Vec<crate::skin::CustomOption>)> {
    let value = ctx.config.skins.get(SKIN_KEYS[slot])?.clone();
    let path = super::play_skin::configured_path(ctx, &value)?;
    Some((value, ctx.skin_custom_options(&path)))
}

fn cycle_skin_option(ctx: &mut Ctx, slot: usize, idx: usize, d: i32) {
    let Some((value, opts)) = skin_options_of(ctx, slot) else {
        return;
    };
    let Some(o) = opts.get(idx) else { return };
    if o.labels.is_empty() {
        return;
    }
    let cur = o.chosen(
        ctx.config
            .skin_options
            .get(&value)
            .and_then(|m| m.get(&o.name))
            .map(String::as_str),
    );
    let n = o.labels.len() as i32;
    let next = (cur as i32 + d).rem_euclid(n) as usize;
    ctx.config
        .skin_options
        .entry(value)
        .or_default()
        .insert(o.name.clone(), o.labels[next].clone());
}

/// Path and adjustable offsets of the skin configured for `SKIN_KEYS[slot]`.
fn skin_offsets_of(ctx: &Ctx, slot: usize) -> Option<(String, Vec<crate::skin::CustomOffset>)> {
    let value = ctx.config.skins.get(SKIN_KEYS[slot])?.clone();
    let path = super::play_skin::configured_path(ctx, &value)?;
    Some((value, ctx.skin_custom_offsets(&path)))
}

fn adjust_skin_offset(ctx: &mut Ctx, slot: usize, idx: usize, field: usize, d: i32) {
    let Some((value, offs)) = skin_offsets_of(ctx, slot) else {
        return;
    };
    let Some(o) = offs.get(idx) else { return };
    let v = ctx
        .config
        .skin_offsets
        .entry(value)
        .or_default()
        .entry(o.name.clone())
        .or_default();
    // alpha in 0..255 steps of 5, angle in degrees, the rest in pixels
    let step = if field == 5 { 5 } else { 1 };
    v[field] = (v[field] + d * step).clamp(-2000, 2000);
    if field == 5 {
        v[5] = v[5].clamp(-255, 255);
    }
}

struct Item {
    label: String,
    value: String,
    act: Act,
}

fn it(label: impl Into<String>, value: impl Into<String>, f: fn(&mut Ctx, i32)) -> Item {
    Item {
        label: label.into(),
        value: value.into(),
        act: Act::Adjust(f),
    }
}

fn cyc<T: Copy + PartialEq>(all: &[T], v: T, d: i32) -> T {
    let i = all.iter().position(|x| *x == v).unwrap_or(0) as i32;
    all[(i + d).rem_euclid(all.len() as i32) as usize]
}

fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
}

fn keys_label(lk: &LaneKeys) -> String {
    let mut v: Vec<String> = lk.keys.iter().map(|k| format!("{k:?}")).collect();
    v.extend(
        lk.pad
            .iter()
            .map(|p| format!("Pad{}#{}", p / 1_000_000, p % 1_000_000)),
    );
    if v.is_empty() {
        "—".into()
    } else {
        v.join(", ")
    }
}

fn slot_keys(k: &mut KeyConfig, s: Slot) -> Option<&mut LaneKeys> {
    fn side(k: &mut KeyConfig, i: usize) -> &mut crate::config::SideKeys {
        if i == 0 {
            &mut k.p1
        } else {
            &mut k.p2
        }
    }
    Some(match s {
        Slot::Key { side: sd, idx } => &mut side(k, sd).keys[idx],
        Slot::ScratchUp(sd) => &mut side(k, sd).scratch_up,
        Slot::ScratchDown(sd) => &mut side(k, sd).scratch_down,
        Slot::Pms(i) => &mut k.pms[i],
        Slot::Start => &mut k.start,
        Slot::Select => &mut k.select,
        Slot::ScratchAxis(_) => return None,
    })
}

fn items(ctx: &Ctx, cat: usize) -> Vec<Item> {
    let c = &ctx.config;
    let p = &c.play;
    match cat {
        0 => vec![
            Item {
                label: "Player name".into(),
                value: c.player_name.clone(),
                act: Act::Text(TextField::PlayerName),
            },
            it("Random (1P)", p.random_1p.label(), |c, d| {
                c.config.play.random_1p = cyc(&LaneOption::ALL, c.config.play.random_1p, d)
            }),
            it("Random (2P)", p.random_2p.label(), |c, d| {
                c.config.play.random_2p = cyc(&LaneOption::ALL, c.config.play.random_2p, d)
            }),
            it(
                "DP option",
                format!("{:?}", p.dp_option).to_uppercase(),
                |c, d| {
                    c.config.play.dp_option = cyc(
                        &[DoubleOption::Off, DoubleOption::Flip],
                        c.config.play.dp_option,
                        d,
                    )
                },
            ),
            it("Battle (SP on both sides)", onoff(p.battle), |c, _| {
                c.config.play.battle = !c.config.play.battle
            }),
            it("Play side", format!("{:?}", p.side), |c, _| {
                c.config.play.side = if c.config.play.side == Side::P1 {
                    Side::P2
                } else {
                    Side::P1
                }
            }),
            it(
                "LN mode (undefined charts)",
                format!("{:?}", p.ln_mode).to_uppercase(),
                |c, d| {
                    c.config.play.ln_mode = cyc(
                        &[LnType::Ln, LnType::Cn, LnType::Hcn],
                        c.config.play.ln_mode,
                        d,
                    )
                },
            ),
            it(
                "Hi-speed mode",
                format!("{:?}", p.hispeed_mode).to_uppercase(),
                |c, d| {
                    c.config.play.hispeed_mode = cyc(
                        &[HiSpeedMode::Floating, HiSpeedMode::Constant],
                        c.config.play.hispeed_mode,
                        d,
                    )
                },
            ),
            it(
                "Duration (ms) / green number",
                format!("{}  (GN {})", p.duration_ms, p.duration_ms * 3 / 5),
                |c, d| {
                    c.config.play.duration_ms =
                        (c.config.play.duration_ms as i32 + d * 10).clamp(100, 5000) as u32
                },
            ),
            it("Lane cover", onoff(p.lane_cover), |c, _| {
                c.config.play.lane_cover = !c.config.play.lane_cover
            }),
            it("SUDDEN+", format!("{:.0}%", p.sudden * 100.0), |c, d| {
                c.config.play.sudden = (c.config.play.sudden + d as f32 * 0.01).clamp(0.0, 0.9)
            }),
            it("HIDDEN+", format!("{:.0}%", p.hidden * 100.0), |c, d| {
                c.config.play.hidden = (c.config.play.hidden + d as f32 * 0.01).clamp(0.0, 0.9)
            }),
            it("LIFT", format!("{:.0}%", p.lift * 100.0), |c, d| {
                c.config.play.lift = (c.config.play.lift + d as f32 * 0.01).clamp(0.0, 0.6)
            }),
            it(
                "Judge offset (ms, + = you hit late)",
                format!("{:+}", p.judge_offset_ms),
                |c, d| {
                    c.config.play.judge_offset_ms =
                        (c.config.play.judge_offset_ms + d).clamp(-500, 500)
                },
            ),
            it(
                "Visual offset (ms)",
                format!("{:+}", p.visual_offset_ms),
                |c, d| {
                    c.config.play.visual_offset_ms =
                        (c.config.play.visual_offset_ms + d).clamp(-500, 500)
                },
            ),
            it("Auto scratch (assist)", onoff(p.auto_scratch), |c, _| {
                c.config.play.auto_scratch = !c.config.play.auto_scratch
            }),
            it("Legacy note (assist)", onoff(p.legacy_note), |c, _| {
                c.config.play.legacy_note = !c.config.play.legacy_note
            }),
            it("Remove mines", onoff(p.no_mine), |c, _| {
                c.config.play.no_mine = !c.config.play.no_mine
            }),
            it("Autoplay", onoff(p.autoplay), |c, _| {
                c.config.play.autoplay = !c.config.play.autoplay
            }),
            it("BGA", onoff(p.bga), |c, _| {
                c.config.play.bga = !c.config.play.bga
            }),
            it("FAST/SLOW display", onoff(p.show_fast_slow), |c, _| {
                c.config.play.show_fast_slow = !c.config.play.show_fast_slow
            }),
            it(
                "H-RANDOM threshold (BPM)",
                format!("{}", p.hran_threshold_bpm),
                |c, d| {
                    c.config.play.hran_threshold_bpm =
                        (c.config.play.hran_threshold_bpm as i32 + d * 10).clamp(60, 400) as u32
                },
            ),
        ],
        1 => vec![
            it("Gauge", p.gauge.label(), |c, d| {
                c.config.play.gauge = cyc(&GaugeType::SELECTABLE, c.config.play.gauge, d)
            }),
            it("Gauge auto shift", p.gauge_auto_shift.label(), |c, d| {
                c.config.play.gauge_auto_shift =
                    cyc(&GaugeAutoShift::ALL, c.config.play.gauge_auto_shift, d)
            }),
            it("Gauge rules", p.gauge_preset.label(), |c, d| {
                c.config.play.gauge_preset = cyc(&GaugePreset::ALL, c.config.play.gauge_preset, d)
            }),
            it("Judge windows", p.judge_preset.label(), |c, d| {
                c.config.play.judge_preset = cyc(&JudgePreset::ALL, c.config.play.judge_preset, d)
            }),
            it(
                "Judge algorithm",
                format!("{:?}", p.judge_algorithm).to_uppercase(),
                |c, d| {
                    c.config.play.judge_algorithm = cyc(
                        &[
                            JudgeAlgorithm::Combo,
                            JudgeAlgorithm::Duration,
                            JudgeAlgorithm::Lowest,
                        ],
                        c.config.play.judge_algorithm,
                        d,
                    )
                },
            ),
            it(
                "Custom judge windows",
                onoff(p.custom_judge.is_some()),
                |c, _| {
                    c.config.play.custom_judge = if c.config.play.custom_judge.is_some() {
                        None
                    } else {
                        Some([100, 100, 100])
                    }
                },
            ),
            it(
                "  PGREAT window %",
                p.custom_judge
                    .map(|j| j[0].to_string())
                    .unwrap_or("—".into()),
                |c, d| {
                    if let Some(j) = c.config.play.custom_judge.as_mut() {
                        j[0] = (j[0] + d as i64 * 5).clamp(10, 400)
                    }
                },
            ),
            it(
                "  GREAT window %",
                p.custom_judge
                    .map(|j| j[1].to_string())
                    .unwrap_or("—".into()),
                |c, d| {
                    if let Some(j) = c.config.play.custom_judge.as_mut() {
                        j[1] = (j[1] + d as i64 * 5).clamp(10, 400)
                    }
                },
            ),
            it(
                "  GOOD window %",
                p.custom_judge
                    .map(|j| j[2].to_string())
                    .unwrap_or("—".into()),
                |c, d| {
                    if let Some(j) = c.config.play.custom_judge.as_mut() {
                        j[2] = (j[2] + d as i64 * 5).clamp(10, 400)
                    }
                },
            ),
        ],
        2 => {
            let k = &c.keys;
            let mut v = Vec::new();
            for side in 0..2 {
                let sk = if side == 0 { &k.p1 } else { &k.p2 };
                for (i, lk) in sk.keys.iter().enumerate().take(7) {
                    v.push(Item {
                        label: format!("{}P key {}", side + 1, i + 1),
                        value: keys_label(lk),
                        act: Act::Bind(Slot::Key { side, idx: i }),
                    });
                }
                v.push(Item {
                    label: format!("{}P scratch ↑", side + 1),
                    value: keys_label(&sk.scratch_up),
                    act: Act::Bind(Slot::ScratchUp(side)),
                });
                v.push(Item {
                    label: format!("{}P scratch ↓", side + 1),
                    value: keys_label(&sk.scratch_down),
                    act: Act::Bind(Slot::ScratchDown(side)),
                });
                let ax: Vec<String> = sk
                    .scratch_axis
                    .iter()
                    .map(|a| format!("Pad{}#{}", a / 1_000_000, a % 1_000_000))
                    .collect();
                v.push(Item {
                    label: format!("{}P turntable axis", side + 1),
                    value: if ax.is_empty() {
                        "—".into()
                    } else {
                        ax.join(", ")
                    },
                    act: Act::Bind(Slot::ScratchAxis(side)),
                });
            }
            for (i, lk) in k.pms.iter().enumerate().take(9) {
                v.push(Item {
                    label: format!("9K button {}", i + 1),
                    value: keys_label(lk),
                    act: Act::Bind(Slot::Pms(i)),
                });
            }
            v.push(Item {
                label: "START".into(),
                value: keys_label(&k.start),
                act: Act::Bind(Slot::Start),
            });
            v.push(Item {
                label: "SELECT".into(),
                value: keys_label(&k.select),
                act: Act::Bind(Slot::Select),
            });
            v.push(it(
                "Any gamepad axis = turntable",
                onoff(k.any_axis_is_scratch),
                |c, _| c.config.keys.any_axis_is_scratch = !c.config.keys.any_axis_is_scratch,
            ));
            v.push(Item {
                label: "Reset key config".into(),
                value: "↵".into(),
                act: Act::ResetKeys,
            });
            v
        }
        3 => vec![
            it(
                "Touch controls",
                format!("{:?}", c.touch.mode).to_uppercase(),
                |c, d| {
                    c.config.touch.mode = cyc(
                        &[TouchMode::Auto, TouchMode::On, TouchMode::Off],
                        c.config.touch.mode,
                        d,
                    )
                },
            ),
            it(
                "Layout",
                match c.touch.style {
                    TouchStyle::Iidx => "IIDX (turntable + 7)",
                    TouchStyle::Keyboard => "KEYBOARD",
                },
                |c, d| {
                    c.config.touch.style = cyc(
                        &[TouchStyle::Iidx, TouchStyle::Keyboard],
                        c.config.touch.style,
                        d,
                    )
                },
            ),
            it(
                "Controls height",
                format!("{:.0}%", c.touch.height * 100.0),
                |c, d| {
                    c.config.touch.height =
                        (c.config.touch.height + d as f32 * 0.01).clamp(0.12, 0.6)
                },
            ),
            it(
                "Opacity",
                format!("{:.0}%", c.touch.opacity * 100.0),
                |c, d| {
                    c.config.touch.opacity =
                        (c.config.touch.opacity + d as f32 * 0.05).clamp(0.1, 1.0)
                },
            ),
            it(
                "Double play in landscape",
                onoff(c.touch.allow_dp),
                |c, _| c.config.touch.allow_dp = !c.config.touch.allow_dp,
            ),
            it(
                "Scratch sensitivity (deg)",
                format!("{:.0}", c.touch.scratch_threshold_deg),
                |c, d| {
                    c.config.touch.scratch_threshold_deg =
                        (c.config.touch.scratch_threshold_deg + d as f32).clamp(1.0, 45.0)
                },
            ),
            it("Play side (1P/2P)", format!("{:?}", p.side), |c, _| {
                c.config.play.side = if c.config.play.side == Side::P1 {
                    Side::P2
                } else {
                    Side::P1
                }
            }),
        ],
        4 => {
            let themes = Theme::list(&ctx.paths.themes);
            let mut v = vec![
                it("Fullscreen (F11)", onoff(c.display.fullscreen), |c, _| {
                    c.config.display.fullscreen = !c.config.display.fullscreen;
                    c.want_fullscreen = Some(c.config.display.fullscreen);
                }),
                it(
                    "Present mode",
                    format!("{:?}", c.display.present_mode).to_uppercase(),
                    |c, d| {
                        c.config.display.present_mode = cyc(
                            &[
                                PresentMode::LowLatency,
                                PresentMode::VSync,
                                PresentMode::Immediate,
                            ],
                            c.config.display.present_mode,
                            d,
                        );
                        c.want_present_mode = true;
                    },
                ),
                it(
                    "Max FPS (0 = unlimited)",
                    format!("{}", c.display.max_fps),
                    |c, d| {
                        let steps = [0u32, 60, 75, 120, 144, 165, 240, 360, 480, 1000];
                        c.config.display.max_fps = cyc(&steps, c.config.display.max_fps, d)
                    },
                ),
                it(
                    "UI scale",
                    format!("{:.0}%", c.display.ui_scale * 100.0),
                    |c, d| {
                        c.config.display.ui_scale =
                            (c.config.display.ui_scale + d as f32 * 0.05).clamp(0.5, 3.0)
                    },
                ),
                it("Show FPS", onoff(c.display.show_fps), |c, _| {
                    c.config.display.show_fps = !c.config.display.show_fps
                }),
                Item {
                    label: "Theme".into(),
                    value: c.theme.clone(),
                    act: if themes.len() > 1 {
                        Act::Adjust(|c, d| {
                            let list = Theme::list(&c.paths.themes);
                            let i =
                                list.iter().position(|t| *t == c.config.theme).unwrap_or(0) as i32;
                            c.config.theme =
                                list[(i + d).rem_euclid(list.len() as i32) as usize].clone();
                            c.want_theme_reload = true;
                        })
                    } else {
                        Act::None
                    },
                },
                Item {
                    label: "GPU".into(),
                    value: ctx.gpu_name.clone(),
                    act: Act::None,
                },
            ];
            // built-in default skin / skins in <root>/skins
            type Adjust = fn(&mut Ctx, i32);
            const MODES: [(&str, Adjust); 7] = [
                ("7KEYS", |c, d| cycle_skin(c, d, "7KEYS")),
                ("5KEYS", |c, d| cycle_skin(c, d, "5KEYS")),
                ("14KEYS", |c, d| cycle_skin(c, d, "14KEYS")),
                ("10KEYS", |c, d| cycle_skin(c, d, "10KEYS")),
                ("9KEYS", |c, d| cycle_skin(c, d, "9KEYS")),
                ("SELECT", |c, d| cycle_skin(c, d, "SELECT")),
                ("RESULT", |c, d| cycle_skin(c, d, "RESULT")),
            ];
            for (slot, (m, f)) in MODES.into_iter().enumerate() {
                let cur = match c.skins.get(m).map(String::as_str).unwrap_or("") {
                    "" => "DEFAULT".into(),
                    p => skin_label(ctx, p),
                };
                let label = if m.ends_with("KEYS") {
                    format!("Play skin ({m})")
                } else {
                    format!("{}{} skin", &m[..1], m[1..].to_lowercase())
                };
                v.push(it(label, cur, f));
                // the skin's own options, right under it
                if let Some((value, opts)) = skin_options_of(ctx, slot) {
                    let chosen = c.skin_options.get(&value);
                    for (i, o) in opts.iter().enumerate() {
                        let k = o.chosen(chosen.and_then(|m| m.get(&o.name)).map(String::as_str));
                        v.push(Item {
                            label: format!("    {}", o.name),
                            value: o.labels.get(k).cloned().unwrap_or_default(),
                            act: Act::SkinOption(slot, i),
                        });
                    }
                }
                // and its position adjustments (beatoraja offsets)
                if let Some((value, offs)) = skin_offsets_of(ctx, slot) {
                    let set = c.skin_offsets.get(&value);
                    for (i, o) in offs.iter().enumerate() {
                        let cur = set
                            .and_then(|m| m.get(&o.name))
                            .copied()
                            .unwrap_or_default();
                        for (f, on) in o.fields.iter().enumerate() {
                            if *on {
                                v.push(Item {
                                    label: format!(
                                        "    {} {}",
                                        o.name,
                                        crate::skin::OFFSET_FIELDS[f]
                                    ),
                                    value: cur[f].to_string(),
                                    act: Act::SkinOffset(slot, i, f),
                                });
                            }
                        }
                    }
                }
            }
            v
        }
        5 => vec![
            it(
                "Master volume",
                format!("{:.0}%", c.audio.master * 100.0),
                |c, d| {
                    c.config.audio.master =
                        (c.config.audio.master + d as f32 * 0.05).clamp(0.0, 1.0);
                    c.mixer.set_master(c.config.audio.master);
                },
            ),
            it(
                "Keysound volume",
                format!("{:.0}%", c.audio.key * 100.0),
                |c, d| {
                    c.config.audio.key = (c.config.audio.key + d as f32 * 0.05).clamp(0.0, 1.0);
                    c.mixer.set_bus_volume(Bus::Key, c.config.audio.key);
                },
            ),
            it(
                "BGM volume",
                format!("{:.0}%", c.audio.bgm * 100.0),
                |c, d| {
                    c.config.audio.bgm = (c.config.audio.bgm + d as f32 * 0.05).clamp(0.0, 1.0);
                    c.mixer.set_bus_volume(Bus::Bgm, c.config.audio.bgm);
                },
            ),
            it(
                "System volume",
                format!("{:.0}%", c.audio.system * 100.0),
                |c, d| {
                    c.config.audio.system =
                        (c.config.audio.system + d as f32 * 0.05).clamp(0.0, 1.0);
                    c.mixer.set_bus_volume(Bus::System, c.config.audio.system);
                },
            ),
            it(
                "Buffer size (restart)",
                format!("{} frames", c.audio.buffer_frames),
                |c, d| {
                    c.config.audio.buffer_frames = cyc(
                        &[0, 64, 128, 256, 512, 1024, 2048],
                        c.config.audio.buffer_frames,
                        d,
                    )
                },
            ),
            it("Song preview", onoff(c.audio.preview), |c, _| {
                c.config.audio.preview = !c.config.audio.preview
            }),
            Item {
                label: "Output".into(),
                value: format!(
                    "{} {} Hz, {:.1} ms",
                    ctx.mixer.device_name,
                    ctx.mixer.sample_rate,
                    ctx.mixer.latency_us() as f64 / 1000.0
                ),
                act: Act::None,
            },
        ],
        6 => {
            let mut v = vec![
                Item {
                    label: "Data folder (portable)".into(),
                    value: ctx.paths.root.display().to_string(),
                    act: Act::None,
                },
                it(
                    "Scan the data folder for BMS",
                    onoff(c.scan_root),
                    |c, _| c.config.scan_root = !c.config.scan_root,
                ),
            ];
            for (i, d) in c.bms_dirs.iter().enumerate() {
                v.push(Item {
                    label: format!("Folder: {d}"),
                    value: "remove ×".into(),
                    act: Act::RemoveFolder(i),
                });
            }
            if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
                v.push(Item {
                    label: "Browse for a BMS folder…".into(),
                    value: "📂".into(),
                    act: Act::BrowseFolder,
                });
            }
            v.push(Item {
                label: "Add BMS folder… (or drop a folder on the window)".into(),
                value: "+".into(),
                act: Act::Text(TextField::AddFolder),
            });
            v.push(Item {
                label: "Rescan library".into(),
                value: format!("{} charts", ctx.songs.len()),
                act: Act::Rescan,
            });
            v
        }
        7 => {
            let mut v = Vec::new();
            for (i, t) in c.tables.iter().enumerate() {
                let loaded = ctx
                    .tables
                    .iter()
                    .find(|d| d.source_url == t.url)
                    .map(|d| format!(" [{}]", d.entries.len()))
                    .unwrap_or_default();
                v.push(Item {
                    label: format!(
                        "{}{loaded}",
                        if t.name.is_empty() { &t.url } else { &t.name }
                    ),
                    value: onoff(t.enabled).into(),
                    act: Act::ToggleTable(i),
                });
                v.push(Item {
                    label: format!("    {}", t.url),
                    value: "remove ×".into(),
                    act: Act::RemoveTable(i),
                });
            }
            v.push(Item {
                label: "Add table URL…".into(),
                value: "+".into(),
                act: Act::Text(TextField::AddTable),
            });
            v.push(Item {
                label: "Update all tables".into(),
                value: "↻".into(),
                act: Act::RefreshTables,
            });
            v.push(Item {
                label: "Restore default tables".into(),
                value: "↵".into(),
                act: Act::RestoreDefaultTables,
            });
            v
        }
        8 => {
            let ir = &c.ir_server;
            vec![
                it("Internet ranking", onoff(ir.enabled), |c, _| {
                    c.config.ir_server.enabled = !c.config.ir_server.enabled
                }),
                Item {
                    label: "Name".into(),
                    value: ir.name.clone(),
                    act: Act::Text(TextField::IrName),
                },
                Item {
                    label: "URL".into(),
                    value: ir.url.clone(),
                    act: Act::Text(TextField::IrUrl),
                },
                Item {
                    label: "API token".into(),
                    value: if ir.token.is_empty() {
                        "—".into()
                    } else {
                        "••••••••".into()
                    },
                    act: Act::Text(TextField::IrToken),
                },
                it("Auto submit scores", onoff(ir.auto_submit), |c, _| {
                    c.config.ir_server.auto_submit = !c.config.ir_server.auto_submit
                }),
                Item {
                    label: "Protocol: docs/IR_PROTOCOL.md".into(),
                    value: String::new(),
                    act: Act::None,
                },
            ]
        }
        _ => Vec::new(),
    }
}

pub struct SettingsScene {
    cat: usize,
    sel: usize,
    focus_items: bool,
    edit: Option<(TextField, String)>,
    learning: Option<(Slot, Instant)>,
    cat_rects: Vec<(Rect, usize)>,
    item_rects: Vec<(Rect, usize)>,
    scroll: f32,
    axis_seen: std::collections::HashMap<u64, f32>,
    back_rect: Rect,
}

impl SettingsScene {
    pub fn new(_ctx: &mut Ctx) -> SettingsScene {
        SettingsScene {
            cat: 0,
            sel: 0,
            focus_items: false,
            edit: None,
            learning: None,
            cat_rects: Vec::new(),
            item_rects: Vec::new(),
            scroll: 0.0,
            axis_seen: Default::default(),
            back_rect: Rect::default(),
        }
    }

    fn activate(&mut self, ctx: &mut Ctx, d: i32, enter: bool) {
        let list = items(ctx, self.cat);
        let Some(item) = list.get(self.sel) else {
            return;
        };
        match item.act {
            Act::None => return,
            Act::Adjust(f) => f(ctx, if d == 0 { 1 } else { d }),
            Act::SkinOption(slot, i) => cycle_skin_option(ctx, slot, i, if d == 0 { 1 } else { d }),
            Act::SkinOffset(slot, i, f) => {
                adjust_skin_offset(ctx, slot, i, f, if d == 0 { 1 } else { d })
            }
            Act::Text(tf) if enter || d != 0 => {
                let cur = match tf {
                    TextField::PlayerName => ctx.config.player_name.clone(),
                    TextField::IrUrl => ctx.config.ir_server.url.clone(),
                    TextField::IrName => ctx.config.ir_server.name.clone(),
                    _ => String::new(),
                };
                self.edit = Some((tf, cur));
                return;
            }
            Act::Text(_) => return,
            Act::Bind(slot) => {
                if enter || d != 0 {
                    self.learning = Some((slot, Instant::now()));
                }
                return;
            }
            Act::RemoveFolder(i) => {
                if i < ctx.config.bms_dirs.len() {
                    ctx.config.bms_dirs.remove(i);
                    ctx.start_scan();
                }
            }
            Act::ToggleTable(i) => {
                if let Some(t) = ctx.config.tables.get_mut(i) {
                    t.enabled = !t.enabled;
                }
                ctx.load_cached_tables();
                ctx.refresh_tables(false);
            }
            Act::RemoveTable(i) => {
                if i < ctx.config.tables.len() {
                    ctx.config.tables.remove(i);
                    ctx.load_cached_tables();
                }
            }
            Act::Rescan => ctx.start_scan(),
            Act::BrowseFolder => {
                #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
                if let Some(p) = rfd::FileDialog::new().set_title("BMS folder").pick_folder() {
                    let s = p.to_string_lossy().into_owned();
                    if !ctx.config.bms_dirs.contains(&s) {
                        ctx.config.bms_dirs.push(s);
                        ctx.start_scan();
                    }
                }
            }
            Act::RefreshTables => ctx.refresh_tables(true),
            Act::ResetKeys => ctx.config.keys = KeyConfig::default(),
            Act::RestoreDefaultTables => {
                for (n, u) in vibebms_tables::DEFAULT_TABLES {
                    if !ctx.config.tables.iter().any(|t| t.url == *u) {
                        ctx.config.tables.push(TableConfig {
                            name: n.to_string(),
                            url: u.to_string(),
                            enabled: true,
                        });
                    }
                }
                ctx.load_cached_tables();
                ctx.refresh_tables(false);
            }
        }
        ctx.save_config();
    }

    fn commit_text(&mut self, ctx: &mut Ctx, tf: TextField, s: String) {
        let s = s.trim().to_string();
        match tf {
            TextField::PlayerName => ctx.config.player_name = s,
            TextField::AddFolder => {
                if !s.is_empty() {
                    if !std::path::Path::new(&s).is_dir() && !ctx.paths.root.join(&s).is_dir() {
                        ctx.toast(format!("Not a folder: {s}"));
                        return;
                    }
                    ctx.config.bms_dirs.push(s);
                    ctx.start_scan();
                }
            }
            TextField::AddTable => {
                if !s.is_empty() {
                    ctx.config.tables.push(TableConfig {
                        name: String::new(),
                        url: s,
                        enabled: true,
                    });
                    ctx.refresh_tables(false);
                }
            }
            TextField::IrUrl => ctx.config.ir_server.url = s,
            TextField::IrName => ctx.config.ir_server.name = s,
            TextField::IrToken => ctx.config.ir_server.token = s,
        }
        ctx.save_config();
    }

    fn learn_key(
        &mut self,
        ctx: &mut Ctx,
        key: Option<KeyCode>,
        pad: Option<u64>,
        axis: Option<u64>,
    ) {
        let Some((slot, _)) = self.learning.take() else {
            return;
        };
        if let Slot::ScratchAxis(side) = slot {
            if let Some(a) = axis {
                let sk = if side == 0 {
                    &mut ctx.config.keys.p1
                } else {
                    &mut ctx.config.keys.p2
                };
                sk.scratch_axis = vec![a];
            }
        } else if let Some(lk) = slot_keys(&mut ctx.config.keys, slot) {
            if let Some(k) = key {
                lk.keys = vec![k];
            }
            if let Some(p) = pad {
                lk.pad = vec![p];
            }
        }
        ctx.save_config();
        // advance to the next row for quick setup
        let n = items(ctx, self.cat).len();
        self.sel = (self.sel + 1).min(n.saturating_sub(1));
    }
}

impl Scene for SettingsScene {
    fn update(&mut self, _ctx: &mut Ctx, _gfx: &mut Gfx, _now: Instant) -> Next {
        Next::Stay
    }

    fn input(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
        // key learning
        if self.learning.is_some() {
            match ev {
                InputEv::Key {
                    code: KeyCode::Escape,
                    pressed: true,
                    ..
                } => self.learning = None,
                InputEv::Key {
                    code: KeyCode::Delete,
                    pressed: true,
                    ..
                } => {
                    if let Some((slot, _)) = self.learning.take() {
                        if let Some(lk) = slot_keys(&mut ctx.config.keys, slot) {
                            *lk = LaneKeys::default();
                        } else if let Slot::ScratchAxis(s) = slot {
                            (if s == 0 {
                                &mut ctx.config.keys.p1
                            } else {
                                &mut ctx.config.keys.p2
                            })
                            .scratch_axis
                            .clear();
                        }
                        ctx.save_config();
                    }
                }
                InputEv::Key {
                    code,
                    pressed: true,
                    repeat: false,
                    ..
                } => self.learn_key(ctx, Some(*code), None, None),
                InputEv::Pad {
                    id, pressed: true, ..
                } => self.learn_key(ctx, None, Some(*id), None),
                InputEv::Axis { id, value, .. } => {
                    let first = *self.axis_seen.entry(*id).or_insert(*value);
                    if (value - first).abs() > 0.3 {
                        self.axis_seen.clear();
                        self.learn_key(ctx, None, None, Some(*id));
                    }
                }
                InputEv::Pointer {
                    phase: Phase::Down, ..
                } => self.learning = None,
                _ => {}
            }
            return Next::Stay;
        }
        // text editing
        if let Some((tf, s)) = self.edit.as_mut() {
            if let InputEv::Key {
                code,
                pressed: true,
                text,
                ..
            } = ev
            {
                match code {
                    KeyCode::Escape => self.edit = None,
                    KeyCode::Enter | KeyCode::NumpadEnter => {
                        let (tf, s) = (*tf, std::mem::take(s));
                        self.edit = None;
                        self.commit_text(ctx, tf, s);
                    }
                    KeyCode::Backspace => {
                        s.pop();
                    }
                    _ => {
                        if let Some(t) = text {
                            s.extend(t.chars().filter(|c| !c.is_control()));
                        }
                    }
                }
            } else if let InputEv::Dropped(p) = ev {
                s.push_str(&p.to_string_lossy());
            }
            return Next::Stay;
        }
        let n = items(ctx, self.cat).len();
        match ev {
            InputEv::Key {
                code,
                pressed: true,
                ..
            } => match code {
                KeyCode::Escape | KeyCode::Backspace => {
                    if self.focus_items {
                        self.focus_items = false;
                    } else {
                        ctx.save_config();
                        return Next::Pop;
                    }
                }
                KeyCode::F1 | KeyCode::Tab => {
                    ctx.save_config();
                    return Next::Pop;
                }
                KeyCode::ArrowUp => {
                    if self.focus_items {
                        self.sel = (self.sel + n - 1) % n.max(1);
                    } else {
                        self.cat = (self.cat + CATEGORIES.len() - 1) % CATEGORIES.len();
                        self.sel = 0;
                    }
                }
                KeyCode::ArrowDown => {
                    if self.focus_items {
                        self.sel = (self.sel + 1) % n.max(1);
                    } else {
                        self.cat = (self.cat + 1) % CATEGORIES.len();
                        self.sel = 0;
                    }
                }
                KeyCode::ArrowLeft => {
                    if self.focus_items {
                        self.activate(ctx, -1, false)
                    }
                }
                KeyCode::ArrowRight => {
                    if self.focus_items {
                        self.activate(ctx, 1, false)
                    } else {
                        self.focus_items = true
                    }
                }
                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                    if self.focus_items {
                        self.activate(ctx, 0, true)
                    } else {
                        self.focus_items = true
                    }
                }
                KeyCode::PageUp => {
                    self.cat = (self.cat + CATEGORIES.len() - 1) % CATEGORIES.len();
                    self.sel = 0;
                }
                KeyCode::PageDown => {
                    self.cat = (self.cat + 1) % CATEGORIES.len();
                    self.sel = 0;
                }
                _ => {}
            },
            InputEv::Wheel { dy } => {
                self.scroll = (self.scroll - dy * 40.0).max(0.0);
            }
            InputEv::Pointer {
                phase: Phase::Down,
                x,
                y,
                ..
            } => {
                if self.back_rect.contains(*x, *y) {
                    ctx.save_config();
                    return Next::Pop;
                }
                if let Some(c) = ui::hit(&self.cat_rects, *x, *y) {
                    self.cat = c;
                    self.sel = 0;
                    self.scroll = 0.0;
                    self.focus_items = false;
                } else if let Some(i) = ui::hit(&self.item_rects, *x, *y) {
                    self.focus_items = true;
                    self.sel = i;
                    let r = self
                        .item_rects
                        .iter()
                        .find(|(_, k)| *k == i)
                        .map(|(r, _)| *r)
                        .unwrap_or_default();
                    let d = if *x < r.x + r.w * 0.78 { -1 } else { 1 };
                    let list = items(ctx, self.cat);
                    let is_adjust = matches!(
                        list.get(i).map(|it| it.act),
                        Some(Act::Adjust(_) | Act::SkinOption(..) | Act::SkinOffset(..))
                    );
                    let value_area = *x > r.x + r.w * 0.55;
                    if is_adjust && value_area {
                        self.activate(ctx, d, false);
                    } else if !is_adjust {
                        self.activate(ctx, 0, true);
                    } else {
                        self.activate(ctx, 1, false);
                    }
                }
            }
            _ => {}
        }
        Next::Stay
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let ui = ctx.ui();
        let th = ctx.theme.clone();
        let (w, h) = (ctx.width, ctx.height);
        d.b.gradient(
            Rect::new(0.0, 0.0, w, h),
            crate::ui::lift(th.background, 0.04),
            th.background,
        );
        let header = 48.0 * ui;
        d.b.rect(Rect::new(0.0, 0.0, w, header), th.panel);
        d.b.rect(
            Rect::new(0.0, header - 2.0, w, 2.0),
            with_alpha(th.accent, 0.6),
        );
        d.text(
            "SETTINGS",
            16.0 * ui,
            12.0 * ui,
            22.0 * ui,
            th.accent,
            Align::Left,
        );
        d.text(
            "↑↓ select  ←→ change  Enter edit  Esc back",
            w * 0.5,
            15.0 * ui,
            15.0 * ui,
            th.text_dim,
            Align::Center,
        );
        self.back_rect = ui::button(
            d,
            Rect::new(w - 110.0 * ui, 7.0 * ui, 100.0 * ui, 34.0 * ui),
            "◀ BACK",
            &th,
            ui,
            false,
        );

        let portrait = ctx.portrait();
        let (cat_r, item_r) = if portrait {
            let ch = 2.0 * 40.0 * ui;
            (
                Rect::new(0.0, header, w, ch),
                Rect::new(0.0, header + ch, w, h - header - ch),
            )
        } else {
            let cw = 220.0 * ui;
            (
                Rect::new(0.0, header, cw, h - header),
                Rect::new(cw, header, w - cw, h - header),
            )
        };
        self.cat_rects.clear();
        for (i, c) in CATEGORIES.iter().enumerate() {
            let r = if portrait {
                let per = 5;
                let bw = w / per as f32;
                Rect::new(
                    (i % per) as f32 * bw + 2.0,
                    cat_r.y + (i / per) as f32 * 40.0 * ui + 2.0,
                    bw - 4.0,
                    36.0 * ui,
                )
            } else {
                Rect::new(
                    cat_r.x + 8.0 * ui,
                    cat_r.y + 8.0 * ui + i as f32 * 44.0 * ui,
                    cat_r.w - 16.0 * ui,
                    40.0 * ui,
                )
            };
            ui::button(d, r, c, &th, ui * 0.95, i == self.cat);
            if i == self.cat && !self.focus_items {
                // keyboard focus on the categories: accent tab on the left
                d.b.rect(Rect::new(r.x - 4.0, r.y + 6.0, 3.0, r.h - 12.0), th.accent);
            }
            self.cat_rects.push((r, i));
        }
        let list = items(ctx, self.cat);
        let rh = 40.0 * ui;
        // keep selection visible
        let sel_y = self.sel as f32 * rh;
        if sel_y - self.scroll > item_r.h - rh * 2.0 {
            self.scroll = sel_y - item_r.h + rh * 2.0;
        }
        if sel_y < self.scroll {
            self.scroll = sel_y;
        }
        self.item_rects.clear();
        d.b.set_clip(Some(item_r));
        for (i, item) in list.iter().enumerate() {
            let y = item_r.y + 8.0 * ui + i as f32 * rh - self.scroll;
            if y + rh < item_r.y || y > item_r.bottom() {
                continue;
            }
            let r = Rect::new(item_r.x + 10.0 * ui, y, item_r.w - 20.0 * ui, rh - 4.0);
            let selected = i == self.sel && self.focus_items;
            ui::row(d, r, &th, selected);
            d.text_fit(
                &item.label,
                r.x + 12.0 * ui,
                r.y + (r.h - 17.0 * ui) * 0.5,
                17.0 * ui,
                th.text,
                Align::Left,
                r.w * 0.55,
            );
            let interactive = !matches!(item.act, Act::None);
            let vcol = if interactive { th.accent } else { th.text_dim };
            let value = if matches!(
                item.act,
                Act::Adjust(_) | Act::SkinOption(..) | Act::SkinOffset(..)
            ) {
                format!("◀ {} ▶", item.value)
            } else {
                item.value.clone()
            };
            d.text_fit(
                &value,
                r.right() - 12.0 * ui,
                r.y + (r.h - 17.0 * ui) * 0.5,
                17.0 * ui,
                vcol,
                Align::Right,
                r.w * 0.42,
            );
            self.item_rects.push((r, i));
        }
        d.b.set_clip(None);
        // modal: text edit / key learning
        if let Some((tf, s)) = &self.edit {
            let r = Rect::new(w * 0.1, h * 0.38, w * 0.8, 90.0 * ui);
            d.b.rect(Rect::new(0.0, 0.0, w, h), with_alpha(th.background, 0.7));
            ui::panel(d, r, &th);
            let caret = if (now.elapsed().as_millis() / 500) % 2 == 0 {
                "_"
            } else {
                " "
            };
            let shown = if *tf == TextField::IrToken {
                "•".repeat(s.chars().count())
            } else {
                s.clone()
            };
            d.text(
                &format!("{tf:?}  (Enter = OK, Esc = cancel)"),
                r.x + 14.0 * ui,
                r.y + 10.0 * ui,
                15.0 * ui,
                th.text_dim,
                Align::Left,
            );
            d.text_fit(
                &format!("{shown}{caret}"),
                r.x + 14.0 * ui,
                r.y + 42.0 * ui,
                22.0 * ui,
                th.text,
                Align::Left,
                r.w - 28.0 * ui,
            );
        }
        if let Some((slot, _)) = &self.learning {
            let r = Rect::new(w * 0.2, h * 0.38, w * 0.6, 90.0 * ui);
            d.b.rect(Rect::new(0.0, 0.0, w, h), with_alpha(th.background, 0.7));
            ui::panel(d, r, &th);
            let what = if matches!(slot, Slot::ScratchAxis(_)) {
                "Spin the turntable"
            } else {
                "Press a key or controller button"
            };
            d.text(
                what,
                r.x + r.w * 0.5,
                r.y + 18.0 * ui,
                22.0 * ui,
                th.text,
                Align::Center,
            );
            d.text(
                "Esc = cancel, Delete = clear",
                r.x + r.w * 0.5,
                r.y + 54.0 * ui,
                15.0 * ui,
                th.text_dim,
                Align::Center,
            );
        }
    }
}

/// Display name of a configured skin (its header name, else the file stem).
fn skin_label(ctx: &Ctx, value: &str) -> String {
    let Some(p) = super::play_skin::configured_path(ctx, value) else {
        return "DEFAULT".into();
    };
    crate::skin::header(&p)
        .map(|h| h.1)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

/// Cycle a skin setting: the built-in default, then matching installed skins.
fn cycle_skin(c: &mut Ctx, d: i32, mode: &str) {
    let want = vibebms_core::PlayMode::ALL
        .iter()
        .copied()
        .find(|m| m.label() == mode)
        .map(super::play_skin::skin_type)
        .or_else(|| {
            [crate::skin::SkinType::Select, crate::skin::SkinType::Result]
                .into_iter()
                .find(|t| t.config_key() == Some(mode))
        });
    let mut opts: Vec<String> = vec![String::new()];
    for (p, t, _) in crate::skin::lr2::find_skins(&c.paths.skins) {
        if Some(t) == want {
            let rel = p
                .strip_prefix(&c.paths.root)
                .map(|r| r.to_path_buf())
                .unwrap_or(p);
            opts.push(rel.to_string_lossy().into_owned());
        }
    }
    let cur = c.config.skins.get(mode).cloned().unwrap_or_default();
    let i = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
    let next = opts[(i + d).rem_euclid(opts.len() as i32) as usize].clone();
    if opts.len() == 1 {
        c.toast(format!(
            "Put more {mode} skins in {}",
            c.paths.skins.display()
        ));
    }
    c.config.skins.insert(mode.to_string(), next);
}
