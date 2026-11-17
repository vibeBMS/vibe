//! The charts of `tools/lr2-measure` played by vibeBMS as its README tells
//! a player to play them in LR2, with the timing errors a player has: each
//! chart gives vibeBMS's row of the README's tables, which is what
//! `docs/LR2_BEHAVIOUR.md` (S-OLR2) predicts LR2 shows. The README's other
//! rows (the rules vibeBMS does not have) are worked out by `gen.py`.

use std::path::{Path, PathBuf};
use vibebms_core::chart::Chart;
use vibebms_core::engine::{Engine, JudgeAlgorithm, Op};
use vibebms_core::gauge::{GaugePreset, GaugeType};
use vibebms_core::judge::{Judge, JudgePreset};
use vibebms_core::play::{build_chart, build_engine, RuleOptions};

const MS: i64 = 1000;
/// The first trial (two measures at BPM 150) and a trial's length.
const LEAD: i64 = 3200 * MS;
const TRIAL: i64 = 3200 * MS;
const TRIALS: i64 = 16;
/// A player's timing errors, one per trial in turn (ms, + late).
const ERRORS: [i64; 5] = [-25, -10, 0, 10, 25];
const KEY1: u8 = 0;
const KEY6: u8 = 5;
const KEY7: u8 = 6;
const SCRATCH: u8 = 7;

fn rules(algorithm: JudgeAlgorithm, gauge: GaugeType) -> RuleOptions {
    RuleOptions {
        judge_preset: JudgePreset::Lr2,
        gauge_preset: GaugePreset::Lr2,
        judge_algorithm: algorithm,
        gauge,
        ..RuleOptions::default()
    }
}

fn chart(name: &str, rules: &RuleOptions) -> Chart {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/lr2-measure/charts")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    build_chart(Path::new(name), &bytes, rules, 1, None)
        .unwrap()
        .chart
}

/// Presses and holds (time, lane, key) and readings: the engine advanced
/// to each time in turn; a reading gives the gauge then.
enum Step {
    /// Down, and up 20 ms later.
    Press(i64, u8, u8),
    /// Down for good.
    Hold(i64, u8, u8),
    Read(i64),
}

fn run(e: &mut Engine, mut steps: Vec<Step>, end: i64) -> Vec<f32> {
    let at = |s: &Step| match s {
        Step::Press(t, ..) | Step::Hold(t, ..) | Step::Read(t) => *t,
    };
    steps.sort_by_key(at);
    let mut readings = Vec::new();
    for s in steps {
        e.apply(Op::Update(at(&s)));
        match s {
            Step::Press(t, lane, key) => {
                e.apply(Op::Input(t, lane, key, true));
                e.apply(Op::Update(t + MS));
                e.apply(Op::Input(t + 20 * MS, lane, key, false));
            }
            Step::Hold(t, lane, key) => e.apply(Op::Input(t, lane, key, true)),
            Step::Read(_) => readings.push(e.gauge.value()),
        }
    }
    e.apply(Op::Update(end));
    readings
}

/// The A11 chart `name` played with `algorithm`: `presses(t)` the presses
/// of the trial starting at t.
fn a11(name: &str, algorithm: JudgeAlgorithm, presses: fn(i64) -> Vec<Step>) -> [u32; 6] {
    let r = rules(algorithm, GaugeType::Normal);
    let c = chart(name, &r);
    let (mut e, _) = build_engine(&c, &r, None, false);
    let steps = (0..TRIALS)
        .flat_map(|k| {
            let err = ERRORS[k as usize % ERRORS.len()] * MS;
            presses(LEAD + k * TRIAL).into_iter().map(move |s| match s {
                Step::Press(t, l, key) => Step::Press(t + err, l, key),
                s => s,
            })
        })
        .collect();
    run(&mut e, steps, c.length() + 2_000 * MS);
    Judge::ALL.map(|j| e.score.count(j))
}

/// README, A11: under the LR2 judge a press takes the lane's earliest
/// unresolved note and, after a BAD, the next one (the BAD chain), whatever
/// the note choice option says: the counts S-OLR2 predicts.
#[test]
fn the_a11_charts_give_the_lr2_judges_counts() {
    use JudgeAlgorithm::{Combo, Duration, Lowest};
    let bad = |c: [u32; 6]| c[Judge::Bad as usize];
    let good = |c: [u32; 6]| c[Judge::Good as usize];
    let poor = |c: [u32; 6]| c[Judge::Poor as usize];
    for a in [Combo, Duration, Lowest] {
        // 1: key 1 and key 7 together on the key-7 note: the first key-1
        // note a BAD, the second taken by the same press
        let one = a11("a11-1.bme", a, |t| {
            vec![
                Step::Press(t + 120 * MS, KEY1, 0),
                Step::Press(t + 120 * MS, KEY7, 0),
            ]
        });
        assert_eq!((bad(one), poor(one)), (16, 0), "{a:?}");
        // 2: the same, EASY: a GOOD, no chain, the second note missed
        let two = a11("a11-2.bme", a, |t| {
            vec![
                Step::Press(t + 90 * MS, KEY1, 0),
                Step::Press(t + 90 * MS, KEY7, 0),
            ]
        });
        assert_eq!((good(two), poor(two)), (16, 16), "{a:?}");
        // 3: scratch with key 6 (two BADs by the chain), then the other
        // scratch key with key 7 (the next scratch note is a trial away)
        let three = a11("a11-3.bme", a, |t| {
            vec![
                Step::Press(t + 160 * MS, SCRATCH, 0),
                Step::Press(t + 160 * MS, KEY6, 0),
                Step::Press(t + 240 * MS, SCRATCH, 1),
                Step::Press(t + 240 * MS, KEY7, 0),
            ]
        });
        assert_eq!((bad(three), poor(three)), (32, 0), "{a:?}");
    }
}

