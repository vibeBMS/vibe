//! Golden tests of the judge engine, through its public API only.
//!
//! This file is the acceptance gate for the clean-room rewrite of
//! `engine.rs` and of the window builder in `judge.rs` (see the behaviour
//! specification, §1, §2, §7 and §9). It uses nothing but public items of
//! `vibebms_core` and states expected results as plain data.
//!
//! * Tests marked `#[ignore = "clean-room: new behaviour (§9 Qn)"]`
//!   describe the behaviour the rewrite must have where the specification
//!   fixes a quirk of the old engine. They fail against the old engine;
//!   the rewrite un-ignores them.
//! * Every other test passes against both engines.
//! * The fixtures under `tests/golden/` (window tables, gauge modifiers,
//!   long deterministic plays) were recorded from the old engine. Runs
//!   are built so that none of the fixed quirks can show in them (see
//!   `safe_drive`). `GOLDEN_BLESS=1` rewrites them: do not do that to
//!   make a rewrite pass.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use vibebms_core::chart::{Chart, ChartMeta, JudgeRank, LnType, Note, NoteKind, SoundId};
use vibebms_core::engine::{
    Engine, EngineConfig, EngineEvent, InputEvent, JudgeAlgorithm, Op, PlayNote,
};
use vibebms_core::gauge::{GaugeAutoShift, GaugePreset, GaugeType, GrooveGauge};
use vibebms_core::judge::{
    judge_rules, JudgePreset, JudgeRules, JudgeWindows, NoteType, BAD, GOOD, GREAT, MISS, PGREAT,
    POOR,
};
use vibebms_core::mode::PlayMode;

// ---------------------------------------------------------------------------
// Charts, engines and events
// ---------------------------------------------------------------------------

const MS: i64 = 1_000;
const NONE: [EngineEvent; 0] = [];
const SEC: i64 = 1_000_000;

fn normal(lane: u8, time: i64, sound: SoundId) -> Note {
    Note {
        lane,
        time,
        beat: 0.0,
        pos: 0.0,
        sound,
        kind: NoteKind::Normal,
    }
}

fn long(lane: u8, time: i64, end: i64, sound: SoundId, end_sound: SoundId) -> Note {
    typed(lane, time, end, sound, end_sound, None)
}

fn typed(
    lane: u8,
    time: i64,
    end: i64,
    sound: SoundId,
    end_sound: SoundId,
    ln_type: Option<LnType>,
) -> Note {
    Note {
        lane,
        time,
        beat: 0.0,
        pos: 0.0,
        sound,
        kind: NoteKind::Long {
            end_time: end,
            end_beat: 0.0,
            end_pos: 0.0,
            end_sound,
            ln_type,
        },
    }
}

fn mine(lane: u8, time: i64, damage: f32, sound: SoundId) -> Note {
    Note {
        lane,
        time,
        beat: 0.0,
        pos: 0.0,
        sound,
        kind: NoteKind::Mine { damage },
    }
}

fn hidden(lane: u8, time: i64, sound: SoundId) -> Note {
    Note {
        lane,
        time,
        beat: 0.0,
        pos: 0.0,
        sound,
        kind: NoteKind::Invisible,
    }
}

/// A chart of `mode` with `notes` (sorted by time, then lane, as charts are).
fn chart_of(mode: PlayMode, rank: JudgeRank, total: Option<f64>, mut notes: Vec<Note>) -> Chart {
    notes.sort_by_key(|n| (n.time, n.lane));
    Chart {
        meta: ChartMeta {
            mode: Some(mode),
            judge_rank: rank,
            total,
            ..Default::default()
        },
        notes,
        ..Default::default()
    }
}

/// 7K, #RANK EASY (the scenarios of §1.19).
fn chart7(notes: Vec<Note>) -> Chart {
    chart_of(PlayMode::Beat7K, JudgeRank::Rank(3), None, notes)
}

/// How an engine is made (as the play scene's `make_engine`).
#[derive(Clone, Debug)]
struct Setup {
    preset: JudgePreset,
    rates: Option<[i64; 3]>,
    ln: LnType,
    algorithm: JudgeAlgorithm,
    gauge_preset: GaugePreset,
    gauge: GaugeType,
    gas: GaugeAutoShift,
    auto: Vec<bool>,
    bss: bool,
    player: Option<usize>,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            preset: JudgePreset::Beatoraja,
            rates: None,
            ln: LnType::Ln,
            algorithm: JudgeAlgorithm::Combo,
            gauge_preset: GaugePreset::Beatoraja,
            gauge: GaugeType::Normal,
            gas: GaugeAutoShift::None,
            auto: Vec::new(),
            bss: true,
            player: None,
        }
    }
}

impl Setup {
    fn ln(ln: LnType) -> Setup {
        Setup {
            ln,
            ..Setup::default()
        }
    }
}

fn gauge_for(c: &Chart, s: &Setup) -> GrooveGauge {
    let fam = c.mode().family();
    GrooveGauge::new(
        s.gauge_preset,
        fam,
        s.gauge,
        s.gas,
        c.total_of(s.ln, s.gauge_preset, s.player),
        c.total_judgements_of(s.ln, s.player),
    )
}

fn config_for(c: &Chart, s: &Setup) -> EngineConfig {
    EngineConfig {
        rules: judge_rules(s.preset, c.mode().family(), c.meta.judge_rank, s.rates),
        default_ln: s.ln,
        algorithm: s.algorithm,
        auto_lanes: s.auto.clone(),
        scratch_release_ends_bss: s.bss,
        player: s.player,
    }
}

fn engine(c: &Chart, s: &Setup) -> Engine {
    Engine::new(c, config_for(c, s), gauge_for(c, s))
}

fn ev(time: i64, lane: u8, key: u8, pressed: bool) -> InputEvent {
    InputEvent {
        time,
        lane,
        key,
        pressed,
    }
}

/// Key 0 of `lane` down at `t`, then `update(t)`.
fn press(e: &mut Engine, lane: u8, t: i64) {
    e.input(ev(t, lane, 0, true));
    e.update(t);
}

/// Key 0 of `lane` up at `t`, then `update(t)`.
fn release(e: &mut Engine, lane: u8, t: i64) {
    e.input(ev(t, lane, 0, false));
    e.update(t);
}

/// `key` of `lane` down or up at `t`, then `update(t)`.
fn key(e: &mut Engine, lane: u8, k: u8, t: i64, down: bool) {
    e.input(ev(t, lane, k, down));
    e.update(t);
}

/// As `key`, but with the engine first advanced to `t` without the input:
/// the input then changes nothing that lies before it (§9 Q1-neutral).
fn key_synced(e: &mut Engine, lane: u8, k: u8, t: i64, down: bool) {
    e.update(t);
    key(e, lane, k, t, down);
}

fn drain(e: &mut Engine) -> Vec<EngineEvent> {
    e.drain_events().collect()
}

fn drain_no_volume(e: &mut Engine) -> Vec<EngineEvent> {
    e.drain_events()
        .filter(|v| !matches!(v, EngineEvent::Volume { .. }))
        .collect()
}

/// Head judgement event.
fn jh(lane: u8, judge: usize, diff: i64, combo: u32) -> EngineEvent {
    EngineEvent::Judge {
        lane,
        judge,
        diff,
        combo,
        tail: false,
    }
}

/// Tail judgement event.
fn jt(lane: u8, judge: usize, diff: i64, combo: u32) -> EngineEvent {
    EngineEvent::Judge {
        lane,
        judge,
        diff,
        combo,
        tail: true,
    }
}

fn snd(sound: SoundId, lane: u8) -> EngineEvent {
    EngineEvent::Sound { sound, lane }
}

fn vol(sound: SoundId, volume: f32) -> EngineEvent {
    EngineEvent::Volume { sound, volume }
}

fn mine_ev(lane: u8, damage: f32) -> EngineEvent {
    EngineEvent::Mine { lane, damage }
}

fn note_states(e: &Engine) -> Vec<(u8, u8, Option<i64>, Option<i64>)> {
    e.notes()
        .iter()
        .map(|n| (n.state, n.end_state, n.play_time, n.end_play_time))
        .collect()
}

// ---------------------------------------------------------------------------
// §1.19 scenarios
// ---------------------------------------------------------------------------

/// S1: hits, timing, a missed note at the end.
#[test]
fn s01_perfect_and_timing() {
    let c = chart7(vec![
        normal(0, 2 * SEC, 10),
        normal(0, 2_500 * MS, 11),
        normal(0, 3 * SEC, 12),
        normal(0, 3_500 * MS, 13),
    ]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
    release(&mut e, 0, 2_050 * MS);
    assert_eq!(drain(&mut e), NONE);
    press(&mut e, 0, 2_540 * MS);
    assert_eq!(drain(&mut e), [snd(11, 0), jh(0, GREAT, -40_000, 2)]);
    press(&mut e, 0, 2_900 * MS);
    assert_eq!(drain(&mut e), [snd(12, 0), jh(0, GOOD, 100_000, 3)]);
    e.update(5 * SEC);
    assert_eq!(drain(&mut e), [jh(0, POOR, -1_500_000, 0)]);
    let s = &e.score;
    assert_eq!(s.ex_score(), 3);
    assert_eq!(e.combo(), 0);
    assert_eq!(s.max_combo, 3);
    assert_eq!(s.combo_breaks, 1);
    assert_eq!((s.total_notes, s.passed), (4, 4));
    assert_eq!(s.just[PGREAT], 1);
    assert_eq!(s.counts[GREAT], [0, 1]);
    assert_eq!(s.counts[GOOD], [1, 0]);
    assert_eq!(s.counts[POOR], [0, 1]);
    assert_eq!(e.ghost, [0, 1, 2, 4]);
    assert_eq!(
        e.judge_log,
        [
            (2 * SEC, 0, 0),
            (2_500 * MS, 1, -1),
            (3 * SEC, 2, 1),
            (3_500 * MS, 4, -1)
        ]
    );
    assert_eq!(
        e.timing_log,
        [(2 * SEC, 0), (2_500 * MS, -40_000), (3 * SEC, 100_000)]
    );
    assert!(!e.is_finished(3_500 * MS));
    assert!(e.is_finished(5 * SEC));
    // NORMAL: TOTAL 260 over 4 notes -> +65 a PGREAT/GREAT, +32.5 a GOOD
    assert_eq!(
        e.gauge.values(),
        [97.0, 95.5, 94.0, 90.0, 84.0, 0.0, 97.0, 94.0, 90.0]
    );
    assert_eq!(
        note_states(&e),
        [
            (1, 0, Some(0), None),
            (2, 0, Some(-40_000), None),
            (3, 0, Some(100_000), None),
            (5, 0, Some(-1_500_000), None),
        ]
    );
}

/// S2: empty POOR far from a note, and in front of one (7K keeps the
/// combo, 5K breaks it).
#[test]
fn s02_empty_poor() {
    for (mode, keeps) in [(PlayMode::Beat7K, true), (PlayMode::Beat5K, false)] {
        let c = chart_of(
            mode,
            JudgeRank::Rank(3),
            None,
            vec![normal(0, SEC, 10), normal(0, 2 * SEC, 11)],
        );
        let mut e = engine(&c, &Setup::default());
        e.update(0);
        // no note within +500 ms: only the first note's sound
        press(&mut e, 0, 300 * MS);
        assert_eq!(drain(&mut e), [snd(10, 0)]);
        release(&mut e, 0, 310 * MS);
        press(&mut e, 0, SEC);
        release(&mut e, 0, 1_010 * MS);
        assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
        let g = e.gauge.value();
        press(&mut e, 0, 1_600 * MS);
        let combo = if keeps { 1 } else { 0 };
        assert_eq!(drain(&mut e), [snd(11, 0), jh(0, MISS, 400_000, combo)]);
        assert_eq!(e.score.combo_breaks, u32::from(!keeps));
        assert_eq!(e.score.count(MISS), 1);
        assert_eq!(e.score.passed, 1);
        assert!(e.ghost == [0], "an empty POOR is not in the ghost");
        assert_eq!(e.gauge.value(), g - 2.0);
        assert_eq!(e.timing_log.len(), 1, "no timing for an empty POOR");
        assert_eq!(e.judge_log[1], (2 * SEC, MISS as u8, 1));
        // the note is still there
        release(&mut e, 0, 1_610 * MS);
        press(&mut e, 0, 2 * SEC);
        assert_eq!(drain(&mut e), [snd(11, 0), jh(0, PGREAT, 0, combo + 1)]);
        assert_eq!(e.notes()[1].play_time, Some(0));
    }
}

/// LN 2.0 s – 3.0 s on lane 0 (sounds 10 / 50).
fn ln_chart() -> Chart {
    chart7(vec![long(0, 2 * SEC, 3 * SEC, 10, 50)])
}

/// S3 (a): LN mode, held through the end: judged after the end passes.
#[test]
fn s03a_ln_held_through() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    assert_eq!(e.score.total_notes, 1);
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    assert_eq!(e.holding(0), Some(0));
    assert_eq!(e.notes()[0].state, 0, "no judgement until the end");
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), NONE);
    e.update(3 * SEC + 1);
    assert_eq!(drain(&mut e), [jh(0, PGREAT, 0, 1), snd(50, 0)]);
    assert_eq!(e.holding(0), None);
    assert_eq!(note_states(&e), [(1, 1, Some(0), None)]);
    assert!(e.notes()[0].is_done());
    assert_eq!(e.ghost, [0]);
    assert_eq!(e.timing_log, [(2 * SEC, 0)]);
    assert_eq!(e.judge_log, [(2 * SEC, 0, 0)]);
    assert!(e.is_finished(3 * SEC + 1));
}

/// S3 (b): head GREAT, released on time: the worse of the two, the larger
/// error, at once.
#[test]
fn s03b_ln_head_great_release_on_time() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2_040 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    release(&mut e, 0, 3 * SEC);
    assert_eq!(drain(&mut e), [jh(0, GREAT, -40_000, 1), snd(50, 0)]);
    assert_eq!(e.holding(0), None);
}

/// S3 (c) and Q2 (a rule): an early release inside the windows waits for
/// the end, and reports the release's error.
#[test]
fn q02_ln_early_release_inside_windows_waits_for_the_end() {
    let c = ln_chart();
    for (rel, judge) in [
        (2_900 * MS, PGREAT),
        (2_850 * MS, GREAT),
        (2_810 * MS, GOOD),
    ] {
        let mut e = engine(&c, &Setup::ln(LnType::Ln));
        e.update(SEC);
        press(&mut e, 0, 2 * SEC);
        release(&mut e, 0, rel);
        assert_eq!(drain(&mut e), [snd(10, 0)]);
        // still "held" until the end passes
        assert_eq!(e.holding(0), Some(0));
        e.update(3 * SEC);
        assert_eq!(drain(&mut e), NONE);
        e.update(3 * SEC + 1);
        assert_eq!(
            drain(&mut e),
            [jh(0, judge, 3 * SEC - rel, 1), snd(50, 0)],
            "release at {rel}"
        );
    }
}

/// S3 (d): released outside the release windows: BAD at once, the head
/// keysound muted.
#[test]
fn s03d_ln_released_too_early() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_750 * MS);
    assert_eq!(
        drain(&mut e),
        [snd(10, 0), vol(10, 0.0), jh(0, BAD, 250_000, 0), snd(50, 0)]
    );
    assert_eq!(e.score.combo_breaks, 1);
    assert_eq!(e.timing_log, [(2 * SEC, 250_000)]);
    assert_eq!(note_states(&e), [(4, 4, Some(250_000), None)]);
}

/// S3 (e): never pressed: POOR once the head is beyond BAD (late).
#[test]
fn s03e_ln_never_pressed() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    e.update(2_280 * MS);
    assert_eq!(drain(&mut e), NONE);
    e.update(2_281 * MS);
    assert_eq!(drain(&mut e), [jh(0, POOR, -281_000, 0)]);
    assert_eq!(note_states(&e), [(5, 5, Some(-281_000), None)]);
    assert_eq!(e.score.passed, 1);
}

/// S3: the head released late (after the end) in the same update as the
/// end passes: the release is handled first.
#[test]
fn s03_ln_release_after_end_in_one_update() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2_010 * MS);
    release(&mut e, 0, 3_050 * MS);
    assert_eq!(
        drain(&mut e),
        [snd(10, 0), jh(0, PGREAT, -50_000, 1), snd(50, 0)]
    );
}

/// S4 (a), (b): CN tail judged on release.
#[test]
fn s04ab_cn_release_judges_the_tail() {
    let c = ln_chart();
    for (rel, judge, diff) in [
        (3_150 * MS, GREAT, -150_000),
        (2_850 * MS, GREAT, 150_000),
        (3_100 * MS, PGREAT, -100_000),
        (3_190 * MS, GOOD, -190_000),
        (3_270 * MS, BAD, -270_000),
    ] {
        let mut e = engine(&c, &Setup::ln(LnType::Cn));
        assert_eq!(e.score.total_notes, 2);
        e.update(SEC);
        press(&mut e, 0, 2 * SEC);
        assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
        release(&mut e, 0, rel);
        let combo = if judge <= GOOD { 2 } else { 0 };
        assert_eq!(
            drain(&mut e),
            [jt(0, judge, diff, combo), snd(50, 0)],
            "release at {rel}"
        );
        assert_eq!(e.holding(0), None);
        assert_eq!(e.ghost, [0, judge as u8]);
        assert_eq!(e.notes()[0].end_play_time, Some(diff));
        assert_eq!(e.timing_log[1], (3 * SEC, diff));
    }
}

