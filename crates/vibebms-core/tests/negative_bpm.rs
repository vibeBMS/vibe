//! A BMS chart's negative BPM, as LR2 plays it (`docs/RULES.md`, "Negative
//! BPM"): its absolute value is the tempo, and from the time it is reached
//! (its fold point) the judge's clock runs back; a later negative BPM's
//! fold point turns it back from there.

use std::path::Path;
use vibebms_core::bms::{parse_bms, ParseOptions};
use vibebms_core::chart::Chart;
use vibebms_core::engine::{Engine, InputEvent, Op};
use vibebms_core::judge::Judge;
use vibebms_core::play::{build_engine, build_nonstop_chart, RuleOptions};

const MS: i64 = 1_000;
const SEC: i64 = 1_000_000;

/// 120 BPM (a measure 2 s), `#BPM01 -120` at measure 2 (4 s), `#BPM02 60`
/// at measure 3 (6 s, a measure 4 s from there), `#BPM03 -240` at
/// measure 4 (10 s). A note on lane 0 at 2 s, 1 at 3.9 s, 2 at 4.1 s, 3
/// at 5 s, 4 at 6 s, 1 at 9.9 s.
const CHART: &str = "#BPM 120\n#RANK 2\n#BPM01 -120\n#BPM02 60\n#BPM03 -240\n#BPM04 -90\n\
#WAV01 a.wav\n\
#00111:01\n\
#00112:0000000000000000000000000000000000000001\n\
#00208:01\n\
#00213:0001000000000000000000000000000000000000\n\
#00214:0001\n\
#00308:02\n\
#00315:01\n\
#00312:00000000000000000000000000000000000000000000000000000000000000000000000000000001\n\
#00408:03\n";

fn chart(text: &str) -> Chart {
    parse_bms(text.as_bytes(), "bms", &ParseOptions::default()).unwrap()
}

/// The note on `lane` at `time`: its index in the engine.
fn note(e: &Engine, lane: u8, time: i64) -> usize {
    (e.notes().iter())
        .position(|n| n.lane == lane && (n.time - time).abs() < MS)
        .unwrap_or_else(|| panic!("no note on {lane} at {time}"))
}

fn press(e: &mut Engine, lane: u8, t: i64) {
    for pressed in [true, false] {
        e.input(InputEvent {
            time: t,
            lane,
            key: 0,
            pressed,
        });
    }
    e.update(t);
}

/// Updates every 10 ms from -1 s to `end`, with `at` before each.
fn run(e: &mut Engine, end: i64, mut at: impl FnMut(&mut Engine, i64)) {
    let mut t = -SEC;
    while t <= end {
        at(e, t);
        e.update(t);
        t += 10 * MS;
    }
}

#[test]
fn a_negative_bpm_is_its_absolute_value_and_a_fold_point() {
    let c = chart(CHART);
    let tm = &c.timing;
    // the notes' times as at 120 and 60 BPM; folds at 4 s and 10 s (an
    // unreferenced negative #BPM04 makes none)
    let times: Vec<i64> = c.notes.iter().map(|n| n.time).collect();
    assert_eq!(
        times,
        [
            2 * SEC,
            3_900 * MS,
            4_100 * MS,
            5 * SEC,
            6 * SEC,
            9_900 * MS
        ]
    );
    assert_eq!(tm.folds(), &[4 * SEC, 10 * SEC]);
    assert_eq!(tm.bpm_at_time(5e6), 120.0);
    assert_eq!(tm.bpm_at_time(7e6), 60.0);
    assert_eq!(tm.bpm_at_time(11e6), 240.0);
    // the BPM shown: absolute (to the last note)
    assert_eq!((c.meta.min_bpm, c.meta.max_bpm), (60.0, 120.0));
    // the judge's clock: back from 4 s, the positive BPM at 6 s goes on
    // with it, back from 10 s
    assert_eq!(tm.judge_time(3 * SEC), 3 * SEC);
    assert_eq!(tm.judge_time(5 * SEC), 3 * SEC);
    assert_eq!(tm.judge_time(7 * SEC), SEC);
    assert_eq!(tm.judge_time(10_500 * MS), 9_500 * MS);
    // the lane drawn back from measure 2 (beat 8): a beat each 0.5 s,
    // from 6 s each second (60 BPM)
    assert!((tm.drawn_pos_at_time(5e6) - 6.0).abs() < 1e-9);
    assert!((tm.drawn_pos_at_time(7e6) - 3.0).abs() < 1e-9);
    // and from measure 4 (beat 16) at 240 BPM
    assert!((tm.drawn_pos_at_time(10.5e6) - 14.0).abs() < 1e-9);
    // the note's own positions stay forward
    assert!(c.notes[3].pos > c.notes[2].pos);
}

