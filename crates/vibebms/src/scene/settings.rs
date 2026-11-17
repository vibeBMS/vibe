//! In-game settings: every option lives here (no external launcher).
//!
//! Keyboard: ↑↓ select, ←→ change, Enter activate, Esc back.
//! Mouse/touch: tap a category, tap the left/right half of a value to change it.
//! Controller: the turntable selects, white keys decide / raise, blue keys
//! lower / go back, SELECT leaves.

use super::keybind::{keys_label, Slot};
use crate::app::{Ctx, Draw, Next, Scene};
use crate::config::{
    Bpms, HiSpeedMode, HsFix, KeyConfig, PresentMode, Side, TableConfig, Target, TouchMode,
    TouchSide, TouchStyle,
};
use crate::gfx::text::Align;
use crate::gfx::{with_alpha, Gfx, Rect};
use crate::input::{nav_from_key, nav_from_pad, InputEv, Nav, Phase};
use crate::platform::Instant;
use crate::theme;
use crate::ui;
use vibebms_audio::Bus;
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::LnType;
use winit::keyboard::KeyCode;

const CATEGORIES: [&str; 10] = [
    "PLAY",
    "JUDGE & GAUGE",
    "KEY CONFIG",
    "TOUCH",
    "DISPLAY",
    "AUDIO",
    "LIBRARY",
    "TABLES",
    "IR",
    "SKINS",
];

/// The skin catalog's page.
const SKINS_CAT: usize = 9;

#[derive(Clone, Copy, Debug, PartialEq)]
enum TextField {
    PlayerName,
    AddFolder,
    AddTable,
    IrUrl,
    IrName,
    IrToken,
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
    /// Open the touch layout editor.
    EditTouchLayout,
    /// Open a skinned screen: 0 key config, 1 skin select, 2 skin select
    /// on the sound sets.
    OpenScreen(usize),
    RestoreDefaultTables,
    BrowseFolder,
    /// Cycle custom option `.1` of the skin chosen for `SKIN_KEYS[.0]`.
    SkinOption(usize, usize),
    /// Adjust field `.2` (x, y, w, h, angle, alpha) of offset `.1` of that skin.
    SkinOffset(usize, usize, usize),
    /// Web: ask the browser to keep the songs.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    KeepSongs,
    /// Web: delete song folder `.0` kept in the browser (`usize::MAX`: all),
    /// on a second press.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    DeleteKept(usize),
    /// Read the skin catalog again.
    SkinCatalog,
    /// Install catalog skin `.0` (an installed one again on a second
    /// press; a downloading one: cancel).
    SkinGet(usize),
    /// Open catalog skin `.0`'s page.
    SkinPage(usize),
    /// Delete catalog skin `.0`, on a second press.
    SkinDelete(usize),
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The kept song folder a first press armed for deleting.
    static ARMED: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// `n` bytes for people.
#[cfg(target_arch = "wasm32")]
fn bytes(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.0} MB", n / 1e6)
    } else {
        format!("{:.0} KB", n / 1e3)
    }
}

/// Days a kept song folder may go unplayed (and unchanged) before the
/// LIBRARY page offers to delete it.
#[cfg(target_arch = "wasm32")]
const STALE_DAYS: i64 = 30;
/// `Act::DeleteKept` of the folders not played for [`STALE_DAYS`].
#[cfg(target_arch = "wasm32")]
const STALE: usize = usize::MAX - 1;