/// S4: a CN tail released beyond BAD (late) is a POOR at once.
#[test]
fn s04_cn_release_beyond_bad_late() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    e.update(3_200 * MS);
    release(&mut e, 0, 3_281 * MS);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            jt(0, POOR, -281_000, 0),
            snd(50, 0)
        ]
    );
}

/// S4 (c): released early beyond the release windows: BAD (mute).
#[test]
fn s04c_cn_released_early_bad() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    release(&mut e, 0, 2_790 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jt(0, BAD, 210_000, 0), snd(50, 0)]
    );
    // and earlier still: POOR
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    release(&mut e, 0, 2_500 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jt(0, POOR, 500_000, 0), snd(50, 0)]
    );
    assert_eq!(e.timing_log.len(), 1, "a POOR has no timing");
}

/// S4 (d): held too long: tail POOR once the end is beyond BAD (late), no
/// end sound.
#[test]
fn s04d_cn_held_too_long() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    e.update(3_280 * MS);
    assert_eq!(drain(&mut e), NONE);
    e.update(3_400 * MS);
    assert_eq!(drain(&mut e), [jt(0, POOR, -400_000, 0)]);
    assert_eq!(e.holding(0), None);
    // the key is still down; letting go now does nothing
    release(&mut e, 0, 3_500 * MS);
    assert_eq!(drain(&mut e), NONE);
    assert_eq!(note_states(&e), [(1, 5, Some(0), Some(-400_000))]);
}

/// S4 (e): never pressed: head POOR and tail POOR in one step, both with
/// the head's diff (Q4, a rule: `q04_missed_cn_tail_uses_the_head_diff`).
#[test]
fn s04e_cn_never_pressed() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    e.update(2_280 * MS);
    assert_eq!(drain(&mut e), NONE);
    e.update(2_300 * MS);
    assert_eq!(
        drain(&mut e),
        [jh(0, POOR, -300_000, 0), jt(0, POOR, -300_000, 0)]
    );
    assert_eq!(e.ghost, [4, 4]);
    assert_eq!(e.score.count(POOR), 2);
    assert_eq!(e.score.combo_breaks, 2);
    assert!(e.notes()[0].is_done());
}

/// A HCN chart: #TOTAL 1, 2 judgements: PG/GR +0.5, a GREAT tick +0.25, a
/// BAD tick -1.5, POOR -6 (NORMAL, starting at 20).
fn hcn_chart() -> Chart {
    chart_of(
        PlayMode::Beat7K,
        JudgeRank::Rank(3),
        Some(1.0),
        vec![long(0, 2 * SEC, 3 * SEC, 10, 50)],
    )
}

/// S5 and Q7 (a rule): holding a HCN body raises the gauge by a GREAT at
/// half rate every full 200 ms (strictly more than 200 ms banked), however
/// the time is cut into steps.
#[test]
fn s05_hcn_held_ticks() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
    assert_eq!(e.gauge.value(), 20.5);
    assert_eq!(e.hcn_passing(0), Some((0, true)));
    let mut ticks = Vec::new();
    let mut last = e.gauge.value();
    let mut t = 2 * SEC;
    while t < 2_950 * MS {
        t += 50 * MS;
        e.update(t);
        if e.gauge.value() != last {
            ticks.push(t);
            last = e.gauge.value();
        }
    }
    assert_eq!(
        ticks,
        [2_250 * MS, 2_450 * MS, 2_650 * MS, 2_850 * MS],
        "a tick only once more than 200 ms is banked"
    );
    assert_eq!(e.gauge.value(), 21.5);
    assert_eq!(drain(&mut e), NONE);
    assert!(!e.hcn_damaged(0));
    // the body leaves the line at its end
    e.update(3 * SEC);
    assert_eq!(e.hcn_passing(0), None);
    // one long step pays every tick due in it
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    e.update(2_900 * MS);
    assert_eq!(e.gauge.value(), 21.5);
    // exactly 200 ms banked: no tick yet
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    e.update(2_200 * MS);
    assert_eq!(e.gauge.value(), 20.5);
    e.update(2_200 * MS + 1);
    assert_eq!(e.gauge.value(), 20.75);
}

/// S5: released early: the tail POOR at once (muted), then the body
/// drains the gauge by a BAD at half rate every 200 ms.
#[test]
fn s05_hcn_released_drains() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    key_synced(&mut e, 0, 0, 2_050 * MS, false);
    let evs = drain(&mut e);
    assert_eq!(
        evs[..3],
        [vol(10, 0.0), jt(0, POOR, 950_000, 0), snd(50, 0)]
    );
    assert!(evs[3..].iter().all(|v| *v == vol(10, 0.0)));
    assert_eq!(e.gauge.value(), 14.5);
    assert_eq!(e.holding(0), None);
    assert_eq!(e.hcn_passing(0), Some((0, false)));
    assert!(e.hcn_damaged(0));
    let mut t = 2_050 * MS;
    while t < 2_950 * MS {
        t += 50 * MS;
        e.update(t);
    }
    assert_eq!(e.gauge.value(), 8.5);
    assert_eq!(drain_no_volume(&mut e), NONE);
    // pressing again on the body: no keysound, no judgement
    key_synced(&mut e, 0, 0, 2_960 * MS, true);
    assert_eq!(drain_no_volume(&mut e), NONE);
    assert_eq!(e.hcn_passing(0), Some((0, true)));
}

/// S5: the tick counter is signed and kept while the body flips between
/// rising and falling; it is reset only when the body ends (Q7, a rule).
#[test]
fn q07_hcn_counter_is_signed_and_kept() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    e.update(2_150 * MS); // +150 ms
    key_synced(&mut e, 0, 0, 2_150 * MS, false); // tail POOR (-6), then falling
    assert_eq!(e.gauge.value(), 14.5);
    e.update(2_600 * MS); // +150 - 450 = -300 ms: one BAD tick, -100 ms left
    assert_eq!(e.gauge.value(), 13.0);
    // pressed again: rising from -100 ms; a tick needs more than 300 ms
    key_synced(&mut e, 0, 0, 2_600 * MS, true);
    e.update(2_900 * MS); // +200 ms banked: no tick
    assert_eq!(e.gauge.value(), 13.0);
    e.update(2_950 * MS); // +250: one GREAT tick
    assert_eq!(e.gauge.value(), 13.25);
    // the body ends: the counter is dropped
    e.update(3_100 * MS);
    assert_eq!(e.gauge.value(), 13.25);
    assert_eq!(e.hcn_passing(0), None);
}

/// S6: a mine passing while the lane is held: raw gauge damage, Mine and
/// its sound; never judged, never counted.
#[test]
fn s06_mine_hit_while_held() {
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    assert_eq!(e.score.total_notes, 0);
    e.update(SEC);
    press(&mut e, 0, 1_900 * MS);
    // an empty press: the lane's only note is a mine, so no sound
    assert_eq!(drain(&mut e), NONE);
    e.update(2_100 * MS);
    assert_eq!(drain(&mut e), [mine_ev(0, 10.0), snd(90, 0)]);
    assert_eq!(e.gauge.value(), 10.0);
    assert_eq!(
        e.gauge.values(),
        [10.0, 10.0, 10.0, 90.0, 90.0, 90.0, 90.0, 90.0, 90.0]
    );
    assert_eq!(e.score.passed, 0);
    assert!(e.ghost.is_empty() && e.judge_log.is_empty());
    assert!(!e.notes()[0].is_done(), "a mine is never done");
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), NONE);
    assert!(e.is_finished(3 * SEC));
}

/// Mines: not hit when up; on a scratch either key counts; never on an
/// auto lane; the damage ignores the gauge's guts.
#[test]
fn s06_mine_variants() {
    // up: nothing
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    e.update(2_100 * MS);
    assert_eq!(drain(&mut e), NONE);
    // scratch, key 1
    let c = chart7(vec![mine(7, 2 * SEC, 5.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    key(&mut e, 7, 1, 1_500 * MS, true);
    e.update(2_100 * MS);
    assert_eq!(drain(&mut e), [mine_ev(7, 5.0), snd(90, 7)]);
    // auto lane: its keys are never down
    let mut e = engine(
        &c,
        &Setup {
            auto: vec![false, false, false, false, false, false, false, true],
            ..Setup::default()
        },
    );
    e.update(SEC);
    key(&mut e, 7, 1, 1_500 * MS, true);
    e.update(2_100 * MS);
    assert_eq!(drain(&mut e), NONE);
    // HARD at 30 %: a mine takes its damage whole (no guts)
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, NO_SOUND_ID)]);
    let mut e = engine(
        &c,
        &Setup {
            gauge: GaugeType::Hard,
            ..Setup::default()
        },
    );
    e.gauge.set_values(&[30.0; 9]);
    e.update(SEC);
    press(&mut e, 0, 1_900 * MS);
    e.update(2_100 * MS);
    assert_eq!(drain(&mut e), [mine_ev(0, 10.0)], "a silent mine: no Sound");
    assert_eq!(e.gauge.value(), 20.0);
}

const NO_SOUND_ID: SoundId = vibebms_core::chart::NO_SOUND;

/// §9.3 decision 1 (Q1 ties): at equal times the input goes first: a mine
/// at exactly the time of a press is hit, one at exactly the time of a
/// release is not (as the old engine did).
#[test]
fn q01_mine_at_the_input_time() {
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(1_500 * MS);
    press(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), [mine_ev(0, 10.0), snd(90, 0)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 1_200 * MS);
    e.update(1_500 * MS);
    release(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), NONE);
}

/// With the engine already updated to the input's time before the input
/// arrives, the mine passed with the old key state: the tie rule only
/// applies within the update that handles the input.
#[test]
fn mine_at_the_input_time_after_an_update() {
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(1_500 * MS);
    key_synced(&mut e, 0, 0, 2 * SEC, true);
    assert_eq!(drain(&mut e), NONE);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 1_200 * MS);
    key_synced(&mut e, 0, 0, 2 * SEC, false);
    assert_eq!(drain(&mut e), [mine_ev(0, 10.0), snd(90, 0)]);
}

/// PMS, #RANK EASY: GD ±117, BAD ±183, empty POOR -175..+500.
fn pms_chart(notes: Vec<Note>) -> Chart {
    chart_of(PlayMode::Popn9K, JudgeRank::Rank(3), None, notes)
}

/// S7 variant 1: a PMS BAD keeps the note; a later press outside GOOD
/// passes it by; its miss is silent.
#[test]
fn s07_pms_bad_then_silent_poor() {
    let c = pms_chart(vec![normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2_150 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, BAD, -150_000, 0)]);
    assert_eq!(e.score.combo_breaks, 1);
    assert_eq!(note_states(&e), [(0, 0, Some(-150_000), None)]);
    release(&mut e, 0, 2_160 * MS);
    press(&mut e, 0, 2_170 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    e.update(2_300 * MS);
    assert_eq!(drain(&mut e), NONE);
    assert_eq!(e.ghost, [4]);
    assert_eq!(e.score.passed, 1);
    assert_eq!(e.score.count(POOR), 0);
    assert_eq!(e.score.count(BAD), 1);
    assert_eq!(e.judge_log.len(), 1);
    assert_eq!(note_states(&e), [(5, 0, Some(-150_000), None)]);
}

/// S7 variant 2: an early BAD, then a hit inside GOOD.
#[test]
fn s07_pms_bad_then_hit() {
    let c = pms_chart(vec![normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 1_850 * MS);
    release(&mut e, 0, 1_860 * MS);
    press(&mut e, 0, 2_040 * MS);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, BAD, 150_000, 0),
            snd(10, 0),
            jh(0, GREAT, -40_000, 1)
        ]
    );
    assert_eq!(e.ghost, [1]);
    assert_eq!(e.score.passed, 1);
}

/// S7 variant 3: a PMS empty POOR breaks the combo, and a second one on
/// the same note is passed by.
#[test]
fn s07_pms_empty_poor_once() {
    let c = pms_chart(vec![normal(0, SEC, 9), normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, SEC);
    release(&mut e, 0, 1_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 1_700 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, 300_000, 0)]);
    assert_eq!(e.score.combo_breaks, 1);
    release(&mut e, 0, 1_710 * MS);
    press(&mut e, 0, 1_750 * MS);
    assert_eq!(drain(&mut e), [snd(9, 0)]);
    release(&mut e, 0, 1_760 * MS);
    // inside GOOD it can still be hit
    press(&mut e, 0, 2_100 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GOOD, -100_000, 1)]);
}

/// PMS one-POOR rule on a CN: a head BAD (kept), then missed: the head's
/// POOR is silent, the tail's is not.
#[test]
fn s07_pms_cn_head_bad_then_missed() {
    let c = pms_chart(vec![long(0, 2 * SEC, 3 * SEC, 10, 50)]);
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    assert_eq!(e.score.total_notes, 2);
    e.update(SEC);
    press(&mut e, 0, 1_850 * MS);
    release(&mut e, 0, 1_860 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, BAD, 150_000, 0)]);
    assert_eq!(e.holding(0), None, "a PMS BAD does not start the hold");
    e.update(2_200 * MS);
    let evs = drain(&mut e);
    assert_eq!(evs.len(), 1);
    assert!(matches!(
        evs[0],
        EngineEvent::Judge {
            judge: POOR,
            tail: true,
            ..
        }
    ));
    assert_eq!(e.ghost, [4, 4]);
    assert_eq!(e.score.passed, 2);
    assert_eq!(e.score.count(POOR), 1);
}

/// S8: PMS LN mode, a 200 ms grace after an early release; pressing again
/// inside it resumes the hold.
#[test]
fn s08_pms_ln_release_margin() {
    let c = pms_chart(vec![long(0, 2 * SEC, 3 * SEC, 10, 50)]);
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    assert_eq!(e.windows().ln_margin, 200_000);
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_500 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    press(&mut e, 0, 2_600 * MS);
    assert_eq!(drain(&mut e), NONE, "re-pressing a held note: nothing");
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), NONE);
    e.update(3 * SEC + 1);
    assert_eq!(drain(&mut e), [jh(0, PGREAT, 0, 1), snd(50, 0)]);
    // not pressed again: BAD once the grace has passed
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_500 * MS);
    drain(&mut e);
    e.update(2_699_999);
    assert_eq!(drain(&mut e), NONE);
    e.update(2_700 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jh(0, BAD, 500_000, 0), snd(50, 0)]
    );
}

/// PMS CN: the grace period is the same for a CN's early BAD release, and
/// pressing again cancels it.
#[test]
fn s08_pms_cn_release_margin() {
    let c = pms_chart(vec![long(0, 2 * SEC, 3 * SEC, 10, 50)]);
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_750 * MS, false);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
    key_synced(&mut e, 0, 0, 2_900 * MS, true);
    key_synced(&mut e, 0, 0, 3_010 * MS, false);
    assert_eq!(drain(&mut e), [jt(0, PGREAT, -10_000, 2), snd(50, 0)]);
    // without: BAD 200 ms after the release
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_750 * MS, false);
    drain(&mut e);
    e.update(2_949_999);
    assert_eq!(drain(&mut e), NONE);
    e.update(2_950 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jt(0, BAD, 250_000, 0), snd(50, 0)]
    );
}

/// Scratch CN 2.0–3.0 s on 7K's scratch lane.
fn scratch_cn() -> Chart {
    chart7(vec![long(7, 2 * SEC, 3 * SEC, 10, 50)])
}

fn turntable(ln: LnType) -> Setup {
    Setup {
        ln,
        bss: false,
        ..Setup::default()
    }
}

/// S9 and Q10 (a rule): turntable mode: letting the scratch stop inside
/// the release windows does not end a CN; spinning back does.
#[test]
fn q10_turntable_back_spin() {
    let c = scratch_cn();
    let mut e = engine(&c, &turntable(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    assert_eq!(drain(&mut e), [snd(10, 7), jh(7, PGREAT, 0, 1)]);
    key(&mut e, 7, 0, 2_950 * MS, false);
    assert_eq!(drain(&mut e), NONE);
    assert_eq!(e.holding(7), Some(0));
    key(&mut e, 7, 1, 2_980 * MS, true);
    assert_eq!(drain(&mut e), [snd(50, 7), jt(7, PGREAT, 20_000, 2)]);
    assert_eq!(e.holding(7), None);
    // keyboard mode: the release ends it
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    key(&mut e, 7, 0, 2_950 * MS, false);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 7),
            jh(7, PGREAT, 0, 1),
            jt(7, PGREAT, 50_000, 2),
            snd(50, 7)
        ]
    );
}

