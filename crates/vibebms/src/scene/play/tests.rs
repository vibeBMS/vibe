use super::control::{control, Control};
use super::finish::settled_log;
use super::session::Feed;
use super::sound::voice;
use super::*;
use crate::config::{Bpms, PlayConfig};
use crate::touch;
use std::path::Path;
use vibebms_core::bot;
use vibebms_core::engine::{Engine, Op};
use vibebms_core::gauge::{GaugePreset, GaugeType};
use vibebms_core::judge::JudgePreset;
use vibebms_core::lane_option::{DoubleOption, LaneOption};
use vibebms_core::play::{build_battle_chart, build_chart, build_engine, PlayChart, RuleOptions};

/// FNV-1a over what a play leaves: judgements, logs, score, gauge.
fn digest(e: &Engine) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut put = |v: i64| {
        for b in v.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    };
    for g in &e.ghost {
        put(*g as i64);
    }
    for (t, j, s) in &e.judge_log {
        put(*t);
        put(*j as i64 * 3 + *s as i64);
    }
    for (t, d) in &e.timing_log {
        put(*t);
        put(*d);
    }
    let sc = &e.score;
    for j in 0..6 {
        put(sc.counts[j][0] as i64);
        put(sc.counts[j][1] as i64);
        put(sc.just[j] as i64);
    }
    put(sc.max_combo as i64);
    put(sc.combo_breaks as i64);
    put(sc.total_notes as i64);
    put(e.gauge.value().to_bits() as i64);
    put(e.is_failed() as i64);
    h
}

/// Plays made up chart `keys` with `play` and the bot, then watches
/// the replay as the play scene does (the replay's options and seed
/// make the chart and the rules): both must judge the same. The
/// digest of the play.
fn play_and_watch(keys: usize, play: &RuleOptions) -> (u64, u32) {
    let text = bot::chart_text(keys, keys as i64 * 7);
    let path = Path::new("song/chart.bme");
    let seed = 4242;
    let chart = build_chart(path, text.as_bytes(), play, seed, None)
        .unwrap()
        .chart;
    let (mut e, _) = build_engine(&chart, play, None, false);
    e.record();
    let inputs = bot::inputs(&chart, None, 99);
    bot::drive(&mut e, &inputs, chart.length() + 2_000_000);
    // the replay as saved and loaded
    let r = Replay {
        version: crate::replay::VERSION,
        sha256: String::new(),
        md5: String::new(),
        title: String::new(),
        ln: 0,
        date: 0,
        seed,
        play: PlayConfig {
            rules: play.clone(),
            ..PlayConfig::default()
        },
        ex_score: e.score.ex_score(),
        clear: ClearType::default(),
        ghost: e.ghost.clone(),
        ops: e.recorded().to_vec(),
    };
    let r: Replay = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
    let chart = build_chart(path, text.as_bytes(), &r.play, r.seed, None)
        .unwrap()
        .chart;
    let (mut w, _) = build_engine(&chart, &r.play, None, false);
    // fed frame by frame as the scene does
    let mut feed = Feed::new(Arc::new(r.clone()), 0);
    let mut t = -1_500_000;
    while feed.at < r.ops.len() {
        feed.run(&mut w, t, |_, _| {});
        t += 16_667;
    }
    assert_eq!(w.ghost, r.ghost);
    assert_eq!(digest(&w), digest(&e));
    (digest(&e), e.score.ex_score())
}