#[test]
fn negative_bpms_that_make_no_fold() {
    // at beat 0 the negative BPM is the tempo only; a header #BPM below 0
    // is still 130 (LR2's is not known); without negative BPM nothing
    // changes
    let c = chart("#BPM 120\n#BPM01 -60\n#00008:01\n#00111:01\n");
    assert!(c.timing.folds().is_empty());
    assert_eq!(c.notes[0].time, 4 * SEC);
    let c = chart("#BPM -120\n#00111:01\n");
    assert!(c.timing.folds().is_empty());
    assert_eq!(c.meta.initial_bpm, 130.0);
    let c = chart("#BPM 120\n#BPM01 60\n#00108:01\n#00211:01\n");
    assert!(c.timing.folds().is_empty());
    assert!(!format!("{:?}", c.timing).contains("folds"));
}

#[test]
fn the_judge_reaches_notes_by_the_folded_clock() {
    let c = chart(CHART);
    let (mut e, _) = build_engine(&c, &RuleOptions::default(), None, false);
    e.record();
    let a = note(&e, 0, 2 * SEC);
    let b = note(&e, 1, 3_900 * MS);
    let cc = note(&e, 2, 4_100 * MS);
    let d = note(&e, 3, 5 * SEC);
    let ee = note(&e, 4, 6 * SEC);
    let g = note(&e, 1, 9_900 * MS);
    run(&mut e, 13 * SEC, |e, t| match t {
        // before the fold: as ever
        2_000_000 => press(e, 0, t),
        // the note 100 ms after the fold: early by 100 ms as the clock
        // turns at 4 s
        4_000_000 => press(e, 2, t),
        // the note 100 ms before it, left: the clock comes back to it
        4_100_000 => press(e, 1, t),
        // the note at 5 s: the clock is at 3 s, so nothing is in reach
        5_000_000 => press(e, 3, t),
        // the last note, 100 ms before the second fold: reached again
        10_100_000 => press(e, 1, t),
        _ => {}
    });
    let st = |i: usize| e.notes()[i].state;
    assert_eq!(st(a), Some(Judge::PGreat));
    assert_eq!(st(b), Some(Judge::PGreat));
    assert!(st(cc).is_some_and(|j| j < Judge::Poor), "{:?}", st(cc));
    assert!(e
        .timing_log
        .iter()
        .any(|&(t, d)| t == 4_100 * MS && d == 100 * MS));
    // never reached by the clock
    assert_eq!(st(d), Some(Judge::Poor), "jumped over at the second fold");
    assert_eq!(st(ee), Some(Judge::Poor));
    assert_eq!(st(g), Some(Judge::PGreat));
    assert!(e.is_finished(13 * SEC));

    // its replay (chart times) judges the same
    let ops: Vec<Op> = e.recorded().to_vec();
    let (mut again, _) = build_engine(&c, &RuleOptions::default(), None, false);
    for op in ops {
        again.apply(op);
    }
    assert_eq!(again.judge_log, e.judge_log);
}

#[test]
fn notes_beyond_the_only_fold_are_never_judged() {
    // the chart up to its second fold: the notes after 4.1 s stay
    let text = CHART.replace("#00408:03\n", "");
    let c = chart(&text);
    assert_eq!(c.timing.folds(), &[4 * SEC]);
    for auto in [false, true] {
        let (mut e, _) = build_engine(&c, &RuleOptions::default(), None, auto);
        run(&mut e, 30 * SEC, |_, _| {});
        let left: Vec<i64> = (e.notes().iter())
            .filter(|n| n.state.is_none())
            .map(|n| n.time)
            .collect();
        // autoplay plays what the clock reaches; a player who presses
        // nothing misses the notes before the fold point, but for the one
        // 100 ms before it: the clock turned before it was missed
        let reached = if auto { &[][..] } else { &[3_900 * MS][..] };
        let beyond = [4_100 * MS, 5 * SEC, 6 * SEC, 9_900 * MS];
        assert_eq!(left, [reached, &beyond].concat(), "auto {auto}");
        assert!(!e.is_finished(30 * SEC));
        assert!(e.score.passed < e.score.total_notes);
    }
}

#[test]
fn a_nonstop_course_keeps_its_stages_folds() {
    let text = CHART.replace("#00408:03\n", "");
    let stages = [
        (Path::new("a/one.bms"), text.as_bytes()),
        (Path::new("b/two.bms"), text.as_bytes()),
    ];
    let joins = [vibebms_core::course::Join::Cut];
    let (c, _) = build_nonstop_chart(&stages, &joins, &RuleOptions::default(), 1).unwrap();
    let folds = c.chart.timing.folds();
    assert_eq!(folds.len(), 2, "{folds:?}");
    assert_eq!(folds[0], 4 * SEC);
    assert!(folds[1] > 10 * SEC);
}