/// Turntable mode: a same-key release outside the windows ends a CN; the
/// other key's release does nothing; holding on is a tail POOR.
#[test]
fn s09_turntable_releases() {
    let c = scratch_cn();
    let mut e = engine(&c, &turntable(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    drain(&mut e);
    key(&mut e, 7, 0, 2_500 * MS, false);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jt(7, POOR, 500_000, 0), snd(50, 7)]
    );
    // the other key going up: nothing; then the tail is held too long
    let mut e = engine(&c, &turntable(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    key(&mut e, 7, 1, 2_100 * MS, true); // back-spin at once: POOR
    let evs = drain(&mut e);
    assert_eq!(evs[2..], [snd(50, 7), jt(7, POOR, 900_000, 0)]);
    let mut e = engine(&c, &turntable(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    drain(&mut e);
    key(&mut e, 7, 1, 2_100 * MS, false);
    key(&mut e, 7, 0, 3_100 * MS, false); // inside the windows: held on
    assert_eq!(drain(&mut e), NONE);
    e.update(3_300 * MS);
    assert_eq!(drain(&mut e), [jt(7, POOR, -300_000, 0)]);
}

/// Turntable mode, LN mode: the same key's release counts wherever it is;
/// the other key's does not, and spinning back is not an end.
#[test]
fn s09_turntable_ln_mode() {
    let c = scratch_cn();
    let mut e = engine(&c, &turntable(LnType::Ln));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    key(&mut e, 7, 1, 2_500 * MS, true); // another key: nothing in LN mode
    key(&mut e, 7, 1, 2_510 * MS, false);
    assert_eq!(drain(&mut e), [snd(10, 7)]);
    assert_eq!(e.holding(7), Some(0));
    key(&mut e, 7, 0, 2_950 * MS, false);
    assert_eq!(drain(&mut e), NONE);
    e.update(3 * SEC + 1);
    assert_eq!(drain(&mut e), [jh(7, PGREAT, 50_000, 1), snd(50, 7)]);
}

/// Keyboard mode: the other scratch key's press while holding a scratch CN
/// is a back-spin end too; on a key lane it is not.
#[test]
fn s09_keyboard_back_spin() {
    let c = scratch_cn();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    key(&mut e, 7, 0, 2 * SEC, true);
    key(&mut e, 7, 1, 2_850 * MS, true);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 7),
            jh(7, PGREAT, 0, 1),
            snd(50, 7),
            jt(7, GREAT, 150_000, 2)
        ]
    );
    // the key lane: key 1 is just another key of the lane
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    key(&mut e, 0, 0, 2 * SEC, true);
    key(&mut e, 0, 1, 2_850 * MS, true);
    key(&mut e, 0, 0, 2_900 * MS, false);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
    key(&mut e, 0, 1, 2_950 * MS, false);
    assert_eq!(drain(&mut e), [jt(0, PGREAT, 50_000, 2), snd(50, 0)]);
}

/// S10: the IIDX preset judges the frame-quantised error.
#[test]
fn s10_iidx_quantised_diffs() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 3 * SEC, 11)]);
    let s = Setup {
        preset: JudgePreset::Iidx,
        ..Setup::default()
    };
    let mut e = engine(&c, &s);
    e.update(SEC);
    press(&mut e, 0, 1_980 * MS);
    release(&mut e, 0, 2_100 * MS);
    press(&mut e, 0, 3_021 * MS);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 16_667, 1),
            snd(11, 0),
            jh(0, GREAT, -25_000, 2)
        ]
    );
    assert_eq!(e.timing_log, [(2 * SEC, 16_667), (3 * SEC, -25_000)]);
}

/// S11: a late-stamped input while the earlier note is still in reach.
#[test]
fn s11_late_input_sees_a_recent_note() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_500 * MS, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    e.update(2_300 * MS);
    e.input(ev(2_120 * MS, 0, 0, true));
    e.update(2_320 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, -120_000, 1)]);
}

/// S12: inputs with one timestamp are handled in arrival order.
#[test]
fn s12_same_timestamp() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(1, 2 * SEC, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    e.input(ev(2 * SEC, 1, 0, true));
    e.input(ev(2 * SEC, 0, 0, true));
    e.update(2 * SEC);
    assert_eq!(
        drain(&mut e),
        [
            snd(11, 1),
            jh(1, PGREAT, 0, 1),
            snd(10, 0),
            jh(0, PGREAT, 0, 2)
        ]
    );
    // a press and a release of one key at one time: judged, then up
    let c = chart7(vec![normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    e.input(ev(2 * SEC, 0, 0, true));
    e.input(ev(2 * SEC, 0, 0, false));
    assert!(e.held().is_empty());
    e.update(2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, PGREAT, 0, 1)]);
    assert!(e.held().is_empty());
    // the same on an LN: the hold starts and ends at once
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    e.input(ev(2 * SEC, 0, 0, true));
    e.input(ev(2 * SEC, 0, 0, false));
    e.update(2 * SEC);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            vol(10, 0.0),
            jh(0, BAD, 1_000_000, 0),
            snd(50, 0)
        ]
    );
    // inputs out of order: sorted by time (stable)
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(1, 2_010 * MS, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    e.input(ev(2_010 * MS, 1, 0, true));
    e.input(ev(2 * SEC, 0, 0, true));
    e.update(2_020 * MS);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            snd(11, 1),
            jh(1, PGREAT, 0, 2)
        ]
    );
}

fn auto_all() -> Vec<bool> {
    vec![true; 8]
}

/// S13: autoplay. A normal note at its time; an LN-mode note's end after
/// its end time (strictly); a CN tail at or after it.
#[test]
fn s13_autoplay_timing() {
    let c = chart7(vec![
        normal(1, 2 * SEC, 9),
        long(0, 2 * SEC, 3 * SEC, 10, 50),
    ]);
    let mut e = engine(
        &c,
        &Setup {
            auto: auto_all(),
            ..Setup::default()
        },
    );
    e.update(SEC);
    e.update(2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), snd(9, 1), jh(1, PGREAT, 0, 1)]);
    assert_eq!(e.holding(0), Some(0));
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), NONE);
    e.update(3 * SEC + 1);
    assert_eq!(drain(&mut e), [jh(0, PGREAT, 0, 2), snd(50, 0)]);
    assert_eq!(e.score.ex_score(), 4);
    let mut e = engine(
        &c,
        &Setup {
            auto: auto_all(),
            ln: LnType::Cn,
            ..Setup::default()
        },
    );
    e.update(SEC);
    e.update(2 * SEC);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            snd(9, 1),
            jh(1, PGREAT, 0, 2)
        ]
    );
    e.update(3 * SEC - 1);
    assert_eq!(drain(&mut e), NONE);
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), [jt(0, PGREAT, 0, 3), snd(50, 0)]);
    assert_eq!(e.ghost, [0, 0, 0]);
    assert!(e.is_finished(3 * SEC + 1));
}

/// Autoplay and AUTO SCRATCH: an auto lane takes no input (but records
/// it), its notes are PGREAT, an auto HCN body rises.
#[test]
fn s13_auto_lanes() {
    let c = chart_of(
        PlayMode::Beat7K,
        JudgeRank::Rank(3),
        Some(1.0),
        vec![
            normal(0, 4 * SEC, 9),
            long(7, 2 * SEC, 3 * SEC, 10, 50),
            normal(7, 3_500 * MS, 11),
        ],
    );
    let mut auto = vec![false; 7];
    auto.push(true);
    let mut e = engine(
        &c,
        &Setup {
            auto,
            ln: LnType::Hcn,
            ..Setup::default()
        },
    );
    assert!(e.is_auto(7) && !e.is_auto(0) && !e.is_auto(8));
    e.record();
    e.update(SEC);
    key(&mut e, 7, 0, 1_500 * MS, true);
    assert_eq!(drain(&mut e), NONE, "no empty POOR sound on an auto lane");
    assert!(e.held().is_empty());
    e.update(2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 7), jh(7, PGREAT, 0, 1)]);
    assert_eq!(e.hcn_passing(7), Some((0, true)));
    assert!(e.notes()[0].is_hcn());
    e.update(2_900 * MS); // 900 ms of body: four GREAT ticks
    let g = e.gauge.value();
    e.update(3 * SEC);
    assert_eq!(drain(&mut e), [jt(7, PGREAT, 0, 2), snd(50, 7)]);
    e.update(5 * SEC);
    assert_eq!(
        drain(&mut e),
        [snd(11, 7), jh(7, PGREAT, 0, 3), jh(0, POOR, -1_000_000, 0)]
    );
    assert!(g > 20.0);
    assert_eq!(
        e.recorded()[..3],
        [
            Op::Update(SEC),
            Op::Input(1_500 * MS, 7, 0, true),
            Op::Update(1_500 * MS)
        ]
    );
}

/// S14: LR2 preset: an LN release inside GOOD keeps a PGREAT head; a CN
/// tail 30 ms late at VERY HARD is a PGREAT.
#[test]
fn s14_lr2_release_window() {
    let c = chart_of(
        PlayMode::Beat7K,
        JudgeRank::Rank(2),
        None,
        vec![long(0, 2 * SEC, 3 * SEC, 10, 50)],
    );
    let lr2 = |ln| Setup {
        preset: JudgePreset::Lr2,
        gauge_preset: GaugePreset::Lr2,
        ln,
        ..Setup::default()
    };
    let mut e = engine(&c, &lr2(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_975 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    e.update(3 * SEC + 1);
    assert_eq!(drain(&mut e), [jh(0, PGREAT, 25_000, 1), snd(50, 0)]);
    let c = chart_of(
        PlayMode::Beat7K,
        JudgeRank::Rank(0),
        None,
        vec![long(0, 2 * SEC, 3 * SEC, 10, 50)],
    );
    let mut e = engine(&c, &lr2(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 3_030 * MS);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            jt(0, PGREAT, -30_000, 2),
            snd(50, 0)
        ]
    );
    // LR2 empty POOR: up to 1 s early, never late
    let c = chart_of(
        PlayMode::Beat7K,
        JudgeRank::Rank(2),
        None,
        vec![normal(0, 2 * SEC, 10), normal(0, 4 * SEC, 11)],
    );
    let mut e = engine(&c, &lr2(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 1_100 * MS);
    release(&mut e, 0, 1_110 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, 900_000, 0)]);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 2_190 * MS); // 190 ms after a hit note: no empty POOR
    assert_eq!(drain(&mut e), [snd(10, 0)]);
}

// ---------------------------------------------------------------------------
// §1.7 note selection: worked examples
// ---------------------------------------------------------------------------

fn two_notes() -> Chart {
    chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_200 * MS, 11)])
}

/// Worked example 1: the algorithms differ on a press between two notes.
#[test]
fn selection_worked_example_1() {
    for (algorithm, hit) in [
        (JudgeAlgorithm::Combo, [snd(11, 0), jh(0, GREAT, 30_000, 1)]),
        (
            JudgeAlgorithm::Duration,
            [snd(11, 0), jh(0, GREAT, 30_000, 1)],
        ),
        (
            JudgeAlgorithm::Lowest,
            [snd(10, 0), jh(0, BAD, -170_000, 0)],
        ),
    ] {
        let c = two_notes();
        let mut e = engine(
            &c,
            &Setup {
                algorithm,
                ..Setup::default()
            },
        );
        e.update(SEC);
        press(&mut e, 0, 2_170 * MS);
        assert_eq!(drain(&mut e), hit, "{algorithm:?}");
        e.update(2_600 * MS);
        let miss = if algorithm == JudgeAlgorithm::Lowest {
            jh(0, POOR, -400_000, 0)
        } else {
            jh(0, POOR, -600_000, 0)
        };
        assert_eq!(drain(&mut e), [miss], "{algorithm:?}");
    }
}

/// Combo: the next note is taken only once the current one is beyond GOOD
/// (late) and the next one is inside GOOD (early).
#[test]
fn selection_combo_gate() {
    let c = two_notes();
    // the first note still inside GOOD (-140 ms): kept
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2_140 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GOOD, -140_000, 1)]);
    // exactly at GOOD's late edge (-150 ms): not beyond it, so kept
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2_150 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GOOD, -150_000, 1)]);
    // Duration: the closer one, ties to the earlier
    let d = Setup {
        algorithm: JudgeAlgorithm::Duration,
        ..Setup::default()
    };
    let mut e = engine(&c, &d);
    e.update(SEC);
    press(&mut e, 0, 2_100 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GOOD, -100_000, 1)]);
    let mut e = engine(&c, &d);
    e.update(SEC);
    press(&mut e, 0, 2_101 * MS);
    assert_eq!(drain(&mut e), [snd(11, 0), jh(0, GOOD, 99_000, 1)]);
}

/// Worked example 2: a hit note far behind gives nothing; the next note's
/// empty POOR window is used.
#[test]
fn selection_worked_example_2() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_500 * MS, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 2_200 * MS);
    assert_eq!(drain(&mut e), [snd(11, 0), jh(0, MISS, 300_000, 1)]);
    assert_eq!(e.notes()[1].state, 0);
}

/// An empty POOR prefers the closer of a hit note behind and a note
/// ahead; a graded note ahead always wins.
#[test]
fn selection_empty_poor_closer() {
    // 2.0 hit; 2.3 ahead. Press 2.1: 2.0 is -100 (MISS), 2.3 is +200 (MISS,
    // farther): the hit note's empty POOR.
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_300 * MS, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 2_060 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, -60_000, 1)]);
    release(&mut e, 0, 2_070 * MS);
    press(&mut e, 0, 2_090 * MS); // +210: BAD on the note ahead wins
    assert_eq!(drain(&mut e), [snd(11, 0), jh(0, BAD, 210_000, 0)]);
}

/// Notes on other lanes, mines and invisible notes are never targets.
#[test]
fn selection_skips_mines() {
    let c = chart7(vec![
        mine(0, 1_900 * MS, 1.0, 90),
        normal(0, 2_100 * MS, 10),
        hidden(0, 2_050 * MS, 20),
        normal(1, 2 * SEC, 11),
    ]);
    let mut e = engine(&c, &Setup::default());
    assert_eq!(e.notes().len(), 3, "invisible notes are not taken");
    e.update(SEC);
    key_synced(&mut e, 0, 0, 2 * SEC, true);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GOOD, 100_000, 1)]);
}

// ---------------------------------------------------------------------------
// §1.9 the keysound of an empty press
// ---------------------------------------------------------------------------

#[test]
fn empty_press_keysound() {
    let c = chart7(vec![hidden(0, 500 * MS, 20), normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, SEC); // the invisible note before the press
    release(&mut e, 0, 1_010 * MS);
    assert_eq!(drain(&mut e), [snd(20, 0)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, 400 * MS); // nothing before: the first visible note
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    // after the visible note was hit: it is later than the invisible one
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 2_600 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
    // ties go to the visible note
    let c = chart7(vec![
        hidden(0, SEC, 20),
        normal(0, SEC, 10),
        normal(0, 3 * SEC, 11),
    ]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, SEC);
    release(&mut e, 0, 1_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 1_800 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0)]);
}

/// A judged long note is looked past; a mine is a silent choice; nothing
/// at all while a HCN body is at the line.
#[test]
fn empty_press_keysound_skips() {
    let c = chart7(vec![normal(0, SEC, 12), long(0, 2 * SEC, 3 * SEC, 13, 53)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 3 * SEC);
    drain(&mut e);
    press(&mut e, 0, 3_600 * MS);
    assert_eq!(drain(&mut e), [snd(12, 0)]);
    // an unjudged long note is not looked past (here between the earliest
    // late bound and BAD's: in no window, so no target)
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, 2_285 * MS);
    assert_eq!(drain(&mut e)[0], snd(13, 0));
    // a mine is chosen and silent
    let c = chart7(vec![normal(0, SEC, 12), mine(0, 2 * SEC, 1.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    e.update(2_500 * MS);
    drain(&mut e);
    press(&mut e, 0, 2_500 * MS);
    assert_eq!(drain(&mut e), NONE);
    // a HCN body at the line: silence
    let c = chart7(vec![
        long(0, 2 * SEC, 3 * SEC, 13, 53),
        normal(0, 4 * SEC, 14),
    ]);
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_100 * MS, false);
    drain(&mut e);
    key_synced(&mut e, 0, 0, 2_500 * MS, true);
    assert_eq!(drain_no_volume(&mut e), NONE);
    // a CN body is not a HCN body: the judged CN is looked past, nothing
    // else is before the press, so the lane's first note
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_100 * MS, false);
    drain(&mut e);
    key_synced(&mut e, 0, 0, 2_500 * MS, true);
    assert_eq!(drain(&mut e), [snd(13, 0)]);
}

// ---------------------------------------------------------------------------
// Quirks of §1.17: the rules kept (Q2, Q5, Q7, Q10 above and here) and the
// fixed ones (§9), with their new behaviour
// ---------------------------------------------------------------------------

/// §9 Q1: an input takes effect at its own time: a mine that passed
/// before a press (in the same update) is not hit.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q1)"]
fn q01_mine_before_a_press_in_one_update() {
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(1_500 * MS);
    press(&mut e, 0, 2_100 * MS);
    assert_eq!(drain(&mut e), NONE);
    assert_eq!(e.gauge.value(), 20.0);
}

/// §9 Q1: a mine that passed before a release (in the same update) is hit.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q1)"]
fn q01_mine_before_a_release_in_one_update() {
    let c = chart7(vec![mine(0, 2 * SEC, 10.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 1_200 * MS);
    e.update(1_500 * MS);
    release(&mut e, 0, 2_100 * MS);
    assert_eq!(drain(&mut e), [mine_ev(0, 10.0), snd(90, 0)]);
    assert_eq!(e.gauge.value(), 10.0);
}

/// §9 Q1: a HCN body's time up to a release counts as held.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q1)"]
fn q01_hcn_body_up_to_a_release_is_held() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    e.update(2_300 * MS); // 300 ms: one GREAT tick, 100 ms banked
    assert_eq!(e.gauge.value(), 20.75);
    // the 200 ms up to the release are held: another tick, then the tail
    release(&mut e, 0, 2_500 * MS);
    assert_eq!(e.gauge.value(), 21.0 - 6.0);
    // and a press: the time up to it is not held
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_100 * MS, false); // +100 ms held, tail POOR
    assert_eq!(e.gauge.value(), 14.5);
    e.update(2_200 * MS); // +100 - 100 = 0
    press(&mut e, 0, 2_550 * MS); // 350 ms released: -350, one BAD tick
    assert_eq!(e.gauge.value(), 13.0);
}

/// §9 Q3: a CN's early release keeps the input's own time (as an LN's).
#[test]
#[ignore = "clean-room: new behaviour (§9 Q3)"]
fn q03_cn_pending_release_keeps_the_input_time() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    e.update(2_800 * MS);
    e.input(ev(2_790 * MS, 0, 0, false));
    e.update(2_810 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jt(0, BAD, 210_000, 0), snd(50, 0)]
    );
}

