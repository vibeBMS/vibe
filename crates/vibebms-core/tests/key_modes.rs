//! Qwilight-style 4 / 6 / 8 key charts played through the engine: what the
//! chart reads as, and that every note is judged.

use std::path::Path;
use vibebms_core::bot;
use vibebms_core::play::{build_chart, build_engine, RuleOptions};
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
