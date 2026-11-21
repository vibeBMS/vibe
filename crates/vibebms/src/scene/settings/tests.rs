use super::*;
use crate::config::IrAccount;
use crate::courses::{Course, Stage};
use crate::gfx::Rect;
use crate::platform::Instant;
use crate::scene::keybind::Slot;
use crate::skinstore::{SkinEntry, Source};
use vibebms_core::course::CourseKind;

fn key(code: KeyCode) -> InputEv {
    InputEv::Key {
        code,
        pressed: true,
        repeat: false,
        time: Instant::now(),
        text: None,
    }
}

fn typed(s: &str) -> InputEv {
    InputEv::Text(s.into())
}

fn tap(s: &mut SettingsScene, ctx: &mut Ctx, (x, y): (f32, f32)) {
    s.place(ctx);
    for phase in [Phase::Down, Phase::Up] {
        let ev = InputEv::Pointer {
            id: 1,
            phase,
            x,
            y,
            time: Instant::now(),
            touch: true,
        };
        s.input(ctx, &ev);
    }
}

fn mid(r: Rect) -> (f32, f32) {
    r.center()
}

/// A tap in the middle of what `pick` finds in the layout.
fn tap_on(s: &mut SettingsScene, ctx: &mut Ctx, pick: impl Fn(&draw::Layout) -> Rect) {
    s.place(ctx);
    let at = mid(pick(&s.lay));
    tap(s, ctx, at);
}

/// A data folder of its own, with a folder, a course of two stages, the
/// three kinds of IR account and a catalog skin: every list has a row.
fn setup(name: &str) -> (std::path::PathBuf, Ctx) {
    let root = std::env::temp_dir().join(format!("vibebms-settings-{name}-{}", std::process::id()));
    let mut ctx = Ctx::for_test(root.clone());
    SESSION.with(|s| *s.borrow_mut() = Session::default());
    ctx.config.bms_dirs.push("songs".into());
    ctx.config.ir.accounts = vec![
        IrAccount {
            provider: ProviderId::Bokutachi,
            submit: true,
            ..Default::default()
        },
        IrAccount {
            provider: ProviderId::BmsIr,
            user_id: "191167".into(),
            ..Default::default()
        },
        IrAccount {
            provider: ProviderId::VibeJson(0),
            url: "http://127.0.0.1:9/api".into(),
            ..Default::default()
        },
    ];
    let stage = |t: &str| Stage {
        md5: t.into(),
        title: t.into(),
        ..Default::default()
    };
    ctx.lib.courses.add(Course {
        title: "TEST".into(),
        kind: CourseKind::Nonstop,
        stages: vec![stage("a"), stage("b")],
        ..Default::default()
    });
    ctx.skins.store.catalog.skins.push(SkinEntry {
        id: "skin".into(),
        name: "A skin".into(),
        license: "free".into(),
        source: Source::Official,
        ..Default::default()
    });
    (root, ctx)
}

/// The rows of `OLD` that moved to the song select's panels
/// (`per_play::MOVED` points to them).
const MOVED_IDS: &[&str] = &[
    "play.random_1p",
    "play.random_2p",
    "play.dp_option",
    "play.battle",
    "play.side",
    "play.hispeed",
    "play.hs_fix",
    "play.lane_cover",
    "play.sudden",
    "play.hidden",
    "play.lift",
    "play.target",
    "play.gauge",
    "play.gauge_auto_shift",
    "play.auto_scratch",
    "play.legacy_note",
    "play.no_mine",
    "play.expand_judge",
    "play.judge_area",
    "play.mark_notes",
    "play.bpm_guide",
    "play.autoplay",
    "play.bga",
    "play.show_fast_slow",
];