/// The LN-mode counterpart (unchanged): the input's time.
#[test]
fn q03_ln_pending_release_keeps_the_input_time() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Ln));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    e.update(2_800 * MS);
    e.input(ev(2_750 * MS, 0, 0, false));
    e.update(2_810 * MS);
    assert_eq!(
        drain(&mut e),
        [vol(10, 0.0), jh(0, BAD, 250_000, 0), snd(50, 0)]
    );
}

/// Q4 (a rule, §9.3 decision 5): a missed CN's tail POOR reuses the head's
/// diff (the tail is missed because the head was): never FAST.
#[test]
fn q04_missed_cn_tail_uses_the_head_diff() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    e.update(2_300 * MS);
    assert_eq!(
        drain(&mut e),
        [jh(0, POOR, -300_000, 0), jt(0, POOR, -300_000, 0)]
    );
    assert_eq!(e.judge_log, [(2 * SEC, 4, -1), (3 * SEC, 4, -1)]);
    assert_eq!(e.score.counts[POOR], [0, 2]);
    assert_eq!(note_states(&e), [(5, 5, Some(-300_000), Some(-300_000))]);
    // found after the end: still the head's diff
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    e.update(4 * SEC);
    assert_eq!(
        drain(&mut e),
        [jh(0, POOR, -2_000_000, 0), jt(0, POOR, -2_000_000, 0)]
    );
}

/// Q5 (a rule): five-key scratch LN end: GOOD is as wide as GREAT, so a
/// release there is never GOOD.
#[test]
fn q05_5k_scratch_ln_end_good_is_great_wide() {
    let c = chart_of(
        PlayMode::Beat5K,
        JudgeRank::Rank(3),
        None,
        vec![long(5, 2 * SEC, 3 * SEC, 10, 50)],
    );
    let e = engine(&c, &Setup::ln(LnType::Cn));
    assert_eq!(
        e.windows().scratch_ln_end,
        [
            (-130_000, 130_000),
            (-160_000, 160_000),
            (-160_000, 160_000),
            (-260_000, 260_000)
        ]
    );
    for (rel, judge) in [(3_160 * MS, GREAT), (3_161 * MS, BAD)] {
        let mut e = engine(&c, &Setup::ln(LnType::Cn));
        e.update(SEC);
        key(&mut e, 5, 0, 2 * SEC, true);
        key(&mut e, 5, 0, rel, false);
        let evs = drain(&mut e);
        assert_eq!(
            evs[2],
            jt(5, judge, 3 * SEC - rel, 2 * u32::from(judge < BAD))
        );
    }
}

/// §9 Q6: a HCN body's Volume is sent once when it goes quiet (and once
/// when it sounds again), not on every step.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q6)"]
fn q06_hcn_body_volume_once() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    key_synced(&mut e, 0, 0, 2_050 * MS, false);
    // the early release's mute, the tail, the end sound; then the body's
    // mute once
    assert_eq!(
        drain(&mut e),
        [
            vol(10, 0.0),
            jt(0, POOR, 950_000, 0),
            snd(50, 0),
            vol(10, 0.0)
        ]
    );
    let mut t = 2_050 * MS;
    while t < 2_400 * MS {
        t += 50 * MS;
        e.update(t);
    }
    assert_eq!(drain(&mut e), NONE);
    key_synced(&mut e, 0, 0, 2_500 * MS, true);
    assert_eq!(drain(&mut e), [vol(10, 1.0)]);
    e.update(2_600 * MS);
    e.update(2_700 * MS);
    assert_eq!(drain(&mut e), NONE);
    key_synced(&mut e, 0, 0, 2_800 * MS, false);
    assert_eq!(drain(&mut e), [vol(10, 0.0)]);
    e.update(2_900 * MS);
    e.update(3_100 * MS);
    assert_eq!(drain(&mut e), NONE);
}

/// The old Volume rule's part that stays: a quiet body after a BAD (or
/// worse) tail is muted, and pressing again unmutes it.
#[test]
fn q06_hcn_body_volume_follows_the_key() {
    let c = hcn_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    drain(&mut e);
    key_synced(&mut e, 0, 0, 2_050 * MS, false);
    e.update(2_100 * MS);
    let evs = drain(&mut e);
    assert!(evs.contains(&vol(10, 0.0)) && !evs.contains(&vol(10, 1.0)));
    e.update(2_500 * MS);
    drain(&mut e);
    key(&mut e, 0, 0, 2_500 * MS, true);
    e.update(2_550 * MS);
    let evs = drain(&mut e);
    assert!(evs.contains(&vol(10, 1.0)) && !evs.contains(&vol(10, 0.0)));
    // a good tail: no Volume at all
    let mut e = engine(&c, &Setup::ln(LnType::Hcn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    key_synced(&mut e, 0, 0, 2_900 * MS, false);
    e.update(2_950 * MS);
    let evs = drain(&mut e);
    assert!(!evs.iter().any(|v| matches!(v, EngineEvent::Volume { .. })));
}

/// §9 Q8: a late-stamped input sees the notes as they were at its time,
/// even when the engine had already moved on.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q8)"]
fn q08_late_input_sees_passed_notes() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_500 * MS, 11)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_010 * MS);
    drain(&mut e);
    e.update(2_600 * MS);
    e.input(ev(2_120 * MS, 0, 0, true));
    e.update(2_620 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, -120_000, 1)]);
}

/// §9 Q9: an auto lane starts a long note whose head arrives while it still
/// holds the previous one: the previous one ends first.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q9)"]
fn q09_touching_cns_under_autoplay() {
    let c = chart7(vec![
        long(0, 2 * SEC, 3 * SEC, 10, 50),
        long(0, 3 * SEC, 4 * SEC, 11, 51),
    ]);
    let mut e = engine(
        &c,
        &Setup {
            auto: auto_all(),
            ln: LnType::Cn,
            ..Setup::default()
        },
    );
    let mut all = Vec::new();
    let mut t = 0;
    while t < 5 * SEC {
        e.update(t);
        all.extend(drain(&mut e));
        t += 16_667;
    }
    assert_eq!(
        all,
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            jt(0, PGREAT, 0, 2),
            snd(50, 0),
            snd(11, 0),
            jh(0, PGREAT, 0, 3),
            jt(0, PGREAT, 0, 4),
            snd(51, 0)
        ]
    );
    assert_eq!(e.ghost, [0, 0, 0, 0]);
}

/// §9 Q9 in LN mode.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q9)"]
fn q09_touching_lns_under_autoplay() {
    let c = chart7(vec![
        long(0, 2 * SEC, 3 * SEC, 10, 50),
        long(0, 3 * SEC, 4 * SEC, 11, 51),
    ]);
    let mut e = engine(
        &c,
        &Setup {
            auto: auto_all(),
            ..Setup::default()
        },
    );
    let mut all = Vec::new();
    let mut t = 0;
    while t < 5 * SEC {
        e.update(t);
        all.extend(drain(&mut e));
        t += 16_667;
    }
    assert_eq!(
        all,
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            snd(50, 0),
            snd(11, 0),
            jh(0, PGREAT, 0, 2),
            snd(51, 0)
        ]
    );
}

/// Q9's precondition, unchanged: long notes a frame apart are both played.
#[test]
fn q09_separate_lns_under_autoplay() {
    for ln in [LnType::Ln, LnType::Cn, LnType::Hcn] {
        let c = chart7(vec![
            long(0, 2 * SEC, 3 * SEC, 10, 50),
            long(0, 3_040 * MS, 4 * SEC, 11, 51),
        ]);
        let mut e = engine(
            &c,
            &Setup {
                auto: auto_all(),
                ln,
                ..Setup::default()
            },
        );
        let mut t = 0;
        while t < 5 * SEC {
            e.update(t);
            t += 16_667;
        }
        let n = if ln == LnType::Ln { 2 } else { 4 };
        assert_eq!(e.ghost, vec![0; n], "{ln:?}");
    }
}

/// §9 Q9: one long update under autoplay passes several long notes of a
/// lane; each one is started (the one held ended first).
#[test]
#[ignore = "clean-room: new behaviour (§9 Q9)"]
fn q09_one_big_update_under_autoplay() {
    let c = chart7(vec![
        long(0, 2 * SEC, 2_500 * MS, 10, 50),
        long(0, 3 * SEC, 3_500 * MS, 11, 51),
        normal(0, 4 * SEC, 12),
    ]);
    let auto = |ln| Setup {
        auto: auto_all(),
        ln,
        ..Setup::default()
    };
    let mut e = engine(&c, &auto(LnType::Ln));
    e.update(0);
    e.update(10 * SEC);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            snd(50, 0),
            snd(11, 0),
            snd(12, 0),
            jh(0, PGREAT, 0, 2),
            jh(0, PGREAT, 0, 3),
            snd(51, 0)
        ]
    );
    let mut e = engine(&c, &auto(LnType::Cn));
    e.update(0);
    e.update(10 * SEC);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            jt(0, PGREAT, 0, 2),
            snd(50, 0),
            snd(11, 0),
            jh(0, PGREAT, 0, 3),
            snd(12, 0),
            jh(0, PGREAT, 0, 4),
            jt(0, PGREAT, 0, 5),
            snd(51, 0)
        ]
    );
}

/// Custom rules with a gap between BAD (early) and empty POOR, to see the
/// search's handling of a note that falls in no window.
fn gap_rules() -> JudgeRules {
    let mut r = judge_rules(
        JudgePreset::Beatoraja,
        vibebms_core::mode::RuleFamily::SevenKeys,
        JudgeRank::Rank(3),
        None,
    );
    let t = vec![
        (-10_000, 10_000),
        (-20_000, 20_000),
        (-50_000, 50_000),
        (-300_000, 100_000),
        (200_000, 500_000),
    ];
    r.windows.note = t.clone();
    r.windows.scratch = t;
    r
}

fn gap_engine(c: &Chart) -> Engine {
    let s = Setup {
        algorithm: JudgeAlgorithm::Duration,
        ..Setup::default()
    };
    let mut cfg = config_for(c, &s);
    cfg.rules = gap_rules();
    Engine::new(c, cfg, gauge_for(c, &s))
}

/// §9 Q11: a note in no window never discards an earlier valid choice.
#[test]
#[ignore = "clean-room: new behaviour (§9 Q11)"]
fn q11_none_keeps_the_choice() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_380 * MS, 11)]);
    let mut e = gap_engine(&c);
    e.update(SEC);
    press(&mut e, 0, 2_250 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, BAD, -250_000, 0)]);
}

/// The same rules without a competing note: the first note is BAD.
#[test]
fn q11_gap_rules_single_note() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(0, 2_700 * MS, 11)]);
    let mut e = gap_engine(&c);
    e.update(SEC);
    press(&mut e, 0, 2_250 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, BAD, -250_000, 0)]);
}

/// §9 Q12: the search's bounds are inclusive at both ends: a note exactly
/// at the latest early bound (+500 ms) is in reach (empty POOR).
#[test]
#[ignore = "clean-room: new behaviour (§9 Q12)"]
fn q12_search_includes_the_early_bound() {
    let c = chart7(vec![normal(0, SEC, 9), normal(0, 2_500 * MS, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    press(&mut e, 0, SEC);
    release(&mut e, 0, 1_010 * MS);
    drain(&mut e);
    press(&mut e, 0, 2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, MISS, 500_000, 1)]);
}

/// Just inside the bound (unchanged): an empty POOR; just outside: none.
#[test]
fn q12_search_bound_neighbours() {
    let c = chart7(vec![normal(0, SEC, 9), normal(0, 2_500 * MS, 10)]);
    for (t, want) in [
        (2 * SEC + 1, vec![snd(10, 0), jh(0, MISS, 499_999, 1)]),
        (2 * SEC - 1, vec![snd(9, 0)]),
    ] {
        let mut e = engine(&c, &Setup::default());
        e.update(0);
        press(&mut e, 0, SEC);
        release(&mut e, 0, 1_010 * MS);
        drain(&mut e);
        press(&mut e, 0, t);
        assert_eq!(drain(&mut e), want);
    }
    // the late edge is inclusive: a note exactly at BAD's late bound
    let c = chart7(vec![normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    press(&mut e, 0, 2_280 * MS);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, BAD, -280_000, 0)]);
}

// ---------------------------------------------------------------------------
// The rest of the API (§1.15)
// ---------------------------------------------------------------------------

/// Inputs are recorded first, even ones then ignored (an auto lane, a lane
/// out of range); replaying the ops reproduces everything.
#[test]
fn recording_and_replaying() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(7, 2 * SEC, 11)]);
    let mut auto = vec![false; 7];
    auto.push(true);
    let s = Setup {
        auto,
        ..Setup::default()
    };
    let mut e = engine(&c, &s);
    assert!(e.recorded().is_empty());
    e.update(SEC);
    e.record();
    e.record(); // idempotent
    e.input(ev(1_500 * MS, 7, 0, true));
    e.input(ev(1_500 * MS, 99, 0, true));
    assert!(e.held().is_empty());
    e.update(1_500 * MS);
    assert!(e.held().is_empty());
    press(&mut e, 0, 1_990 * MS);
    e.update(3 * SEC);
    let ops = e.recorded().to_vec();
    assert_eq!(
        ops,
        [
            Op::Input(1_500 * MS, 7, 0, true),
            Op::Input(1_500 * MS, 99, 0, true),
            Op::Update(1_500 * MS),
            Op::Input(1_990 * MS, 0, 0, true),
            Op::Update(1_990 * MS),
            Op::Update(3 * SEC)
        ]
    );
    let mut r = engine(&c, &s);
    r.update(SEC);
    r.record();
    for op in &ops {
        r.apply(*op);
    }
    assert_eq!(r.recorded(), ops, "apply records again");
    let evs = drain(&mut e);
    assert_eq!(
        evs,
        [
            snd(10, 0),
            jh(0, PGREAT, 10_000, 1),
            snd(11, 7),
            jh(7, PGREAT, 0, 2)
        ]
    );
    assert_eq!(drain(&mut r), evs);
    assert_eq!(r.ghost, e.ghost);
    assert_eq!(r.score, e.score);
    assert_eq!(note_states(&r), note_states(&e));
}