/// README, A7: vibeBMS's readings of the GROOVE gauge on autoplay, in the
/// rest after each group of 10 notes, as LR2 shows them (2 % steps).
#[test]
fn the_a7_readings_are_vibebms_default_total() {
    let shown = |v: f32| (v / 2.0 + 1e-4).floor() as u32 * 2;
    for (n, groups, want) in [
        (200, 9, vec![28, 38, 48, 58, 68, 76, 86, 96, 100]),
        (
            500,
            16,
            vec![
                24, 30, 34, 40, 44, 50, 54, 60, 66, 70, 76, 80, 86, 90, 96, 100,
            ],
        ),
        (
            1000,
            23,
            vec![
                22, 26, 30, 34, 36, 40, 44, 48, 50, 54, 58, 62, 64, 68, 72, 76, 78, 82, 86, 90, 92,
                96, 100,
            ],
        ),
    ] {
        let r = rules(JudgeAlgorithm::Combo, GaugeType::Normal);
        let c = chart(&format!("a7-n{n}.bme"), &r);
        assert_eq!(c.total_judgements_of(r.ln_mode, None), n);
        let (mut e, _) = build_engine(&c, &r, None, true);
        let reads = (1..=groups).map(|g| Step::Read(LEAD + g * TRIAL - 100 * MS));
        let got = run(&mut e, reads.collect(), c.length());
        assert_eq!(got.into_iter().map(shown).collect::<Vec<_>>(), want, "{n}");
    }
}

/// README, A4: vibeBMS's HARD gauge after three empty POORs, the notes
/// before them hit.
#[test]
fn the_a4_readings_are_vibebms_note_factor() {
    let shown = |v: f32| (v / 2.0 + 1e-4).floor() as u32 * 2;
    for (n, want) in [
        (20, 40),
        (29, 50),
        (30, 58),
        (31, 58),
        (45, 64),
        (60, 70),
        (90, 72),
        (110, 74),
        (124, 74),
        (125, 76),
    ] {
        let r = rules(JudgeAlgorithm::Combo, GaugeType::Hard);
        let c = chart(&format!("a4-n{n}.bme"), &r);
        let (mut e, _) = build_engine(&c, &r, None, false);
        let last = c.notes.iter().map(|x| x.time).max().unwrap();
        let mut steps: Vec<Step> = c
            .notes
            .iter()
            .filter(|x| x.time < last)
            .map(|x| Step::Press(x.time, KEY7, 0))
            .collect();
        for early in [900, 700, 500] {
            steps.push(Step::Press(last - early * MS, KEY1, 0));
        }
        steps.push(Step::Read(last - 300 * MS));
        let got = run(&mut e, steps, last);
        assert_eq!(shown(got[0]), want, "{n} notes");
    }
}

/// README, A8: vibeBMS's GROOVE gauge after each mine, key 1 held through
/// them from the rest before the first; `ZZ` leaves 2 %.
#[test]
fn the_a8_readings_are_vibebms_mines() {
    let r = rules(JudgeAlgorithm::Combo, GaugeType::Normal);
    let c = chart("a8-mines.bme", &r);
    let (mut e, _) = build_engine(&c, &r, None, false);
    let mut steps: Vec<Step> = (0..8)
        .map(|i| Step::Press(LEAD + i * 100 * MS, KEY7, 0))
        .collect();
    steps.push(Step::Hold(LEAD + TRIAL, KEY1, 0));
    // the mines are at the start of trials 2 .. 5
    steps.extend((2..6).map(|k| Step::Read(LEAD + k * TRIAL + 1000 * MS)));
    let got = run(&mut e, steps, c.length());
    assert_eq!(got, [96.0, 76.0, 26.0, 2.0]);
}

/// README, A3: a `#RANK` chart's counts against the references of `#RANK`
/// 0, 2 and 3; vibeBMS plays 4, 5 and -1 as 2 (NORMAL).
#[test]
fn the_a3_charts_tell_the_ranks_apart() {
    let counts = |rank: &str, late: i64| {
        let r = rules(JudgeAlgorithm::Combo, GaugeType::Normal);
        let c = chart(&format!("a3-rank{rank}-{late}.bme"), &r);
        let (mut e, _) = build_engine(&c, &r, None, false);
        let steps = (0..TRIALS)
            .flat_map(|k| {
                let t = LEAD + k * TRIAL + late * MS + ERRORS[k as usize % 5] * MS;
                [Step::Press(t, KEY1, 0), Step::Press(t, KEY7, 0)]
            })
            .collect();
        run(&mut e, steps, c.length() + 2_000 * MS);
        Judge::ALL.map(|j| e.score.count(j))
    };
    for late in [70, 110] {
        let normal = counts("2", late);
        for rank in ["4", "5", "m1"] {
            assert_eq!(counts(rank, late), normal, "#RANK {rank}, {late} ms");
        }
    }
    // the references differ from each other at one lateness or the other
    for (a, b) in [("0", "2"), ("2", "3"), ("0", "3")] {
        assert!(counts(a, 70) != counts(b, 70) || counts(a, 110) != counts(b, 110));
    }
}
