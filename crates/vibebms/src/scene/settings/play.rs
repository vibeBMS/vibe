use super::{percent, Act, Row, TextField};
use crate::app::Ctx;
use crate::config::{HiSpeedMode, HsFix, Side, Target, HISPEED_STEP};
use crate::scene::option_panel::{cycle, dp_index, onoff, set_dp_index};
use crate::scene::select::REFERENCE_BPMS;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::LnType;

/// The play side, flipped.
pub(super) fn flip_side(c: &mut Ctx, _: i32) {
    let s = &mut c.config.play.side;
    *s = if *s == Side::P1 { Side::P2 } else { Side::P1 };
}

/// A lane cover / lift share: 1 % steps up to `max`.
fn cover(v: &mut f32, d: i32, max: f32) {
    *v = (*v + d as f32 * 0.01).clamp(0.0, max);
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let c = &ctx.config;
    let p = &c.play;
    let bpms = &REFERENCE_BPMS;
    vec![
        Row::new(
            "Player name",
            c.player_name.clone(),
            Act::Text(TextField::PlayerName),
        ),
        Row::adjust("Random (1P)", p.random_1p.label(), |c, d| {
            c.config.play.random_1p = cycle(&LaneOption::ALL, c.config.play.random_1p, d)
        }),
        Row::adjust("Random (2P)", p.random_2p.label(), |c, d| {
            c.config.play.random_2p = cycle(&LaneOption::ALL, c.config.play.random_2p, d)
        }),
        Row::adjust(
            "DP option",
            format!("{:?}", p.dp_option).to_uppercase(),
            |c, d| {
                let all = [DoubleOption::Off, DoubleOption::Flip];
                c.config.play.dp_option = cycle(&all, c.config.play.dp_option, d)
            },
        ),
        Row::adjust(
            "Battle (SP on both sides)",
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
        ),
        Row::adjust("Play side", format!("{:?}", p.side), flip_side),
        Row::adjust(
            "LN mode (undefined charts)",
            format!("{:?}", p.ln_mode).to_uppercase(),
            |c, d| {
                let all = [LnType::Ln, LnType::Cn, LnType::Hcn];
                c.config.play.ln_mode = cycle(&all, c.config.play.ln_mode, d)
            },
        ),
        Row::adjust(
            "Hi-speed mode",
            format!("{:?}", p.hispeed_mode).to_uppercase(),
            |c, d| {
                let all = [HiSpeedMode::Floating, HiSpeedMode::Constant];
                c.config.play.hispeed_mode = cycle(&all, c.config.play.hispeed_mode, d)
            },
        ),
        Row::adjust(
            "Hi-speed",
            format!(
                "{:.2}  (GN {} at 150 BPM)",
                p.hispeed_at(bpms),
                p.green(bpms)
            ),
            |c, d| {
                c.config
                    .play
                    .add_hispeed(d as f32 * HISPEED_STEP, &REFERENCE_BPMS)
            },
        ),
        Row::adjust(
            "HS-FIX (keep the green number)",
            p.hs_fix.label(),
            |c, d| {
                let f = cycle(&HsFix::ALL, c.config.play.hs_fix, d);
                c.config.play.set_hs_fix(f, &REFERENCE_BPMS)
            },
        ),
        Row::adjust("Lane cover", onoff(p.lane_cover), |c, _| {
            c.config.play.lane_cover ^= true
        }),
        Row::adjust("SUDDEN+", percent(p.sudden), |c, d| {
            cover(&mut c.config.play.sudden, d, 0.9)
        }),
        Row::adjust("HIDDEN+", percent(p.hidden), |c, d| {
            cover(&mut c.config.play.hidden, d, 0.9)
        }),
        Row::adjust("LIFT", percent(p.lift), |c, d| {
            cover(&mut c.config.play.lift, d, 0.6)
        }),
        Row::adjust(
            "Judge offset (ms, + = you hit late)",
            format!("{:+}", p.judge_offset_ms),
            |c, d| {
                let o = &mut c.config.play.judge_offset_ms;
                *o = (*o + d).clamp(-500, 500)
            },
        ),
        Row::adjust(
            "Visual offset (ms)",
            format!("{:+}", p.visual_offset_ms),
            |c, d| {
                let o = &mut c.config.play.visual_offset_ms;
                *o = (*o + d).clamp(-500, 500)
            },
        ),
        Row::adjust("Auto scratch (assist)", onoff(p.auto_scratch), |c, _| {
            c.config.play.auto_scratch ^= true
        }),
        Row::adjust("Legacy note (assist)", onoff(p.legacy_note), |c, _| {
            c.config.play.legacy_note ^= true
        }),
        Row::adjust("Remove mines", onoff(p.no_mine), |c, _| {
            c.config.play.no_mine ^= true
        }),
        Row::adjust("Autoplay", onoff(p.autoplay), |c, _| {
            c.config.play.autoplay ^= true
        }),
        Row::adjust("BGA", onoff(p.bga), |c, _| c.config.play.bga ^= true),
        Row::adjust("Target score", p.target.label(), |c, d| {
            c.config.play.target = cycle(&Target::ALL, c.config.play.target, d)
        }),
        Row::adjust("FAST/SLOW display", onoff(p.show_fast_slow), |c, _| {
            c.config.play.show_fast_slow ^= true
        }),
        Row::adjust(
            "H-RANDOM threshold (BPM)",
            p.hran_threshold_bpm.to_string(),
            |c, d| {
                let t = &mut c.config.play.hran_threshold_bpm;
                *t = (*t as i32 + d * 10).clamp(60, 400) as u32
            },
        ),
    ]
}
