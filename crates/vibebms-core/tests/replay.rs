//! A replay file judged again without the app: read, the chart and the
//! judge made from its options, its inputs played. The files were written
//! by the app before the play setup moved here.

use std::path::Path;
use vibebms_core::play::{build_chart, build_engine};
use vibebms_core::replay::Replay;
use vibebms_core::score::ClearType;

const CHART: &[u8] = include_bytes!("data/replay/chart.bme");

#[test]
fn a_stored_replay_judges_as_it_did() {
    let r: Replay = Replay::decode(include_bytes!("data/replay/v2.json.gz")).unwrap();
    // format 2 is older than the judge; 3 to 5 changed only the LR2
    // preset, which is this play's judge: 5 its note choice, long notes,
    // mines and whole milliseconds, so it scores otherwise judged again;
    // its ghost is the play as it was
    assert!(r.older_engine());
    assert_eq!((r.ex_score, r.clear), (536, ClearType::Hard));
    let chart = build_chart(Path::new("song/chart.bme"), CHART, &r.play, r.seed, None)
        .unwrap()
        .chart;
    let (mut e, _) = build_engine(&chart, &r.play, None, false);
    for op in &r.ops {
        e.apply(*op);
    }
    assert_eq!(e.score.ex_score(), 532);
    assert_eq!(e.ghost.len(), r.ghost.len());
    assert_eq!(r.ex_at(r.ghost.len()), r.ex_score);
    // written again, it reads the same
    let again: Replay = Replay::decode(&r.encode().unwrap()).unwrap();
    assert_eq!((again.play, again.ops), (r.play, r.ops));
}

#[test]
fn an_older_format_is_read() {
    let r: Replay = Replay::decode(include_bytes!("data/replay/v1.json.gz")).unwrap();
    assert!(r.older_engine());
    assert_eq!(r.ex_score, 536);
    let not_one: Option<Replay> = Replay::decode(b"not a replay");
    assert!(not_one.is_none());
}

/// A replay keeps the note choice it was played with, and judged again
/// with it (as watching it does) gives the play as it was.
#[test]
fn a_replay_keeps_its_note_choice() {
    use vibebms_core::engine::JudgeAlgorithm;
    use vibebms_core::play::RuleOptions;
    let text = vibebms_core::bot::chart_text(7, 3);
    let mut ghosts = Vec::new();
    for algorithm in [JudgeAlgorithm::Combo, JudgeAlgorithm::Score] {
        let rules = RuleOptions {
            judge_algorithm: algorithm,
            ..RuleOptions::default()
        };
        let chart = build_chart(
            Path::new("song/chart.bme"),
            text.as_bytes(),
            &rules,
            1,
            None,
        )
        .unwrap()
        .chart;
        let (mut e, _) = build_engine(&chart, &rules, None, false);
        e.record();
        let inputs = vibebms_core::bot::inputs(&chart, None, 5);
        vibebms_core::bot::drive(&mut e, &inputs, chart.length() + 2_000_000);
        let r = Replay {
            version: vibebms_core::replay::VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed: 1,
            play: rules,
            ex_score: e.score.ex_score(),
            clear: ClearType::Failed,
            ghost: e.ghost.clone(),
            ops: e.recorded().to_vec(),
            course: None,
        };
        let r: Replay = Replay::decode(&r.encode().unwrap()).unwrap();
        assert_eq!(r.play.judge_algorithm, algorithm);
        let (mut again, _) = build_engine(&chart, &r.play, None, false);
        for op in &r.ops {
            again.apply(*op);
        }
        assert_eq!(again.ghost, r.ghost, "{algorithm:?}");
        assert_eq!(again.score.ex_score(), r.ex_score);
        ghosts.push(r.ghost);
    }
    // (the chart and presses are ones where the choice matters)
    assert_ne!(ghosts[0], ghosts[1]);
}