/// `held()`: the keys down, then the queued inputs in arrival order.
#[test]
fn held_with_queued_inputs() {
    let c = chart7(vec![normal(0, 5 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    key(&mut e, 3, 0, 100 * MS, true);
    key(&mut e, 7, 1, 100 * MS, true);
    key(&mut e, 7, 0, 100 * MS, true);
    assert_eq!(e.held(), [(3, 0), (7, 0), (7, 1)]);
    e.input(ev(200 * MS, 1, 0, true));
    e.input(ev(150 * MS, 0, 1, true));
    e.input(ev(200 * MS, 3, 0, false));
    e.input(ev(210 * MS, 1, 0, false));
    e.input(ev(220 * MS, 1, 0, true));
    assert_eq!(e.held(), [(7, 0), (7, 1), (0, 1), (1, 0)]);
    e.update(300 * MS);
    assert_eq!(e.held(), [(0, 1), (1, 0), (7, 0), (7, 1)]);
    // a key index is reduced to its lowest bit
    key(&mut e, 2, 3, 400 * MS, true);
    assert_eq!(e.held(), [(0, 1), (1, 0), (2, 1), (7, 0), (7, 1)]);
}

/// §9.3 decision 7: `held()` reports a queued input's key reduced to its
/// lowest bit, as it is stored once processed.
#[test]
#[ignore = "clean-room: new behaviour (§9.3 decision 7)"]
fn held_reduces_queued_key_indices() {
    let c = chart7(vec![normal(0, 5 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(0);
    e.input(ev(100 * MS, 2, 3, true));
    assert_eq!(e.held(), [(2, 1)]);
    e.input(ev(110 * MS, 2, 1, false));
    assert!(e.held().is_empty());
    e.input(ev(120 * MS, 2, 2, true));
    assert_eq!(e.held(), [(2, 0)]);
    e.update(200 * MS);
    assert_eq!(e.held(), [(2, 0)]);
}

/// `take_sounds` takes Sound and Volume only, in order.
#[test]
fn take_sounds_leaves_the_rest() {
    let c = ln_chart();
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    release(&mut e, 0, 2_790 * MS);
    assert_eq!(e.take_sounds(), [snd(10, 0), vol(10, 0.0), snd(50, 0)]);
    assert!(e.take_sounds().is_empty());
    assert_eq!(drain(&mut e), [jh(0, PGREAT, 0, 1), jt(0, BAD, 210_000, 0)]);
}

/// An input stamped after `now` is handled at `now`, its diff from its own
/// time.
#[test]
fn future_stamped_input() {
    let c = chart7(vec![normal(0, 2 * SEC, 10)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    e.input(ev(2_050 * MS, 0, 0, true));
    e.update(2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), jh(0, GREAT, -50_000, 1)]);
}

/// Time going backwards: nothing passes again.
#[test]
fn update_backwards() {
    let c = chart7(vec![
        mine(0, 1_700 * MS, 5.0, 90),
        normal(1, 1_800 * MS, 11),
        normal(2, 2 * SEC, 12),
    ]);
    let mut auto = vec![false; 8];
    auto[1] = true;
    let mut e = engine(
        &c,
        &Setup {
            auto,
            ..Setup::default()
        },
    );
    e.update(SEC);
    press(&mut e, 0, 1_100 * MS);
    e.update(1_900 * MS);
    assert_eq!(
        drain(&mut e),
        [mine_ev(0, 5.0), snd(90, 0), snd(11, 1), jh(1, PGREAT, 0, 1)]
    );
    e.update(1_500 * MS);
    e.update(1_900 * MS);
    assert_eq!(drain(&mut e), NONE);
    // the settle phase still runs at the earlier time: no miss there
    e.update(2_200 * MS);
    e.update(SEC);
    assert_eq!(drain(&mut e), NONE);
    e.update(2_300 * MS);
    assert_eq!(drain(&mut e), [jh(2, POOR, -300_000, 0)]);
}

/// EX-HARD: `Failed` once, right after the judgement that emptied it;
/// judging goes on.
#[test]
fn ex_hard_fails_once() {
    let notes = (0..10)
        .map(|k| normal(0, 2 * SEC + k * 100 * MS, 10))
        .collect();
    let c = chart7(notes);
    let mut e = engine(
        &c,
        &Setup {
            gauge: GaugeType::ExHard,
            ..Setup::default()
        },
    );
    e.update(SEC);
    e.update(5 * SEC);
    let evs = drain(&mut e);
    assert_eq!(evs.len(), 11);
    assert_eq!(evs[7], EngineEvent::Failed);
    assert!(evs[..7]
        .iter()
        .chain(&evs[8..])
        .all(|v| matches!(v, EngineEvent::Judge { judge: POOR, .. })));
    assert!(e.is_failed());
    assert_eq!(e.ghost.len(), 10);
    assert_eq!(e.gauge.value(), 0.0);
    // CONTINUE: never failed
    let mut e = engine(
        &c,
        &Setup {
            gauge: GaugeType::ExHard,
            gas: GaugeAutoShift::Continue,
            ..Setup::default()
        },
    );
    e.update(SEC);
    e.update(5 * SEC);
    assert!(!drain(&mut e).contains(&EngineEvent::Failed));
    assert!(!e.is_failed());
    // HARD -> GROOVE: HAZARD empties at the first POOR and shifts
    let mut e = engine(
        &c,
        &Setup {
            gauge: GaugeType::Hazard,
            gas: GaugeAutoShift::SurvivalToGroove,
            ..Setup::default()
        },
    );
    e.update(SEC);
    e.update(5 * SEC);
    assert!(!e.is_failed());
    assert_eq!(e.gauge.current_type(), GaugeType::Normal);
}

/// A mine can fail the gauge too.
#[test]
fn mine_fails_the_gauge() {
    let c = chart7(vec![mine(0, 2 * SEC, 100.0, 90), normal(1, 3 * SEC, 10)]);
    let mut e = engine(
        &c,
        &Setup {
            gauge: GaugeType::Hard,
            ..Setup::default()
        },
    );
    e.update(SEC);
    press(&mut e, 0, 1_500 * MS);
    e.update(2_100 * MS);
    assert_eq!(
        drain(&mut e),
        [mine_ev(0, 100.0), snd(90, 0), EngineEvent::Failed]
    );
}

/// `is_finished`: past the last end time (mines included) and every
/// judgement passed.
#[test]
fn is_finished_boundary() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), mine(1, 2_500 * MS, 1.0, 90)]);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    assert!(!e.is_finished(10 * SEC));
    press(&mut e, 0, 2 * SEC);
    assert!(!e.is_finished(2_500 * MS));
    assert!(e.is_finished(2_500 * MS + 1));
    // an empty chart: finished once past 0
    let c = chart7(vec![]);
    let e = engine(&c, &Setup::default());
    assert!(!e.is_finished(0));
    assert!(e.is_finished(1));
}

/// `Op`: replay JSON format (untagged) and lane shifting.
#[test]
fn op_format_and_shift() {
    let ops = vec![Op::Update(-5), Op::Input(2_000_000, 3, 1, true)];
    let json = serde_json::to_string(&ops).unwrap();
    assert_eq!(json, "[-5,[2000000,3,1,true]]");
    let back: Vec<Op> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ops);
    assert_eq!(Op::Update(7).shifted(8), Op::Update(7));
    assert_eq!(
        Op::Input(1, 3, 0, false).shifted(8),
        Op::Input(1, 11, 0, false)
    );
    assert_eq!(
        Op::Input(1, 250, 0, true).shifted(10),
        Op::Input(1, 4, 0, true)
    );
}

/// `notes()`: the lane's notes in chart order; kinds and LN types; the
/// chart index; lanes past the mode's dropped.
#[test]
fn play_notes() {
    let c = chart7(vec![
        normal(0, SEC, 1),
        typed(1, SEC, 2 * SEC, 2, 3, Some(LnType::Cn)),
        typed(2, SEC, 2 * SEC, 4, 5, Some(LnType::Hcn)),
        long(3, SEC, 2 * SEC, 6, 7),
        mine(4, SEC, 3.0, 8),
        hidden(5, SEC, 9),
        normal(0, 500 * MS, 11),
    ]);
    let e = engine(&c, &Setup::ln(LnType::Ln));
    let n: Vec<&PlayNote> = e.notes().iter().collect();
    assert_eq!(n.len(), 6);
    assert_eq!(
        n.iter()
            .map(|n| (n.lane, n.time, n.end_time, n.chart_index))
            .collect::<Vec<_>>(),
        [
            (0, 500 * MS, 500 * MS, 0),
            (0, SEC, SEC, 1),
            (1, SEC, 2 * SEC, 2),
            (2, SEC, 2 * SEC, 3),
            (3, SEC, 2 * SEC, 4),
            (4, SEC, SEC, 5),
        ]
    );
    assert_eq!(
        n.iter().map(|n| n.ln_type()).collect::<Vec<_>>(),
        [
            None,
            None,
            Some(LnType::Cn),
            Some(LnType::Hcn),
            Some(LnType::Ln),
            None
        ]
    );
    assert!(n[3].is_hcn() && !n[2].is_hcn());
    assert!(n[4].is_long() && !n[0].is_long() && !n[5].is_long());
    assert!(n[5].is_mine());
    assert_eq!((n[2].sound, n[2].end_sound), (2, 3));
    assert_eq!(n[0].end_sound, NO_SOUND_ID);
    // judgements: 1 + 2 (typed CN) + 2 (typed HCN) + 1 (LN) + 1, no mine
    assert_eq!(e.score.total_notes, 7);
    assert_eq!(e.windows(), &e.rules().windows);
    assert_eq!(e.player(), None);
    assert!(e.judges(0) && e.judges(200));
}

/// §9.3 decision 6: notes on lanes the mode does not have are dropped and
/// not counted, so the chart finishes.
#[test]
#[ignore = "clean-room: new behaviour (§9.3 decision 6)"]
fn notes_past_the_lane_count_are_not_counted() {
    let c = chart7(vec![
        normal(0, 2 * SEC, 10),
        normal(9, 2 * SEC, 11),
        long(12, 2 * SEC, 3 * SEC, 12, 52),
    ]);
    for ln in [LnType::Ln, LnType::Cn] {
        let mut e = engine(&c, &Setup::ln(ln));
        assert_eq!(e.notes().len(), 1);
        assert_eq!(e.score.total_notes, 1, "{ln:?}");
        e.update(SEC);
        press(&mut e, 0, 2 * SEC);
        e.update(4 * SEC);
        assert!(e.is_finished(4 * SEC), "{ln:?}");
    }
}

/// A long note of no type follows the configured LN mode; a typed one its
/// own.
#[test]
fn typed_long_notes() {
    let c = chart7(vec![
        typed(0, 2 * SEC, 3 * SEC, 10, 50, Some(LnType::Ln)),
        long(1, 2 * SEC, 3 * SEC, 11, 51),
    ]);
    let mut e = engine(&c, &Setup::ln(LnType::Cn));
    assert_eq!(e.score.total_notes, 3);
    e.update(SEC);
    e.input(ev(2 * SEC, 0, 0, true));
    e.input(ev(2 * SEC, 1, 0, true));
    e.update(2 * SEC);
    assert_eq!(drain(&mut e), [snd(10, 0), snd(11, 1), jh(1, PGREAT, 0, 1)]);
}

/// `auto_lanes` shorter than the lanes: the rest are manual.
#[test]
fn auto_lanes_padded() {
    let c = chart7(vec![normal(7, 2 * SEC, 10)]);
    let e = engine(
        &c,
        &Setup {
            auto: vec![true],
            ..Setup::default()
        },
    );
    assert!(e.is_auto(0) && !e.is_auto(7) && !e.is_auto(100));
}

/// Zero-length long notes and two notes at one time on a lane play
/// through.
#[test]
fn degenerate_charts() {
    for ln in [LnType::Ln, LnType::Cn, LnType::Hcn] {
        let c = chart7(vec![
            long(0, 2 * SEC, 2 * SEC, 10, 50),
            normal(1, 2 * SEC, 11),
            normal(1, 2 * SEC, 12),
        ]);
        for auto in [false, true] {
            let mut e = engine(
                &c,
                &Setup {
                    ln,
                    auto: vec![auto; 8],
                    ..Setup::default()
                },
            );
            e.update(SEC);
            if !auto {
                press(&mut e, 0, 2 * SEC);
                press(&mut e, 1, 2 * SEC);
                release(&mut e, 1, 2_010 * MS);
                press(&mut e, 1, 2_020 * MS);
            }
            e.update(3 * SEC);
            assert!(e.is_finished(3 * SEC), "{ln:?} auto {auto}");
        }
    }
    // an empty chart and a mines-only chart
    for c in [chart7(vec![]), chart7(vec![mine(0, SEC, 1.0, 90)])] {
        let mut e = engine(&c, &Setup::default());
        press(&mut e, 0, 500 * MS);
        e.update(2 * SEC);
        assert!(e.is_finished(2 * SEC));
        assert_eq!(e.score.total_notes, 0);
    }
}

/// Negative note times are passed from the first update (which starts
/// at min(now, 0) − 1 µs).
#[test]
fn first_update_window() {
    let c = chart7(vec![normal(0, -SEC, 10), normal(1, 0, 11)]);
    let mut e = engine(
        &c,
        &Setup {
            auto: vec![true; 8],
            ..Setup::default()
        },
    );
    e.update(-2 * SEC);
    assert_eq!(drain(&mut e), NONE);
    e.update(0);
    assert_eq!(
        drain(&mut e),
        [
            snd(10, 0),
            jh(0, PGREAT, 0, 1),
            snd(11, 1),
            jh(1, PGREAT, 0, 2)
        ]
    );
    // a first update at a later time passes the earlier notes at once
    let mut e = engine(
        &c,
        &Setup {
            auto: vec![true; 8],
            ..Setup::default()
        },
    );
    e.update(SEC);
    // the note before 0 is not passed: never played, swept as a miss
    assert_eq!(e.ghost, [0, 4]);
}

/// Charts with hostile values (huge or zero measure lengths, zero or
/// negative BPM, huge stops, a huge #DEFEXRANK) play through under
/// autoplay without panicking.
#[test]
fn hostile_charts_play_through() {
    let heads = [
        "#BPM 0",
        "#BPM -150",
        "#00102:1e300",
        "#00102:0",
        "#00102:-1",
        "#BPM02 0\n#00108:02",
        "#STOP02 1e300\n#00109:02",
        "#STOP02 -1e9\n#00109:02",
        "#DEFEXRANK 1e300",
        "#DEFEXRANK -1",
        "#TOTAL 0",
        "#TOTAL -50",
    ];
    for head in heads {
        let src = format!(
            "#BPM 150\n#WAV01 k.wav\n{head}\n#00112:0101\n#00113:01\n#00253:0101\n#00316:01\n#001D4:0A\n"
        );
        let c = vibebms_core::bms::parse_bms(src.as_bytes(), "bms", &Default::default()).unwrap();
        for ln in [LnType::Ln, LnType::Cn, LnType::Hcn] {
            for auto in [true, false] {
                let mut e = engine(
                    &c,
                    &Setup {
                        ln,
                        auto: vec![auto; 8],
                        ..Setup::default()
                    },
                );
                let end = c.length().clamp(0, 300 * SEC);
                let mut t = -SEC;
                while t <= end + SEC {
                    if !auto {
                        press(&mut e, 1, t + 3);
                        release(&mut e, 1, t + 4);
                    }
                    e.update(t);
                    t += 40 * MS;
                }
                drain(&mut e);
            }
        }
    }
}

/// Combo: in dense jacks a slightly late press keeps to its own note,
/// and the next press takes the next one.
#[test]
fn combo_algorithm_on_dense_jacks() {
    let notes = (0..8)
        .map(|k| normal(0, 2 * SEC + k * 100 * MS, 10 + k as u32))
        .collect();
    let c = chart7(notes);
    let mut e = engine(&c, &Setup::default());
    e.update(SEC);
    let mut want = Vec::new();
    for k in 0..8i64 {
        let t = 2 * SEC + k * 100 * MS + 30 * MS; // 30 ms late: GREAT
        press(&mut e, 0, t);
        release(&mut e, 0, t + 20 * MS);
        want.push(snd(10 + k as u32, 0));
        want.push(jh(0, GREAT, -30_000, k as u32 + 1));
    }
    assert_eq!(drain(&mut e), want);
}

/// A battle side judges as its single play chart, whatever the lane
/// option, LN mode and AUTO SCRATCH, driven as the play scene drives it
/// (the test bot); the play's ops moved to the second side judge the same.
#[test]
fn battle_side_judges_as_the_single_play_chart() {
    use vibebms_core::lane_option::{apply, battle, LaneOption};
    for keys in [5usize, 7] {
        let src = vibebms_core::bot::chart_text(keys, 31 + keys as i64);
        let base =
            vibebms_core::bms::parse_bms(src.as_bytes(), "bme", &Default::default()).unwrap();
        let mode = base.mode();
        let lpp = mode.lanes_per_player() as u8;
        for (i, opt) in LaneOption::ALL.into_iter().enumerate() {
            let ln = [LnType::Hcn, LnType::Ln, LnType::Cn][i % 3];
            let seed = 77 + i as i64;
            let mut single = base.clone();
            apply(&mut single, 0, opt, seed, 125_000);
            let mut both = single.clone();
            battle(&mut both);
            let auto: Vec<bool> = (0..2 * lpp as usize)
                .map(|l| i % 2 == 1 && (l % lpp as usize) == lpp as usize - 1)
                .collect();
            let s = |player| Setup {
                ln,
                auto: auto.clone(),
                player,
                ..Setup::default()
            };
            let inputs = vibebms_core::bot::inputs(&single, None, seed);
            let end = single.length() + 2 * SEC;
            let mut a = engine(&single, &s(None));
            a.record();
            vibebms_core::bot::drive(&mut a, &inputs, end);
            let mut b = engine(&both, &s(Some(0)));
            vibebms_core::bot::drive(&mut b, &inputs, end);
            let mut c = engine(&both, &s(Some(1)));
            for op in a.recorded() {
                c.apply(op.shifted(lpp));
            }
            assert!(a.score.ex_score() > 0, "{keys}K {opt:?}");
            for (side, x) in [("1P", &b), ("2P", &c)] {
                assert_eq!(x.score, a.score, "{keys}K {opt:?} {side}");
                assert_eq!(x.ghost, a.ghost, "{keys}K {opt:?} {side}");
                assert_eq!(x.judge_log, a.judge_log, "{keys}K {opt:?} {side}");
                assert_eq!(x.timing_log, a.timing_log, "{keys}K {opt:?} {side}");
                assert_eq!(bits(&x.gauge.values()), bits(&a.gauge.values()));
                assert!(x.is_finished(end));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Battle: one engine judges one side
// ---------------------------------------------------------------------------

/// A 7K chart as a battle chart (14K, both sides the same).
fn battle_of(c: &Chart) -> Chart {
    let mut b = c.clone();
    vibebms_core::lane_option::battle(&mut b);
    b
}

#[test]
fn battle_side_takes_only_its_lanes() {
    let sp = chart7(vec![
        normal(0, 2 * SEC, 10),
        long(1, 2 * SEC, 3 * SEC, 11, 51),
        mine(7, 2_500 * MS, 5.0, 90),
        hidden(2, SEC, 20),
    ]);
    let bt = battle_of(&sp);
    assert_eq!(bt.mode(), PlayMode::Beat14K);
    for p in [0usize, 1] {
        let s = Setup {
            ln: LnType::Cn,
            player: Some(p),
            ..Setup::default()
        };
        let mut e = engine(&bt, &s);
        assert_eq!(e.player(), Some(p));
        assert_eq!(e.score.total_notes, 3);
        let base = 8 * p as u8;
        assert!(e
            .notes()
            .iter()
            .all(|n| n.lane >= base && n.lane < base + 8));
        assert_eq!(e.notes().len(), 3);
        for lane in 0..16u8 {
            assert_eq!(e.judges(lane), lane / 8 == p as u8);
        }
        assert!(!e.judges(16), "past the lanes: lane 16 is a third player's");
        e.record();
        e.input(ev(SEC, 8 - base, 0, true)); // the other side: recorded only
        assert!(e.held().is_empty());
        assert_eq!(e.recorded().len(), 1);
        e.update(SEC);
        press(&mut e, base, 2 * SEC);
        assert_eq!(drain(&mut e), [snd(10, base), jh(base, PGREAT, 0, 1)]);
        // the other side's empty press finds none of this side's sounds
        press(&mut e, (base + 10) % 16, 2_100 * MS);
        assert_eq!(drain(&mut e), NONE);
        e.update(4 * SEC);
        assert_eq!(e.ghost, [0, 4, 4]);
        assert!(e.is_finished(4 * SEC));
    }
}

/// A single play's ops, moved by the lanes per player, judge the battle
/// chart's second side the same.
#[test]
fn battle_second_side_replays_the_first() {
    for (mode, ln) in [
        (PlayMode::Beat7K, LnType::Hcn),
        (PlayMode::Beat5K, LnType::Cn),
    ] {
        let sp = gen_chart(mode, 77, false, JudgeRank::Rank(2), 12);
        let bt = battle_of(&sp);
        let lpp = mode.lanes_per_player() as u8;
        let s = Setup {
            ln,
            ..Setup::default()
        };
        let inputs = gen_inputs(&sp, &s, 5);
        let mut a = engine(&sp, &s);
        a.record();
        let ta = safe_drive(&mut a, &inputs, sp.length() + 2 * SEC, false);
        let mut b = engine(
            &bt,
            &Setup {
                player: Some(1),
                ..s.clone()
            },
        );
        let mut tb = Vec::new();
        for op in a.recorded() {
            b.apply(op.shifted(lpp));
            tb.extend(drain(&mut b).into_iter().map(|v| unshift(v, lpp)));
        }
        assert_eq!(
            tb,
            ta.events.into_iter().flatten().collect::<Vec<_>>(),
            "{mode:?}"
        );
        assert_eq!(b.score, a.score);
        assert_eq!(b.ghost, a.ghost);
        assert_eq!(b.judge_log, a.judge_log);
        assert_eq!(b.timing_log, a.timing_log);
        assert_eq!(b.gauge.values(), a.gauge.values());
        // and the first side with the same ops unshifted
        let mut c = engine(
            &bt,
            &Setup {
                player: Some(0),
                ..s.clone()
            },
        );
        for op in a.recorded() {
            c.apply(*op);
        }
        assert_eq!(c.score, a.score);
        assert_eq!(c.ghost, a.ghost);
    }
}

fn unshift(v: EngineEvent, by: u8) -> EngineEvent {
    match v {
        EngineEvent::Judge {
            lane,
            judge,
            diff,
            combo,
            tail,
        } => EngineEvent::Judge {
            lane: lane - by,
            judge,
            diff,
            combo,
            tail,
        },
        EngineEvent::Sound { sound, lane } => EngineEvent::Sound {
            sound,
            lane: lane - by,
        },
        EngineEvent::Mine { lane, damage } => EngineEvent::Mine {
            lane: lane - by,
            damage,
        },
        v => v,
    }
}

// ---------------------------------------------------------------------------
// Gauge interaction (§1.15)
// ---------------------------------------------------------------------------

/// A judgement updates every gauge with its change at rate 1; an empty
/// POOR too.
#[test]
fn judgements_update_every_gauge() {
    let c = chart7(vec![normal(0, 2 * SEC, 10), normal(1, 2 * SEC, 11)]);
    for (preset, gp) in [
        (JudgePreset::Beatoraja, GaugePreset::Beatoraja),
        (JudgePreset::Lr2, GaugePreset::Lr2),
        (JudgePreset::Iidx, GaugePreset::Iidx),
    ] {
        let s = Setup {
            preset,
            gauge_preset: gp,
            ..Setup::default()
        };
        let mut e = engine(&c, &s);
        let mut model = gauge_for(&c, &s);
        e.update(SEC);
        press(&mut e, 0, 1_950 * MS); // GOOD in every preset? GREAT/GOOD
        let j = match drain(&mut e)[1] {
            EngineEvent::Judge { judge, .. } => judge,
            _ => unreachable!(),
        };
        model.update(j, 1.0);
        assert_eq!(e.gauge, model, "{preset:?}");
        press(&mut e, 1, 1_700 * MS + 1); // an empty POOR (both presets)
        model.update(MISS, 1.0);
        assert_eq!(e.gauge, model, "{preset:?}");
        e.update(3 * SEC);
        model.update(POOR, 1.0);
        assert_eq!(e.gauge, model, "{preset:?}");
    }
}

/// HCN ticks are gauge updates at rate 0.5.
#[test]
fn hcn_ticks_update_every_gauge() {
    let c = hcn_chart();
    let s = Setup {
        ln: LnType::Hcn,
        gauge: GaugeType::Hard,
        ..Setup::default()
    };
    let mut e = engine(&c, &s);
    let mut model = gauge_for(&c, &s);
    e.update(SEC);
    press(&mut e, 0, 2 * SEC);
    model.update(PGREAT, 1.0);
    e.update(2_201 * MS);
    model.update(GREAT, 0.5);
    assert_eq!(e.gauge, model);
    key_synced(&mut e, 0, 0, 2_300 * MS, false);
    model.update(POOR, 1.0);
    e.update(2_600 * MS); // -200 ms banked: no tick yet
    assert_eq!(e.gauge, model);
    e.update(2_601 * MS);
    model.update(BAD, 0.5);
    assert_eq!(e.gauge, model);
}

/// Gauge types and auto shift as the engine drives them, on one play.
#[test]
fn gauge_types_over_a_play() {
    let c = gen_chart(PlayMode::Beat7K, 3, false, JudgeRank::Rank(2), 10);
    let mut seen = BTreeMap::new();
    for gauge in GaugeType::ALL {
        for gas in GaugeAutoShift::ALL {
            let s = Setup {
                gauge,
                gas,
                ..Setup::default()
            };
            let inputs = gen_inputs(&c, &s, 1);
            let mut e = engine(&c, &s);
            safe_drive(&mut e, &inputs, c.length() + SEC, false);
            seen.insert(
                format!("{gauge:?} {gas:?}"),
                (
                    e.gauge.current_type(),
                    e.is_failed(),
                    bits(&e.gauge.values()),
                ),
            );
        }
    }
    let text: String = seen
        .iter()
        .map(|(k, (t, f, v))| format!("{k}: {t:?} {f} {v}\n"))
        .collect();
    golden_text("gauge_types.txt", &text);
}

// ---------------------------------------------------------------------------
// Replays and long deterministic plays
// ---------------------------------------------------------------------------

/// Record a play, save its ops as replay JSON, load and apply them to a
/// fresh engine: every event, log, score, note and gauge the same.
#[test]
fn replay_round_trip() {
    for (mode, ln, preset) in [
        (PlayMode::Beat7K, LnType::Ln, JudgePreset::Beatoraja),
        (PlayMode::Beat14K, LnType::Cn, JudgePreset::Lr2),
        (PlayMode::Popn9K, LnType::Hcn, JudgePreset::Iidx),
    ] {
        let c = gen_chart(mode, 9, true, JudgeRank::Rank(2), 15);
        let s = Setup {
            ln,
            preset,
            ..Setup::default()
        };
        let inputs = gen_inputs(&c, &s, 4);
        let mut e = engine(&c, &s);
        e.record();
        let trace = safe_drive(&mut e, &inputs, c.length() + SEC, false);
        let json = serde_json::to_vec(e.recorded()).unwrap();
        let ops: Vec<Op> = serde_json::from_slice(&json).unwrap();
        let mut r = engine(&c, &s);
        let mut events = Vec::new();
        for op in &ops {
            r.apply(*op);
            if matches!(op, Op::Update(_)) {
                events.push(drain(&mut r));
            }
        }
        assert_eq!(events, trace.events);
        assert_eq!(r.score, e.score);
        assert_eq!(r.ghost, e.ghost);
        assert_eq!(r.judge_log, e.judge_log);
        assert_eq!(r.timing_log, e.timing_log);
        assert_eq!(note_states(&r), note_states(&e));
        assert_eq!(bits(&r.gauge.values()), bits(&e.gauge.values()));
        assert_eq!(r.held(), e.held());
    }
}

/// Seeded LCG for the made-up charts and players.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x0005_DEEC_E66D)
    }
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + i64::from(self.next()) % (hi - lo + 1)
    }
}

/// A made-up chart: every lane of `mode` with notes, long notes (typed at
/// random when `typed`), mines and invisible notes, `secs` long. Notes on
/// one lane are at least 60 ms apart (after a long note's end).
fn gen_chart(mode: PlayMode, seed: u64, typed_ln: bool, rank: JudgeRank, secs: i64) -> Chart {
    let mut r = Rng::new(seed);
    let mut notes = Vec::new();
    let gaps = [60, 80, 100, 120, 150, 200, 250, 300, 400, 500, 700, 1000];
    for lane in 0..mode.lane_count() as u8 {
        let mut t = SEC + r.range(0, 500) * MS;
        let mut k = 0u32;
        while t < secs * SEC {
            let sound = u32::from(lane) * 1000 + k;
            k += 1;
            let roll = r.below(100);
            let end = if roll < 7 {
                notes.push(mine(lane, t, [5.0, 10.0, 2.5][r.below(3) as usize], sound));
                t
            } else if roll < 10 {
                notes.push(hidden(lane, t, sound));
                t
            } else if roll < 28 {
                let end = t + r.range(12, 90) * 10 * MS;
                let ty = if typed_ln {
                    [None, Some(LnType::Ln), Some(LnType::Cn), Some(LnType::Hcn)]
                        [r.below(4) as usize]
                } else {
                    None
                };
                notes.push(typed(lane, t, end, sound, sound + 500, ty));
                end
            } else {
                notes.push(normal(lane, t, sound));
                t
            };
            t = end + gaps[r.below(gaps.len() as u32) as usize] * MS;
        }
    }
    chart_of(mode, rank, None, notes)
}

/// A made-up player of `c` under `s`: hits with some error, now and then
/// misses, presses where there is no note, lets long notes go early or
/// late, presses a HCN body again. Input times are never on a whole
/// millisecond, so never at a note's or mine's time. It keeps clear of
/// what §9 changes: no press has a note exactly at the search's early
/// bound. (It also hits the notes before a charge note cleanly, a guard
/// from when Q4 was to change; kept so the fixture stays as recorded.)
fn gen_inputs(c: &Chart, s: &Setup, seed: u64) -> Vec<InputEvent> {
    let mode = c.mode();
    let w = judge_rules(s.preset, mode.family(), c.meta.judge_rank, s.rates).windows;
    let (_, early) = w.extent();
    let mut r = Rng::new(seed + 1000);
    let mut out = Vec::new();
    for lane in 0..mode.lane_count() {
        if s.player.is_some_and(|p| mode.player_of_lane(lane) != p)
            || s.auto.get(lane).copied().unwrap_or(false)
        {
            continue;
        }
        let sc = mode.is_scratch(lane);
        let all: Vec<&Note> = c.notes.iter().filter(|n| n.lane as usize == lane).collect();
        let notes: Vec<&Note> = all.iter().copied().filter(|n| n.is_playable()).collect();
        let charge = |n: &Note| match n.kind {
            NoteKind::Long { ln_type, .. } => ln_type.unwrap_or(s.ln) != LnType::Ln,
            _ => false,
        };
        // a press at `t` must not have a note exactly at the early bound
        let clear = |t: i64| {
            let mut t = t;
            while all.iter().any(|n| (n.time - t - early).abs() < 10 * MS) {
                t += 7 * MS;
            }
            t
        };
        let mut free = i64::MIN / 2;
        for (k, n) in notes.iter().enumerate() {
            let next = notes.get(k + 1).map_or(i64::MAX / 2, |m| m.time);
            let prev_end = if k > 0 {
                notes[k - 1].end_time()
            } else {
                i64::MIN / 2
            };
            // the notes before a charge note are hit cleanly
            let guarded = charge(n)
                || notes[k + 1..]
                    .iter()
                    .take_while(|m| m.time < n.end_time() + 700 * MS)
                    .any(|m| charge(m));
            let roll = r.below(100);
            if !guarded && roll < 7 && next - n.end_time() > 450 * MS {
                continue; // missed
            }
            let k_key = if sc { (k % 2) as u8 } else { 0 };
            let jitter = if guarded {
                r.range(-30, 30)
            } else if roll < 80 || next - n.end_time() < 400 * MS {
                r.range(-60, 60)
            } else {
                r.range(-140, 140)
            };
            let mut press = n.time + jitter * MS + 333;
            press = press.max(free + MS).max(prev_end + 20 * MS + 333);
            let press = clear(press);
            let end = n.end_time();
            let limit = next - 30 * MS;
            let mut push = |t: i64, k: u8, down: bool| {
                out.push(ev(t, lane as u8, k, down));
            };
            push(press, k_key, true);
            let mut up = if let NoteKind::Long { .. } = n.kind {
                match r.below(10) {
                    0 | 1 => (n.time + end) / 2 + 333,
                    2 => end + 400 * MS + 333,
                    _ => end + r.range(-90, 90) * MS + 333,
                }
            } else {
                press + r.range(20, 70) * MS
            };
            up = up.min(limit).max(press + MS);
            let hcn = matches!(n.kind, NoteKind::Long { ln_type, .. }
                if ln_type.unwrap_or(s.ln) == LnType::Hcn);
            // turntable mode: a same-key release inside the windows keeps
            // a charge note held (Q10); spin back unless there is time
            let spin = r.below(2) == 0 || next < end + 700 * MS;
            if sc && !s.bss && charge(n) && spin {
                // turntable: spin back to end it
                let back = clear(up.max(press + MS));
                push(back, 1 - k_key, true);
                push(back + 10 * MS, k_key, false);
                push(back + 30 * MS, 1 - k_key, false);
                free = back + 30 * MS;
            } else if hcn && up < end - 400 * MS && next > end + 700 * MS {
                // let the body go, then hold it again
                push(up, k_key, false);
                let again = clear(up + 100 * MS);
                push(again, k_key, true);
                let fin = (end + r.range(-50, 50) * MS + 333)
                    .min(limit)
                    .max(again + MS);
                push(fin, k_key, false);
                free = fin;
            } else {
                push(up, k_key, false);
                free = up;
            }
            // an empty press where there is no note
            if !guarded && roll >= 95 && next - free > 800 * MS {
                let t = clear(free + 150 * MS);
                push(t, k_key, true);
                push(t + 20 * MS, k_key, false);
                free = t + 20 * MS;
            }
        }
    }
    out.sort_by_key(|e| e.time);
    out
}

/// What a drive saw: the events of every update, in order.
struct Trace {
    events: Vec<Vec<EngineEvent>>,
    digest: Digest,
}

/// Drives `e` with `inputs` and 1/60 s frames to `end`, every input at its
/// own time: the engine is first advanced to the input's time, then given
/// the input and updated at that time again. Nothing then depends on how
/// inputs are batched (§9 Q1, Q3, Q8).
fn safe_drive(e: &mut Engine, inputs: &[InputEvent], end: i64, quiet_volume: bool) -> Trace {
    let mut trace = Trace {
        events: Vec::new(),
        digest: Digest::default(),
    };
    let step = |e: &mut Engine, t: i64, tr: &mut Trace| {
        e.update(t);
        let evs: Vec<EngineEvent> = e.drain_events().collect();
        for v in &evs {
            if quiet_volume && matches!(v, EngineEvent::Volume { .. }) {
                continue;
            }
            tr.digest.event(v);
        }
        tr.digest.put("g", &gauge_key(&e.gauge));
        tr.digest
            .put("c", &format!("{} {}", e.combo(), e.is_failed()));
        tr.events.push(evs);
    };
    let mut i = 0;
    let mut f = -SEC;
    loop {
        while let Some(input) = inputs.get(i).filter(|x| x.time <= f) {
            step(e, input.time, &mut trace);
            e.input(*input);
            step(e, input.time, &mut trace);
            i += 1;
        }
        step(e, f, &mut trace);
        if f >= end {
            break;
        }
        f += 16_667;
    }
    trace
}

/// The gauges as hashed: exact bits, except where the specification
/// allows rounding (damage multipliers of §3: 5K EX-HARD, LR2 HARD and
/// EX-HARD), there in thousandths.
fn gauge_key(g: &GrooveGauge) -> String {
    let mut s = format!("{:?}:", g.current_type());
    for x in &g.gauges {
        let lr2 = x.elem.death > 0.0 && matches!(x.ty, GaugeType::Hard | GaugeType::ExHard);
        let five_ex =
            x.ty == GaugeType::ExHard && x.elem.values[POOR] == -20.0 && x.elem.death == 0.0;
        let approx = lr2 || five_ex;
        if approx {
            let _ = write!(s, " ~{}", (x.value() * 1000.0).round() as i64);
        } else {
            let _ = write!(s, " {:08x}", x.value().to_bits());
        }
    }
    s
}

fn bits(v: &[f32]) -> String {
    v.iter()
        .map(|x| format!("{:08x}", x.to_bits()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// FNV-1a, kept in parts so a mismatch says which part differs.
#[derive(Default, Clone)]
struct Digest {
    events: u64,
    count: usize,
    gauge: u64,
}

fn fnv(h: u64, s: &str) -> u64 {
    let mut h = if h == 0 { 0xcbf2_9ce4_8422_2325 } else { h };
    for b in s.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl Digest {
    fn event(&mut self, v: &EngineEvent) {
        self.events = fnv(self.events, &format!("{v:?};"));
        self.count += 1;
    }
    fn put(&mut self, _k: &str, v: &str) {
        self.gauge = fnv(self.gauge, v);
    }
}

/// One long play, summarised for the fixture.
fn run_summary(name: &str, c: &Chart, s: &Setup, seed: u64) -> String {
    let inputs = gen_inputs(c, s, seed);
    let mut e = engine(c, s);
    e.record();
    let hcn = s.ln == LnType::Hcn
        || c.notes.iter().any(|n| {
            matches!(
                n.kind,
                NoteKind::Long {
                    ln_type: Some(LnType::Hcn),
                    ..
                }
            )
        });
    let tr = safe_drive(&mut e, &inputs, c.length() + 2 * SEC, hcn);
    let mut out = String::new();
    let sc = &e.score;
    let _ = writeln!(out, "[{name}]");
    let _ = writeln!(
        out,
        "score: ex {} counts {:?} just {:?} max_combo {} breaks {} total {} passed {}",
        sc.ex_score(),
        sc.counts,
        sc.just,
        sc.max_combo,
        sc.combo_breaks,
        sc.total_notes,
        sc.passed
    );
    let _ = writeln!(out, "events: {} {:016x}", tr.digest.count, tr.digest.events);
    let _ = writeln!(out, "gauge history: {:016x}", tr.digest.gauge);
    let _ = writeln!(out, "gauge: {}", gauge_key(&e.gauge));
    let _ = writeln!(
        out,
        "ghost: {:016x} {}",
        fnv(0, &format!("{:?}", e.ghost)),
        e.ghost.len()
    );
    let _ = writeln!(
        out,
        "judge_log: {:016x}",
        fnv(0, &format!("{:?}", e.judge_log))
    );
    let _ = writeln!(
        out,
        "timing_log: {:016x}",
        fnv(0, &format!("{:?}", e.timing_log))
    );
    let damaged: Vec<usize> = (0..e.notes().len()).filter(|i| e.hcn_damaged(*i)).collect();
    let _ = writeln!(
        out,
        "notes: {:016x} damaged {:016x}",
        fnv(0, &format!("{:?}", note_states(&e))),
        fnv(0, &format!("{damaged:?}"))
    );
    let _ = writeln!(
        out,
        "end: combo {} failed {} finished {} held {:?} ops {}",
        e.combo(),
        e.is_failed(),
        e.is_finished(c.length() + 2 * SEC),
        e.held(),
        e.recorded().len()
    );
    out
}

/// Compare `text` with the fixture `name` (or write it with GOLDEN_BLESS=1).
fn golden_text(name: &str, text: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("GOLDEN_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (GOLDEN_BLESS=1 records it)", path.display()));
    if want != text {
        // the first differing section, for a readable failure
        let w: Vec<&str> = want.split("\n[").collect();
        let g: Vec<&str> = text.split("\n[").collect();
        for (a, b) in w.iter().zip(&g) {
            assert_eq!(b, a, "{name}: first differing entry (got left, want right)");
        }
        assert_eq!(text, want, "{name}");
    }
}

/// The long plays: every mode, LN mode, judge preset, algorithm and gauge,
/// plus AUTO SCRATCH, autoplay, turntable mode, EXPAND JUDGE, ranks,
/// battle sides and the test bot's charts.
#[test]
fn golden_runs() {
    let modes = [
        PlayMode::Beat5K,
        PlayMode::Beat7K,
        PlayMode::Beat10K,
        PlayMode::Beat14K,
        PlayMode::Popn9K,
    ];
    let lns = [LnType::Ln, LnType::Cn, LnType::Hcn];
    let presets = [JudgePreset::Beatoraja, JudgePreset::Lr2, JudgePreset::Iidx];
    let algos = [
        JudgeAlgorithm::Combo,
        JudgeAlgorithm::Duration,
        JudgeAlgorithm::Lowest,
    ];
    let gauges = [GaugeType::Normal, GaugeType::Hard, GaugeType::ExHard];
    let mut text = String::new();
    let mut k = 0usize;
    for mode in modes {
        for ln in lns {
            for preset in presets {
                let gp = match preset {
                    JudgePreset::Beatoraja => GaugePreset::Beatoraja,
                    JudgePreset::Lr2 => GaugePreset::Lr2,
                    JudgePreset::Iidx => GaugePreset::Iidx,
                };
                let s = Setup {
                    preset,
                    gauge_preset: gp,
                    ln,
                    algorithm: algos[k % 3],
                    gauge: gauges[(k / 3) % 3],
                    ..Setup::default()
                };
                let c = gen_chart(mode, k as u64, k % 4 == 1, JudgeRank::Rank(2), 25);
                let name = format!("{mode:?} {ln:?} {preset:?} {:?} {:?}", s.algorithm, s.gauge);
                text += &run_summary(&name, &c, &s, k as u64);
                k += 1;
            }
        }
    }
    // the algorithms on one dense chart
    for algorithm in algos {
        for preset in presets {
            let c = gen_chart(PlayMode::Beat7K, 500, false, JudgeRank::Rank(3), 20);
            let s = Setup {
                algorithm,
                preset,
                ..Setup::default()
            };
            text += &run_summary(&format!("algorithm {algorithm:?} {preset:?}"), &c, &s, 501);
        }
    }
    // ranks and EXPAND JUDGE
    for (i, rank) in [
        JudgeRank::Rank(0),
        JudgeRank::Rank(1),
        JudgeRank::Rank(4),
        JudgeRank::DefExRank(120.0),
        JudgeRank::Bmson(150.0),
    ]
    .into_iter()
    .enumerate()
    {
        for preset in presets {
            let c = gen_chart(PlayMode::Beat7K, 600 + i as u64, false, rank, 15);
            let s = Setup {
                preset,
                ..Setup::default()
            };
            text += &run_summary(&format!("rank {rank:?} {preset:?}"), &c, &s, 7);
        }
    }
    for rates in [[200, 100, 100], [400, 50, 300], [50, 50, 50]] {
        for mode in [PlayMode::Beat7K, PlayMode::Popn9K] {
            let c = gen_chart(mode, 700, false, JudgeRank::Rank(2), 15);
            let s = Setup {
                rates: Some(rates),
                ..Setup::default()
            };
            text += &run_summary(&format!("rates {rates:?} {mode:?}"), &c, &s, 8);
        }
    }
    // AUTO SCRATCH, autoplay, turntable mode
    for mode in [PlayMode::Beat5K, PlayMode::Beat7K, PlayMode::Beat14K] {
        for ln in lns {
            let c = gen_chart(mode, 800, true, JudgeRank::Rank(2), 15);
            let sc: Vec<bool> = (0..mode.lane_count()).map(|l| mode.is_scratch(l)).collect();
            let s = Setup {
                ln,
                auto: sc,
                ..Setup::default()
            };
            text += &run_summary(&format!("auto scratch {mode:?} {ln:?}"), &c, &s, 9);
            let s = Setup {
                ln,
                auto: vec![true; mode.lane_count()],
                ..Setup::default()
            };
            text += &run_summary(&format!("autoplay {mode:?} {ln:?}"), &c, &s, 9);
            let s = Setup {
                ln,
                bss: false,
                ..Setup::default()
            };
            text += &run_summary(&format!("turntable {mode:?} {ln:?}"), &c, &s, 10);
        }
    }
    // gauges with their damage modifiers: 5K EX-HARD at a low TOTAL, LR2
    // HARD on a short chart, and the auto shifts
    for (mode, gp, gauge, gas, total) in [
        (
            PlayMode::Beat5K,
            GaugePreset::Beatoraja,
            GaugeType::ExHard,
            GaugeAutoShift::Continue,
            Some(120.0),
        ),
        (
            PlayMode::Beat10K,
            GaugePreset::Beatoraja,
            GaugeType::ExHard,
            GaugeAutoShift::BestClear,
            None,
        ),
        (
            PlayMode::Beat7K,
            GaugePreset::Lr2,
            GaugeType::Hard,
            GaugeAutoShift::Continue,
            Some(150.0),
        ),
        (
            PlayMode::Beat7K,
            GaugePreset::Lr2,
            GaugeType::ExHard,
            GaugeAutoShift::SelectToUnder,
            None,
        ),
        (
            PlayMode::Beat7K,
            GaugePreset::Beatoraja,
            GaugeType::Hazard,
            GaugeAutoShift::SurvivalToGroove,
            None,
        ),
        (
            PlayMode::Popn9K,
            GaugePreset::Beatoraja,
            GaugeType::ExHardClass,
            GaugeAutoShift::None,
            None,
        ),
        (
            PlayMode::Beat7K,
            GaugePreset::Iidx,
            GaugeType::AssistEasy,
            GaugeAutoShift::None,
            Some(500.0),
        ),
    ] {
        let mut c = gen_chart(mode, 900, false, JudgeRank::Rank(2), 8);
        c.meta.total = total;
        let s = Setup {
            gauge_preset: gp,
            gauge,
            gas,
            ..Setup::default()
        };
        text += &run_summary(
            &format!("gauge {mode:?} {gp:?} {gauge:?} {gas:?} {total:?}"),
            &c,
            &s,
            11,
        );
    }
    // battle sides
    for (mode, ln) in [
        (PlayMode::Beat7K, LnType::Cn),
        (PlayMode::Beat5K, LnType::Hcn),
    ] {
        let sp = gen_chart(mode, 1000, false, JudgeRank::Rank(2), 15);
        let bt = battle_of(&sp);
        for p in [0, 1] {
            let s = Setup {
                ln,
                player: Some(p),
                ..Setup::default()
            };
            text += &run_summary(&format!("battle {mode:?} {ln:?} side {p}"), &bt, &s, 12);
        }
    }
    // the test bot's charts (BMS text: BPM change, stop, #LNTYPE 1 long
    // notes, mines)
    for keys in [5, 7, 10, 14] {
        let src = vibebms_core::bot::chart_text(keys, keys as i64 * 7);
        let c = vibebms_core::bms::parse_bms(src.as_bytes(), "bme", &Default::default()).unwrap();
        for ln in lns {
            let s = Setup {
                ln,
                ..Setup::default()
            };
            text += &run_summary(&format!("bot {keys}K {ln:?}"), &c, &s, 13);
        }
    }
    golden_text("runs.txt", &text);
}

// ---------------------------------------------------------------------------
// §2 judge windows, through `judge_rules`
// ---------------------------------------------------------------------------

fn rules(
    preset: JudgePreset,
    mode: PlayMode,
    rank: JudgeRank,
    rates: Option<[i64; 3]>,
) -> JudgeRules {
    judge_rules(preset, mode.family(), rank, rates)
}

fn ms_pairs(v: &[(i64, i64)]) -> Vec<(f64, f64)> {
    v.iter()
        .map(|(l, e)| (*l as f64 / 1000.0, *e as f64 / 1000.0))
        .collect()
}

fn sym(v: &[f64]) -> Vec<(f64, f64)> {
    v.iter().map(|x| (-x, *x)).collect()
}

/// §2.1: the base tables at 100 % (#RANK EASY), the flags of each family.
#[test]
fn windows_base_tables() {
    let r = rules(
        JudgePreset::Beatoraja,
        PlayMode::Beat5K,
        JudgeRank::Rank(3),
        None,
    );
    let w = &r.windows;
    assert_eq!(
        ms_pairs(&w.note),
        [
            (-20., 20.),
            (-50., 50.),
            (-100., 100.),
            (-150., 150.),
            (-150., 500.)
        ]
    );
    assert_eq!(
        ms_pairs(&w.scratch),
        [
            (-30., 30.),
            (-60., 60.),
            (-110., 110.),
            (-160., 160.),
            (-160., 500.)
        ]
    );
    assert_eq!(ms_pairs(&w.ln_end), sym(&[120., 150., 200., 250.]));
    assert_eq!(ms_pairs(&w.scratch_ln_end), sym(&[130., 160., 160., 260.]));
    assert_eq!((w.ln_margin, w.scratch_ln_margin, w.frame_hz), (0, 0, None));
    assert_eq!(r.combo, [true, true, true, false, false, false]);
    assert_eq!(r.vanish, [true, true, true, true, true, false]);
    assert!(!r.miss_one);
    let r = rules(
        JudgePreset::Beatoraja,
        PlayMode::Beat7K,
        JudgeRank::Rank(3),
        None,
    );
    let w = &r.windows;
    assert_eq!(
        ms_pairs(&w.note),
        [
            (-20., 20.),
            (-60., 60.),
            (-150., 150.),
            (-280., 220.),
            (-150., 500.)
        ]
    );
    assert_eq!(
        ms_pairs(&w.scratch),
        [
            (-30., 30.),
            (-70., 70.),
            (-160., 160.),
            (-290., 230.),
            (-160., 500.)
        ]
    );
    assert_eq!(
        ms_pairs(&w.ln_end),
        [(-120., 120.), (-160., 160.), (-200., 200.), (-280., 220.)]
    );
    assert_eq!(
        ms_pairs(&w.scratch_ln_end),
        [(-130., 130.), (-170., 170.), (-210., 210.), (-290., 230.)]
    );
    assert_eq!(w.extent(), (-290_000, 500_000));
    assert_eq!(r.combo, [true, true, true, false, false, true]);
    assert_eq!(r.vanish, [true, true, true, true, true, false]);
    assert!(!r.miss_one);
    let r = rules(
        JudgePreset::Beatoraja,
        PlayMode::Popn9K,
        JudgeRank::Rank(3),
        None,
    );
    let w = &r.windows;
    assert_eq!(
        ms_pairs(&w.note),
        [
            (-20., 20.),
            (-50., 50.),
            (-117., 117.),
            (-183., 183.),
            (-175., 500.)
        ]
    );
    assert_eq!(w.scratch, w.note);
    assert_eq!(ms_pairs(&w.ln_end), sym(&[120., 150., 217., 283.]));
    assert_eq!(w.scratch_ln_end, w.ln_end);
    assert_eq!((w.ln_margin, w.scratch_ln_margin), (200_000, 0));
    assert_eq!(r.combo, [true, true, true, false, false, false]);
    assert_eq!(r.vanish, [true, true, true, false, true, false]);
    assert!(r.miss_one);
    // the double play modes take their family's
    assert_eq!(
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat14K,
            JudgeRank::Rank(1),
            None
        ),
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat7K,
            JudgeRank::Rank(1),
            None
        )
    );
    assert_eq!(
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat10K,
            JudgeRank::Rank(1),
            None
        ),
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat5K,
            JudgeRank::Rank(1),
            None
        )
    );
    // the flags come with the family whatever the preset
    for preset in [JudgePreset::Lr2, JudgePreset::Iidx] {
        for mode in [PlayMode::Beat5K, PlayMode::Beat7K, PlayMode::Popn9K] {
            let a = rules(preset, mode, JudgeRank::Rank(2), None);
            let b = rules(JudgePreset::Beatoraja, mode, JudgeRank::Rank(2), None);
            assert_eq!(
                (a.combo, a.vanish, a.miss_one),
                (b.combo, b.vanish, b.miss_one)
            );
        }
    }
}

/// §2.4: the seven-key rows.
#[test]
fn windows_seven_key_ranks() {
    // rank, note PG/GR/GD, note BAD, scratch BAD, LN end PG/GR/GD
    type Row = (JudgeRank, [f64; 3], (f64, f64), (f64, f64), [f64; 3]);
    let rows: [Row; 7] = [
        (
            JudgeRank::Rank(0),
            [5., 15., 37.5],
            (-70., 55.),
            (-72.5, 57.5),
            [30., 40., 50.],
        ),
        (
            JudgeRank::Rank(1),
            [10., 30., 75.],
            (-140., 110.),
            (-145., 115.),
            [60., 80., 100.],
        ),
        (
            JudgeRank::Rank(2),
            [15., 45., 112.5],
            (-210., 165.),
            (-217.5, 172.5),
            [90., 120., 150.],
        ),
        (
            JudgeRank::Rank(3),
            [20., 60., 150.],
            (-280., 220.),
            (-290., 230.),
            [120., 160., 200.],
        ),
        (
            JudgeRank::Rank(4),
            [25., 75., 187.5],
            (-350., 275.),
            (-362.5, 287.5),
            [150., 200., 250.],
        ),
        (
            JudgeRank::DefExRank(100.0),
            [15., 45., 112.5],
            (-210., 165.),
            (-217.5, 172.5),
            [90., 120., 150.],
        ),
        (
            JudgeRank::Bmson(100.0),
            [20., 60., 150.],
            (-280., 220.),
            (-290., 230.),
            [120., 160., 200.],
        ),
    ];
    for (rank, pgd, bad, sbad, ln) in rows {
        let w = rules(JudgePreset::Beatoraja, PlayMode::Beat7K, rank, None).windows;
        let n = ms_pairs(&w.note);
        assert_eq!(n[..3], sym(&pgd)[..], "{rank:?}");
        assert_eq!(n[3], bad, "{rank:?}");
        assert_eq!(n[4], (-150., 500.), "{rank:?}");
        assert_eq!(ms_pairs(&w.scratch)[3], sbad, "{rank:?}");
        assert_eq!(ms_pairs(&w.scratch)[4], (-160., 500.), "{rank:?}");
        let l = ms_pairs(&w.ln_end);
        assert_eq!(l[..3], sym(&ln)[..], "{rank:?}");
        assert_eq!(l[3], bad, "{rank:?}");
    }
    for (rank, v) in [
        (JudgeRank::Rank(7), [15., 45., 112.5, 210.]),
        (JudgeRank::Rank(-1), [15., 45., 112.5, 210.]),
        (JudgeRank::Bmson(0.0), [20., 60., 150., 280.]),
        (JudgeRank::DefExRank(120.0), [18., 54., 135., 252.]),
        (JudgeRank::DefExRank(1e300), [2000., 6000., 15000., 28000.]),
        (JudgeRank::DefExRank(0.5), [0.2, 0.6, 1.5, 2.8]),
        (JudgeRank::DefExRank(0.0), [15., 45., 112.5, 210.]),
        (JudgeRank::DefExRank(f64::NAN), [15., 45., 112.5, 210.]),
    ] {
        let w = rules(JudgePreset::Beatoraja, PlayMode::Beat7K, rank, None).windows;
        let late: Vec<f64> = ms_pairs(&w.note)[..4].iter().map(|p| -p.0).collect();
        assert_eq!(late, v, "{rank:?}");
    }
    // VERY EASY: BAD (late) wider than empty POOR: the first match wins
    let w = rules(
        JudgePreset::Beatoraja,
        PlayMode::Beat7K,
        JudgeRank::Rank(4),
        None,
    )
    .windows;
    assert_eq!(w.judge(NoteType::Note, -300_000), BAD);
    assert_eq!(w.judge(NoteType::Note, -351_000), 5);
    assert_eq!(w.judge(NoteType::Note, 400_000), 4);
    assert_eq!(w.judge(NoteType::LongEnd, 400_000), 4);
}

/// §2.4: five-key rows and the scratch LN end GOOD (Q5).
#[test]
fn windows_five_key_ranks() {
    for (rank, note, sc_ln) in [
        (
            JudgeRank::Rank(3),
            [20., 50., 100., 150.],
            [130., 160., 160., 260.],
        ),
        (
            JudgeRank::Rank(2),
            [15., 37.5, 75., 112.5],
            [97.5, 120., 120., 195.],
        ),
        (
            JudgeRank::Rank(0),
            [5., 12.5, 25., 37.5],
            [32.5, 40., 40., 65.],
        ),
    ] {
        let w = rules(JudgePreset::Beatoraja, PlayMode::Beat5K, rank, None).windows;
        assert_eq!(ms_pairs(&w.note)[..4], sym(&note)[..], "{rank:?}");
        assert_eq!(ms_pairs(&w.scratch_ln_end), sym(&sc_ln), "{rank:?}");
    }
}

/// §2.4: PMS rows (PG, BAD and empty POOR fixed; GR/GD between them).
#[test]
fn windows_pms_ranks() {
    for (rank, note, ln) in [
        (
            JudgeRank::Rank(0),
            [20., 20., 38.61, 183.],
            [120., 120., 120., 283.],
        ),
        (
            JudgeRank::Rank(1),
            [20., 25., 58.5, 183.],
            [120., 120., 120., 283.],
        ),
        (
            JudgeRank::Rank(2),
            [20., 35., 81.9, 183.],
            [120., 120., 151.9, 283.],
        ),
        (
            JudgeRank::DefExRank(100.0),
            [20., 35., 81.9, 183.],
            [120., 120., 151.9, 283.],
        ),
        (
            JudgeRank::Rank(3),
            [20., 50., 117., 183.],
            [120., 150., 217., 283.],
        ),
        (
            JudgeRank::Rank(4),
            [20., 66.5, 155.61, 183.],
            [120., 199.5, 283., 283.],
        ),
        (
            JudgeRank::DefExRank(300.0),
            [20., 105., 183., 183.],
            [120., 283., 283., 283.],
        ),
        (
            JudgeRank::DefExRank(120.0),
            [20., 42., 98.28, 183.],
            [120., 126., 182.28, 283.],
        ),
    ] {
        let w = rules(JudgePreset::Beatoraja, PlayMode::Popn9K, rank, None).windows;
        assert_eq!(ms_pairs(&w.note)[..4], sym(&note)[..], "{rank:?}");
        assert_eq!(ms_pairs(&w.note)[4], (-175., 500.), "{rank:?}");
        assert_eq!(ms_pairs(&w.ln_end), sym(&ln), "{rank:?}");
    }
}

/// §2.4: custom rates (EXPAND JUDGE).
#[test]
fn windows_custom_rates() {
    let n = |mode, rank, rates| {
        ms_pairs(
            &rules(JudgePreset::Beatoraja, mode, rank, Some(rates))
                .windows
                .note,
        )
    };
    let w = n(PlayMode::Beat7K, JudgeRank::Rank(3), [200, 100, 100]);
    assert_eq!(w[..2], sym(&[40., 60.])[..]);
    let w = n(PlayMode::Beat7K, JudgeRank::Rank(3), [400, 50, 300]);
    assert_eq!(w[..3], [(-80., 80.), (-80., 80.), (-280., 220.)]);
    // PG x10 = ±200 fits inside BAD; GR and GD are capped at it
    let w = n(PlayMode::Beat7K, JudgeRank::Rank(3), [1000, 1000, 1000]);
    assert_eq!(
        w[..4],
        [(-200., 200.), (-280., 220.), (-280., 220.), (-280., 220.)]
    );
    let w = n(PlayMode::Beat7K, JudgeRank::Rank(2), [50, 50, 50]);
    assert_eq!(w[..3], sym(&[7.5, 22.5, 56.25])[..]);
    let w = n(PlayMode::Popn9K, JudgeRank::Rank(1), [200, 200, 200]);
    assert_eq!(w[..3], sym(&[40., 50., 117.])[..]);
    // BAD and empty POOR never change; 100 % rates are no rates
    let w = n(PlayMode::Beat7K, JudgeRank::Rank(3), [0, 0, 0]);
    assert_eq!(
        w,
        [(0., 0.), (0., 0.), (0., 0.), (-280., 220.), (-150., 500.)]
    );
    assert_eq!(
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat7K,
            JudgeRank::Rank(2),
            Some([100; 3])
        ),
        rules(
            JudgePreset::Beatoraja,
            PlayMode::Beat7K,
            JudgeRank::Rank(2),
            None
        )
    );
    // rates are a beatoraja-preset thing
    for preset in [JudgePreset::Lr2, JudgePreset::Iidx] {
        assert_eq!(
            rules(
                preset,
                PlayMode::Beat7K,
                JudgeRank::Rank(2),
                Some([300, 300, 300])
            ),
            rules(preset, PlayMode::Beat7K, JudgeRank::Rank(2), None)
        );
    }
}

