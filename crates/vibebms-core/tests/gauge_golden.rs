//! Golden tests of the gauge damage modifiers (behaviour specification
//! §3), through the public API: `gauge_elements`, `Gauge::new` and
//! `Gauge::change_for`.
//!
//! The two modifiers multiply only the negative entries of a gauge's
//! change table (BAD, POOR, empty POOR) when the gauge is made, by the
//! larger of a TOTAL factor and a note-count factor:
//! * beatoraja-preset five-key EX-HARD (5K and 10K);
//! * LR2-preset HARD and EX-HARD (every key mode).
//!
//! Values are compared to within 1e-4 (relative), as §3 allows.
//!
//! Where the LR2 vectors come from (gap A9; S-ids are the sources in
//! `docs/RULES.md`, "LR2 equivalence"):
//! * `lr2_damage_examples`, `lr2_damage_total_factor`,
//!   `lr2_damage_note_factor`, `lr2_damage_only_hard_and_ex_hard` and the
//!   LR2 rows of `damage_grid` pin vibeBMS's rule (specification §3.2),
//!   recorded from its code. Its TOTAL factor is S1's measurements with
//!   the ones digit dropped, as S1 says; its note-count factor agrees
//!   with LR2 from 125 notes up and is unconfirmed below (gap A4).
//! * `lr2_*_as_measured` and `lr2_hard_low_gauge_rule` are LR2's own
//!   behaviour as documented in public sources, each test naming its own.

use std::path::PathBuf;

use vibebms_core::gauge::{gauge_elements, Gauge, GaugeElement, GaugePreset, GaugeType};
// the judgements by the names the expected data uses
use vibebms_core::judge::Judge::{
    Bad as BAD, Good as GOOD, Great as GREAT, Miss as MISS, PGreat as PGREAT, Poor as POOR,
};
use vibebms_core::mode::RuleFamily;

fn elem(preset: GaugePreset, fam: RuleFamily, ty: GaugeType) -> &'static GaugeElement {
    &gauge_elements(preset, fam)[ty as usize]
}

/// The negative changes (BAD, POOR, empty POOR) of a gauge made for
/// `total` and `notes`.
fn damage(e: &GaugeElement, ty: GaugeType, total: f64, notes: usize) -> [f32; 3] {
    let g = Gauge::new(ty, e, total, notes);
    [g.change_for(BAD), g.change_for(POOR), g.change_for(MISS)]
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * b.abs().max(1.0)
}

fn assert_damage(got: [f32; 3], want: [f32; 3], what: &str) {
    assert!(
        got.iter().zip(&want).all(|(a, b)| close(*a, *b)),
        "{what}: got {got:?}, want {want:?}"
    );
}

/// §3.1 worked examples: base BAD -10, POOR -20, empty POOR -10.
#[test]
fn five_key_ex_hard_examples() {
    let fam = RuleFamily::FiveKeys;
    let e = elem(GaugePreset::Beatoraja, fam, GaugeType::ExHard);
    assert_eq!(e.values[3..], [-10.0, -20.0, -10.0]);
    let m = |total, n| damage(e, GaugeType::ExHard, total, n)[1] / -20.0;
    for (total, n, mult) in [
        (300.0, 2000, 1.0),
        (200.0, 400, 2.4),
        (125.0, 1500, 5.0),
        (235.0, 750, 1.5),
        (300.0, 100, 4.4),
        (300.0, 50, 5.392),
        (300.0, 0, 10.096),
    ] {
        assert!(
            close(m(total, n), mult),
            "TOTAL {total} n {n}: {}",
            m(total, n)
        );
    }
    assert_damage(
        damage(e, GaugeType::ExHard, 200.0, 400),
        [-24.0, -48.0, -24.0],
        "200/400",
    );
    assert_damage(
        damage(e, GaugeType::ExHard, 125.0, 1500),
        [-50.0, -100.0, -50.0],
        "125/1500",
    );
}

/// §3.1: the TOTAL factor is a step function (no interpolation); a TOTAL
/// that is not a number gives 1.
#[test]
fn five_key_ex_hard_total_steps() {
    let e = elem(
        GaugePreset::Beatoraja,
        RuleFamily::FiveKeys,
        GaugeType::ExHard,
    );
    let m = |total| damage(e, GaugeType::ExHard, total, 5000)[1] / -20.0;
    for (total, f) in [
        (1e9, 1.0),
        (240.0, 1.0),
        (239.99, 1.11),
        (230.0, 1.11),
        (229.99, 1.25),
        (210.0, 1.25),
        (209.99, 1.5),
        (200.0, 1.5),
        (199.99, 1.666),
        (180.0, 1.666),
        (179.99, 2.0),
        (160.0, 2.0),
        (159.99, 2.5),
        (150.0, 2.5),
        (149.99, 3.333),
        (130.0, 3.333),
        (129.99, 5.0),
        (120.0, 5.0),
        (119.99, 10.0),
        (0.0, 10.0),
        (-5.0, 10.0),
        (f64::NAN, 1.0),
    ] {
        assert!(close(m(total), f), "TOTAL {total}: {}", m(total));
    }
}