/// Every row of the old twelve pages (by what it sets or does) and where
/// it was. Rows that only show while something happens (an update, plays
/// in the outbox, the web's storage) are left out.
const OLD: &[(&str, &[&str])] = &[
    (
        "PLAY",
        &[
            "player_name",
            "play.random_1p",
            "play.random_2p",
            "play.dp_option",
            "play.battle",
            "play.side",
            "play.ln_mode",
            "play.hispeed_mode",
            "play.hs_type",
            "play.hispeed",
            "play.hs_fix",
            "play.hs_anim_ms",
            "play.lane_cover",
            "play.sudden",
            "play.hidden",
            "play.lift",
            "play.judge_offset_ms",
            "play.visual_offset_ms",
            "play.auto_scratch",
            "play.legacy_note",
            "play.no_mine",
            "play.autoplay",
            "play.bga",
            "play.target",
            "play.show_fast_slow",
            "play.hran_threshold_bpm",
        ],
    ),
    (
        "JUDGE & GAUGE",
        &[
            "play.gauge",
            "play.p2.gauge",
            "play.p2.auto_scratch",
            "play.p2.judge_offset_ms",
            "play.gauge_auto_shift",
            "play.gauge_preset",
            "play.judge_preset",
            "play.judge_algorithm",
            "play.expand_judge",
            "play.judge_rates.0",
            "play.judge_rates.1",
            "play.judge_rates.2",
            "play.judge_area",
            "play.mark_notes",
            "play.bpm_guide",
            "play.timing_auto_adjust",
        ],
    ),
    (
        "KEY CONFIG",
        &[
            "keys.any_axis_is_scratch",
            "keys.tuning.turntable_dead_band",
            "keys.tuning.turntable_stop_ms",
            "keys.tuning.chatter_ms",
            "touch.layout_editor",
            "keys.reset",
        ],
    ),
    (
        "TOUCH",
        &[
            "touch.mode",
            "touch.style",
            "touch.layout_editor",
            "touch.landscape_height",
            "touch.portrait_height",
            "touch.screen_inches",
            "touch.opacity",
            "touch.start_select_side",
            "touch.allow_dp",
            "touch.scratch_threshold_deg",
            "perspective.strength",
            "perspective.length",
            "perspective.touch_height",
            "play.side",
        ],
    ),
    (
        "DISPLAY",
        &[
            "display.fullscreen",
            "display.present_mode",
            "display.max_fps",
            "display.ui_scale",
            "display.show_fps",
            "display.show_latency",
            "info.gpu",
            "skins.choice",
        ],
    ),
    (
        "AUDIO",
        &[
            "audio.master",
            "audio.key",
            "audio.bgm",
            "audio.system",
            "audio.buffer_frames",
            "audio.preview",
            "audio.mute_inactive",
            "audio.sound_set",
            "info.output",
        ],
    ),
    (
        "LIBRARY",
        &[
            "info.data_folder",
            "scan_root",
            "library.folder",
            #[cfg(desktop)]
            "library.browse",
            "library.add_folder",
            "library.rescan",
            "library.import_scores",
            #[cfg(desktop)]
            "library.browse_scores",
        ],
    ),
    (
        "TABLES",
        &[
            "tables.table",
            "tables.remove",
            "tables.add",
            "tables.update_all",
            "third_party_mirrors",
            "name_song_folders",
            "tables.restore_defaults",
        ],
    ),
    (
        "COURSES",
        &[
            "courses.new",
            "courses.import_lr2",
            #[cfg(desktop)]
            "courses.browse_lr2",
            "courses.course",
            "course.title",
            "course.kind",
            "course.stage",
            "course.stage_up",
            "course.join",
            "course.add",
            "course.shift_up",
            "course.shift_down",
            "course.delete",
            "random_mix.stages",
            "random_mix.mode",
            "random_mix.min_level",
            "random_mix.max_level",
            "random_mix.min_bpm",
            "random_mix.max_bpm",
            "random_mix.bpm_range",
            "random_mix.optimum_level",
            "random_mix.join",
        ],
    ),
    (
        "IR",
        &[
            "ir.submit",
            "ir.primary",
            "ir.beside",
            "ir.enabled",
            "ir.name",
            "ir.url",
            "ir.key",
            "ir.submit_plays",
            "ir.lamps",
            "ir.remove",
            "ir.player",
            "ir.view",
            "ir.rivals",
            "ir.table",
            "ir.add_server",
            "ir.outbox",
        ],
    ),
    (
        "SKINS",
        &[
            "catalog.update",
            "catalog.get",
            "catalog.page",
            "catalog.terms",
        ],
    ),
    ("SYSTEM", &["info.version"]),
];