/// §2.5: LR2 preset.
#[test]
fn windows_lr2() {
    let lr2 = |rank| rules(JudgePreset::Lr2, PlayMode::Beat7K, rank, None).windows;
    for (rank, pgd) in [
        (JudgeRank::Rank(0), [8., 24., 40.]),
        (JudgeRank::Rank(1), [15., 30., 60.]),
        (JudgeRank::Rank(2), [18., 40., 100.]),
        (JudgeRank::Rank(3), [21., 60., 120.]),
        (JudgeRank::Rank(4), [18., 40., 100.]),
        (JudgeRank::Rank(-1), [18., 40., 100.]),
        (JudgeRank::DefExRank(100.0), [18., 40., 100.]),
        (JudgeRank::DefExRank(120.0), [19.8, 52., 112.]),
        (JudgeRank::DefExRank(80.0), [16.2, 34., 76.]),
        (JudgeRank::DefExRank(50.0), [11.36, 26.88, 49.6]),
        (JudgeRank::DefExRank(0.5), [18., 40., 100.]),
        (JudgeRank::Bmson(150.0), [31.5, 90., 180.]),
        (JudgeRank::Bmson(200.0), [42., 120., 200.]),
        (JudgeRank::Bmson(0.5), [0.32, 0.96, 1.6]),
        (JudgeRank::Bmson(10_000.0), [200., 200., 200.]),
        (JudgeRank::Bmson(1e9), [200., 200., 200.]),
        (JudgeRank::Bmson(0.0), [21., 60., 120.]),
    ] {
        let w = lr2(rank);
        assert_eq!(ms_pairs(&w.note)[..3], sym(&pgd)[..], "{rank:?}");
        assert_eq!(
            ms_pairs(&w.note)[3..],
            [(-200., 200.), (0., 1000.)],
            "{rank:?}"
        );
        assert_eq!(w.scratch, w.note);
        let gd = (-pgd[2], pgd[2]);
        assert_eq!(ms_pairs(&w.ln_end), [gd, gd, gd, (-200., 200.)], "{rank:?}");
        assert_eq!(w.scratch_ln_end, w.ln_end);
        assert_eq!((w.ln_margin, w.scratch_ln_margin, w.frame_hz), (0, 0, None));
        assert_eq!(w.extent(), (-200_000, 1_000_000));
    }
    // PMS under LR2: no margin either
    let w = rules(JudgePreset::Lr2, PlayMode::Popn9K, JudgeRank::Rank(2), None).windows;
    assert_eq!(w.ln_margin, 0);
}

