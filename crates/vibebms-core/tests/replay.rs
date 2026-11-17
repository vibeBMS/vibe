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
    assert!(!r.older_engine());
    assert_eq!((r.ex_score, r.clear), (536, ClearType::Hard));
    let chart = build_chart(Path::new("song/chart.bme"), CHART, &r.play, r.seed, None)
        .unwrap()
        .chart;
    let (mut e, _) = build_engine(&chart, &r.play, None, false);
    for op in &r.ops {
        e.apply(*op);
    }
    assert_eq!(e.score.ex_score(), r.ex_score);
    assert_eq!(e.ghost, r.ghost);
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
