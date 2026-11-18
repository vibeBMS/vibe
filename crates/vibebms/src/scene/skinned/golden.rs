//! The skin values (option, number, button, text, rate) the skinned scenes
//! give for one fixed chart, record and play, against
//! `tests/golden/skin_values.txt`: rewriting the value tables must not
//! change them. `GOLDEN_BLESS=1` writes the file anew.

use crate::app::Ctx;
use crate::library::{BestScore, Song};
use crate::platform::{Duration, Instant};
use crate::scene::decide::{DecideScene, DecideState};
use crate::scene::play::{ghost_ex, PlayRequest, PlayScene, PlayState, Role, Session, SideState};
use crate::scene::result::{ResultData, ResultScene, SideResult};
use crate::scene::result_skin::ResultState;
use crate::scene::select::{Bar, Node, SelectScene, SelectState};
use crate::skin::SkinState;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibebms_core::bot;
use vibebms_core::engine::Engine;
use vibebms_core::gauge::GaugeType;
use vibebms_core::play::{build_chart, build_engine, RuleOptions};
use vibebms_core::score::{Assist, ClearType};
use vibebms_core::LnType;

fn song() -> Song {
    Song {
        path: "golden/song/chart.bme".into(),
        folder: "golden".into(),
        md5: "0123456789abcdef0123456789abcdef".into(),
        sha256: "f".repeat(64),
        title: "Golden Song".into(),
        subtitle: "[ANOTHER]".into(),
        artist: "Composer".into(),
        subartist: "obj: Charter".into(),
        genre: "TEST".into(),
        level: "11".into(),
        difficulty: 4,
        mode: vibebms_core::PlayMode::Beat7K,
        min_bpm: 120.0,
        max_bpm: 180.0,
        main_bpm: 150.0,
        start_bpm: 150.0,
        notes: 1000,
        length_ms: 125_000,
        total: 300.0,
        has_ln: true,
        has_bga: true,
        has_stop: true,
        stagefile: "stage.png".into(),
        note_kinds: [800, 100, 80, 20, 0],
        judge: Some(2),
        density: [8.0, 15.5, 10.25],
        ..Song::default()
    }
}

fn best() -> BestScore {
    BestScore {
        clear: ClearType::Hard,
        ex_score: 1500,
        notes: 1000,
        judges: [600, 300, 50, 20, 10, 20],
        max_combo: 400,
        min_bp: 50,
        min_cb: 30,
        play_count: 12,
        clear_count: 5,
        last_played: 1_700_000_000,
        ..Default::default()
    }
}

fn ctx() -> Ctx {
    let root = std::env::temp_dir().join(format!("vibebms-golden-{}", std::process::id()));
    let mut ctx = Ctx::for_test(root);
    let s = song();
    let ln = s.score_ln(ctx.config.play.ln_mode) as i32;
    let key = crate::library::RecordKey::new(&s.sha256, ln, ctx.config.play.judge_preset);
    ctx.lib.scores.insert(key, best());
    ctx.tables.list.push(bmseeker::table::DifficultyTable {
        source_url: "https://example.invalid/table".into(),
        header: bmseeker::table::TableHeader {
            name: "Golden Table".into(),
            symbol: "★".into(),
            ..Default::default()
        },
        entries: vec![bmseeker::table::TableEntry {
            md5: s.md5.clone(),
            level: "12".into(),
            ..Default::default()
        }],
        fetched_at: 0,
    });
    ctx.lib.songs = Arc::new(vec![s]);
    ctx
}

/// Every value `st` has for the ids skins use, one id per line (ids with
/// nothing to say left out). The clock (21-26), the version (1010) and the
/// date in the time zone of the machine (9212) are not fixed.
fn dump(out: &mut String, scene: &str, st: &dyn SkinState) {
    let clock = |id: i32| (21..=26).contains(&id);
    let unfixed_text = |id: i32| id == 1010 || id == 9212;
    for id in (0..2500).chain(9000..9600) {
        let mut line = String::new();
        if st.option(id) {
            line += " o";
        }
        if let Some(n) = st.number(id).filter(|_| !clock(id)) {
            write!(line, " n={n}").unwrap();
        }
        if let Some(b) = st.button(id).filter(|_| !clock(id)) {
            write!(line, " b={b}").unwrap();
        }
        if let Some(t) = st.text(id).filter(|_| !unfixed_text(id)) {
            write!(line, " t={t:?}").unwrap();
        }
        let r = st.rate(id);
        if r != 0.0 {
            write!(line, " r={r}").unwrap();
        }
        if !line.is_empty() {
            writeln!(out, "{scene} {id}:{line}").unwrap();
        }
    }
    for rank in [false, true] {
        if let Some(d) = st.distribution(rank) {
            writeln!(out, "{scene} distribution {rank}: {d:?}").unwrap();
        }
    }
}

fn request() -> PlayRequest {
    PlayRequest {
        song: song(),
        autoplay: false,
        course: None,
        replay: None,
        vs: None,
        practice: None,
    }
}