/// §2.6: IIDX preset, rank-independent, frames of 120 Hz.
#[test]
fn windows_iidx() {
    for rank in [
        JudgeRank::Rank(0),
        JudgeRank::Rank(4),
        JudgeRank::DefExRank(300.0),
    ] {
        for mode in [PlayMode::Beat5K, PlayMode::Beat7K, PlayMode::Popn9K] {
            let w = rules(JudgePreset::Iidx, mode, rank, Some([500; 3])).windows;
            let note = [
                (-16_667, 16_667),
                (-33_333, 33_333),
                (-116_667, 116_667),
                (-250_000, 250_000),
                (-150_000, 500_000),
            ];
            assert_eq!(w.note, note);
            assert_eq!(w.scratch, note);
            let ln = [
                (-116_667, 116_667),
                (-158_333, 158_333),
                (-200_000, 200_000),
                (-250_000, 250_000),
            ];
            assert_eq!(w.ln_end, ln);
            assert_eq!(w.scratch_ln_end, ln);
            assert_eq!(
                (w.ln_margin, w.scratch_ln_margin, w.frame_hz),
                (0, 0, Some(120))
            );
        }
    }
    let w = rules(
        JudgePreset::Iidx,
        PlayMode::Beat7K,
        JudgeRank::Rank(2),
        None,
    )
    .windows;
    for (d, q) in [
        (4_000, 0),
        (-4_200, -8_333),
        (20_000, 16_667),
        (-21_000, -25_000),
        (4_166, 0),
        (4_167, 8_333),
        (-4_167, -8_333),
        (500_000, 500_000),
        (495_834, 500_000),
        (0, 0),
    ] {
        assert_eq!(w.quantize(d), q, "{d}");
    }
    let b = rules(
        JudgePreset::Beatoraja,
        PlayMode::Beat7K,
        JudgeRank::Rank(2),
        None,
    )
    .windows;
    assert_eq!(b.quantize(-4_200), -4_200);
}

