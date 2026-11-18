//! The options a player changes between songs. They live in the song
//! select's panels (PLAY OPTION, ASSIST, SYSTEM OPTION) and its tabs;
//! settings keep a pointer to each for search, and, until those panels
//! carry every one of them, the rows themselves at the end of GAMEPLAY.

use super::row::onoff;
use super::row::{percent, Row, Rows};
use crate::app::Ctx;
use crate::config::{BgaMode, HsFix, HsType, Side, HISPEED_STEP};
use crate::scene::option_panel::{cycle, dp_index, set_dp_index};
use crate::scene::select::REFERENCE_BPMS;
use crate::scene::target;
use vibebms_core::gauge::GaugeAutoShift;
use vibebms_core::lane_option::{DoubleOption, LaneOption};

/// The per-play rows still shown in settings (GAMEPLAY › ALSO IN THE SONG
/// SELECT). Once the select's panels carry every row in [`MOVED`], this
/// goes false and only the pointers stay.
pub(super) const SHOW_PER_PLAY: bool = false;

/// An option that lives in the song select: what search finds for it.
pub(super) struct Moved {
    pub label: &'static str,
    pub keys: &'static str,
    /// Where it is on the song select.
    pub place: &'static str,
    /// The settings rows it stands for (none: never a settings row).
    pub ids: &'static [&'static str],
}

pub(super) const MOVED: &[Moved] = &[
    Moved {
        label: tr_key!("Random"),
        keys: "random mirror s-random h-random r-random lane option 1p 2p",
        place: tr_key!("PLAY OPTION keys 1 / 2 (2P: 6 / 7), hold START"),
        ids: &["play.random_1p", "play.random_2p"],
    },
    Moved {
        label: tr_key!("DP option and battle"),
        keys: "flip double play battle sp both sides",
        place: tr_key!("PLAY OPTION key 4 (DP OPTION · BATTLE)"),
        ids: &["play.dp_option", "play.battle"],
    },
    Moved {
        label: tr_key!("Play side"),
        keys: "1p 2p side left right",
        place: tr_key!("PLAY OPTION › MORE (F6)"),
        ids: &["play.side"],
    },
    Moved {
        label: tr_key!("Hi-speed and HS-FIX"),
        keys: "hispeed hi speed green number gn scroll speed hs-fix floating",
        place: tr_key!("PLAY OPTION › MORE and key 5 (F7 / F8)"),
        ids: &["play.hispeed", "play.hs_fix"],
    },
    Moved {
        label: tr_key!("Lane cover, SUDDEN+, HIDDEN+, LIFT"),
        keys: "lanecover sudden hidden lift white number",
        place: tr_key!("PLAY OPTION › MORE › LANE COVER; in play, the usual cover keys"),
        ids: &["play.lane_cover", "play.sudden", "play.hidden", "play.lift"],
    },
    Moved {
        label: tr_key!("Target score"),
        keys: "target pacemaker rival ghost",
        place: tr_key!("PLAY OPTION, the turntable"),
        ids: &["play.target"],
    },
    Moved {
        label: tr_key!("Gauge"),
        keys: "gauge groove easy hard ex-hard hazard assist",
        place: tr_key!("PLAY OPTION key 3 (F3)"),
        ids: &["play.gauge"],
    },
    Moved {
        label: tr_key!("Gauge auto shift"),
        keys: "gas gauge auto shift best clear",
        place: tr_key!("SYSTEM OPTION key 2, hold START + SELECT"),
        ids: &["play.gauge_auto_shift"],
    },
    Moved {
        label: tr_key!("Assist options"),
        keys: "auto scratch legacy note mines no mine expand judge judge area mark note bpm guide \
               scroll remove mine add longnote add ln add chart modifier",
        place: tr_key!("ASSIST, hold SELECT"),
        ids: &[
            "play.auto_scratch",
            "play.legacy_note",
            "play.no_mine",
            "play.expand_judge",
            "play.judge_area",
            "play.mark_notes",
            "play.bpm_guide",
        ],
    },
    Moved {
        label: tr_key!("Random trainer"),
        keys: "random trainer lane order fixed",
        place: tr_key!(
            "RANDOM: TRAINER (PLAY OPTION keys 1 / 2); its order in GAMEPLAY › ADVANCED"
        ),
        ids: &[],
    },
    Moved {
        label: tr_key!("FREQ"),
        keys: "freq pitch speed semitone tempo rate faster slower",
        place: tr_key!("ASSIST, hold SELECT and turn the turntable"),
        ids: &[],
    },
    Moved {
        label: tr_key!("Autoplay"),
        keys: "auto play demo",
        place: tr_key!("the AUTOPLAY button, or F5"),
        ids: &["play.autoplay"],
    },
    Moved {
        label: tr_key!("BGA"),
        keys: "bga movie video background animation",
        place: tr_key!("SYSTEM OPTION key 1"),
        ids: &["play.bga"],
    },
    Moved {
        label: tr_key!("FAST / SLOW display"),
        keys: "fast slow timing display early late",
        place: tr_key!("SYSTEM OPTION"),
        ids: &["play.show_fast_slow"],
    },
    Moved {
        label: tr_key!("Sort"),
        keys: "sort order title level clear score",
        place: tr_key!("the SORT tab"),
        ids: &[],
    },
    Moved {
        label: tr_key!("Key mode filter"),
        keys: "filter keys 7keys 14keys 5keys",
        place: tr_key!("the KEYS tab"),
        ids: &[],
    },
    Moved {
        label: tr_key!("Difficulty filter"),
        keys: "filter difficulty beginner normal hyper another insane",
        place: tr_key!("the DIFFICULTY tab"),
        ids: &[],
    },
    Moved {
        label: tr_key!("Random select"),
        keys: "random select pick not clear not hard",
        place: tr_key!("RANDOM SELECT, the bar at the top of each folder"),
        ids: &[],
    },
];