/// Autoplay through the scene's chart and judge making, frame by
/// frame: every key mode and LN mode, with the lane options, battle
/// and HCN bodies; every note PGREAT, the play finished.
#[test]
fn autoplay_plays_every_chart_through() {
    for keys in [5, 7, 10, 14] {
        for (ln, random) in [
            (LnType::Ln, LaneOption::Off),
            (LnType::Cn, LaneOption::SRandom),
            (LnType::Hcn, LaneOption::HRandom),
        ] {
            let play = RuleOptions {
                ln_mode: ln,
                random_1p: random,
                battle: keys < 10 && ln == LnType::Cn,
                ..RuleOptions::default()
            };
            let text = bot::chart_text(keys, keys as i64 * 3);
            let path = Path::new("song/chart.bme");
            let chart = build_chart(path, text.as_bytes(), &play, 9, None)
                .unwrap()
                .chart;
            let players = if chart.mode().is_double() && play.battle {
                vec![Some(0), Some(1)]
            } else {
                vec![None]
            };
            for player in players {
                let (mut e, _) = build_engine(&chart, &play, player, true);
                let end = chart.length() + 2_000_000;
                let mut t = -1_000_000;
                while t < end {
                    e.update(t);
                    e.drain_events().count();
                    t += 16_667;
                }
                let what = format!("{keys}K {ln:?} {random:?} {player:?}");
                assert!(e.score.total_notes > 0, "{what}");
                assert!(e.ghost.iter().all(|j| *j == 0), "{what}: {:?}", e.ghost);
                assert!(e.is_finished(end) && !e.is_failed(), "{what}");
            }
        }
    }
}

/// Replays of plays with these options judge the same as before the
/// play scene took several judges (the digests were taken then).
#[test]
fn replays_judge_as_before() {
    let d = RuleOptions::default;
    let cases: Vec<(usize, RuleOptions)> = vec![
        (7, d()),
        (5, d()),
        (14, d()),
        (10, d()),
        (
            7,
            RuleOptions {
                random_1p: LaneOption::Random,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                random_1p: LaneOption::SRandom,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                random_1p: LaneOption::HRandom,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                random_1p: LaneOption::AllScratch,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                ln_mode: LnType::Cn,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                ln_mode: LnType::Hcn,
                gauge: GaugeType::Hard,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                judge_preset: JudgePreset::Lr2,
                gauge_preset: GaugePreset::Lr2,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                judge_preset: JudgePreset::Iidx,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                auto_scratch: true,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                legacy_note: true,
                no_mine: true,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                expand_judge: true,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                judge_algorithm: vibebms_core::engine::JudgeAlgorithm::Duration,
                gauge: GaugeType::ExHard,
                ..d()
            },
        ),
        (
            7,
            RuleOptions {
                battle: true,
                ..d()
            },
        ),
        (
            5,
            RuleOptions {
                battle: true,
                auto_scratch: true,
                random_2p: LaneOption::Mirror,
                ..d()
            },
        ),
        (
            14,
            RuleOptions {
                random_1p: LaneOption::Random,
                random_2p: LaneOption::RRandom,
                dp_option: DoubleOption::Flip,
                ..d()
            },
        ),
    ];
    let got: Vec<(u64, u32)> = cases.iter().map(|(k, p)| play_and_watch(*k, p)).collect();
    let want = [
        (0x6e7befe494f86dc7, 474),
        (0xbf6ca1516b2dc3a6, 336),
        (0x758a4f4820ee6ab0, 932),
        (0x941e7fce3f15bed2, 654),
        (0x5fc6086c2dda718e, 480),
        (0x40624123b4d9a015, 470),
        (0xce76d9ba6ff28c18, 471),
        (0x8cd4c2f2d3cbd667, 493),
        (0xd42210a62c598fbb, 558),
        (0x32364c7127b745df, 558),
        (0x7ba9f6a676c617c1, 441),
        (0xbcd1d1647dd9fff4, 434),
        (0xffe1584f208e5414, 550),
        (0x47dc09ae4c89eab1, 468),
        (0xe4283ac5977abf9e, 950),
        (0xbf481f7d42daafcf, 473),
        (0x4135924bdf310df1, 945),
        (0xe349c363d5d76f7b, 780),
        (0x6a8d0564b558f304, 947),
    ];
    if want.len() != got.len() {
        panic!("digests: {got:x?}");
    }
    assert_eq!(got, want);
}

