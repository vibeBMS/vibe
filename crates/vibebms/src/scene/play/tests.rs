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
        course: None,
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
                assert!(
                    e.ghost.iter().all(|j| *j == Judge::PGreat),
                    "{what}: {:?}",
                    e.ghost
                );
                assert!(e.is_finished(end) && !e.is_failed(), "{what}");
            }
        }
    }
}

/// Replays of plays with these options judge the same as before the
/// play scene took several judges (the digests were taken then; those with
/// LNs again when an LN judged after an early release took its larger
/// error for FAST / SLOW, the judgements the same).
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
        (0xa76a1763fdf66101, 474),
        (0xbf6ca1516b2dc3a6, 336),
        (0x4123cac824bd1084, 932),
        (0xaf54e52ddda836f6, 654),
        (0x489f7e223e851801, 480),
        (0x2cab6841aa33d654, 470),
        (0xe6485c1bb000e555, 471),
        (0x1f81fa3451400fb3, 493),
        (0xd42210a62c598fbb, 558),
        (0x32364c7127b745df, 558),
        // the LR2 judge as docs/LR2_BEHAVIOUR.md has it (replay format 5)
        (0x1bdbeee50682a22e, 434),
        // the IIDX preset as the arcade (replay format 7)
        (0x3b74b92abb25d2f7, 434),
        (0x942b757e8f497dae, 550),
        (0x47dc09ae4c89eab1, 468),
        (0x4a1e755cb4dd1dbc, 950),
        (0x5fd2ceb15419ce21, 473),
        (0x39e58bd8cc14d9ce, 945),
        (0x80c2d3c11291ba9f, 780),
        (0x7faa492050c6e7d1, 947),
    ];
    if want.len() != got.len() {
        panic!("digests: {got:x?}");
    }
    assert_eq!(got, want);
}

/// A KEY CONVERT play's replay and a course stage's keep what made their
/// chart and gauges: watched, they judge as they did.
#[test]
fn converted_and_course_replays_judge_as_played() {
    use vibebms_core::convert::KeyConvert;
    use vibebms_core::course::CourseKind;
    let convert = |key_convert| RuleOptions {
        key_convert,
        ..RuleOptions::default()
    };
    let lr2 = RuleOptions {
        gauge_preset: GaugePreset::Lr2,
        judge_preset: JudgePreset::Lr2,
        ..RuleOptions::default()
    };
    for (keys, play) in [
        (7, convert(KeyConvert::To9)),
        (5, convert(KeyConvert::To7)),
        (7, lr2.for_course(CourseKind::Expert)),
        (7, lr2.for_course(CourseKind::Class)),
    ] {
        play_and_watch(keys, &play);
    }
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
            course: None,
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
        course: None,
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
        course: None,
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
    assert!(bad(|r| r.play.ln_force = true));
    // a KEY CONVERT play to 9 keys; to 7 a 7 key chart is as it is
    use vibebms_core::convert::KeyConvert;
    assert!(bad(|r| r.play.key_convert = KeyConvert::To9));
    assert!(!bad(|r| r.play.key_convert = KeyConvert::To7));
    // a chart typed CN plays the same in AUTO and FORCE CN, not FORCE LN
    let typed = crate::library::Song {
        ln_profile: Some(
            vibebms_core::LnProfile {
                undefined: false,
                typed: [false, true, false],
            }
            .bits(),
        ),
        ..song.clone()
    };
    let force = vibebms_core::LnPref::forced;
    assert_eq!(ghost_refusal(&r, &typed, force(LnType::Cn)), None);
    assert!(ghost_refusal(&r, &typed, force(LnType::Ln)).is_some());
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

/// A course's rules: LR2's class settings under the LR2 set, a table
/// course's constraints, NONSTOP's view (S-OLR2b 3.3, 5.1).
#[test]
fn course_rules_and_view() {
    use vibebms_core::course::{CourseDef, CourseKind};
    let run = |kind, constraints: &[&str]| {
        let def = CourseDef {
            kind,
            ..Default::default()
        };
        let c = constraints.iter().map(|s| s.to_string()).collect();
        CourseRun::new("C".into(), def, Vec::new(), c)
    };
    let mut p = crate::config::PlayConfig::default();
    p.rules.gauge = GaugeType::Easy;
    p.rules.random_1p = LaneOption::Random;
    p.sudden = 0.3;
    // a table course with gauge_lr2: LR2's class settings
    let class = run(CourseKind::Class, &["gauge_lr2", "cn"]);
    let r = class.rules(&p.rules);
    assert_eq!(
        (r.gauge_preset, r.gauge),
        (GaugePreset::Lr2, GaugeType::Class)
    );
    assert_eq!(
        (r.random_1p, r.ln_mode),
        (LaneOption::Off, vibebms_core::LnType::Cn)
    );
    let mut view = std::borrow::Cow::Borrowed(&p);
    class.view(&mut view);
    assert_eq!(view.sudden, 0.0);
    // beatoraja's set: its dan gauge, the lane option kept (MIRROR at most
    // when it says so)
    let table = run(CourseKind::Class, &["grade_mirror"]);
    let r = table.rules(&p.rules);
    assert_eq!(
        (r.gauge, r.random_1p),
        (GaugeType::Class, LaneOption::Random)
    );
    assert_eq!(table.lanes(), Some(true));
    // NONSTOP: HS-FIX off, any lane option
    p.hs_fix = crate::config::HsFix::Main;
    let ns = run(CourseKind::Nonstop, &[]);
    let mut view = std::borrow::Cow::Borrowed(&p);
    ns.view(&mut view);
    assert_eq!((view.hs_fix, view.sudden), (crate::config::HsFix::Off, 0.3));
    assert_eq!(ns.lanes(), None);
}

/// A practice play keeps no replay (and goes to no result screen, so no
/// score, lamp or IR entry); a play of the player's own does.
#[test]
fn practice_plays_keep_no_replay() {
    let mut req = PlayRequest {
        song: crate::library::Song::default(),
        autoplay: false,
        course: None,
        replay: None,
        vs: None,
        practice: None,
    };
    assert!(super::setup::keeps_replay(&req));
    req.practice = Some(PracticeRun {
        practice: vibebms_core::practice::Practice {
            from: 0,
            to: 3,
            lead_in: 1,
            speed: 100,
            rank: None,
            total: None,
        },
        gauge: GaugeType::Hard,
        gauge_start: None,
        repeat: false,
        seed: 1,
        result: Default::default(),
        viewer: false,
    });
    assert!(!super::setup::keeps_replay(&req));
}