/// The play side, flipped.
fn flip_side(c: &mut Ctx, _: i32) {
    let s = &mut c.config.play.side;
    *s = if *s == Side::P1 { Side::P2 } else { Side::P1 };
}

/// A lane cover / lift share: 1 % steps up to `max`.
fn cover(v: &mut f32, d: i32, max: f32) {
    *v = (*v + d as f32 * 0.01).clamp(0.0, max);
}

/// Where the moved option `id` is on the song select.
fn place(id: &str) -> &'static str {
    MOVED
        .iter()
        .find(|m| m.ids.contains(&id))
        .map_or("", |m| m.place)
}

/// A per-play row: its description says where it is on the song select.
fn moved(r: Row) -> Row {
    let p = place(r.id);
    r.desc(tr!("On the song select: {p}.", p = p))
}

pub(super) fn rows(b: &mut Rows) {
    let ctx = b.ctx;
    let p = &ctx.config.play;
    let bpms = &REFERENCE_BPMS;
    let rows = [
        Row::adjust(
            "play.random_1p",
            tr!("Random (1P)"),
            p.random_1p.label(),
            |c, d| c.config.play.random_1p = cycle(&LaneOption::ALL, c.config.play.random_1p, d),
        ),
        Row::adjust(
            "play.random_2p",
            tr!("Random (2P)"),
            p.random_2p.label(),
            |c, d| c.config.play.random_2p = cycle(&LaneOption::ALL, c.config.play.random_2p, d),
        ),
        Row::adjust(
            "play.dp_option",
            tr!("DP option"),
            format!("{:?}", p.dp_option).to_uppercase(),
            |c, d| {
                let all = [DoubleOption::Off, DoubleOption::Flip];
                c.config.play.dp_option = cycle(&all, c.config.play.dp_option, d)
            },
        ),
        Row::adjust(
            "play.battle",
            tr!("Battle"),
            match dp_index(p) {
                2 => "BATTLE",
                3 => "BATTLE AS",
                _ => "OFF",
            },
            |c, d| {
                // OFF, BATTLE, BATTLE AS (AUTO SCRATCH on both sides)
                let p = &mut c.config.play;
                let i = (dp_index(p).max(1) - 1 + d as i64).rem_euclid(3);
                let flip = p.dp_option;
                set_dp_index(p, if i == 0 { 0 } else { i + 1 });
                if i == 0 {
                    p.dp_option = flip;
                }
            },
        )
        .keys("sp on both sides"),
        Row::adjust(
            "play.side",
            tr!("Play side"),
            format!("{:?}", p.side),
            flip_side,
        ),
        Row::adjust(
            "play.hispeed",
            tr!("Hi-speed"),
            match p.hs_type {
                HsType::Normal => {
                    format!("LEVEL {}  (GN {} at 150 BPM)", p.hs_level, p.green(bpms))
                }
                _ => format!(
                    "{:.2}  (GN {} at 150 BPM)",
                    p.hispeed_shown(bpms),
                    p.green(bpms)
                ),
            },
            |c, d| {
                c.config
                    .play
                    .add_hispeed(d as f32 * HISPEED_STEP, &REFERENCE_BPMS)
            },
        )
        .keys("green number gn"),
        Row::adjust("play.hs_fix", tr!("HS-FIX"), p.hs_fix.label(), |c, d| {
            let f = cycle(&HsFix::ALL, c.config.play.hs_fix, d);
            c.config.play.set_hs_fix(f, &REFERENCE_BPMS)
        })
        .keys("keep the green number"),
        Row::adjust(
            "play.lane_cover",
            tr!("Lane cover"),
            onoff(p.lane_cover),
            |c, _| c.config.play.lane_cover ^= true,
        ),
        Row::adjust("play.sudden", tr!("SUDDEN+"), percent(p.sudden), |c, d| {
            cover(&mut c.config.play.sudden, d, 0.9)
        }),
        Row::adjust("play.hidden", tr!("HIDDEN+"), percent(p.hidden), |c, d| {
            cover(&mut c.config.play.hidden, d, 0.9)
        }),
        Row::adjust("play.lift", tr!("LIFT"), percent(p.lift), |c, d| {
            cover(&mut c.config.play.lift, d, 0.6)
        }),
        Row::adjust(
            "play.target",
            tr!("Target score"),
            target::label(p, &target::rivals(ctx)),
            |c, d| {
                let rivals = target::rivals(c);
                target::step(&mut c.config.play, &rivals, d);
            },
        ),
        Row::adjust("play.gauge", tr!("Gauge"), p.gauge.label(), |c, d| {
            c.config.play.gauge = cycle(
                c.config.play.gauge_preset.selectable(),
                c.config.play.gauge,
                d,
            )
        }),
        Row::adjust(
            "play.gauge_auto_shift",
            tr!("Gauge auto shift"),
            p.gauge_auto_shift.label(),
            |c, d| {
                let s = &mut c.config.play.gauge_auto_shift;
                *s = cycle(&GaugeAutoShift::ALL, *s, d)
            },
        ),
        Row::adjust(
            "play.auto_scratch",
            tr!("Auto scratch"),
            onoff(p.auto_scratch),
            |c, _| c.config.play.auto_scratch ^= true,
        ),
        Row::adjust(
            "play.legacy_note",
            tr!("Legacy note"),
            onoff(p.legacy_note),
            |c, _| c.config.play.legacy_note ^= true,
        ),
        Row::adjust(
            "play.no_mine",
            tr!("Remove mines"),
            onoff(p.no_mine),
            |c, _| c.config.play.no_mine ^= true,
        ),
        Row::adjust(
            "play.expand_judge",
            tr!("EXPAND JUDGE"),
            onoff(p.expand_judge),
            |c, _| c.config.play.expand_judge ^= true,
        ),
        Row::adjust(
            "play.judge_area",
            tr!("JUDGE AREA"),
            onoff(p.judge_area),
            |c, _| c.config.play.judge_area ^= true,
        ),
        Row::adjust(
            "play.mark_notes",
            tr!("MARK NOTE"),
            onoff(p.mark_notes),
            |c, _| c.config.play.mark_notes ^= true,
        ),
        Row::adjust(
            "play.bpm_guide",
            tr!("BPM GUIDE"),
            onoff(p.bpm_guide),
            |c, _| c.config.play.bpm_guide ^= true,
        ),
        Row::adjust(
            "play.autoplay",
            tr!("Autoplay"),
            onoff(p.autoplay),
            |c, _| c.config.play.autoplay ^= true,
        ),
        Row::adjust("play.bga", tr!("BGA"), p.bga.label(), |c, d| {
            c.config.play.bga = cycle(&BgaMode::ALL, c.config.play.bga, d)
        }),
        Row::adjust(
            "play.show_fast_slow",
            tr!("FAST / SLOW display"),
            onoff(p.show_fast_slow),
            |c, _| c.config.play.show_fast_slow ^= true,
        ),
    ];
    b.extend(rows.into_iter().map(moved));
}