/// G-BATTLE: a replay made on the single play chart plays again as the
/// opponent on the battle chart's second side, judging the same; the
/// player's side judges as the single play chart with the opponent's
/// lane option (what the player's replay keeps).
#[test]
fn a_replay_plays_again_as_the_opponent() {
    let d = RuleOptions::default;
    let cases: Vec<(usize, RuleOptions, RuleOptions)> = vec![
        (7, d(), d()),
        (
            7,
            RuleOptions {
                random_1p: LaneOption::SRandom,
                ln_mode: LnType::Cn,
                auto_scratch: true,
                ..d()
            },
            RuleOptions {
                no_mine: true,
                gauge: GaugeType::Hard,
                ..d()
            },
        ),
        (
            5,
            RuleOptions {
                random_1p: LaneOption::HRandom,
                ln_mode: LnType::Hcn,
                no_mine: true,
                judge_preset: JudgePreset::Lr2,
                ..d()
            },
            RuleOptions {
                random_1p: LaneOption::Mirror,
                legacy_note: true,
                judge_preset: JudgePreset::Iidx,
                ..d()
            },
        ),
    ];
    for (keys, theirs, mine) in cases {
        let text = bot::chart_text(keys, 11);
        let path = Path::new("song/chart.bme");
        let seed = 777;
        // the opponent's play
        let sp = build_chart(path, text.as_bytes(), &theirs, seed, None)
            .unwrap()
            .chart;
        let (mut e, _) = build_engine(&sp, &theirs, None, false);
        e.record();
        bot::drive(&mut e, &bot::inputs(&sp, None, 5), sp.length() + 2_000_000);
        let r = Arc::new(Replay {
            version: crate::replay::VERSION,
            sha256: String::new(),
            md5: String::new(),
            title: String::new(),
            ln: 0,
            date: 0,
            seed,
            play: PlayConfig {
                rules: theirs.clone(),
                ..PlayConfig::default()
            },
            ex_score: e.score.ex_score(),
            clear: ClearType::default(),
            ghost: e.ghost.clone(),
            ops: e.recorded().to_vec(),
        });
        // the battle
        let PlayChart {
            chart: bt, facts, ..
        } = build_battle_chart(path, text.as_bytes(), &mine, &theirs, seed).unwrap();
        assert!(!facts.battle);
        assert_eq!(facts.lane, vec![theirs.random_1p]);
        let lpp = bt.mode().lanes_per_player() as u8;
        let (mut g, _) = build_engine(&bt, &theirs, Some(1), false);
        let mut feed = Feed::new(r.clone(), lpp);
        let mut t = -1_500_000;
        let mut lit = 0;
        while feed.at < r.ops.len() {
            feed.run(&mut g, t, |lane, _| {
                assert!(lane >= lpp);
                lit += 1;
            });
            t += 16_667;
        }
        assert!(lit > 100);
        assert_eq!(g.ghost, r.ghost);
        assert_eq!(digest(&g), digest(&e), "{keys}K opponent");
        // the player's side and the player's replay
        let (mut me, _) = build_engine(&bt, &mine, Some(0), false);
        let mut played = mine.clone();
        played.random_1p = theirs.random_1p;
        let solo = build_chart(path, text.as_bytes(), &played, seed, None)
            .unwrap()
            .chart;
        let (mut alone, _) = build_engine(&solo, &played, None, false);
        let inputs = bot::inputs(&solo, None, 8);
        bot::drive(&mut me, &inputs, solo.length() + 2_000_000);
        bot::drive(&mut alone, &inputs, solo.length() + 2_000_000);
        assert_eq!(digest(&me), digest(&alone), "{keys}K player");
    }
}

/// For trying G-BATTLE by hand: the test bot plays a chart and its
/// replay becomes the chart's best and latest.
/// `VIBEBMS_BOT_CHART=<chart> VIBEBMS_BOT_ROOT=<data folder> cargo test
/// -p vibebms bot_replay -- --ignored`
#[test]
#[ignore]
fn bot_replay() {
    let chart = std::env::var("VIBEBMS_BOT_CHART").unwrap();
    let root = std::path::PathBuf::from(std::env::var("VIBEBMS_BOT_ROOT").unwrap());
    let path = Path::new(&chart);
    let bytes = std::fs::read(path).unwrap();
    let c = vibebms_core::load_chart_bytes(path, &bytes, &Default::default()).unwrap();
    let song = crate::library::song_from_chart(path, String::new(), &c);
    let play = PlayConfig {
        rules: RuleOptions {
            random_1p: LaneOption::Random,
            ..RuleOptions::default()
        },
        ..PlayConfig::default()
    };
    let seed = 31337;
    let PlayChart {
        chart: c,
        ln_profile: prof,
        ..
    } = build_chart(path, &bytes, &play, seed, None).unwrap();
    let (mut e, _) = build_engine(&c, &play, None, false);
    e.record();
    bot::drive(&mut e, &bot::inputs(&c, None, 3), c.length() + 2_000_000);
    let r = Replay {
        version: crate::replay::VERSION,
        sha256: song.sha256.clone(),
        md5: song.md5.clone(),
        title: song.title.clone(),
        ln: prof.score_ln(play.ln_mode) as i32,
        date: 0,
        seed,
        play,
        ex_score: e.score.ex_score(),
        clear: ClearType::Normal,
        ghost: e.ghost.clone(),
        ops: e.recorded().to_vec(),
    };
    crate::replay::save(&r, &root, crate::replay::Slot::Best).unwrap();
    crate::replay::save(&r, &root, crate::replay::Slot::Last).unwrap();
    println!("EX {} of {}", r.ex_score, e.score.max_ex());
}