#[test]
fn every_old_row_is_reachable() {
    let (root, ctx) = setup("tree");
    let armed = Armed::default();
    let tree = search::walk(&ctx, &armed);
    let has = |id: &str| tree.iter().any(|(_, r)| r.id == id);
    // (the per-play rows are on the song select, and only there)
    for (page, ids) in OLD {
        for id in *ids {
            match MOVED_IDS.contains(id) {
                true => assert!(!has(id), "{page}: {id} is in settings too"),
                false => assert!(has(id), "{page}: {id} is nowhere"),
            }
        }
    }
    // every skin type's choice, every BMS-IR table, every binding
    let count = |id: &str| tree.iter().filter(|(_, r)| r.id == id).count();
    assert!(count("skins.choice") >= crate::scene::skinselect::TYPES.len());
    assert_eq!(
        count("ir.table"),
        vibebms_ir::providers::bmsir_read::TABLES.len()
    );
    let mut slots = vec![Slot::Start, Slot::Select, Slot::Start2, Slot::Select2];
    for side in 0..2 {
        slots.extend((0..7).map(|idx| Slot::Key { side, idx }));
        slots.extend([
            Slot::ScratchUp(side),
            Slot::ScratchDown(side),
            Slot::ScratchAxis(side),
        ]);
    }
    slots.extend((0..9).map(Slot::Pms));
    for n in [4u8, 6, 8] {
        slots.extend((0..n as usize).map(|i| Slot::Few(n, i)));
    }
    for slot in slots {
        assert!(
            tree.iter()
                .any(|(_, r)| matches!(r.act, Act::Bind(s) if s == slot)),
            "{slot:?} cannot be bound"
        );
    }
    // every row that does something says what in a line
    for (path, r) in &tree {
        if r.acts() && !matches!(r.kind, Kind::Cell { .. }) {
            assert!(!r.desc.is_empty(), "{path:?} {}: no description", r.label);
        }
    }
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn search_finds_rows_and_points_to_the_song_select() {
    let (root, mut ctx) = setup("search");
    let mut s = SettingsScene::new(&mut ctx);
    s.input(&mut ctx, &key(KeyCode::Slash));
    assert!(s.search.is_some());
    s.input(&mut ctx, &typed("judge offset"));
    let first = &s.rows[0];
    assert_eq!(first.id, "play.judge_offset_ms");
    assert!(
        first.desc.starts_with("GAMEPLAY › TIMING"),
        "{}",
        first.desc
    );
    // → changes it in place
    let before = ctx.config.play.judge_offset_ms;
    s.input(&mut ctx, &key(KeyCode::ArrowRight));
    assert_eq!(ctx.config.play.judge_offset_ms, before + 1);
    assert!(s.search.is_some());
    // the options of the song select: a pointer first
    let find = |s: &mut SettingsScene, ctx: &mut Ctx, q: &str| {
        let t = s.search.as_mut().unwrap();
        t.input.buf = q.into();
        s.refresh(ctx);
        s.rows.iter().map(|r| r.label.clone()).collect::<Vec<_>>()
    };
    let random = find(&mut s, &mut ctx, "random");
    assert_eq!(random[0], "Random → song select");
    assert!(s.rows[0].desc.contains("PLAY OPTION"));
    let sort = find(&mut s, &mut ctx, "sort");
    assert_eq!(sort, ["Sort → song select"]);
    // rows of sub-pages, and of folded groups
    let ir = find(&mut s, &mut ctx, "bms-ir");
    assert!(ir.iter().any(|l| l == "BMS-IR"), "{ir:?}");
    let table = vibebms_ir::providers::bmsir_read::TABLES[0].1;
    find(&mut s, &mut ctx, table);
    let cell = s.rows.iter().find(|r| r.id == "ir.table").unwrap();
    assert_eq!(cell.label, table);
    assert!(
        cell.desc.starts_with("ONLINE › BMS-IR › TABLES"),
        "{}",
        cell.desc
    );
    assert!(find(&mut s, &mut ctx, "nothing like this").is_empty());
    // Enter goes there: the page, the group opened, the row chosen
    find(&mut s, &mut ctx, "chatter");
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.search.is_none());
    assert_eq!(s.view, View::Page(Page::Controls));
    assert_eq!(s.row().map(|r| r.id), Some("keys.tuning.chatter_ms"));
    // and into a sub-page, its breadcrumb behind it
    s.input(&mut ctx, &key(KeyCode::Slash));
    s.input(&mut ctx, &typed("player id"));
    s.list.sel = s.rows.iter().position(|r| r.id == "ir.player").unwrap();
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Sub(Sub::Account(ProviderId::BmsIr)));
    assert_eq!(
        s.parents.last().map(|p| &p.0),
        Some(&View::Page(Page::Online))
    );
    assert_eq!(s.row().map(|r| r.id), Some("ir.player"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn keys_walk_cards_pages_groups_and_sub_pages() {
    let (root, mut ctx) = setup("keys");
    let mut s = SettingsScene::new(&mut ctx);
    assert_eq!(s.view, View::Top);
    // the cards: 4 × 2
    s.input(&mut ctx, &key(KeyCode::ArrowRight));
    s.input(&mut ctx, &key(KeyCode::ArrowDown));
    assert_eq!(s.list.sel, 5);
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Page(Page::Courses));
    // Esc: the rail, the cards (on this page's), out
    s.input(&mut ctx, &key(KeyCode::Escape));
    assert!(s.rail);
    s.input(&mut ctx, &key(KeyCode::ArrowUp));
    assert_eq!(s.view, View::Page(Page::Library));
    s.input(&mut ctx, &key(KeyCode::Escape));
    assert_eq!(s.view, View::Top);
    assert_eq!(s.list.sel, 4);
    assert!(matches!(
        s.input(&mut ctx, &key(KeyCode::Escape)),
        Next::Pop
    ));

    // GAMEPLAY: the banner and headers are skipped; → changes the value
    let mut s = SettingsScene::at(&mut ctx, Page::Gameplay);
    assert_eq!(s.row().map(|r| r.id), Some("play.judge_offset_ms"));
    s.input(&mut ctx, &key(KeyCode::ArrowRight));
    assert_eq!(ctx.config.play.judge_offset_ms, 1);
    assert!(s.row().unwrap().value.contains("+1"));
    s.input(&mut ctx, &key(KeyCode::ArrowUp));
    assert!(s.row().unwrap().focusable());
    // a group opens on Enter, and its rows come after it
    let group = s.rows.iter().position(|r| r.id == "group").unwrap();
    let n = s.rows.len();
    s.list.sel = group;
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.rows.len() > n);
    assert!(matches!(s.rows[group].kind, Kind::Group { open: true, .. }));
    // it stays open for the session
    let again = SettingsScene::at(&mut ctx, Page::Gameplay);
    assert_eq!(again.rows.len(), s.rows.len());
    // a sub-page and back to the row it was opened from
    let expand = s
        .rows
        .iter()
        .position(|r| r.id == "play.judge_rates")
        .unwrap();
    s.list.sel = expand;
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Sub(Sub::ExpandJudge));
    assert_eq!(s.row().map(|r| r.id), Some("play.judge_rates.0"));
    let rate = ctx.config.play.judge_rates[0];
    s.input(&mut ctx, &key(KeyCode::ArrowLeft));
    assert_eq!(ctx.config.play.judge_rates[0], rate - 5);
    s.input(&mut ctx, &key(KeyCode::Escape));
    assert_eq!(s.view, View::Page(Page::Gameplay));
    assert_eq!(s.list.sel, expand);
    s.input(&mut ctx, &key(KeyCode::Enter));
    s.input(&mut ctx, &key(KeyCode::Escape));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Sub(Sub::ExpandJudge));
    for _ in 0..2 {
        s.input(&mut ctx, &key(KeyCode::Escape));
    }
    assert!(s.rail);

    // CONTROLS › 1P KEYS: Enter waits for a key, the key binds, on to the
    // next row
    let mut s = SettingsScene::at(&mut ctx, Page::Controls);
    assert_eq!(s.row().map(|r| r.id), Some("keys.side"));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Sub(Sub::Keys(KeySet::Side(0))));
    let first = s.list.sel;
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.learn.on());
    s.input(&mut ctx, &key(KeyCode::KeyQ));
    assert_eq!(ctx.config.keys.p1.keys[0].keys, vec![KeyCode::KeyQ]);
    assert!(!s.learn.on());
    assert_eq!(s.list.sel, first + 1);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_bms_ir_tables_are_a_grid() {
    let (root, mut ctx) = setup("grid");
    let mut s = SettingsScene::at(&mut ctx, Page::Online);
    s.push(&mut ctx, Sub::Account(ProviderId::BmsIr));
    s.place(&ctx);
    let cols = s.grid_cols;
    assert_eq!(cols, 3);
    let first = s
        .rows
        .iter()
        .position(|r| matches!(r.kind, Kind::Cell { .. }))
        .unwrap();
    s.list.sel = first;
    s.input(&mut ctx, &key(KeyCode::ArrowRight));
    assert_eq!(s.list.sel, first + 1);
    s.input(&mut ctx, &key(KeyCode::ArrowDown));
    assert_eq!(s.list.sel, first + 1 + cols);
    s.input(&mut ctx, &key(KeyCode::ArrowLeft));
    assert_eq!(s.list.sel, first + cols);
    // Enter toggles the table
    let table = vibebms_ir::providers::bmsir_read::TABLES[cols].0;
    let reads = |ctx: &Ctx| {
        let a = ctx.config.ir.account(ProviderId::BmsIr).unwrap();
        a.tables.iter().any(|t| t == table)
    };
    assert!(!reads(&ctx));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(reads(&ctx));
    assert!(matches!(s.rows[first + cols].kind, Kind::Cell { on: true }));
    // laid out three a line
    let (a, b) = (s.lay.pos[first], s.lay.pos[first + 1]);
    assert_eq!(a.y, b.y);
    assert!(b.x > a.x);
    assert!(s.lay.pos[first + cols].y > a.y);
    // ↑ from the first line leaves the grid
    s.list.sel = first;
    s.input(&mut ctx, &key(KeyCode::ArrowUp));
    assert!(s.list.sel < first);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_controller_walks_the_same() {
    let (root, mut ctx) = setup("pad");
    let mut s = SettingsScene::new(&mut ctx);
    s.nav(&mut ctx, Nav::Down);
    assert_eq!(s.list.sel, 1);
    s.nav(&mut ctx, Nav::Decide);
    assert_eq!(s.view, View::Page(Page::Controls));
    assert!(!s.rail);
    // a white key opens a sub-page, a blue key goes back
    s.nav(&mut ctx, Nav::Decide);
    assert_eq!(s.view, View::Sub(Sub::Keys(KeySet::Side(0))));
    s.nav(&mut ctx, Nav::Close);
    assert_eq!(s.view, View::Page(Page::Controls));
    // on a value they raise and lower it
    let any = s
        .rows
        .iter()
        .position(|r| r.id == "keys.any_axis_is_scratch")
        .unwrap();
    s.list.sel = any;
    let before = ctx.config.keys.any_axis_is_scratch;
    s.nav(&mut ctx, Nav::Decide);
    assert_ne!(ctx.config.keys.any_axis_is_scratch, before);
    s.nav(&mut ctx, Nav::Close);
    assert_eq!(ctx.config.keys.any_axis_is_scratch, before);
    // START: the pages; the turntable picks one
    s.nav(&mut ctx, Nav::Option);
    assert!(s.rail);
    s.nav(&mut ctx, Nav::Down);
    assert_eq!(s.view, View::Page(Page::Display));
    assert!(matches!(s.nav(&mut ctx, Nav::Settings), Next::Pop));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn taps_open_cards_pages_rows_and_search() {
    let (root, mut ctx) = setup("touch");
    let mut s = SettingsScene::new(&mut ctx);
    tap_on(&mut s, &mut ctx, |l| l.cards[6].0);
    assert_eq!(s.view, View::Page(Page::Online));
    // a rail page
    tap_on(&mut s, &mut ctx, |l| l.rail[2].0);
    assert_eq!(s.view, View::Page(Page::Display));
    // the right half of a value raises it, the left half lowers it
    s.place(&ctx);
    let i = s
        .rows
        .iter()
        .position(|r| r.id == "display.ui_scale")
        .unwrap();
    let r = s.lay.pos[i];
    let before = ctx.config.display.ui_scale;
    tap(&mut s, &mut ctx, (r.right() - 4.0, r.y + r.h * 0.5));
    assert!(ctx.config.display.ui_scale > before);
    assert_eq!(s.list.sel, i);
    // (the scale changed the layout: place again)
    s.place(&ctx);
    let r = s.lay.pos[i];
    tap(&mut s, &mut ctx, (r.x + r.w * 0.6, r.y + r.h * 0.5));
    assert!((ctx.config.display.ui_scale - before).abs() < 1e-4);
    // a group's row folds it
    let g = s.rows.iter().position(|r| r.id == "group").unwrap();
    let n = s.rows.len();
    s.list.select(g);
    s.place(&ctx);
    tap_on(&mut s, &mut ctx, |l| l.pos[g]);
    assert!(s.rows.len() > n);
    // the search field, then BACK closes it, and BACK again goes up
    s.place(&ctx);
    tap_on(&mut s, &mut ctx, |l| l.search);
    assert!(s.search.is_some());
    s.input(&mut ctx, &typed("volume"));
    assert!(s.rows.iter().any(|r| r.id == "audio.master"));
    tap_on(&mut s, &mut ctx, |l| l.back);
    assert!(s.search.is_none());
    tap_on(&mut s, &mut ctx, |l| l.back);
    assert!(s.rail);
    tap_on(&mut s, &mut ctx, |l| l.back);
    assert_eq!(s.view, View::Top);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn portrait_has_two_card_columns_and_page_chips() {
    let (root, mut ctx) = setup("portrait");
    ctx.view.width = 720.0;
    ctx.view.height = 1280.0;
    let mut s = SettingsScene::new(&mut ctx);
    s.place(&ctx);
    assert_eq!(s.card_cols, 2);
    let (a, b) = (s.lay.cards[0].0, s.lay.cards[2].0);
    assert!(b.y > a.bottom());
    s.input(&mut ctx, &key(KeyCode::ArrowDown));
    assert_eq!(s.list.sel, 2);
    tap_on(&mut s, &mut ctx, |l| l.cards[3].0);
    assert_eq!(s.view, View::Page(Page::Skins));
    s.place(&ctx);
    // the pages: two lines of four over the rows
    assert_eq!(s.lay.rail.len(), 8);
    assert!(s.lay.rail[4].0.y > s.lay.rail[0].0.y);
    assert!(s.lay.content.y > s.lay.rail[7].0.bottom());
    std::fs::remove_dir_all(&root).ok();
}

/// SYSTEM's profiles: a new one named, switched to (the app does the
/// switch), renamed, and the other deleted on a second press only.
#[test]
fn profiles_are_made_switched_renamed_and_deleted() {
    let (root, mut ctx) = setup("profiles");
    ctx.open_records();
    let mut s = SettingsScene::at(&mut ctx, Page::System);
    let pick = |s: &mut SettingsScene, id: &str| {
        s.list.sel = s.rows.iter().position(|r| r.id == id).unwrap();
    };
    pick(&mut s, "profile.new");
    s.input(&mut ctx, &key(KeyCode::Enter));
    s.input(&mut ctx, &typed("Guest"));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(ctx.profiles.registry.name("guest"), "Guest");
    let rows: Vec<&str> = (s.rows.iter())
        .filter(|r| r.id == "profile")
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(rows, ["DEFAULT", "Guest"]);
    // its page: switch to it
    s.list.sel = s.rows.iter().position(|r| r.label == "Guest").unwrap();
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(s.view, View::Sub(Sub::Profile("guest".into())));
    assert!(!s.rows.iter().any(|r| r.id == "profile.delete_using"));
    pick(&mut s, "profile.use");
    s.input(&mut ctx, &key(KeyCode::Enter));
    let to = ctx.profiles.switch_to.take();
    assert_eq!(to.as_deref(), Some("guest"));
    assert!(ctx.switch_profile("guest"));
    assert_eq!(ctx.config.player_name, "Guest");
    // in use: renamed; not deleted
    let mut s = SettingsScene::at(&mut ctx, Page::System);
    s.list.sel = s.rows.iter().position(|r| r.label == "Guest").unwrap();
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.rows.iter().any(|r| r.id == "profile.delete_using"));
    pick(&mut s, "profile.name");
    s.input(&mut ctx, &key(KeyCode::Enter));
    for _ in 0.."Guest".len() {
        s.input(&mut ctx, &key(KeyCode::Backspace));
    }
    s.input(&mut ctx, &typed("Visitor"));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert_eq!(ctx.profiles.registry.name("guest"), "Visitor");
    // the default: never deleted either
    s.back(&mut ctx);
    s.list.sel = s.rows.iter().position(|r| r.label == "DEFAULT").unwrap();
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.rows.iter().any(|r| r.id == "profile.default"));
    assert!(!s.rows.iter().any(|r| r.id == "profile.delete"));
    // back on the default, the other goes on a second press
    assert!(ctx.switch_profile(""));
    let mut s = SettingsScene::at(&mut ctx, Page::System);
    s.list.sel = s.rows.iter().position(|r| r.label == "Visitor").unwrap();
    s.input(&mut ctx, &key(KeyCode::Enter));
    pick(&mut s, "profile.delete");
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(ctx.profiles.registry.has("guest"), "one press only arms it");
    assert_eq!(
        s.row().map(|r| r.value.as_str()),
        Some("press again to delete")
    );
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(!ctx.profiles.registry.has("guest"));
    assert!(!root.join("profiles/guest").exists());
    // its page closed with it
    assert_eq!(s.view, View::Page(Page::System));
    std::fs::remove_dir_all(&root).ok();
}

/// Every page in every language: no `{placeholder}` left unfilled, and the
/// language's text is shown (the language row itself stays findable).
#[test]
fn every_language_fills_every_row() {
    use crate::i18n::{self, Lang};
    let (root, ctx) = setup("langs");
    let armed = Armed::default();
    let english: Vec<String> = search::walk(&ctx, &armed)
        .into_iter()
        .map(|(_, r)| r.label)
        .collect();
    for lang in Lang::ALL {
        i18n::set(lang);
        let tree = search::walk(&ctx, &armed);
        for (_, r) in &tree {
            for s in [&r.label, &r.desc, &r.value] {
                let open = s.find('{');
                assert!(
                    open.is_none_or(|i| !s[i..].contains('}')),
                    "{}: {s:?} has an unfilled placeholder",
                    lang.tag()
                );
            }
        }
        let labels: Vec<&String> = tree.iter().map(|(_, r)| &r.label).collect();
        let same = labels
            .iter()
            .zip(&english)
            .filter(|(a, b)| **a == *b)
            .count();
        if lang != Lang::En {
            assert!(
                same * 2 < labels.len(),
                "{}: {same} of {} rows in English",
                lang.tag(),
                labels.len()
            );
        }
        // search finds the language row in any language by its English name
        let (rows, _) = search::find(&ctx, &armed, "language");
        assert!(rows.iter().any(|r| r.id == "language"), "{}", lang.tag());
    }
    i18n::set(Lang::En);
    let _ = std::fs::remove_dir_all(root);
}

/// Third parties' copies stay off until the player has said OK to what
/// they are; once said, the row turns them on and off without asking.
#[test]
fn third_party_sources_ask_once() {
    let (root, mut ctx) = setup("third-party");
    ctx.config.third_party_mirrors = false;
    ctx.config.third_party_acknowledged = false;
    let mut s = SettingsScene::at(&mut ctx, Page::Library);
    if !s.rows.iter().any(|r| r.id == "third_party_mirrors") {
        s.fold(&ctx, "library.downloads");
    }
    let pick = |s: &mut SettingsScene| {
        s.rail = false;
        s.list.sel = (s.rows.iter())
            .position(|r| r.id == "third_party_mirrors")
            .unwrap();
    };
    let on = |ctx: &Ctx| ctx.net.hosts.config.third_party;
    pick(&mut s);
    // Enter asks; Esc (and Cancel, chosen at first) leave them off
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.ask.is_some());
    s.input(&mut ctx, &key(KeyCode::Escape));
    assert!(s.ask.is_none());
    assert_eq!(
        s.view,
        View::Page(Page::Library),
        "Esc closed only the dialog"
    );
    s.input(&mut ctx, &key(KeyCode::ArrowRight));
    assert!(s.ask.is_some(), "→ asks too");
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(!ctx.config.third_party_mirrors && !ctx.config.third_party_acknowledged);
    assert!(!on(&ctx));
    // OK: on, and remembered
    s.input(&mut ctx, &key(KeyCode::Enter));
    s.input(&mut ctx, &key(KeyCode::ArrowLeft));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.ask.is_none());
    assert!(ctx.config.third_party_mirrors && ctx.config.third_party_acknowledged);
    assert!(on(&ctx));
    assert_eq!(s.row().map(|r| r.id), Some("third_party_mirrors"));
    // off and on again: no question
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.ask.is_none() && !ctx.config.third_party_mirrors && !on(&ctx));
    s.input(&mut ctx, &key(KeyCode::Enter));
    assert!(s.ask.is_none() && ctx.config.third_party_mirrors && on(&ctx));
    // the OK is saved with the settings
    let saved = crate::profiles::load_config(&ctx.paths);
    assert!(saved.third_party_acknowledged && saved.third_party_mirrors);
    std::fs::remove_dir_all(&root).ok();
}

