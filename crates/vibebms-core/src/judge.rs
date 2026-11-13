//! Judge windows.
//!
//! Window tables are pairs of `(late, early)` bounds in microseconds where the
//! timing difference is `note_time - press_time` (positive = pressed early,
//! FAST). Index order is PGREAT, GREAT, GOOD, BAD, (empty) POOR.
//!
//! * `Beatoraja` preset: values and #RANK scaling rule of beatoraja
//!   (`JudgeProperty` / `JudgeWindowRule`).
//! * `Lr2` preset: the classic LR2 per-#RANK windows (±8/24/40 … ±21/60/120,
//!   BAD ±200 ms).
//! * `Iidx` preset: windows counted in frames of the 120 Hz LIGHTNING MODEL
//!   cabinet (PG ±2F, GR ±4F, GD ±14F, BAD ±30F = ±16.67/33.33/116.67/250 ms)
//!   with input timing quantised to the 120 Hz frame grid. Konami does not
//!   publish the windows; these are community figures, see `docs/RULES.md`.

use crate::chart::JudgeRank;
use crate::mode::RuleFamily;
use serde::{Deserialize, Serialize};

pub const PGREAT: usize = 0;
pub const GREAT: usize = 1;
pub const GOOD: usize = 2;
pub const BAD: usize = 3;
/// Missed note (note passed without being hit).
pub const POOR: usize = 4;
/// Empty poor / 空POOR (pressed with no note in range).
pub const MISS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum JudgePreset {
    #[default]
    Beatoraja,
    Lr2,
    Iidx,
}

