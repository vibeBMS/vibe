use super::Row;
use crate::app::Ctx;
use crate::scene::option_panel::{cycle, onoff};
use vibebms_core::engine::JudgeAlgorithm;
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;

/// EXPAND JUDGE's window `k` (PGREAT, GREAT, GOOD) in 5 % steps.
fn window(c: &mut Ctx, k: usize, d: i32) {
    let j = &mut c.config.play.judge_rates[k];
    *j = (*j + d as i64 * 5).clamp(10, 400)
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let p = &ctx.config.play;
    vec![
        Row::adjust("Gauge", p.gauge.label(), |c, d| {
            c.config.play.gauge = cycle(&GaugeType::SELECTABLE, c.config.play.gauge, d)
        }),
        // the second player's own in VS
        Row::adjust("VS 2P gauge", p.p2.gauge.label(), |c, d| {
            c.config.play.p2.gauge = cycle(&GaugeType::SELECTABLE, c.config.play.p2.gauge, d)
        }),
        Row::adjust("VS 2P auto scratch", onoff(p.p2.auto_scratch), |c, _| {
            c.config.play.p2.auto_scratch ^= true
        }),
        Row::adjust(
            "VS 2P judge offset (ms)",
            format!("{:+}", p.p2.judge_offset_ms),
            |c, d| {
                let o = &mut c.config.play.p2.judge_offset_ms;
                *o = (*o + d).clamp(-500, 500)
            },
        ),
        Row::adjust("Gauge auto shift", p.gauge_auto_shift.label(), |c, d| {
            let s = &mut c.config.play.gauge_auto_shift;
            *s = cycle(&GaugeAutoShift::ALL, *s, d)
        }),
        Row::adjust("Gauge rules", p.gauge_preset.label(), |c, d| {
            let g = &mut c.config.play.gauge_preset;
            *g = cycle(&GaugePreset::ALL, *g, d)
        }),
        Row::adjust("Judge windows", p.judge_preset.label(), |c, d| {
            let j = &mut c.config.play.judge_preset;
            *j = cycle(&JudgePreset::ALL, *j, d)
        }),
        Row::adjust(
            "Judge algorithm",
            format!("{:?}", p.judge_algorithm).to_uppercase(),
            |c, d| {
                use JudgeAlgorithm::*;
                let a = &mut c.config.play.judge_algorithm;
                *a = cycle(&[Combo, Duration, Lowest], *a, d)
            },
        ),
        Row::adjust("EXPAND JUDGE", onoff(p.expand_judge), |c, _| {
            c.config.play.expand_judge ^= true
        }),
        Row::adjust("  PGREAT window %", p.judge_rates[0].to_string(), |c, d| {
            window(c, 0, d)
        }),
        Row::adjust("  GREAT window %", p.judge_rates[1].to_string(), |c, d| {
            window(c, 1, d)
        }),
        Row::adjust("  GOOD window %", p.judge_rates[2].to_string(), |c, d| {
            window(c, 2, d)
        }),
        Row::adjust("JUDGE AREA", onoff(p.judge_area), |c, _| {
            c.config.play.judge_area ^= true
        }),
        Row::adjust("MARK NOTE", onoff(p.mark_notes), |c, _| {
            c.config.play.mark_notes ^= true
        }),
        Row::adjust("BPM GUIDE", onoff(p.bpm_guide), |c, _| {
            c.config.play.bpm_guide ^= true
        }),
        Row::adjust(
            "Judge timing auto adjust",
            onoff(p.timing_auto_adjust),
            |c, _| c.config.play.timing_auto_adjust ^= true,
        ),
    ]
}