/// The kept folders (indexes into `folders`) written and played last more
/// than [`STALE_DAYS`] ago (a song never played counts from when it came).
#[cfg(target_arch = "wasm32")]
fn stale_folders(ctx: &Ctx, folders: &[crate::platform::vfs::Kept]) -> Vec<usize> {
    let cut = crate::platform::unix_now() as i64 - STALE_DAYS * 86_400;
    folders
        .iter()
        .enumerate()
        .filter(|(_, k)| k.newest > 0 && k.newest < cut)
        .filter(|(_, k)| {
            !ctx.songs.iter().any(|s| {
                std::path::Path::new(&s.path).starts_with(&k.dir)
                    && ctx
                        .scores
                        .iter()
                        .any(|((sha, _), b)| *sha == s.sha256 && b.last_played >= cut)
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// Web: the songs kept in the browser, and how much room they take.
#[cfg(target_arch = "wasm32")]
fn kept_songs(ctx: &Ctx, v: &mut Vec<Item>) {
    let st = crate::platform::opfs::status();
    if !st.available {
        v.push(Item {
            label: "Songs stay until the page closes (this browser keeps no files)".into(),
            value: String::new(),
            act: Act::None,
        });
        return;
    }
    let room = if st.quota > 0.0 {
        format!("{} of {}", bytes(st.usage), bytes(st.quota))
    } else {
        bytes(st.usage)
    };
    let saving = if st.pending > 0 {
        format!(" · saving {}", st.pending)
    } else {
        String::new()
    };
    v.push(Item {
        label: format!("Songs kept in this browser: {room}{saving}"),
        value: if st.persisted {
            "kept for good".into()
        } else {
            "ask to keep ↵".into()
        },
        act: if st.persisted {
            Act::None
        } else {
            Act::KeepSongs
        },
    });
    let folders = crate::platform::vfs::folder_sizes(&ctx.paths.root.join("bms"));
    let armed = ARMED.with(|a| a.get());
    let stale = stale_folders(ctx, &folders);
    if !stale.is_empty() {
        let total: u64 = stale.iter().map(|&i| folders[i].bytes).sum();
        v.push(Item {
            label: format!(
                "Delete songs not played for {STALE_DAYS} days ({} folder{}, {})",
                stale.len(),
                if stale.len() == 1 { "" } else { "s" },
                bytes(total as f64)
            ),
            value: if armed == Some(STALE) {
                "press again to delete".into()
            } else {
                "delete ×".into()
            },
            act: Act::DeleteKept(STALE),
        });
    }
    if !folders.is_empty() {
        let total: u64 = folders.iter().map(|f| f.bytes).sum();
        v.push(Item {
            label: format!(
                "Delete every song kept here ({} folder{}, {})",
                folders.len(),
                if folders.len() == 1 { "" } else { "s" },
                bytes(total as f64)
            ),
            value: if armed == Some(usize::MAX) {
                "press again to delete".into()
            } else {
                "delete ×".into()
            },
            act: Act::DeleteKept(usize::MAX),
        });
    }
    for (i, k) in folders.iter().enumerate() {
        let name = k
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        v.push(Item {
            label: format!("    {name}  ({} files)", k.files),
            value: if armed == Some(i) {
                "press again to delete".into()
            } else {
                format!("{}  delete ×", bytes(k.bytes as f64))
            },
            act: Act::DeleteKept(i),
        });
    }
}

/// `config.skins` keys, in menu order.
const SKIN_KEYS: [&str; 12] = [
    "7KEYS",
    "5KEYS",
    "14KEYS",
    "10KEYS",
    "9KEYS",
    "SELECT",
    "DECIDE",
    "RESULT",
    "COURSE_RESULT",
    "KEY_CONFIG",
    "SKIN_SELECT",
    "SOUND_SET",
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
    if let Some(o) = opts.get(idx) {
        super::skinned::step_option(ctx, &value, o, d);
    }
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
    if let Some(o) = offs.get(idx) {
        super::skinned::step_offset(ctx, &value, &o.name, field, d);
    }
}

/// The row opening skinned screen `n` (0 key config, 1 skin select, 2
/// sound sets in the skin select), when its skin is chosen.
fn open_item(ctx: &Ctx, n: usize) -> Option<Item> {
    use crate::skin::SkinType;
    let kind = if n == 0 {
        SkinType::KeyConfig
    } else {
        SkinType::SkinSelect
    };
    super::skinned::configured_skin(ctx, kind)?;
    let label = match n {
        0 => "    Open the key config screen",
        1 => "    Open the skin select screen",
        _ => "    Choose in the skin select screen",
    };
    Some(Item {
        label: label.into(),
        value: "↵".into(),
        act: Act::OpenScreen(n),
    })
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

fn touch_style_label(s: TouchStyle) -> &'static str {
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

fn onoff(b: bool) -> &'static str {
    if b {
        "ON"
    } else {
        "OFF"
    }
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
            it(
                "Battle (SP on both sides)",
                match super::option_panel::dp_index(p) {
                    2 => "BATTLE",
                    3 => "BATTLE AS",
                    _ => "OFF",
                },
                |c, d| {
                    // OFF, BATTLE, BATTLE AS (AUTO SCRATCH on both sides)
                    let p = &mut c.config.play;
                    let i = super::option_panel::dp_index(p).max(1) - 1;
                    let i = (i + d as i64).rem_euclid(3);
                    let flip = p.dp_option;
                    super::option_panel::set_dp_index(p, if i == 0 { 0 } else { i + 1 });
                    if i == 0 {
                        p.dp_option = flip;
                    }
                },
            ),
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
                "Hi-speed",
                format!(
                    "{:.2}  (GN {} at 150 BPM)",
                    p.hispeed_at(&Bpms::flat(150.0)),
                    p.green(&Bpms::flat(150.0))
                ),
                |c, d| {
                    c.config
                        .play
                        .add_hispeed(d as f32 * crate::config::HISPEED_STEP, &Bpms::flat(150.0))
                },
            ),
            it(
                "HS-FIX (keep the green number)",
                p.hs_fix.label().to_string(),
                |c, d| {
                    let f = cyc(&HsFix::ALL, c.config.play.hs_fix, d);
                    c.config.play.set_hs_fix(f, &Bpms::flat(150.0))
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
            it("Target score", p.target.label(), |c, d| {
                c.config.play.target = cyc(&Target::ALL, c.config.play.target, d)
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
            it("EXPAND JUDGE", onoff(p.expand_judge), |c, _| {
                c.config.play.expand_judge = !c.config.play.expand_judge
            }),
            it("  PGREAT window %", p.judge_rates[0].to_string(), |c, d| {
                let j = &mut c.config.play.judge_rates[0];
                *j = (*j + d as i64 * 5).clamp(10, 400)
            }),
            it("  GREAT window %", p.judge_rates[1].to_string(), |c, d| {
                let j = &mut c.config.play.judge_rates[1];
                *j = (*j + d as i64 * 5).clamp(10, 400)
            }),
            it("  GOOD window %", p.judge_rates[2].to_string(), |c, d| {
                let j = &mut c.config.play.judge_rates[2];
                *j = (*j + d as i64 * 5).clamp(10, 400)
            }),
            it("JUDGE AREA", onoff(p.judge_area), |c, _| {
                c.config.play.judge_area = !c.config.play.judge_area
            }),
            it("MARK NOTE", onoff(p.mark_notes), |c, _| {
                c.config.play.mark_notes = !c.config.play.mark_notes
            }),
            it("BPM GUIDE", onoff(p.bpm_guide), |c, _| {
                c.config.play.bpm_guide = !c.config.play.bpm_guide
            }),
            it(
                "Judge timing auto adjust",
                onoff(p.timing_auto_adjust),
                |c, _| c.config.play.timing_auto_adjust = !c.config.play.timing_auto_adjust,
            ),
        ],
        2 => {
            let k = &c.keys;
            let mut v: Vec<Item> = open_item(ctx, 0).into_iter().collect();
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
                label: "Touch controller layout".into(),
                value: format!("{}  edit ↵", touch_style_label(c.touch.style)),
                act: Act::EditTouchLayout,
            });
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
            it("Layout", touch_style_label(c.touch.style), |c, d| {
                c.config.touch.style = cyc(&TouchStyle::ALL, c.config.touch.style, d)
            }),
            Item {
                label: "Edit the touch layout".into(),
                value: "↵".into(),
                act: Act::EditTouchLayout,
            },
            it(
                "Controls height (landscape)",
                format!("{:.0}%", c.touch.landscape_height * 100.0),
                |c, d| {
                    let h = &mut c.config.touch.landscape_height;
                    let (lo, hi) = crate::touch::CONTROLS_HEIGHT;
                    *h = (*h + d as f32 * 0.01).clamp(lo, hi)
                },
            ),
            it(
                "Controls height (portrait)",
                format!("{:.0}%", c.touch.portrait_height * 100.0),
                |c, d| {
                    let h = &mut c.config.touch.portrait_height;
                    let (lo, hi) = crate::touch::CONTROLS_HEIGHT;
                    *h = (*h + d as f32 * 0.01).clamp(lo, hi)
                },
            ),
            it(
                "Screen size (keys at real size)",
                match c.touch.screen_inches {
                    0.0 => format!("AUTO ({:.1}\")", ctx.screen.guessed_inches(ctx.scale)),
                    n => format!("{n:.1}\""),
                },
                |c, d| {
                    const SIZES: [f32; 19] = [
                        0.0, 5.4, 6.1, 6.7, 7.9, 8.3, 10.2, 10.9, 11.0, 12.9, 13.0, 13.3, 14.0,
                        15.6, 17.0, 24.0, 27.0, 32.0, 43.0,
                    ];
                    c.config.touch.screen_inches = cyc(&SIZES, c.config.touch.screen_inches, d);
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
                "START / SELECT side",
                start_select_label(c.touch.start_select_side),
                |c, d| {
                    let s = &mut c.config.touch.start_select_side;
                    *s = cyc(&TouchSide::ALL, *s, d)
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
            it(
                "Perspective (touch perspective skin)",
                format!("{:.0}%", c.perspective.strength * 100.0),
                |c, d| {
                    let v = &mut c.config.perspective.strength;
                    *v = (*v + d as f32 * 0.05).clamp(0.0, crate::config::PERSPECTIVE_MAX)
                },
            ),
            it(
                "    Lane length",
                format!("{:.0}%", c.perspective.length * 100.0),
                |c, d| {
                    let (lo, hi) = crate::config::LENGTH_RANGE;
                    let v = &mut c.config.perspective.length;
                    *v = (*v + d as f32 * 0.05).clamp(lo, hi)
                },
            ),
            it(
                "    Touch area height",
                format!("{:.0}%", c.perspective.touch_height * 100.0),
                |c, d| {
                    let (lo, hi) = crate::config::TOUCH_HEIGHT_RANGE;
                    let v = &mut c.config.perspective.touch_height;
                    *v = (*v + d as f32 * 0.01).clamp(lo, hi)
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
                    label: "GPU".into(),
                    value: ctx.gpu_name.clone(),
                    act: Act::None,
                },
            ];
            // built-in default skin / skins in <root>/skins
            type Adjust = fn(&mut Ctx, i32);
            const MODES: [(&str, Adjust); 12] = [
                ("7KEYS", |c, d| cycle_skin(c, d, "7KEYS")),
                ("5KEYS", |c, d| cycle_skin(c, d, "5KEYS")),
                ("14KEYS", |c, d| cycle_skin(c, d, "14KEYS")),
                ("10KEYS", |c, d| cycle_skin(c, d, "10KEYS")),
                ("9KEYS", |c, d| cycle_skin(c, d, "9KEYS")),
                ("SELECT", |c, d| cycle_skin(c, d, "SELECT")),
                ("DECIDE", |c, d| cycle_skin(c, d, "DECIDE")),
                ("RESULT", |c, d| cycle_skin(c, d, "RESULT")),
                ("COURSE_RESULT", |c, d| cycle_skin(c, d, "COURSE_RESULT")),
                ("KEY_CONFIG", |c, d| cycle_skin(c, d, "KEY_CONFIG")),
                ("SKIN_SELECT", |c, d| cycle_skin(c, d, "SKIN_SELECT")),
                ("SOUND_SET", |c, d| cycle_skin(c, d, "SOUND_SET")),
            ];
            for (slot, (m, f)) in MODES.into_iter().enumerate() {
                let cur = match c.skins.get(m).map(String::as_str).unwrap_or("") {
                    "" => "DEFAULT".into(),
                    p => skin_label(ctx, p),
                };
                let label = if m.ends_with("KEYS") {
                    format!("Play skin ({m})")
                } else if m == "SOUND_SET" {
                    "Sound set".into()
                } else {
                    format!(
                        "{}{} skin",
                        &m[..1],
                        m[1..].to_lowercase().replace('_', " ")
                    )
                };
                v.push(it(label, cur, f));
                // the screens these skins draw
                if let Some(n) = ["KEY_CONFIG", "SKIN_SELECT", "SOUND_SET"]
                    .iter()
                    .position(|k| *k == m)
                {
                    if let Some(i) = open_item(ctx, n) {
                        v.push(i);
                    }
                }
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
            it(
                "Sound set",
                super::skinned::choice_name(
                    ctx,
                    crate::skin::SkinType::SoundSet,
                    c.skins.get("SOUND_SET").map(String::as_str).unwrap_or(""),
                )
                .0,
                |c, d| {
                    cycle_skin(c, d, "SOUND_SET");
                    c.play_sound(crate::sysound::Sound::OptionChange);
                },
            ),
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
            #[cfg(target_arch = "wasm32")]
            kept_songs(ctx, &mut v);
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
            v.push(it(
                "Third-party mirrors (darksabun, hexlataia, kyouko archive)",
                onoff(c.third_party_mirrors),
                |c, _| {
                    c.config.third_party_mirrors = !c.config.third_party_mirrors;
                    crate::hosts::set_third_party(c.config.third_party_mirrors);
                },
            ));
            v.push(it(
                "Name song folders after their charts ([artist] title)",
                onoff(c.name_song_folders),
                |c, _| {
                    c.config.name_song_folders = !c.config.name_song_folders;
                    crate::hosts::set_folder_naming(c.config.name_song_folders);
                },
            ));
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
        SKINS_CAT => skin_items(ctx),
        _ => Vec::new(),
    }
}

/// `n` bytes as MB for people.
fn mb(n: u64) -> String {
    format!("{} MB", (n + (1 << 19)) >> 20)
}

/// The skin catalog: each skin, what it is, and what can be done with it.
fn skin_items(ctx: &Ctx) -> Vec<Item> {
    let store = &ctx.skin_store;
    let mut v = vec![Item {
        label: format!(
            "Skin catalog: {} skins, installed into {}",
            store.catalog.skins.len(),
            ctx.paths.skins.display()
        ),
        value: if store.is_fetching() {
            "reading…".into()
        } else {
            "update ↻".into()
        },
        act: Act::SkinCatalog,
    }];
    if crate::platform::IS_WEB {
        v.push(Item {
            label: "In the browser: no Lua skins, none over 400 MB".into(),
            value: String::new(),
            act: Act::None,
        });
    }
    let armed = store.armed.as_ref();
    for (i, e) in store.catalog.skins.iter().enumerate() {
        use crate::skinstore::State;
        let state = store.state(&ctx.paths.skins, e);
        let size = if e.size > 0 {
            format!(" ({})", mb(e.size))
        } else {
            String::new()
        };
        let value = match &state {
            State::Busy(p) => format!("{}  cancel ×", p.label()),
            State::Installed { update: true } => format!("update ↓{size}"),
            State::Installed { .. } if armed == Some(&(e.id.clone(), false)) => {
                "press again to reinstall".into()
            }
            State::Installed { .. } => "installed ✓".into(),
            State::Absent => match e.web_refusal().filter(|_| crate::platform::IS_WEB) {
                Some(_) => "desktop app only".into(),
                None => format!("install ↓{size}"),
            },
        };
        v.push(Item {
            label: format!(
                "{}{}  by {}",
                e.name,
                if e.version.is_empty() {
                    String::new()
                } else {
                    format!(" {}", e.version)
                },
                e.author
            ),
            value,
            act: Act::SkinGet(i),
        });
        let kind = if e.kind == "lr2" {
            "LR2"
        } else if e.lua {
            "beatoraja Lua"
        } else {
            "beatoraja"
        };
        let mut info = format!("    {kind} · {}", e.scenes_label());
        if !e.resolution.is_empty() {
            info.push_str(&format!(" · {}", e.resolution));
        }
        v.push(Item {
            label: info,
            value: "page ↗".into(),
            act: Act::SkinPage(i),
        });
        // the author's terms, on a line of their own (they can be long)
        if !e.license.is_empty() {
            v.push(Item {
                label: format!("    Terms: {}", e.license),
                value: String::new(),
                act: Act::None,
            });
        }
        if matches!(state, State::Installed { .. }) {
            v.push(Item {
                label: format!("    skins/{}", e.folder()),
                value: if armed == Some(&(e.id.clone(), true)) {
                    "press again to delete".into()
                } else {
                    "delete ×".into()
                },
                act: Act::SkinDelete(i),
            });
        }
    }
    v
}

/// What the skins page does with `act` (a delete or reinstall waits for a
/// second press on the same row).
fn skin_act(ctx: &mut Ctx, act: Act) {
    use crate::skinstore::State;
    let arm = |ctx: &mut Ctx, id: &str, delete: bool| {
        let want = Some((id.to_string(), delete));
        if ctx.skin_store.armed == want {
            ctx.skin_store.armed = None;
            true
        } else {
            ctx.skin_store.armed = want;
            false
        }
    };
    if !matches!(act, Act::SkinGet(_) | Act::SkinDelete(_)) {
        ctx.skin_store.armed = None;
    }
    let entry = |ctx: &Ctx, i: usize| ctx.skin_store.catalog.skins.get(i).cloned();
    match act {
        Act::SkinCatalog => ctx.skin_store.refresh(true),
        Act::SkinPage(i) => {
            if let Some(e) = entry(ctx, i) {
                crate::platform::open_url(&e.homepage);
            }
        }
        Act::SkinGet(i) => {
            let Some(e) = entry(ctx, i) else { return };
            match ctx.skin_store.state(&ctx.paths.skins, &e) {
                State::Busy(_) => {
                    ctx.skin_store.cancel(&e.id);
                    return;
                }
                State::Installed { update: false } if !arm(ctx, &e.id, false) => return,
                _ => ctx.skin_store.armed = None,
            }
            let r = ctx.skin_store.install(&ctx.paths, i);
            ctx.toast(r.unwrap_or_else(|e| e));
        }
        Act::SkinDelete(i) => {
            let Some(e) = entry(ctx, i) else { return };
            if !arm(ctx, &e.id, true) {
                return;
            }
            match ctx.skin_store.delete(&ctx.paths, i) {
                Ok(folders) => {
                    // skins chosen from those folders: back to the default
                    let rel = ctx
                        .paths
                        .skins
                        .strip_prefix(&ctx.paths.root)
                        .map(std::path::Path::to_path_buf)
                        .unwrap_or_else(|_| ctx.paths.skins.clone());
                    let gone: Vec<std::path::PathBuf> = folders
                        .iter()
                        .flat_map(|f| [rel.join(f), ctx.paths.skins.join(f)])
                        .collect();
                    for v in ctx.config.skins.values_mut() {
                        if gone
                            .iter()
                            .any(|g| std::path::Path::new(v.as_str()).starts_with(g))
                        {
                            v.clear();
                        }
                    }
                    ctx.skins_changed();
                    ctx.toast(format!("Deleted {}", e.name));
                }
                Err(m) => ctx.toast(m),
            }
        }
        _ => {}
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
    /// Where each row's value is drawn (left, right): a tap left of its
    /// middle lowers it, right of it raises it.
    value_spans: Vec<(f32, f32)>,
    scroll: f32,
    axis_seen: std::collections::HashMap<u64, f32>,
    /// Turntable positions last seen (it moves the selection).
    axis_last: std::collections::HashMap<u64, f32>,
    back_rect: Rect,
    /// The touch layout editor was asked for.
    open_editor: bool,
    /// A skinned screen was asked for (`Act::OpenScreen`).
    open_screen: Option<usize>,
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
            value_spans: Vec::new(),
            scroll: 0.0,
            axis_seen: Default::default(),
            axis_last: Default::default(),
            back_rect: Rect::default(),
            open_editor: false,
            open_screen: None,
        }
    }

    /// Opened on category `cat` (its rows chosen).
    pub fn at(ctx: &mut Ctx, cat: usize) -> SettingsScene {
        let mut s = SettingsScene::new(ctx);
        s.cat = cat.min(CATEGORIES.len() - 1);
        s.focus_items = true;
        s
    }

    fn activate(&mut self, ctx: &mut Ctx, d: i32, enter: bool) {
        let list = items(ctx, self.cat);
        let Some(item) = list.get(self.sel) else {
            return;
        };
        // a delete waits for a second press on the same row
        #[cfg(target_arch = "wasm32")]
        if !matches!(item.act, Act::DeleteKept(_)) {
            ARMED.with(|a| a.set(None));
        }
        if !matches!(item.act, Act::SkinGet(_) | Act::SkinDelete(_)) {
            ctx.skin_store.armed = None;
        }
        match item.act {
            Act::None => return,
            Act::SkinCatalog | Act::SkinGet(_) | Act::SkinPage(_) | Act::SkinDelete(_) => {
                skin_act(ctx, item.act);
                if !matches!(item.act, Act::SkinDelete(_)) {
                    return;
                }
            }
            Act::KeepSongs => {
                #[cfg(target_arch = "wasm32")]
                crate::platform::opfs::request_persist();
                return;
            }
            #[cfg(not(target_arch = "wasm32"))]
            Act::DeleteKept(_) => return,
            #[cfg(target_arch = "wasm32")]
            Act::DeleteKept(i) => {
                if ARMED.with(|a| a.replace(Some(i))) != Some(i) {
                    return;
                }
                ARMED.with(|a| a.set(None));
                let bms = ctx.paths.root.join("bms");
                let folders = crate::platform::vfs::folder_sizes(&bms);
                if i == usize::MAX {
                    crate::platform::vfs::remove_dir(&bms);
                    ctx.toast("Deleted every song kept here".to_string());
                } else if i == STALE {
                    let stale = stale_folders(ctx, &folders);
                    let total: u64 = stale.iter().map(|&k| folders[k].bytes).sum();
                    for &k in &stale {
                        crate::platform::vfs::remove_dir(&folders[k].dir);
                    }
                    ctx.toast(format!(
                        "Deleted {} folder{} ({})",
                        stale.len(),
                        if stale.len() == 1 { "" } else { "s" },
                        bytes(total as f64)
                    ));
                } else if let Some(k) = folders.get(i) {
                    crate::platform::vfs::remove_dir(&k.dir);
                    let name = k.dir.file_name().map(|n| n.to_string_lossy().into_owned());
                    ctx.toast(format!(
                        "Deleted {} ({})",
                        name.unwrap_or_default(),
                        bytes(k.bytes as f64)
                    ));
                }
                return;
            }
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
            Act::EditTouchLayout => {
                self.open_editor = true;
                return;
            }
            Act::OpenScreen(n) => {
                self.open_screen = Some(n);
                return;
            }
            Act::RestoreDefaultTables => {
                for (n, u) in bmseeker::table::DEFAULT_TABLES {
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

    /// Move the selection by `d` rows (or categories, when they have the
    /// focus).
    fn step(&mut self, ctx: &Ctx, d: i32) {
        if self.focus_items {
            let n = items(ctx, self.cat).len().max(1) as i32;
            self.sel = (self.sel as i32 + d).rem_euclid(n) as usize;
        } else {
            let n = CATEGORIES.len() as i32;
            self.cat = (self.cat as i32 + d).rem_euclid(n) as usize;
            self.sel = 0;
        }
    }

    /// A controller (or the keys bound to the lanes), as on the music
    /// select: the turntable moves, a white key decides (raises a value),
    /// a blue key lowers a value or goes back to the categories, START
    /// goes back to them too, SELECT leaves.
    fn nav(&mut self, ctx: &mut Ctx, nav: Nav) -> Next {
        let adjust = || {
            matches!(
                items(ctx, self.cat).get(self.sel).map(|it| it.act),
                Some(Act::Adjust(_) | Act::SkinOption(..) | Act::SkinOffset(..))
            )
        };
        match nav {
            Nav::Up => self.step(ctx, -1),
            Nav::Down => self.step(ctx, 1),
            Nav::Decide if !self.focus_items => self.focus_items = true,
            Nav::Decide if adjust() => self.activate(ctx, 1, false),
            Nav::Decide => self.activate(ctx, 0, true),
            Nav::Close if self.focus_items && adjust() => self.activate(ctx, -1, false),
            Nav::Close | Nav::Option => self.focus_items = false,
            Nav::Back if self.focus_items => self.focus_items = false,
            Nav::Back | Nav::Settings => {
                ctx.save_config();
                return Next::Pop;
            }
        }
        Next::Stay
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
        super::keybind::assign(&mut ctx.config.keys, slot, key, pad, axis);
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
        let next = self.input_inner(ctx, ev);
        if std::mem::take(&mut self.open_editor) {
            return Next::Push(Box::new(super::touch_layout::TouchLayoutScene::new(ctx)));
        }
        if let Some(n) = self.open_screen.take() {
            ctx.save_config();
            return Next::Push(match n {
                0 => Box::new(super::keyconfig::KeyConfigScene::new()),
                1 => Box::new(super::skinselect::SkinSelectScene::new(None)),
                _ => Box::new(super::skinselect::SkinSelectScene::new(Some(
                    crate::skin::SkinType::SoundSet,
                ))),
            });
        }
        next
    }

    fn draw(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        self.draw_inner(ctx, d, now)
    }
}

impl SettingsScene {
    fn input_inner(&mut self, ctx: &mut Ctx, ev: &InputEv) -> Next {
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
                        super::keybind::clear(&mut ctx.config.keys, slot, None);
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
                // keys bound to the lanes (a controller seen as a keyboard)
                other => {
                    if let Some(nav) = nav_from_key(*other, &ctx.config.keys) {
                        return self.nav(ctx, nav);
                    }
                }
            },
            InputEv::Pad {
                id, pressed: true, ..
            } => {
                if let Some(nav) = nav_from_pad(*id, &ctx.config.keys) {
                    return self.nav(ctx, nav);
                }
            }
            InputEv::Axis { id, value, .. } if ctx.config.keys.any_axis_is_scratch => {
                // the turntable moves the selection (a step per 0.06 turned)
                let last = *self.axis_last.entry(*id).or_insert(*value);
                let mut d = value - last;
                if d > 1.0 {
                    d -= 2.0
                } else if d < -1.0 {
                    d += 2.0
                }
                if d.abs() >= 0.06 {
                    self.axis_last.insert(*id, *value);
                    let nav = if d > 0.0 { Nav::Down } else { Nav::Up };
                    return self.nav(ctx, nav);
                }
            }
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
                    // the value's middle (it is drawn at the row's right)
                    let mid = self
                        .item_rects
                        .iter()
                        .position(|(_, k)| *k == i)
                        .and_then(|p| self.value_spans.get(p))
                        .map_or(r.x + r.w * 0.78, |(a, b)| (a + b) * 0.5);
                    let d = if *x < mid { -1 } else { 1 };
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

    fn draw_inner(&mut self, ctx: &mut Ctx, d: &mut Draw, now: Instant) {
        let ui = ctx.ui();
        let (w, h) = (ctx.width, ctx.height);
        d.b.gradient(
            Rect::new(0.0, 0.0, w, h),
            crate::ui::lift(theme::BACKGROUND, 0.04),
            theme::BACKGROUND,
        );
        let header = 48.0 * ui;
        d.b.rect(Rect::new(0.0, 0.0, w, header), theme::PANEL);
        d.b.rect(
            Rect::new(0.0, header - 2.0, w, 2.0),
            with_alpha(theme::ACCENT, 0.6),
        );
        d.text(
            "SETTINGS",
            16.0 * ui,
            12.0 * ui,
            22.0 * ui,
            theme::ACCENT,
            Align::Left,
        );
        d.text(
            "↑↓ select  ←→ change  Enter edit  Esc back",
            w * 0.5,
            15.0 * ui,
            15.0 * ui,
            theme::TEXT_DIM,
            Align::Center,
        );
        self.back_rect = ui::button(
            d,
            Rect::new(w - 110.0 * ui, 7.0 * ui, 100.0 * ui, 34.0 * ui),
            "◀ BACK",
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
            ui::button(d, r, c, ui * 0.95, i == self.cat);
            if i == self.cat && !self.focus_items {
                // keyboard focus on the categories: accent tab on the left
                d.b.rect(
                    Rect::new(r.x - 4.0, r.y + 6.0, 3.0, r.h - 12.0),
                    theme::ACCENT,
                );
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
        self.value_spans.clear();
        d.b.set_clip(Some(item_r));
        for (i, item) in list.iter().enumerate() {
            let y = item_r.y + 8.0 * ui + i as f32 * rh - self.scroll;
            if y + rh < item_r.y || y > item_r.bottom() {
                continue;
            }
            let r = Rect::new(item_r.x + 10.0 * ui, y, item_r.w - 20.0 * ui, rh - 4.0);
            let selected = i == self.sel && self.focus_items;
            ui::row(d, r, selected);
            d.text_fit(
                &item.label,
                r.x + 12.0 * ui,
                r.y + (r.h - 17.0 * ui) * 0.5,
                17.0 * ui,
                theme::TEXT,
                Align::Left,
                r.w * 0.55,
            );
            let interactive = !matches!(item.act, Act::None);
            let vcol = if interactive {
                theme::ACCENT
            } else {
                theme::TEXT_DIM
            };
            let value = if matches!(
                item.act,
                Act::Adjust(_) | Act::SkinOption(..) | Act::SkinOffset(..)
            ) {
                format!("◀ {} ▶", item.value)
            } else {
                item.value.clone()
            };
            let right = r.right() - 12.0 * ui;
            let drawn = d.text_fit(
                &value,
                right,
                r.y + (r.h - 17.0 * ui) * 0.5,
                17.0 * ui,
                vcol,
                Align::Right,
                r.w * 0.42,
            );
            self.item_rects.push((r, i));
            self.value_spans.push((right - drawn, right));
        }
        d.b.set_clip(None);
        // modal: text edit / key learning
        if let Some((tf, s)) = &self.edit {
            let r = Rect::new(w * 0.1, h * 0.38, w * 0.8, 90.0 * ui);
            d.b.rect(
                Rect::new(0.0, 0.0, w, h),
                with_alpha(theme::BACKGROUND, 0.7),
            );
            ui::panel(d, r);
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
                theme::TEXT_DIM,
                Align::Left,
            );
            d.text_fit(
                &format!("{shown}{caret}"),
                r.x + 14.0 * ui,
                r.y + 42.0 * ui,
                22.0 * ui,
                theme::TEXT,
                Align::Left,
                r.w - 28.0 * ui,
            );
        }
        if let Some((slot, _)) = &self.learning {
            let r = Rect::new(w * 0.2, h * 0.38, w * 0.6, 90.0 * ui);
            d.b.rect(
                Rect::new(0.0, 0.0, w, h),
                with_alpha(theme::BACKGROUND, 0.7),
            );
            ui::panel(d, r);
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
                theme::TEXT,
                Align::Center,
            );
            d.text(
                "Esc = cancel, Delete = clear",
                r.x + r.w * 0.5,
                r.y + 54.0 * ui,
                15.0 * ui,
                theme::TEXT_DIM,
                Align::Center,
            );
        }
    }
}

/// Display name of a configured skin (its header name, else the file stem).
fn skin_label(ctx: &Ctx, value: &str) -> String {
    if value == super::play_skin::PERSPECTIVE {
        return "TOUCH PERSPECTIVE".into();
    }
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
    let Some(kind) = skin_type_of(mode) else {
        return;
    };
    let opts = super::skinned::skin_choices(c, kind);
    let cur = c.config.skins.get(mode).cloned().unwrap_or_default();
    let i = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
    let next = opts[(i + d).rem_euclid(opts.len() as i32) as usize].clone();
    if opts.len() == 1 {
        let dir = if kind == crate::skin::SkinType::SoundSet {
            c.paths.root.join("sounds")
        } else {
            c.paths.skins.clone()
        };
        c.toast(format!("Put more {mode} skins in {}", dir.display()));
    }
    c.config.skins.insert(mode.to_string(), next);
}

/// The skin type of a `config.skins` key.
fn skin_type_of(key: &str) -> Option<crate::skin::SkinType> {
    use crate::skin::SkinType::*;
    [
        Play7,
        Play5,
        Play14,
        Play10,
        Play9,
        Select,
        Decide,
        Result,
        CourseResult,
        KeyConfig,
        SkinSelect,
        SoundSet,
    ]
    .into_iter()
    .find(|t| t.key() == Some(key))
}