/// §3.1: the note-count factor's breakpoints, and linear between them.
#[test]
fn five_key_ex_hard_note_factor() {
    let e = elem(
        GaugePreset::Beatoraja,
        RuleFamily::FiveKeys,
        GaugeType::ExHard,
    );
    let m = |n| damage(e, GaugeType::ExHard, 1000.0, n)[1] / -20.0;
    for (n, f) in [
        (100_000, 1.0),
        (1000, 1.0),
        (999, 1.002),
        (750, 1.5),
        (500, 2.0),
        (400, 2.4),
        (250, 3.0),
        (125, 4.0),
        (100, 4.4),
        (62, 5.008),
        (50, 5.392),
        (31, 6.0),
        (15, 7.024),
        (7, 8.048),
        (3, 9.072),
        (2, 9.584),
        (1, 10.096),
        (0, 10.096),
    ] {
        assert!(close(m(n), f), "n {n}: {}", m(n));
    }
    // the larger factor wins
    let both = damage(e, GaugeType::ExHard, 200.0, 2000)[1] / -20.0;
    assert!(close(both, 1.5));
}

/// The modifier is the 5K/10K beatoraja EX-HARD's only: the 7K and PMS
/// EX-HARD, the other 5K gauges, and the IIDX preset are unchanged by it.
#[test]
fn five_key_ex_hard_only() {
    for (preset, fam, ty) in [
        (
            GaugePreset::Beatoraja,
            RuleFamily::SevenKeys,
            GaugeType::ExHard,
        ),
        (GaugePreset::Beatoraja, RuleFamily::Pms, GaugeType::ExHard),
        (
            GaugePreset::Beatoraja,
            RuleFamily::FiveKeys,
            GaugeType::Hard,
        ),
        (
            GaugePreset::Beatoraja,
            RuleFamily::FiveKeys,
            GaugeType::ExHardClass,
        ),
        (GaugePreset::Iidx, RuleFamily::FiveKeys, GaugeType::ExHard),
        (
            GaugePreset::Lr2,
            RuleFamily::FiveKeys,
            GaugeType::ExHardClass,
        ),
    ] {
        let e = elem(preset, fam, ty);
        let base = [e.values[3], e.values[4], e.values[5]];
        assert_eq!(damage(e, ty, 100.0, 10), base, "{preset:?} {fam:?} {ty:?}");
    }
}

/// §3.2 LR2 HARD (base BAD -6, POOR -10, empty POOR -2) examples.
#[test]
fn lr2_damage_examples() {
    for fam in [RuleFamily::FiveKeys, RuleFamily::SevenKeys, RuleFamily::Pms] {
        let e = elem(GaugePreset::Lr2, fam, GaugeType::Hard);
        assert_eq!(e.values[3..], [-6.0, -10.0, -2.0]);
        let d = |total, n| damage(e, GaugeType::Hard, total, n);
        assert_damage(d(300.0, 250), [-18.0, -30.0, -6.0], "300/250");
        assert_damage(d(300.0, 2000), [-6.0, -10.0, -2.0], "300/2000");
        assert_damage(d(200.0, 2000), [-9.0, -15.0, -3.0], "200/2000");
        assert_damage(d(100.0, 2000), [-60.0, -100.0, -20.0], "100/2000");
        // positive changes untouched
        let g = Gauge::new(GaugeType::Hard, e, 100.0, 10);
        assert_eq!(
            [
                g.change_for(PGREAT),
                g.change_for(GREAT),
                g.change_for(GOOD)
            ],
            [0.1, 0.1, 0.05]
        );
        let x = elem(GaugePreset::Lr2, fam, GaugeType::ExHard);
        assert_eq!(x.values[3..], [-12.0, -20.0, -2.0]);
        assert_damage(
            damage(x, GaugeType::ExHard, 300.0, 250),
            [-36.0, -60.0, -6.0],
            "EX-HARD 300/250",
        );
    }
}