/// Every preset × family × rank × rate: the whole window set, against the
/// fixture recorded from the old builder.
#[test]
fn windows_every_combination() {
    let ranks = [
        JudgeRank::Rank(-1),
        JudgeRank::Rank(0),
        JudgeRank::Rank(1),
        JudgeRank::Rank(2),
        JudgeRank::Rank(3),
        JudgeRank::Rank(4),
        JudgeRank::Rank(5),
        JudgeRank::DefExRank(f64::NAN),
        JudgeRank::DefExRank(-5.0),
        JudgeRank::DefExRank(0.0),
        JudgeRank::DefExRank(0.5),
        JudgeRank::DefExRank(1.0),
        JudgeRank::DefExRank(50.0),
        JudgeRank::DefExRank(80.0),
        JudgeRank::DefExRank(99.9),
        JudgeRank::DefExRank(100.0),
        JudgeRank::DefExRank(120.0),
        JudgeRank::DefExRank(133.0),
        JudgeRank::DefExRank(300.0),
        JudgeRank::DefExRank(1e300),
        JudgeRank::Bmson(-1.0),
        JudgeRank::Bmson(0.0),
        JudgeRank::Bmson(0.5),
        JudgeRank::Bmson(33.3),
        JudgeRank::Bmson(100.0),
        JudgeRank::Bmson(150.0),
        JudgeRank::Bmson(200.0),
        JudgeRank::Bmson(10_000.0),
        JudgeRank::Bmson(1e9),
    ];
    let rates: [Option<[i64; 3]>; 5] = [
        None,
        Some([200, 100, 100]),
        Some([400, 50, 300]),
        Some([50, 50, 50]),
        Some([10, 500, 20]),
    ];
    // each combination names its window set; the distinct sets follow
    let mut combos = String::new();
    let mut sets: Vec<String> = Vec::new();
    for preset in JudgePreset::ALL {
        for mode in [PlayMode::Beat5K, PlayMode::Beat7K, PlayMode::Popn9K] {
            for rank in ranks {
                let rr: &[Option<[i64; 3]>] = if preset == JudgePreset::Beatoraja {
                    &rates
                } else {
                    &rates[..1]
                };
                for rate in rr {
                    let set = windows_set(&rules(preset, mode, rank, *rate).windows);
                    let id = sets.iter().position(|s| *s == set).unwrap_or_else(|| {
                        sets.push(set);
                        sets.len() - 1
                    });
                    let _ = writeln!(
                        combos,
                        "{preset:?} {:?} {rank:?} {rate:?} -> w{id}",
                        mode.family()
                    );
                }
            }
        }
    }
    let mut text = combos;
    for (id, set) in sets.iter().enumerate() {
        let _ = write!(text, "\n[w{id}]\n{set}");
    }
    golden_text("windows.txt", &text);
}

/// A window set as text (µs, late..early; ±x when symmetric).
fn windows_set(w: &JudgeWindows) -> String {
    let t = |v: &[(i64, i64)]| {
        v.iter()
            .map(|(l, e)| {
                if *l == -*e {
                    format!("±{e}")
                } else {
                    format!("{l}..{e}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "note {}\nscratch {}\nln_end {}\nscratch_ln_end {}\nmargins {} {} hz {:?}\n",
        t(&w.note),
        t(&w.scratch),
        t(&w.ln_end),
        t(&w.scratch_ln_end),
        w.ln_margin,
        w.scratch_ln_margin,
        w.frame_hz
    )
}
