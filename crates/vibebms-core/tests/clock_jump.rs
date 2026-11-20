//! A judge whose clock is stepped from far back (a replay's ops are its
//! clock: `Op::Update` takes any time) still comes back: a hell charge
//! note's body ticks for no longer than the longest chart lasts.

use vibebms_core::bms::{parse_bms, ParseOptions};
use vibebms_core::engine::Op;
use vibebms_core::play::{build_engine, RuleOptions};
use vibebms_core::LnType;

#[test]
fn a_step_from_far_back_into_a_hell_charge_note_ends() {
    // 120 BPM: a long note from 2 s to 3 s, played as a hell charge note
    let chart = parse_bms(
        b"#BPM 120\n#WAV01 a.wav\n#00151:0101\n",
        "bms",
        &ParseOptions::default(),
    )
    .unwrap();
    let rules = RuleOptions {
        ln_mode: LnType::Hcn,
        ..Default::default()
    };
    for held in [false, true] {
        let (mut e, _) = build_engine(&chart, &rules, None, held);
        e.apply(Op::Update(i64::MIN));
        e.apply(Op::Update(2_500_000));
        e.apply(Op::Update(4_000_000));
        assert!(e.gauge.value().is_finite());
        assert_eq!(e.notes().len(), 1);
    }
}
