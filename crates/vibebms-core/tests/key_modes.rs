//! Qwilight-style 4 / 6 / 8 key charts and KEY CONVERT, played through the
//! engine: what the chart reads as, that every note is judged, and that a
//! replay of a converted play makes the same chart again.

use std::path::Path;
use vibebms_core::bot;
use vibebms_core::convert::KeyConvert;
use vibebms_core::play::{build_chart, build_engine, RuleOptions};
use vibebms_core::replay::Replay;
use vibebms_core::score::ClearType;
use vibebms_core::PlayMode;

/// A made-up 7 key chart (scratch, long notes, mines) with `header` first.
fn text(header: &str, keys: usize, seed: i64) -> String {
    format!("{header}\n{}", bot::chart_text(keys, seed))
}

/// The chart `text` as `rules` make it.
fn chart(text: &str, ext: &str, rules: &RuleOptions) -> vibebms_core::play::PlayChart {
    build_chart(
        Path::new(&format!("song/chart.{ext}")),
        text.as_bytes(),
        rules,
        1,
        None,
    )
    .unwrap()
}

#[test]
fn four_six_eight_keys_play_through() {
    for (header, mode) in [
        ("#4K", PlayMode::Key4),
        ("#6K", PlayMode::Key6),
        ("#8K", PlayMode::Key8),
    ] {
        let rules = RuleOptions::default();
        let c = chart(&text(header, 7, 4), "bms", &rules).chart;
        assert_eq!(c.mode(), mode, "{header}");
        assert!(c
            .notes
            .iter()
            .all(|n| (n.lane as usize) < mode.lane_count()));
        // every lane has notes (the made-up chart fills each channel)
        for lane in 0..mode.lane_count() {
            assert!(
                c.notes.iter().any(|n| n.lane as usize == lane),
                "{header} {lane}"
            );
        }
        // autoplay: every note PGREAT, a full gauge's clear
        let (mut auto, _) = build_engine(&c, &rules, None, true);
        bot::drive(&mut auto, &[], c.length() + 2_000_000);
        let s = &auto.score;
        assert!(s.total_notes > 100, "{header}");
        assert_eq!(s.passed, s.total_notes, "{header}");
        assert_eq!(s.ex_score(), s.max_ex(), "{header}");
        // a player with some timing error and misses: every note judged
        let (mut e, _) = build_engine(&c, &rules, None, false);
        let inputs = bot::inputs(&c, None, 9);
        bot::drive(&mut e, &inputs, c.length() + 2_000_000);
        assert_eq!(e.score.passed, e.score.total_notes, "{header}");
        assert!(e.score.ex_score() > 0 && e.score.ex_score() < e.score.max_ex());
    }
}

#[test]
fn key_convert_makes_the_chart_of_its_table() {
    let to = |k| RuleOptions {
        key_convert: k,
        ..RuleOptions::default()
    };
    let seven = text("", 7, 2);
    let plain = chart(&seven, "bme", &RuleOptions::default());
    // 7K to 9K: no lanes share, every note stays
    let nine = chart(&seven, "bme", &to(KeyConvert::To9));
    assert_eq!(nine.chart.mode(), PlayMode::Popn9K);
    assert_eq!(nine.facts.key_convert, Some(PlayMode::Beat7K));
    assert_eq!(nine.chart.notes.len(), plain.chart.notes.len());
    let lanes = |c: &vibebms_core::chart::Chart, lane: u8| {
        c.notes.iter().filter(|n| n.lane == lane).count()
    };
    // keys 1-7 on buttons 2-8, the scratch on button 1
    for (from, to) in [1, 2, 3, 4, 5, 6, 7, 0].into_iter().enumerate() {
        assert_eq!(lanes(&nine.chart, to), lanes(&plain.chart, from as u8));
    }
    assert_eq!(lanes(&nine.chart, 8), 0);
    // 5K to 7K: keys 1-5 on keys 2-6
    let five = text("", 5, 3);
    let f = chart(&five, "bms", &RuleOptions::default()).chart;
    assert_eq!(f.mode(), PlayMode::Beat5K);
    let fs = chart(&five, "bms", &to(KeyConvert::To7));
    assert_eq!(fs.chart.mode(), PlayMode::Beat7K);
    assert_eq!(fs.facts.key_convert, Some(PlayMode::Beat5K));
    assert_eq!(lanes(&fs.chart, 0), 0);
    assert_eq!(lanes(&fs.chart, 6), 0);
    assert_eq!(lanes(&fs.chart, 1), lanes(&f, 0));
    assert_eq!(lanes(&fs.chart, 7), lanes(&f, 5));
    // 9K to 7K: buttons 1 and 9 share the scratch
    let pms = chart(&seven, "pms", &RuleOptions::default()).chart;
    assert_eq!(pms.mode(), PlayMode::Popn9K);
    let back = chart(&seven, "pms", &to(KeyConvert::To7));
    assert_eq!(back.chart.mode(), PlayMode::Beat7K);
    assert_eq!(back.facts.key_convert, Some(PlayMode::Popn9K));
    assert!(lanes(&back.chart, 7) <= lanes(&pms, 0) + lanes(&pms, 8));
    assert_eq!(lanes(&back.chart, 0), lanes(&pms, 1));
    // a mode without a table plays as it is, and is no conversion
    let fourteen = chart(&text("", 14, 1), "bme", &to(KeyConvert::To7));
    assert_eq!(fourteen.chart.mode(), PlayMode::Beat14K);
    assert_eq!(fourteen.facts.key_convert, None);
    // OFF (and RuleOptions written before KEY CONVERT) converts nothing
    let old: RuleOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(old.key_convert, KeyConvert::Off);
    assert!(!serde_json::to_string(&old).unwrap().contains("key_convert"));
}

/// A replay keeps the conversion: judged again from its options it makes
/// the converted chart, and its inputs score as they did.
#[test]
fn a_replay_keeps_the_conversion() {
    let rules = RuleOptions {
        key_convert: KeyConvert::To9,
        ..RuleOptions::default()
    };
    let src = text("", 7, 6);
    let c = chart(&src, "bme", &rules).chart;
    let (mut e, _) = build_engine(&c, &rules, None, false);
    e.record();
    let inputs = bot::inputs(&c, None, 3);
    bot::drive(&mut e, &inputs, c.length() + 2_000_000);
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
    assert_eq!(r.play.key_convert, KeyConvert::To9);
    let again = chart(&src, "bme", &r.play).chart;
    assert_eq!(again.mode(), PlayMode::Popn9K);
    let (mut j, _) = build_engine(&again, &r.play, None, false);
    for op in &r.ops {
        j.apply(*op);
    }
    assert_eq!(j.ghost, r.ghost);
    assert_eq!(j.score.ex_score(), r.ex_score);
}