/// §3.2: the TOTAL factor is S1's POOR damage over 10 at the multiple of
/// 10 at or below TOTAL (S1: LR2 drops the ones digit), 10 below 120.
#[test]
fn lr2_damage_total_factor() {
    let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard);
    let m = |total| damage(e, GaugeType::Hard, total, 5000)[1] / -10.0;
    for (total, f) in [
        (1e9, 1.0),
        (240.0, 1.0),
        (239.99, 1.11),
        (230.0, 1.11),
        (229.0, 1.25),
        (210.0, 1.25),
        (209.0, 1.5),
        (200.0, 1.5),
        (199.0, 1.666),
        (180.0, 1.666),
        (179.0, 2.0),
        (160.0, 2.0),
        (159.0, 2.5),
        (150.0, 2.5),
        (149.0, 3.333),
        (130.0, 3.333),
        (129.0, 5.0),
        (120.0, 5.0),
        (119.99, 10.0),
        (0.0, 10.0),
        (-100.0, 10.0),
    ] {
        assert!(close(m(total), f as f32), "TOTAL {total}: {}", m(total));
    }
}

/// §3.2: the note-count factor, with its jump at 30 (8 from below, 7 at
/// 30). vibeBMS's rule: from 250 notes up, and at 125 and 0, as S1 measured
/// in LR2 (`lr2_hard_note_factor_as_measured`); the segments below 125 are
/// unconfirmed.
#[test]
fn lr2_damage_note_factor() {
    let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard);
    let m = |n| damage(e, GaugeType::Hard, 1000.0, n)[1] / -10.0;
    for (n, f) in [
        (5000, 1.0),
        (1000, 1.0),
        (999, 1.002),
        (750, 1.5),
        (500, 2.0),
        (499, 2.004),
        (250, 3.0),
        (249, 3.008),
        (125, 4.0),
        (124, 4.0 + 1.0 / 65.0),
        (90, 4.538_461_5),
        (60, 5.0),
        (59, 5.0 + 1.0 / 15.0),
        (45, 6.0),
        (31, 6.933_333),
        (30, 7.0),
        (29, 8.2),
        (25, 9.0),
        (21, 9.8),
        (20, 10.0),
        (1, 10.0),
        (0, 10.0),
    ] {
        assert!(close(m(n), f as f32), "n {n}: {}", m(n));
    }
    // the larger factor wins
    assert!(close(
        damage(e, GaugeType::Hard, 200.0, 400)[1] / -10.0,
        2.4
    ));
    assert!(close(
        damage(e, GaugeType::Hard, 200.0, 900)[1] / -10.0,
        1.5
    ));
    assert!(close(
        damage(e, GaugeType::Hard, 150.0, 400)[1] / -10.0,
        2.5
    ));
}

/// The rest of the LR2 set is not changed by it.
#[test]
fn lr2_damage_only_hard_and_ex_hard() {
    for ty in [
        GaugeType::AssistEasy,
        GaugeType::Easy,
        GaugeType::Normal,
        GaugeType::Hazard,
        GaugeType::Class,
        GaugeType::ExClass,
        GaugeType::ExHardClass,
    ] {
        let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, ty);
        assert_eq!(
            damage(e, ty, 100.0, 10),
            [e.values[3], e.values[4], e.values[5]],
            "{ty:?}"
        );
    }
}