/// GET SKINS lists third parties' copies only with their sources on (an
/// installed one stays, to be deleted), says how many it leaves out, and
/// does not download one while they are off.
#[test]
fn third_party_skins_are_offered_with_their_sources_on() {
    let (root, mut ctx) = setup("third-party-skins");
    let skin = |id: &str, source| SkinEntry {
        id: id.into(),
        name: id.to_uppercase(),
        kind: "beatoraja".into(),
        folder: id.into(),
        downloads: vec!["https://example.com/s.zip".into()],
        source,
        ..Default::default()
    };
    ctx.skins.store.catalog.skins = vec![
        skin("own", Source::Official),
        skin("copy", Source::ThirdParty),
        skin("kept", Source::ThirdParty),
    ];
    std::fs::create_dir_all(ctx.paths.skins.join("kept")).unwrap();
    let set = |ctx: &mut Ctx, on: bool| {
        ctx.config.third_party_mirrors = on;
        ctx.net.hosts = crate::hosts::context(&ctx.config);
    };
    let armed = Armed::default();
    let listed = |ctx: &Ctx| -> (Vec<String>, Vec<&'static str>) {
        let rows = View::Sub(Sub::Catalog).rows(ctx, &armed, Folds::All);
        let skins = (rows.iter())
            .filter(|r| r.id == "catalog.skin")
            .map(|r| r.label.clone())
            .collect();
        (skins, rows.iter().map(|r| r.id).collect())
    };
    set(&mut ctx, false);
    let (skins, ids) = listed(&ctx);
    assert_eq!(skins, ["OWN", "KEPT"]);
    assert!(ids.contains(&"catalog.third_party"));
    // the count on SKINS is what is listed
    let rows = View::Page(Page::Skins).rows(&ctx, &armed, Folds::All);
    let get = rows.iter().find(|r| r.id == "catalog").unwrap();
    assert!(get.value.starts_with("2 "), "{}", get.value);
    // its own page says so, and it is not downloaded
    let sub = View::Sub(Sub::CatalogSkin("copy".into())).rows(&ctx, &armed, Folds::All);
    assert!(sub.iter().any(|r| r.id == "catalog.source"));
    let r = ctx.skins.store.install(&ctx.paths, &ctx.net.hosts, 1);
    assert!(r.unwrap_err().contains("LIBRARY"));
    let sub = View::Sub(Sub::CatalogSkin("own".into())).rows(&ctx, &armed, Folds::All);
    assert!(!sub.iter().any(|r| r.id == "catalog.source"));
    // on: all of them, no note
    set(&mut ctx, true);
    let (skins, ids) = listed(&ctx);
    assert_eq!(skins, ["OWN", "COPY", "KEPT"]);
    assert!(!ids.contains(&"catalog.third_party"));
    std::fs::remove_dir_all(&root).ok();
}