/// A play of a made up 7 key chart (BATTLE: on both sides) by the bot,
/// stopped halfway.
fn play(ctx: &mut Ctx, battle: bool) -> (PlayScene, Instant) {
    let mut scene = PlayScene::new(ctx, request());
    let rules = RuleOptions {
        battle,
        ..ctx.config.play.rules.clone()
    };
    let text = bot::chart_text(7, 21);
    let chart = build_chart(
        Path::new("song/chart.bme"),
        text.as_bytes(),
        &rules,
        7,
        None,
    )
    .unwrap()
    .chart;
    let half = chart.length() / 2;
    let players: &[Option<usize>] = if battle { &[Some(0), Some(1)] } else { &[None] };
    let mut ghost = Vec::new();
    let mut sides = Vec::new();
    for (k, player) in players.iter().enumerate() {
        let (mut e, total) = build_engine(&chart, &rules, *player, false);
        let inputs = bot::inputs(&chart, *player, 99 + k as i64);
        bot::drive(&mut e, &inputs, half);
        if k == 0 {
            // the best play's ghost: another play of the whole chart
            let (mut g, _) = build_engine(&chart, &rules, *player, false);
            bot::drive(
                &mut g,
                &bot::inputs(&chart, *player, 5),
                chart.length() + 2_000_000,
            );
            ghost = g.ghost.clone();
            scene.facts.total = total;
        }
        sides.push(side(e));
    }
    scene.facts.skin_mode = chart.mode();
    scene.facts.input_mode = chart.mode();
    scene.facts.score_ln = LnType::Ln;
    scene.facts.best_ex = Some(ghost_ex(&ghost));
    scene.session = Session::new(chart, sides, false, scene.session.start);
    let now = scene.session.start + Duration::from_micros(half as u64);
    (scene, now)
}

fn side(engine: Engine) -> SideState {
    SideState {
        last_gauge: engine.gauge.value(),
        engine,
        role: Role::Player,
        gauge_log: Vec::new(),
        gauge_up: None,
        finished: None,
        failed: None,
    }
}

/// The result of a play scene's first side (and its second).
fn result(ctx: &mut Ctx, play: &PlayScene) -> ResultScene {
    let ses = &play.session;
    let e = ses.engine().unwrap();
    let p2 = ses.opponent().map(|o| SideResult {
        name: "2P".into(),
        score: o.engine.score.clone(),
        ex: o.engine.score.ex_score(),
        desync: false,
        clear: ClearType::Failed,
        gauge_type: o.engine.gauge.current_type(),
        gauge_value: o.engine.gauge.value(),
        gauge_log: Vec::new(),
        timing_log: o.engine.timing_log.clone(),
        judge_log: o.engine.judge_log.clone(),
    });
    let d = ResultData {
        request: request(),
        title: ses.chart.meta.title.clone(),
        subtitle: ses.chart.meta.subtitle.clone(),
        artist: ses.chart.meta.artist.clone(),
        level: ses.chart.meta.level.clone(),
        mode: ses.mode,
        ln: LnType::Ln,
        score: e.score.clone(),
        clear: ClearType::Normal,
        gauge_type: GaugeType::Normal,
        gauge_value: e.gauge.value(),
        gauge_log: Vec::new(),
        timing_log: e.timing_log.clone(),
        windows: e.windows().note.clone(),
        judge_log: e.judge_log.clone(),
        notes_graph: Default::default(),
        meta: ses.chart.meta.clone(),
        length_us: ses.chart.length(),
        bpm_log: Vec::new(),
        // (not recorded)
        autoplay: true,
        unranked: None,
        assist: Assist::None,
        options: String::new(),
        course: None,
        p2,
        replay: None,
        record: Default::default(),
        course_total: false,
    };
    ResultScene::new(ctx, d)
}

#[test]
fn skin_values_stay_the_same() {
    let mut ctx = ctx();
    let mut out = String::new();

    let mut select = SelectScene::new(&mut ctx);
    select.bars = vec![
        Bar::Song(0),
        Bar::Folder {
            label: "GOLDEN".into(),
            sub: "a folder".into(),
            node: Node::Folder("golden".into()),
            count: 1,
        },
    ];
    for (cursor, name) in [(0, "select-song"), (1, "select-folder")] {
        select.cursor = cursor;
        dump(&mut out, name, &SelectState::new(&select, &ctx));
    }

    let decide = DecideScene::new(request());
    dump(&mut out, "decide", &DecideState::new(&decide, &ctx));

    for (battle, name) in [(false, "play"), (true, "play-battle")] {
        let (scene, now) = play(&mut ctx, battle);
        dump(&mut out, name, &PlayState::new(&scene, &ctx, now));
        let res = result(&mut ctx, &scene);
        let now = Instant::now();
        let res_name = if battle { "result-battle" } else { "result" };
        dump(&mut out, res_name, &ResultState::new(&res, &ctx, now));
    }

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/skin_values.txt");
    if std::env::var_os("GOLDEN_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &out).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap();
    for (w, g) in want.lines().zip(out.lines()) {
        assert_eq!(w, g);
    }
    assert_eq!(want.lines().count(), out.lines().count());
}
