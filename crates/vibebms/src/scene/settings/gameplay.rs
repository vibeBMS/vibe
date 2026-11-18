//! GAMEPLAY: how every play is judged (offsets, rules), the second
//! player's own in VS, and the per-play options still shown here.

use super::per_play::{self, SHOW_PER_PLAY};
use super::row::onoff;
use super::row::{ms, Act, Kind, Row, Rows};
use super::{Sub, TextField};
use crate::app::Ctx;
use crate::config::{HiSpeedMode, HsType};
use crate::scene::option_panel::cycle;
use crate::scene::select::REFERENCE_BPMS;
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::SRandomInterval;
use vibebms_core::LnPref;

/// A judge offset: 1 ms steps within ±500.
fn offset(o: &mut i32, d: i32) {
    *o = (*o + d).clamp(-500, 500)
}

/// EXPAND JUDGE's window `k` (PGREAT, GREAT, GOOD) in 5 % steps.
fn window(c: &mut Ctx, k: usize, d: i32) {
    let j = &mut c.config.play.judge_rates[k];
    *j = (*j + d as i64 * 5).clamp(10, 400)
}

pub(super) fn rows(b: &mut Rows) {
    let p = &b.ctx.config.play;
    b.push(
        Row::note(
            "banner",
            tr!("Per-play options"),
            tr!("moved to the song select · PLAY OPTION, ASSIST, SYSTEM OPTION"),
        )
        .kind(Kind::Banner),
    );
    b.header(tr!("TIMING"));
    b.extend([
        Row::adjust(
            "play.judge_offset_ms",
            tr!("Judge offset"),
            ms(p.judge_offset_ms),
            |c, d| offset(&mut c.config.play.judge_offset_ms, d),
        )
        .desc(tr!(
            "Shifts the judge windows. + when you hit late. Also SYSTEM OPTION keys 5 / 7."
        ))
        .keys("timing latency calibration"),
        Row::adjust(
            "play.visual_offset_ms",
            tr!("Visual offset"),
            ms(p.visual_offset_ms),
            |c, d| offset(&mut c.config.play.visual_offset_ms, d),
        )
        .desc(tr!("Moves the notes on screen; judging stays where it is."))
        .keys("display latency calibration"),
        Row::adjust(
            "play.timing_auto_adjust",
            tr!("Judge auto adjust"),
            onoff(p.timing_auto_adjust),
            |c, _| c.config.play.timing_auto_adjust ^= true,
        )
        .desc(tr!(
            "After each play, moves the judge offset toward your average timing."
        ))
        .keys("timing auto adjust"),
    ]);
    b.header(tr!("RULES"));
    b.extend([
        Row::adjust(
            "play.judge_preset",
            tr!("Judge windows"),
            p.judge_preset.label(),
            |c, d| {
                let j = &mut c.config.play.judge_preset;
                *j = cycle(&JudgePreset::ALL, *j, d)
            },
        )
        .desc(tr!("How strict every play is judged: beatoraja, LR2 or IIDX windows."))
        .keys("judge rank timing"),
        Row::adjust("play.gauge_preset", tr!("Gauge rules"), p.gauge_preset.label(), |c, d| {
            let p = &mut c.config.play;
            p.gauge_preset = cycle(&GaugePreset::ALL, p.gauge_preset, d);
            // the attack gauges are the LR2 set's
            for g in [&mut p.rules.gauge, &mut p.p2.gauge] {
                if !p.rules.gauge_preset.has(*g) {
                    *g = GaugeType::Hard;
                }
            }
        })
        .desc(tr!("The gauge set GAUGE cycles through. LR2 adds the attack gauges.")),
        Row::adjust(
            "play.ln_mode",
            tr!("LN mode"),
            p.ln_pref().label(),
            |c, d| {
                let p = &mut c.config.play;
                let ln = cycle(&LnPref::ALL, p.ln_pref(), d);
                p.set_ln_pref(ln)
            },
        )
        .desc(tr!("For charts that do not say: LN, CN or HCN. FORCE: for every long note, whatever the chart says."))
        .keys("long note charge"),
        Row::adjust(
            "play.hispeed_mode",
            tr!("Hi-speed mode"),
            format!("{:?}", p.hispeed_mode).to_uppercase(),
            |c, d| {
                let all = [HiSpeedMode::Floating, HiSpeedMode::Constant];
                c.config.play.hispeed_mode = cycle(&all, c.config.play.hispeed_mode, d)
            },
        )
        .desc(tr!("FLOATING follows the BPM; CONSTANT keeps the notes at one speed."))
        .keys("hispeed scroll"),
        Row::adjust("play.hs_type", tr!("Hi-speed type"), p.hs_type.label(), |c, d| {
            let t = cycle(&HsType::ALL, c.config.play.hs_type, d);
            c.config.play.set_hs_type(t, &REFERENCE_BPMS)
        })
        .desc(tr!("CLASSIC and LR2 multiply; NORMAL has IIDX's 20 levels; FLOATING keeps the green number."))
        .keys("hispeed classic normal floating lr2"),
    ]);
    b.group(
        "gameplay.advanced",
        tr!("ADVANCED"),
        tr!("Judge algorithm, expanded judge windows, H-RANDOM threshold, S-RANDOM interval, random trainer, hi-speed changes"),
    );
    let rates = &p.judge_rates;
    b.extend([
        Row::adjust(
            "play.judge_algorithm",
            tr!("Judge algorithm"),
            format!("{:?}", p.judge_algorithm).to_uppercase(),
            |c, d| {
                use JudgeAlgorithm::*;
                let a = &mut c.config.play.judge_algorithm;
                *a = cycle(&[Combo, Duration, Lowest, Score], *a, d)
            },
        )
        .desc(tr!("Which note a press judges when several are in reach.")),
        Row::sub(
            "play.judge_rates",
            tr!("Expanded judge windows"),
            format!("{} · {} · {} %", rates[0], rates[1], rates[2]),
            Sub::ExpandJudge,
        )
        .desc(tr!("EXPAND JUDGE sizes of PGREAT / GREAT / GOOD. Over 100 %: assist lamp.")),
        Row::adjust(
            "play.hran_threshold_bpm",
            tr!("H-RANDOM threshold"),
            format!("{} BPM", p.hran_threshold_bpm),
            |c, d| {
                let t = &mut c.config.play.hran_threshold_bpm;
                *t = (*t as i32 + d * 10).clamp(60, 400) as u32
            },
        )
        .desc(tr!("H-RANDOM keeps notes this fast from landing on the same key.")),
        Row::adjust(
            "play.s_random_interval",
            tr!("S-RANDOM interval"),
            p.s_random_interval.label(),
            |c, d| {
                let s = &mut c.config.play.s_random_interval;
                *s = cycle(&SRandomInterval::ALL, *s, d)
            },
        )
        .desc(tr!("How S-RANDOM keeps a note off the key just played: OFF as LR2, 40 ms, or 120 Hz frames."))
        .keys("s-random jack correction interval"),
        Row::new(
            "play.trainer_order",
            tr!("Random trainer order"),
            p.trainer_order.clone(),
            Act::Text(TextField::TrainerOrder),
        )
        .desc(tr!("The lanes the TRAINER lane option plays: the keys from the left (7654321 is MIRROR). L-ASSIST."))
        .keys("random trainer lane order fixed practice"),
        Row::adjust(
            "play.hs_anim_ms",
            tr!("Hi-speed change while playing"),
            match p.hs_anim_ms {
                0 => tr!("AT ONCE").to_string(),
                ms => format!("{ms} ms"),
            },
            |c, d| {
                let ms = &mut c.config.play.hs_anim_ms;
                *ms = (*ms as i32 + d * 50).clamp(0, 1000) as u32
            },
        )
        .desc(tr!("How long the notes take to reach a hi-speed changed in play."))
        .keys("hispeed animation"),
    ]);
    super::play_extras::rows(b);
    b.group(
        "gameplay.vs",
        tr!("VS SECOND PLAYER"),
        tr!("2P gauge, 2P auto scratch, 2P judge offset"),
    );
    b.extend([
        Row::adjust(
            "play.p2.gauge",
            tr!("2P gauge"),
            p.p2.gauge.label(),
            |c, d| {
                c.config.play.p2.gauge = cycle(
                    c.config.play.gauge_preset.selectable(),
                    c.config.play.p2.gauge,
                    d,
                )
            },
        )
        .desc(tr!("The second player's own gauge, in VS.")),
        Row::adjust(
            "play.p2.auto_scratch",
            tr!("2P auto scratch"),
            onoff(p.p2.auto_scratch),
            |c, _| c.config.play.p2.auto_scratch ^= true,
        )
        .desc(tr!("The second player's own assist, in VS.")),
        Row::adjust(
            "play.p2.judge_offset_ms",
            tr!("2P judge offset"),
            ms(p.p2.judge_offset_ms),
            |c, d| offset(&mut c.config.play.p2.judge_offset_ms, d),
        )
        .desc(tr!("The second player's own judge offset, in VS.")),
    ]);
    if SHOW_PER_PLAY {
        b.group(
            "gameplay.per_play",
            tr!("ALSO IN THE SONG SELECT"),
            tr!("Random, gauge, hi-speed, lane cover, target, assist, BGA: changed between songs"),
        );
        per_play::rows(b);
    }
}

/// GAMEPLAY › EXPANDED JUDGE WINDOWS.
pub(super) fn expand_judge(b: &mut Rows) {
    let r = &b.ctx.config.play.judge_rates;
    b.header(tr!("EXPAND JUDGE"));
    let pct = |v: i64| format!("{v} %");
    b.extend([
        Row::adjust(
            "play.judge_rates.0",
            tr!("PGREAT window"),
            pct(r[0]),
            |c, d| window(c, 0, d),
        )
        .desc(tr!(
            "The PGREAT window with EXPAND JUDGE on, in % of the rule's."
        )),
        Row::adjust(
            "play.judge_rates.1",
            tr!("GREAT window"),
            pct(r[1]),
            |c, d| window(c, 1, d),
        )
        .desc(tr!("The GREAT window with EXPAND JUDGE on.")),
        Row::adjust(
            "play.judge_rates.2",
            tr!("GOOD window"),
            pct(r[2]),
            |c, d| window(c, 2, d),
        )
        .desc(tr!("The GOOD window with EXPAND JUDGE on.")),
    ]);
}