impl JudgePreset {
    pub const ALL: [JudgePreset; 3] = [JudgePreset::Beatoraja, JudgePreset::Lr2, JudgePreset::Iidx];
    pub fn label(self) -> &'static str {
        match self {
            JudgePreset::Beatoraja => "beatoraja",
            JudgePreset::Lr2 => "LR2",
            JudgePreset::Iidx => "IIDX",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteType {
    Note,
    Scratch,
    LongEnd,
    LongScratchEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JudgeWindows {
    pub note: Vec<(i64, i64)>,
    pub scratch: Vec<(i64, i64)>,
    pub ln_end: Vec<(i64, i64)>,
    pub scratch_ln_end: Vec<(i64, i64)>,
    /// Grace period after an early LN release before it is judged (µs).
    pub ln_margin: i64,
    pub scratch_ln_margin: i64,
    /// Quantise timing differences to frames of this rate (IIDX: 120 Hz).
    pub frame_hz: Option<u32>,
}

/// Duration of `frames` frames at `hz` in µs (rounded).
pub fn frames_us(frames: i64, hz: u32) -> i64 {
    let hz = hz.max(1) as i64;
    let v = frames * 1_000_000;
    (v + v.signum() * hz / 2) / hz
}

impl JudgeWindows {
    /// Snap a timing difference to the frame grid (no-op without `frame_hz`).
    pub fn quantize(&self, diff: i64) -> i64 {
        match self.frame_hz {
            Some(hz) => {
                let hz = hz.max(1) as i64;
                let f = (diff * hz + diff.signum() * 500_000) / 1_000_000;
                frames_us(f, hz as u32)
            }
            None => diff,
        }
    }

    pub fn table(&self, t: NoteType) -> &[(i64, i64)] {
        match t {
            NoteType::Note => &self.note,
            NoteType::Scratch => &self.scratch,
            NoteType::LongEnd => &self.ln_end,
            NoteType::LongScratchEnd => &self.scratch_ln_end,
        }
    }

    /// Index of the first window containing `diff`, or `table.len()` if none.
    pub fn judge(&self, t: NoteType, diff: i64) -> usize {
        let tab = self.table(t);
        tab.iter()
            .position(|(l, e)| diff >= *l && diff <= *e)
            .unwrap_or(tab.len())
    }

    /// Bound of window `judge` (`early` = upper bound). 0 when out of range.
    pub fn time(&self, t: NoteType, judge: usize, early: bool) -> i64 {
        self.table(t)
            .get(judge)
            .map(|w| if early { w.1 } else { w.0 })
            .unwrap_or(0)
    }

    /// Smallest late bound / largest early bound over note and scratch tables.
    pub fn extent(&self) -> (i64, i64) {
        let mut s = 0;
        let mut e = 0;
        for (l, r) in self.note.iter().chain(self.scratch.iter()) {
            s = s.min(*l);
            e = e.max(*r);
        }
        (s, e)
    }
}

/// Everything rule-dependent the judge engine needs.
#[derive(Clone, Debug, PartialEq)]
pub struct JudgeRules {
    pub windows: JudgeWindows,
    /// Whether each judgement (PG..MISS) keeps the combo.
    pub combo: [bool; 6],
    /// PMS: a note can produce at most one POOR.
    pub miss_one: bool,
    /// Whether each judgement consumes the note.
    pub vanish: [bool; 6],
}

/// Custom window scaling in percent for PG / GR / GD (beatoraja "custom judge").
pub type WindowRate = [i64; 3];

fn us(ms: &[(i64, i64)]) -> Vec<(i64, i64)> {
    ms.iter().map(|(l, e)| (l * 1000, e * 1000)).collect()
}

struct Family {
    note: &'static [(i64, i64)],
    scratch: &'static [(i64, i64)],
    ln_end: &'static [(i64, i64)],
    ln_margin: i64,
    scratch_ln_end: &'static [(i64, i64)],
    combo: [bool; 6],
    miss_one: bool,
    vanish: [bool; 6],
    pms_rule: bool,
}

// beatoraja JudgeProperty values (ms)
const FIVE: Family = Family {
    note: &[(-20, 20), (-50, 50), (-100, 100), (-150, 150), (-150, 500)],
    scratch: &[(-30, 30), (-60, 60), (-110, 110), (-160, 160), (-160, 500)],
    ln_end: &[(-120, 120), (-150, 150), (-200, 200), (-250, 250)],
    ln_margin: 0,
    scratch_ln_end: &[(-130, 130), (-160, 160), (-110, 110), (-260, 260)],
    combo: [true, true, true, false, false, false],
    miss_one: false,
    vanish: [true, true, true, true, true, false],
    pms_rule: false,
};
const SEVEN: Family = Family {
    note: &[(-20, 20), (-60, 60), (-150, 150), (-280, 220), (-150, 500)],
    scratch: &[(-30, 30), (-70, 70), (-160, 160), (-290, 230), (-160, 500)],
    ln_end: &[(-120, 120), (-160, 160), (-200, 200), (-280, 220)],
    ln_margin: 0,
    scratch_ln_end: &[(-130, 130), (-170, 170), (-210, 210), (-290, 230)],
    combo: [true, true, true, false, false, true],
    miss_one: false,
    vanish: [true, true, true, true, true, false],
    pms_rule: false,
};
const PMS: Family = Family {
    note: &[(-20, 20), (-50, 50), (-117, 117), (-183, 183), (-175, 500)],
    scratch: &[],
    ln_end: &[(-120, 120), (-150, 150), (-217, 217), (-283, 283)],
    ln_margin: 200,
    scratch_ln_end: &[],
    combo: [true, true, true, false, false, false],
    miss_one: true,
    vanish: [true, true, true, false, true, false],
    pms_rule: true,
};

fn family(f: RuleFamily) -> &'static Family {
    match f {
        RuleFamily::FiveKeys => &FIVE,
        RuleFamily::SevenKeys => &SEVEN,
        RuleFamily::Pms => &PMS,
    }
}

/// beatoraja `JudgeWindowRule` (NORMAL / PMS).
struct WindowRule {
    /// per #RANK (VERY HARD .. VERY EASY) and per judgement, percent.
    rank: [[i64; 5]; 5],
    fixed: [bool; 5],
}

const RULE_NORMAL: WindowRule = WindowRule {
    rank: [
        [25, 25, 25, 25, 100],
        [50, 50, 50, 50, 100],
        [75, 75, 75, 75, 100],
        [100, 100, 100, 100, 100],
        [125, 125, 125, 125, 100],
    ],
    fixed: [false, false, false, false, true],
};
const RULE_PMS: WindowRule = WindowRule {
    rank: [
        [100, 33, 33, 100, 100],
        [100, 50, 50, 100, 100],
        [100, 70, 70, 100, 100],
        [100, 100, 100, 100, 100],
        [100, 133, 133, 100, 100],
    ],
    fixed: [true, false, false, true, true],
};

impl WindowRule {
    /// Convert a chart's rank to the single percentage beatoraja stores.
    fn judgerank(&self, r: JudgeRank) -> i64 {
        match r {
            JudgeRank::Rank(n) if (0..5).contains(&n) => self.rank[n as usize][1],
            JudgeRank::Rank(_) => self.rank[2][1],
            JudgeRank::DefExRank(v) if v > 0.0 => (v * self.rank[2][1] as f64 / 100.0) as i64,
            JudgeRank::DefExRank(_) => self.rank[2][1],
            JudgeRank::Bmson(v) if v > 0.0 => v as i64,
            JudgeRank::Bmson(_) => 100,
        }
    }

    fn create(&self, org: &[(i64, i64)], judgerank: i64, rate: WindowRate) -> Vec<(i64, i64)> {
        let n = org.len();
        let pct: Vec<i64> = (0..n)
            .map(|i| {
                if self.fixed.get(i).copied().unwrap_or(true) {
                    100
                } else {
                    judgerank
                }
            })
            .collect();
        let mut j: Vec<(i64, i64)> = (0..n)
            .map(|i| {
                if self.fixed.get(i).copied().unwrap_or(true) {
                    org[i]
                } else {
                    (org[i].0 * pct[i] / 100, org[i].1 * pct[i] / 100)
                }
            })
            .collect();
        // keep scaled windows between neighbouring fixed windows
        let mut fixmin: Option<usize> = None;
        for i in 0..n.min(4) {
            if self.fixed[i] {
                fixmin = Some(i);
                continue;
            }
            let fixmax = (i + 1..4).find(|k| self.fixed[*k]);
            for side in 0..2 {
                let get = |w: (i64, i64)| if side == 0 { w.0 } else { w.1 };
                let mut v = get(j[i]);
                if let Some(m) = fixmin {
                    if v.abs() < get(j[m]).abs() {
                        v = get(j[m]);
                    }
                }
                if let Some(m) = fixmax {
                    if m < n && v.abs() > get(j[m]).abs() {
                        v = get(j[m]);
                    }
                }
                if side == 0 {
                    j[i].0 = v
                } else {
                    j[i].1 = v
                }
            }
        }
        // custom window rate for PG/GR/GD, clamped to BAD and monotonic
        if n >= 4 {
            for i in 0..3 {
                for side in 0..2 {
                    let bad = if side == 0 { j[3].0 } else { j[3].1 };
                    let mut v = if side == 0 { j[i].0 } else { j[i].1 } * rate[i] / 100;
                    if v.abs() > bad.abs() {
                        v = bad;
                    }
                    if i > 0 {
                        let prev = if side == 0 { j[i - 1].0 } else { j[i - 1].1 };
                        if v.abs() < prev.abs() {
                            v = prev;
                        }
                    }
                    if side == 0 {
                        j[i].0 = v
                    } else {
                        j[i].1 = v
                    }
                }
            }
        }
        j
    }
}

/// LR2 windows (ms) for #RANK 0..=3: PG, GR, GD; BAD is ±200 for all ranks.
const LR2_WINDOWS: [[i64; 3]; 4] = [[8, 24, 40], [15, 30, 60], [18, 40, 100], [21, 60, 120]];

fn lr2_windows(rank: JudgeRank) -> Vec<(i64, i64)> {
    let (w, scale) = match rank {
        JudgeRank::Rank(n) => (LR2_WINDOWS[n.clamp(0, 3) as usize], 100),
        // LR2 has no DEFEXRANK; scale NORMAL like beatoraja does.
        JudgeRank::DefExRank(v) => (LR2_WINDOWS[2], v.clamp(10.0, 200.0) as i64),
        JudgeRank::Bmson(v) => (LR2_WINDOWS[3], v.clamp(10.0, 200.0) as i64),
    };
    let s = |ms: i64| (ms * scale / 100).min(200);
    let mut v: Vec<(i64, i64)> = w.iter().map(|ms| (-s(*ms), s(*ms))).collect();
    v.push((-200, 200));
    v.push((-150, 500));
    v
}

/// IIDX windows in 120 Hz frames (late, early): PG, GR, GD, BAD, empty POOR.
pub const IIDX_HZ: u32 = 120;
const IIDX_NOTE_FRAMES: [(i64, i64); 5] = [(-2, 2), (-4, 4), (-14, 14), (-30, 30), (-18, 60)];
const IIDX_LN_END_FRAMES: [(i64, i64); 4] = [(-14, 14), (-19, 19), (-24, 24), (-30, 30)];

fn iidx(frames: &[(i64, i64)]) -> Vec<(i64, i64)> {
    frames
        .iter()
        .map(|(l, e)| (frames_us(*l, IIDX_HZ), frames_us(*e, IIDX_HZ)))
        .collect()
}

pub fn judge_rules(
    preset: JudgePreset,
    fam: RuleFamily,
    rank: JudgeRank,
    rate: Option<WindowRate>,
) -> JudgeRules {
    let f = family(fam);
    let rate = rate.unwrap_or([100, 100, 100]);
    let windows = match preset {
        JudgePreset::Beatoraja => {
            let rule = if f.pms_rule { &RULE_PMS } else { &RULE_NORMAL };
            let jr = rule.judgerank(rank);
            let mk = |t: &[(i64, i64)]| rule.create(&us(t), jr, rate);
            let note = mk(f.note);
            let scratch = if f.scratch.is_empty() {
                note.clone()
            } else {
                mk(f.scratch)
            };
            let ln_end = mk(f.ln_end);
            let scratch_ln_end = if f.scratch_ln_end.is_empty() {
                ln_end.clone()
            } else {
                mk(f.scratch_ln_end)
            };
            JudgeWindows {
                note,
                scratch,
                ln_end,
                scratch_ln_end,
                ln_margin: f.ln_margin * 1000,
                scratch_ln_margin: 0,
                frame_hz: None,
            }
        }
        JudgePreset::Lr2 => {
            let note = us(&lr2_windows(rank));
            let ln_end = note[..4].to_vec();
            JudgeWindows {
                note: note.clone(),
                scratch: note,
                ln_end: ln_end.clone(),
                scratch_ln_end: ln_end,
                ln_margin: 0,
                scratch_ln_margin: 0,
                frame_hz: None,
            }
        }
        JudgePreset::Iidx => JudgeWindows {
            note: iidx(&IIDX_NOTE_FRAMES),
            scratch: iidx(&IIDX_NOTE_FRAMES),
            ln_end: iidx(&IIDX_LN_END_FRAMES),
            scratch_ln_end: iidx(&IIDX_LN_END_FRAMES),
            ln_margin: 0,
            scratch_ln_margin: 0,
            frame_hz: Some(IIDX_HZ),
        },
    };
    JudgeRules {
        windows,
        combo: f.combo,
        miss_one: f.miss_one,
        vanish: f.vanish,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beatoraja_7k_rank_scaling() {
        let easy = judge_rules(
            JudgePreset::Beatoraja,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(3),
            None,
        );
        assert_eq!(easy.windows.note[0], (-20_000, 20_000));
        assert_eq!(easy.windows.note[3], (-280_000, 220_000));
        let normal = judge_rules(
            JudgePreset::Beatoraja,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(2),
            None,
        );
        assert_eq!(normal.windows.note[0], (-15_000, 15_000));
        assert_eq!(normal.windows.note[2], (-112_500, 112_500));
        // empty-poor window is fixed
        assert_eq!(normal.windows.note[4], (-150_000, 500_000));
        // DEFEXRANK 100 == NORMAL
        let d = judge_rules(
            JudgePreset::Beatoraja,
            RuleFamily::SevenKeys,
            JudgeRank::DefExRank(100.0),
            None,
        );
        assert_eq!(d.windows.note, normal.windows.note);
        assert_eq!(normal.windows.judge(NoteType::Note, 10_000), PGREAT);
        assert_eq!(normal.windows.judge(NoteType::Note, -40_000), GREAT);
        assert_eq!(normal.windows.judge(NoteType::Note, -1_000_000), 5);
    }

    #[test]
    fn pms_rule_keeps_pg_and_bad_fixed() {
        let hard = judge_rules(
            JudgePreset::Beatoraja,
            RuleFamily::Pms,
            JudgeRank::Rank(0),
            None,
        );
        assert_eq!(hard.windows.note[0], (-20_000, 20_000));
        assert_eq!(hard.windows.note[3], (-183_000, 183_000));
        assert_eq!(hard.windows.note[1], (-20_000, 20_000)); // 50*33% clamped up to PG
    }

    #[test]
    fn lr2_windows_table() {
        let r = judge_rules(
            JudgePreset::Lr2,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(2),
            None,
        );
        assert_eq!(r.windows.note[0], (-18_000, 18_000));
        assert_eq!(r.windows.note[2], (-100_000, 100_000));
        assert_eq!(r.windows.note[3], (-200_000, 200_000));
    }

    #[test]
    fn iidx_120hz_frames() {
        let r = judge_rules(
            JudgePreset::Iidx,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(2),
            None,
        );
        let w = &r.windows;
        assert_eq!(w.note[0], (-16_667, 16_667));
        assert_eq!(w.note[1], (-33_333, 33_333));
        assert_eq!(w.note[3], (-250_000, 250_000));
        // 20 ms snaps to 2 frames (16.67 ms) -> PGREAT; 21 ms snaps to 3 frames -> GREAT
        assert_eq!(w.judge(NoteType::Note, w.quantize(20_000)), PGREAT);
        assert_eq!(w.judge(NoteType::Note, w.quantize(-21_000)), GREAT);
        assert_eq!(w.quantize(4_000), 0);
        assert_eq!(w.quantize(-4_200), -8_333);
    }

    #[test]
    fn custom_rate() {
        let r = judge_rules(
            JudgePreset::Beatoraja,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(3),
            Some([200, 100, 100]),
        );
        assert_eq!(r.windows.note[0], (-40_000, 40_000));
        assert_eq!(r.windows.note[1], (-60_000, 60_000));
    }
}