#[test]
fn vs_players_have_their_own_voices_and_controls() {
    // a keysound and an HCN mute of one side never reach the other's
    assert_ne!(voice(3, 0, true), voice(3, 1, true));
    assert_eq!(voice(3, 0, true), voice(3, 0, false));
    assert_eq!(voice(3, 1, false), voice(3, 0, false));
    // each player's START + keys change their own hi-speed
    let bpm = Bpms::flat(150.0);
    let now = Instant::now();
    let mut p = PlayConfig::default();
    p.p2.hispeed = 3.0;
    let mut c = [Control::default(), Control::default()];
    let ev = |lane, pressed| LaneEv {
        lane,
        sub: 0,
        pressed,
        time: now,
    };
    let mode = PlayMode::Beat14K;
    let mut two = p.for_side(1);
    assert!(control(
        &mut c[1],
        mode,
        &mut two,
        &bpm,
        &ev(touch::START2, true)
    ));
    // a blue key of 2P: +0.25
    assert!(control(&mut c[1], mode, &mut two, &bpm, &ev(9, true)));
    p.set_side(1, &two);
    assert_eq!((p.hispeed, p.p2.hispeed), (2.5, 3.25));
    // 1P's panel is not open: its key goes to the judge
    let mut one = p.for_side(0);
    assert!(!control(&mut c[0], mode, &mut one, &bpm, &ev(1, true)));
}

#[test]
fn only_fitting_replays_are_opponents() {
    use crate::replay::ghost_refusal;
    let song = crate::library::Song {
        mode: PlayMode::Beat7K,
        has_ln: true,
        ..Default::default()
    };
    let r = Replay {
        version: crate::replay::VERSION,
        sha256: String::new(),
        md5: String::new(),
        title: String::new(),
        ln: 0,
        date: 0,
        seed: 1,
        play: PlayConfig::default(),
        ex_score: 0,
        clear: ClearType::default(),
        ghost: Vec::new(),
        ops: vec![Op::Update(0)],
    };
    assert_eq!(ghost_refusal(&r, &song, LnType::Ln), None);
    assert!(ghost_refusal(&r, &song, LnType::Cn).is_some());
    let bad = |f: fn(&mut Replay)| {
        let mut x = r.clone();
        f(&mut x);
        ghost_refusal(&x, &song, LnType::Ln).is_some()
    };
    assert!(bad(|r| r.play.battle = true));
    assert!(bad(|r| r.play.legacy_note = true));
    assert!(bad(|r| r.ops.clear()));
    assert!(bad(|r| r.play.ln_mode = LnType::Hcn));
    let dp = crate::library::Song {
        mode: PlayMode::Beat14K,
        ..song.clone()
    };
    assert!(ghost_refusal(&r, &dp, LnType::Ln).is_some());
}

#[test]
fn gauge_graph_ends_with_the_final_gauge() {
    // failed between two samples: the graph reaches 0
    assert_eq!(settled_log(&[100.0, 40.0], 0.0), vec![100.0, 40.0, 0.0]);
    assert_eq!(settled_log(&[20.0, 82.0], 82.0), vec![20.0, 82.0]);
    assert_eq!(settled_log(&[], 20.0), vec![20.0]);
}