/// Both modifiers over a grid of TOTAL and note counts, against the
/// fixture recorded from the old code (vibeBMS's rule, not LR2
/// measurements).
#[test]
fn damage_grid() {
    let totals = [
        -1.0, 0.0, 50.0, 100.0, 111.0, 112.0, 119.99, 120.0, 128.0, 150.0, 160.0, 176.0, 199.99,
        200.0, 207.5, 229.99, 230.0, 239.99, 240.0, 300.0, 1e6,
    ];
    let notes = [
        0, 1, 2, 3, 5, 7, 10, 15, 20, 21, 25, 29, 30, 31, 45, 59, 60, 62, 90, 124, 125, 200, 249,
        250, 300, 499, 500, 750, 999, 1000, 2000,
    ];
    let mut text = String::new();
    for (preset, fam, ty) in [
        (
            GaugePreset::Beatoraja,
            RuleFamily::FiveKeys,
            GaugeType::ExHard,
        ),
        (GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard),
        (GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::ExHard),
    ] {
        let e = elem(preset, fam, ty);
        for t in totals {
            for n in notes {
                let d = damage(e, ty, t, n);
                text += &format!("{preset:?} {ty:?} {t} {n}: {} {} {}\n", d[0], d[1], d[2]);
            }
        }
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/gauge_damage.txt");
    if std::env::var_os("GOLDEN_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    assert_eq!(want.lines().count(), text.lines().count());
    for (w, g) in want.lines().zip(text.lines()) {
        let (wk, wv) = w.split_once(": ").unwrap();
        let (gk, gv) = g.split_once(": ").unwrap();
        assert_eq!(wk, gk);
        let p = |s: &str| -> Vec<f32> { s.split(' ').map(|x| x.parse().unwrap()).collect() };
        let (wv, gv) = (p(wv), p(gv));
        assert!(
            wv.iter().zip(&gv).all(|(a, b)| close(*b, *a)),
            "{wk}: got {gv:?}, want {wv:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// LR2 as documented (gap A9). S-ids: docs/RULES.md, "LR2 equivalence".
// ---------------------------------------------------------------------------

fn changes(g: &Gauge) -> [f32; 6] {
    [PGREAT, GREAT, GOOD, BAD, POOR, MISS].map(|j| g.change_for(j))
}

/// S1, S3 (and S2, S6 for EASY and HARD): LR2's gauges per PG, GR, GD,
/// BAD, POOR, empty POOR; t / n gains for GROOVE, × 1.2 for EASY. S8:
/// GROOVE starts at 20 %, clears at 80 % and never falls below 2 %.
#[test]
fn lr2_gauge_values_as_measured() {
    let (t, n) = (300.0, 1000);
    for fam in [RuleFamily::FiveKeys, RuleFamily::SevenKeys, RuleFamily::Pms] {
        for (ty, want) in [
            (GaugeType::Normal, [0.3, 0.3, 0.15, -4.0, -6.0, -2.0]),
            (GaugeType::Easy, [0.36, 0.36, 0.18, -3.2, -4.8, -1.6]),
            (GaugeType::Hard, [0.1, 0.1, 0.05, -6.0, -10.0, -2.0]),
            (GaugeType::Class, [0.1, 0.1, 0.05, -2.0, -3.0, -2.0]),
        ] {
            let e = elem(GaugePreset::Lr2, fam, ty);
            let got = changes(&Gauge::new(ty, e, t, n));
            assert!(
                got.iter().zip(&want).all(|(a, b)| close(*a, *b)),
                "{fam:?} {ty:?}: {got:?}"
            );
        }
        for ty in [GaugeType::Normal, GaugeType::Easy] {
            let e = elem(GaugePreset::Lr2, fam, ty);
            assert_eq!((e.init, e.border, e.min), (20.0, 80.0, 2.0), "{ty:?}");
        }
    }
}

/// HARD damage × 0.6 at 30 % or less as LR2 shows it (S1, S2, S3, S6); LR2
/// shows the gauge in 2 % steps (S1), so that is below 32 %, as S7 says.
/// S7: HARD empties below 2 %.
#[test]
fn lr2_hard_low_gauge_rule() {
    let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard);
    let after = |from: f32, j| {
        let mut g = Gauge::new(GaugeType::Hard, e, 300.0, 1000);
        g.set_value(from);
        g.update(j, 1.0);
        g.value()
    };
    assert!(close(after(32.0, POOR), 22.0));
    // S3's table at 30 %: BAD -3.6, POOR -6, empty POOR -1.2
    assert!(close(after(31.9, POOR), 25.9));
    assert!(close(after(30.0, BAD), 26.4));
    assert!(close(after(30.0, MISS), 28.8));
    assert!(close(after(3.3, MISS), 2.1));
    assert_eq!(after(3.0, MISS), 0.0);
}

/// S1: HARD POOR damage measured in LR2 at TOTAL in steps of 10 (1000
/// notes or more), given to the digits shown there; LR2 drops the ones
/// digit ("1の位以下は切り捨て"), so 159 is 150's (S1 also names a script
/// that sets TOTAL 159 for 150's damage).
#[test]
fn lr2_hard_total_factor_as_measured() {
    let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard);
    for (total, poor) in [
        (300.0, 10.0),
        (240.0, 10.0),
        (230.0, 11.1),
        (220.0, 12.5),
        (210.0, 12.5),
        (200.0, 15.0),
        (209.0, 15.0),
        (159.0, 25.0),
        (190.0, 16.66),
        (180.0, 16.66),
        (170.0, 20.0),
        (160.0, 20.0),
        (150.0, 25.0),
        (140.0, 33.33),
        (130.0, 33.33),
        (120.0, 50.0),
        (110.0, 100.0),
        (100.0, 100.0),
    ] {
        let got = -damage(e, GaugeType::Hard, total, 5000)[1];
        assert!(close(got, poor), "TOTAL {total}: {got}");
    }
}

/// S1: below 1000 notes LR2's HARD POOR damage grows 0.02 % a note down to
/// 500 and 0.04 % a note down to 250; S1's graph marks 125 notes at 40 and
/// 0 notes at 100 (TOTAL high enough not to matter).
#[test]
fn lr2_hard_note_factor_as_measured() {
    let e = elem(GaugePreset::Lr2, RuleFamily::SevenKeys, GaugeType::Hard);
    for (n, poor) in [
        (2000, 10.0),
        (1000, 10.0),
        (999, 10.02),
        (750, 15.0),
        (500, 20.0),
        (499, 20.04),
        (375, 25.0),
        (250, 30.0),
        (125, 40.0),
        (0, 100.0),
    ] {
        let got = -damage(e, GaugeType::Hard, 300.0, n)[1];
        assert!(close(got, poor), "{n} notes: {got}");
    }
}
