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
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

crate::choices! {
    /// A judgement, best first. Scores, logs, ghosts and replays keep its
    /// index (`as usize`; serialised as that number).
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum Judge {
        #[default]
        PGreat => "PGREAT",
        Great => "GREAT",
        Good => "GOOD",
        Bad => "BAD",
        /// Missed note (note passed without being hit).
        Poor => "POOR",
        /// Empty poor / 空POOR (pressed with no note in range).
        Miss => "MISS",
    }
}

impl Judge {
    /// The judgement of index `i`.
    pub fn of(i: usize) -> Option<Judge> {
        Judge::ALL.get(i).copied()
    }

    /// Its EX score: PGREAT 2, GREAT 1.
    pub fn ex(self) -> u32 {
        match self {
            Judge::PGreat => 2,
            Judge::Great => 1,
            _ => 0,
        }
    }
}

impl Serialize for Judge {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for Judge {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Judge, D::Error> {
        let i = u8::deserialize(d)?;
        Judge::of(i as usize).ok_or_else(|| D::Error::custom(format!("no judgement {i}")))
    }
}

crate::choices! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub enum JudgePreset {
        #[default]
        Beatoraja => "beatoraja",
        Lr2 => "LR2",
        Iidx => "IIDX",
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

    /// The judgement of the first window containing `diff` (a note
    /// table's fifth window is the empty POOR's).
    pub fn judge(&self, t: NoteType, diff: i64) -> Option<Judge> {
        Judge::ALL
            .into_iter()
            .zip(self.table(t))
            .find(|(_, (l, e))| diff >= *l && diff <= *e)
            .map(|(j, _)| j)
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
    /// LR2's note rules (`docs/LR2_BEHAVIOUR.md`, 1, 4, 5): a press judges
    /// the lane's earliest unresolved object (mines included) and, after a
    /// BAD, the next one too (the BAD chain); windows at whole
    /// milliseconds; LR2's long notes and mines.
    pub lr2: bool,
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
    // GOOD narrower than GREAT: beatoraja's table has it so
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
        let pct = match r {
            JudgeRank::Rank(n) if (0..5).contains(&n) => self.rank[n as usize][1],
            JudgeRank::Rank(_) | JudgeRank::NoNumber => self.rank[2][1],
            JudgeRank::DefExRank(v) if v > 0.0 => (v * self.rank[2][1] as f64 / 100.0) as i64,
            JudgeRank::DefExRank(_) => self.rank[2][1],
            JudgeRank::Bmson(v) if v > 0.0 => v as i64,
            JudgeRank::Bmson(_) => 100,
        };
        // a chart can ask for anything; keep the window arithmetic in range
        pct.clamp(1, 10_000)
    }

    // Written from a behaviour specification (window scaling by rank
    // percentage, then the custom rates), not from another program's code.
    /// Scale a base table (µs) by the rank percentage `judgerank` and the
    /// custom rates `rate` (PG, GR, GD in percent).
    fn create(&self, org: &[(i64, i64)], judgerank: i64, rate: WindowRate) -> Vec<(i64, i64)> {
        // only PG..BAD take part in the ordering rules; empty POOR never does
        let ordered = org.len().min(4);
        let mut out: Vec<(i64, i64)> = org
            .iter()
            .enumerate()
            .map(|(i, &(late, early))| {
                if self.fixed.get(i).copied().unwrap_or(true) {
                    (late, early)
                } else {
                    (late * judgerank / 100, early * judgerank / 100)
                }
            })
            .collect();

        // a scaled window stays between its fixed neighbours
        for i in 0..ordered {
            if self.fixed[i] {
                continue;
            }
            let before = (0..i).rev().find(|&j| self.fixed[j]);
            let after = (i + 1..ordered).find(|&j| self.fixed[j]);
            for side in [Side::Late, Side::Early] {
                let mut v = side.get(out[i]);
                if let Some(j) = before {
                    let b = side.get(out[j]);
                    if v.abs() < b.abs() {
                        v = b;
                    }
                }
                if let Some(j) = after {
                    let a = side.get(out[j]);
                    if v.abs() > a.abs() {
                        v = a;
                    }
                }
                side.set(&mut out[i], v);
            }
        }

        // custom rates for PG, GR, GD; always keep PG <= GR <= GD <= BAD
        if ordered == 4 {
            for i in 0..3 {
                for side in [Side::Late, Side::Early] {
                    let mut v = side.get(out[i]) * rate[i] / 100;
                    let bad = side.get(out[3]);
                    if v.abs() > bad.abs() {
                        v = bad;
                    }
                    if i > 0 {
                        let prev = side.get(out[i - 1]);
                        if v.abs() < prev.abs() {
                            v = prev;
                        }
                    }
                    side.set(&mut out[i], v);
                }
            }
        }
        out
    }
}

/// One side of a `(late, early)` window.
#[derive(Clone, Copy)]
enum Side {
    Late,
    Early,
}

impl Side {
    fn get(self, w: (i64, i64)) -> i64 {
        match self {
            Side::Late => w.0,
            Side::Early => w.1,
        }
    }
    fn set(self, w: &mut (i64, i64), v: i64) {
        match self {
            Side::Late => w.0 = v,
            Side::Early => w.1 = v,
        }
    }
}

/// LR2 windows (µs) of PG, GR, GD by judge rank 0 / 25 (VERY HARD) / 50
/// (HARD) / 75 (NORMAL) / 100 % (EASY); BAD is ±200 ms at every rank.
const LR2_WINDOWS: [[i64; 5]; 3] = [
    [0, 8_000, 15_000, 18_000, 21_000],
    [0, 24_000, 30_000, 40_000, 60_000],
    [0, 40_000, 60_000, 100_000, 120_000],
];
const LR2_BAD: i64 = 200_000;

/// LR2's rules in every key mode (`docs/RULES.md`, sources): BAD and POOR
/// break the combo, an empty POOR keeps it; every judgement but the empty
/// POOR takes the note, so one note can give any number of empty POORs.
const LR2_COMBO: [bool; 6] = [true, true, true, false, false, true];
const LR2_VANISH: [bool; 6] = [true, true, true, true, true, false];

/// The judge rank in percent: `#RANK` 0..3 are 25 to 100, every other
/// value NORMAL (75), and a value that is not a number 0, VERY HARD, as
/// LR2 reads it (S-OLR2 2.2); `#DEFEXRANK` (which LR2 does not read) is a
/// percentage of NORMAL; bmson `judge_rank` is used as is.
fn lr2_rank(rank: JudgeRank) -> i64 {
    match rank {
        JudgeRank::Rank(n) if (0..4).contains(&n) => 25 * (n as i64 + 1),
        JudgeRank::NoNumber => 25,
        JudgeRank::Rank(_) => 75,
        JudgeRank::DefExRank(v) if v >= 1.0 => (v.min(10_000.0) as i64) * 75 / 100,
        JudgeRank::DefExRank(_) => 75,
        JudgeRank::Bmson(v) if v > 0.0 => (v.min(10_000.0) as i64).max(1),
        JudgeRank::Bmson(_) => 100,
    }
}

/// An LR2 window at judge rank `pct`: between the ranks of the table it
/// is interpolated, from EASY up it grows in proportion; whole
/// milliseconds, as LR2's are.
fn lr2_window(row: &[i64; 5], pct: i64) -> i64 {
    let w = if pct >= 100 {
        row[4] * pct / 100
    } else {
        let i = (pct / 25) as usize;
        row[i] + (pct - 25 * i as i64) * (row[i + 1] - row[i]) / 25
    };
    w / 1000 * 1000
}

/// LR2 windows in µs: PG, GR, GD, BAD and empty POOR (more than 200 and
/// less than 1000 ms early, never late: S-OLR2 1.4).
fn lr2_windows(rank: JudgeRank) -> Vec<(i64, i64)> {
    let pct = lr2_rank(rank);
    let mut v: Vec<(i64, i64)> = Vec::new();
    for row in &LR2_WINDOWS {
        let prev = v.last().map_or(0, |w: &(i64, i64)| w.1);
        let w = lr2_window(row, pct).clamp(prev, LR2_BAD);
        v.push((-w, w));
    }
    v.push((-LR2_BAD, LR2_BAD));
    v.push((LR2_BAD + 1, 999_999));
    v
}

/// LR2's judge windows of note windows `note` (PG .. empty POOR, µs).
fn lr2_judge_windows(note: Vec<(i64, i64)>) -> JudgeWindows {
    // LR2 judges a long note's release with the GOOD window for PG..GD
    // (LR2oraja: the LN end table is GOOD-wide at every #RANK), BAD as
    // usual
    let good = note[Judge::Good as usize];
    let ln_end = vec![good, good, good, note[Judge::Bad as usize]];
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

/// LR2's rules once a NONSTOP course's later stage gives its `#RANK`
/// (S-OLR2 2.2, S-OLR2b 3.5): their own table, PG / GR / GD 12 / 24 / 60
/// ms at 0 (and a value that is not a number), 15 / 30 / 80 at 1, 21 / 60
/// / 120 at 3 and 18 / 40 / 100 at any other; BAD and the empty POOR as
/// always.
pub fn lr2_nonstop_rules(fam: RuleFamily, rank: JudgeRank) -> JudgeRules {
    let ms: [i64; 3] = match rank {
        JudgeRank::Rank(0) | JudgeRank::NoNumber => [12, 24, 60],
        JudgeRank::Rank(1) => [15, 30, 80],
        JudgeRank::Rank(3) => [21, 60, 120],
        _ => [18, 40, 100],
    };
    let mut note: Vec<(i64, i64)> = ms.iter().map(|w| (-w * 1000, w * 1000)).collect();
    note.push((-LR2_BAD, LR2_BAD));
    note.push((LR2_BAD + 1, 999_999));
    JudgeRules {
        windows: lr2_judge_windows(note),
        ..judge_rules(JudgePreset::Lr2, fam, JudgeRank::Rank(2), None)
    }
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
        JudgePreset::Lr2 => lr2_judge_windows(lr2_windows(rank)),
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
    let (combo, vanish, miss_one) = match preset {
        JudgePreset::Lr2 => (LR2_COMBO, LR2_VANISH, false),
        _ => (f.combo, f.vanish, f.miss_one),
    };
    JudgeRules {
        windows,
        combo,
        miss_one,
        vanish,
        lr2: preset == JudgePreset::Lr2,
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
        assert_eq!(
            normal.windows.judge(NoteType::Note, 10_000),
            Some(Judge::PGreat)
        );
        assert_eq!(
            normal.windows.judge(NoteType::Note, -40_000),
            Some(Judge::Great)
        );
        assert_eq!(normal.windows.judge(NoteType::Note, -1_000_000), None);
    }

    /// beatoraja preset: a chart without `#RANK` (the parser's default), an
    /// out-of-range one and one that is not a number all judge NORMAL (the
    /// PMS rule's 70 %); a bmson `judge_rank` of 0 or less 100 %
    /// (docs/RULES.md: why a missing `#RANK` stays NORMAL).
    #[test]
    fn beatoraja_rank_fallbacks() {
        for fam in [RuleFamily::FiveKeys, RuleFamily::SevenKeys, RuleFamily::Pms] {
            let w = |rank| judge_rules(JudgePreset::Beatoraja, fam, rank, None).windows;
            let normal = w(JudgeRank::Rank(2));
            assert_eq!(w(JudgeRank::default()), normal);
            for odd in [JudgeRank::Rank(-1), JudgeRank::Rank(5), JudgeRank::NoNumber] {
                assert_eq!(w(odd), normal, "{fam:?} {odd:?}");
            }
            assert_eq!(w(JudgeRank::Bmson(0.0)), w(JudgeRank::Bmson(100.0)));
        }
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
        // empty POOR: more than 200 and less than 1000 ms early, never late
        assert_eq!(r.windows.note[4], (200_001, 999_999));
        let lr2 = |rank| {
            judge_rules(JudgePreset::Lr2, RuleFamily::SevenKeys, rank, None)
                .windows
                .note
        };
        assert_eq!(
            lr2(JudgeRank::Rank(0))[..3],
            [(-8_000, 8_000), (-24_000, 24_000), (-40_000, 40_000)]
        );
        assert_eq!(lr2(JudgeRank::Rank(3))[2], (-120_000, 120_000));
        // #RANK 4 and out of range: NORMAL; not a number: VERY HARD
        assert_eq!(lr2(JudgeRank::NoNumber), lr2(JudgeRank::Rank(0)));
        assert_eq!(lr2(JudgeRank::Rank(4)), r.windows.note);
        assert_eq!(lr2(JudgeRank::Rank(-1)), r.windows.note);
        assert_eq!(lr2(JudgeRank::Rank(12)), r.windows.note);
        // #DEFEXRANK 100 is NORMAL; 120 is 90 %: 60 % of the way to EASY,
        // in whole ms
        assert_eq!(lr2(JudgeRank::DefExRank(100.0)), r.windows.note);
        let d = lr2(JudgeRank::DefExRank(120.0));
        assert_eq!(
            d[..3],
            [(-19_000, 19_000), (-52_000, 52_000), (-112_000, 112_000)]
        );
        // bmson 200 %: EASY doubled, kept inside BAD
        let b = lr2(JudgeRank::Bmson(200.0));
        assert_eq!(
            b[..3],
            [(-42_000, 42_000), (-120_000, 120_000), (-200_000, 200_000)]
        );
    }

    #[test]
    fn lr2_rules_in_every_key_mode() {
        for fam in [RuleFamily::FiveKeys, RuleFamily::SevenKeys, RuleFamily::Pms] {
            let r = judge_rules(JudgePreset::Lr2, fam, JudgeRank::Rank(2), None);
            // an empty POOR keeps the combo and never takes the note
            assert!(r.combo[Judge::Miss as usize] && !r.vanish[Judge::Miss as usize]);
            // BAD and POOR break it; a BAD takes the note
            assert!(!r.combo[Judge::Bad as usize] && !r.combo[Judge::Poor as usize]);
            assert!(r.vanish[Judge::Bad as usize]);
            assert!(!r.miss_one);
        }
    }

    #[test]
    fn lr2_ln_end_uses_good_window() {
        let r = judge_rules(
            JudgePreset::Lr2,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(2),
            None,
        );
        let w = &r.windows;
        // released 25 ms early: still PGREAT (not the ±18 ms note window)
        assert_eq!(w.judge(NoteType::LongEnd, 25_000), Some(Judge::PGreat));
        assert_eq!(w.judge(NoteType::LongEnd, -100_000), Some(Judge::PGreat));
        assert_eq!(w.judge(NoteType::LongEnd, 150_000), Some(Judge::Bad));
        assert_eq!(
            w.judge(NoteType::LongScratchEnd, 99_000),
            Some(Judge::PGreat)
        );
        // VERY HARD: a CN tail 30 ms off is a PGREAT (GOOD there is ±40 ms)
        let vh = judge_rules(
            JudgePreset::Lr2,
            RuleFamily::SevenKeys,
            JudgeRank::Rank(0),
            None,
        );
        assert_eq!(
            vh.windows.judge(NoteType::LongEnd, -30_000),
            Some(Judge::PGreat)
        );
        assert_eq!(
            vh.windows.judge(NoteType::LongEnd, -50_000),
            Some(Judge::Bad)
        );
        assert_eq!(vh.windows.note[0], (-8_000, 8_000));
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
        assert_eq!(
            w.judge(NoteType::Note, w.quantize(20_000)),
            Some(Judge::PGreat)
        );
        assert_eq!(
            w.judge(NoteType::Note, w.quantize(-21_000)),
            Some(Judge::Great)
        );
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