/// A key held for a server on another machine over plain `http` goes to
/// an `https` address only when the player enters it here: not because
/// config.toml names one at the next start (a configuration shared by
/// someone else moves no key).
#[test]
fn a_held_key_goes_only_to_an_address_entered_here() {
    let root = std::env::temp_dir().join(format!("vibebms-settings-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut ctx = Ctx::for_test(root.clone());
    let p = ProviderId::VibeJson(0);
    let file = ctx.paths.private.join("credentials.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    // a key kept by a version before, for a server at home
    std::fs::write(&file, r#"{"keys":{"vibe-json:0":"k0"}}"#).unwrap();
    ctx.config.ir.accounts = vec![IrAccount {
        provider: p,
        url: "http://192.168.1.10:8080".into(),
        ..Default::default()
    }];
    ctx.net.ir = crate::ir::IrService::new(&ctx.paths, 0);
    ctx.net.ir.take_old_tokens(&mut ctx.config.ir);
    assert!(ctx.net.ir.key_held(&ctx.config.ir.accounts[0]));
    // the next start, with config.toml naming an https server for it
    let https = "https://ir.example/api";
    ctx.config.ir.accounts[0].url = https.into();
    ctx.net.ir = crate::ir::IrService::new(&ctx.paths, 0);
    ctx.net.ir.take_old_tokens(&mut ctx.config.ir);
    let a = &ctx.config.ir.accounts[0];
    assert!(ctx.net.ir.key_held(a) && !ctx.net.ir.has_key(a));
    assert!(!std::fs::read_to_string(&file)
        .unwrap()
        .contains("ir.example"));
    // the player enters the address
    online::commit(&mut ctx, TextField::IrUrl(0), https.into());
    let a = &ctx.config.ir.accounts[0];
    assert!(!ctx.net.ir.key_held(a) && ctx.net.ir.has_key(a));
    assert!(crate::ir::IrService::new(&ctx.paths, 0).has_key(a));
    std::fs::remove_dir_all(&root).ok();
}
