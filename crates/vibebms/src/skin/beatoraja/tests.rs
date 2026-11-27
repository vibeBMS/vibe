use super::files::skin_path;
use super::*;

/// The folder of the skin `path`, as the skins folder: all the skin may
/// read.
fn own(path: &Path) -> &Path {
    path.parent().unwrap()
}

/// [`load_with`] with only options chosen, for a skin in a folder of its
/// own.
fn load(
    path: &Path,
    user_options: &HashMap<i32, bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let user = UserSettings {
        options: user_options.clone(),
        ..Default::default()
    };
    load_with(path, own(path), &user, None, image_size)
}

/// [`read_skin`] for a skin in a folder of its own.
fn read(
    path: &Path,
    skin_config: Option<SkinConfig<'_>>,
    state: Option<&dyn SkinState>,
) -> Result<(Value, Option<script::Script>), String> {
    let limits = match skin_config {
        Some(_) => script::LOAD,
        None => script::HEADER,
    };
    read_skin(path, &Home::of(path, own(path)), skin_config, state, limits)
}

#[test]
fn lua_runs_like_beatoraja() {
    // skins written for LuaJ (Lua 5.2) and beatoraja's restricted io / os
    let dir = std::env::temp_dir().join(format!("vibebms-lcompat-{}", std::process::id()));
    let skin = dir.join("theme").join("play");
    std::fs::create_dir_all(skin.join("parts")).unwrap();
    std::fs::write(skin.join("parts").join("list.txt"), "a\nb\n").unwrap();
    std::fs::write(skin.join("parts").join("p.lua"), "return 7").unwrap();
    let lua = r#"
        local main_state = require("main_state")
        local n = 0
        for l in io.lines("skin/othername/play/parts/list.txt") do n = n + 1 end
        local f = io.open("parts/list.txt", "r")
        local first = f:read("*l")
        f:close()
        local w = io.open("log.txt", "w")
        w:write("x")
        w:close()
        return {
            lines = n, first = first, written = io.open("log.txt") == nil,
            part = dofile("skin/othername/play/parts/p.lua"),
            band = bit32.band(0xff, 0x0f), shift = bit32.lshift(1, 4),
            random = math.random(1.0, 1.5), fmt = string.format("%d", 2.7),
            date = type(os.date("%Y")), time = type(os.time()),
            opt = main_state.option(150), num = main_state.number(71),
        }"#;
    std::fs::write(skin.join("t.luaskin"), lua).unwrap();
    struct St;
    impl SkinState for St {
        fn timer(&self, _: i32) -> Option<i64> {
            None
        }
        fn option(&self, id: i32) -> bool {
            id == 150
        }
        fn number(&self, id: i32) -> Option<i64> {
            (id == 71).then_some(1234)
        }
        fn text(&self, _: i32) -> Option<String> {
            None
        }
        fn rate(&self, _: i32) -> f32 {
            0.0
        }
    }
    let path = skin.join("t.luaskin");
    let home = Home::of(&path, &dir);
    let (v, _) = read_skin(&path, &home, None, Some(&St), script::HEADER).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let want = serde_json::json!({
        "lines": 2, "first": "a", "written": true, "part": 7, "band": 15, "shift": 16,
        "random": 1, "fmt": "2", "date": "string", "time": "number", "opt": true,
        "num": 1234,
    });
    assert_eq!(v, want);
}

/// Every example of the path resolution's specification (§5.2).
#[test]
fn skin_path_rules() {
    let dir = std::env::temp_dir().join(format!("vibebms-skinpath-{}", std::process::id()));
    for d in ["bomb/a", "bomb/b", "bg", "lane", "parts", "img", "plain"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    for f in [
        "bomb/a/x.png",
        "bomb/b/x.png",
        "bg/one.png",
        "bg/two.png",
        "lane/lane_blue.png",
        "lane/lane_red.png",
        "parts/a.png",
        "parts/b.PNG",
        "parts/c.jpg",
        "img/f1.png",
        "img/f2.txt",
        "plain/file.png",
    ] {
        std::fs::write(dir.join(f), b"").unwrap();
    }
    let v = serde_json::json!({"filepath": [
        {"name": "Gone", "path": "gone/*.png", "def": "x"},
        {"name": "Bomb", "path": "bomb\\*|1|", "def": "b"},
        {"name": "Back", "path": "bg/*.png", "def": "two"},
        {"name": "Lane", "path": "lane/lane_*.png", "def": "lane_blue.png"},
        {"name": "Plain", "path": "plain/file.png", "def": ""},
    ]});
    let fps = filepaths(&v);
    // an entry without a `*` is no entry
    assert_eq!(fps.len(), 4);
    let none = HashMap::new();
    let with = |k: &str, val: &str| HashMap::from([(k.to_string(), val.to_string())]);
    let home = Home::of(&dir.join("s.json"), &dir);
    let p =
        |files: &HashMap<String, String>, path: &str| skin_path(&home, &fps, files, path).unwrap();
    assert_eq!(p(&none, "bomb/*|1|/x.png"), dir.join("bomb/b/x.png"));
    assert_eq!(p(&none, "bomb\\*|1|\\x.png"), dir.join("bomb/b/x.png"));
    assert_eq!(
        p(&with("Bomb", "A"), "bomb/*|1|/x.png"),
        dir.join("bomb/a/x.png")
    );
    assert_eq!(p(&none, "bg/*.png"), dir.join("bg/two.png"));
    assert_eq!(
        p(&with("Back", "three"), "bg/*.png"),
        dir.join("bg/one.png")
    );
    assert_eq!(
        p(&with("Back", "ONE.png"), "bg/*.png"),
        dir.join("bg/one.png")
    );
    assert_eq!(
        p(&with("Lane", "LANE_RED.PNG"), "lane/lane_*.png"),
        dir.join("lane/lane_red.png")
    );
    assert_eq!(p(&none, "lane/lane_*.png"), dir.join("lane/lane_blue.png"));
    // no candidates: the entry is skipped, then the wildcard rule finds
    // nothing either: the path as given
    assert_eq!(p(&none, "gone/*.png"), dir.join("gone/*.png"));
    // a wildcard without an entry: one of the matching files, at random
    let mut seen = std::collections::HashSet::new();
    for _ in 0..200 {
        let got = p(&none, "parts/*.png");
        assert!(
            got == dir.join("parts/a.png") || got == dir.join("parts/b.PNG"),
            "{got:?}"
        );
        seen.insert(got);
    }
    assert_eq!(p(&none, "img/*|frame|.png"), dir.join("img/f1.png"));
    assert_eq!(p(&none, "parts/none_*.gif"), dir.join("parts/none_*.gif"));
    // no wildcard: the path with its letter case as on disk
    assert_eq!(p(&none, "Plain\\File.PNG"), dir.join("plain/file.png"));
    assert_eq!(p(&none, "plain/missing.png"), dir.join("plain/missing.png"));
    std::fs::remove_dir_all(&dir).ok();
    assert!(!seen.is_empty());
}

#[test]
fn filepath_choices() {
    let dir = std::env::temp_dir().join(format!("vibebms-fpath-{}", std::process::id()));
    for d in ["bomb/a", "bomb/b", "bg"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    for f in ["bomb/a/x.png", "bomb/b/x.png", "bg/one.png", "bg/two.png"] {
        std::fs::write(dir.join(f), b"").unwrap();
    }
    let json = r#"{"type": 0, "name": "F",
        "filepath": [
            {"name": "Bomb", "path": "bomb/*|1|", "def": "b"},
            {"name": "Back", "path": "bg/*.png", "def": "two"}
        ]}"#;
    std::fs::write(dir.join("s.json"), json).unwrap();
    let v = read(&dir.join("s.json"), None, None).unwrap().0;
    let fps = filepaths(&v);
    let home = Home::of(&dir.join("s.json"), &dir);
    let mut user = HashMap::new();
    // defaults, with or without the extension
    let p = skin_path(&home, &fps, &user, "bomb/*|1|/x.png");
    assert_eq!(p, Some(dir.join("bomb/b/x.png")));
    assert_eq!(
        skin_path(&home, &fps, &user, "bg/*.png"),
        Some(dir.join("bg/two.png"))
    );
    user.insert("Bomb".to_string(), "a".to_string());
    assert_eq!(
        skin_path(&home, &fps, &user, "bomb/*|1|/x.png"),
        Some(dir.join("bomb/a/x.png"))
    );
    let opts = custom_options(&dir.join("s.json"), &dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(opts.len(), 2);
    assert_eq!(
        (opts[0].labels.clone(), opts[0].default),
        (vec!["a".into(), "b".into()], 1)
    );
    assert_eq!(
        opts[1].labels,
        vec!["one.png".to_string(), "two.png".into()]
    );
    assert!(opts[1].ops.is_empty());
}

#[test]
fn nested_branches() {
    let v = serde_json::json!([
        {"if": [1], "values": [
            {"id": "a"},
            {"if": [2], "values": [{"id": "b"}, {"if": [-3], "values": [{"id": "c"}]}]},
            {"if": [3], "values": [{"id": "d"}]}
        ]}
    ]);
    let en: HashSet<i64> = [1, 2].into();
    let mut files = Files {
        home: &Home::default(),
        includes: Includes::new(),
    };
    let out = preprocess(v, &en, &mut files, Path::new(""), 0);
    let ids: Vec<&str> = out.as_array().unwrap().iter().map(|x| s(x, "id")).collect();
    assert_eq!(ids, ["a", "b", "c"]);
}

#[test]
fn double_braces_read_as_one() {
    let t = "{\"a\": [{\"if\": [1], \"value\": {{\"x\": 2, \"s\": \"{{\"}}], \"b\": {\"c\": 1}}";
    let v: Value = json5::from_str(&merge_double_braces(t)).unwrap();
    assert_eq!(v["a"][0]["value"]["x"], 2);
    assert_eq!(v["a"][0]["value"]["s"], "{{");
    assert_eq!(v["b"]["c"], 1);
}

#[test]
fn lenient_json_commas() {
    let t = "{\"a\": [ {\"x\":1}\n {\"y\":\"s\"}\n ],\n \"b\": 2 // c\n , d: true\n e: 'q'\n f: 00, g: -007, h: 0.05, i: \"007\"\n}";
    let v: Value = json5::from_str(&add_missing_commas(t)).unwrap();
    assert_eq!(v["a"].as_array().unwrap().len(), 2);
    assert_eq!(v["d"], Value::Bool(true));
    assert_eq!(v["e"], Value::String("q".into()));
    assert_eq!((v["f"].as_i64(), v["g"].as_i64()), (Some(0), Some(-7)));
    assert_eq!(
        (v["h"].as_f64(), &v["i"]),
        (Some(0.05), &Value::String("007".into()))
    );
    // already valid JSON is unchanged
    let ok = "{\"a\": 1,\n \"b\": [1,\n 2]}";
    assert_eq!(add_missing_commas(ok), ok);
}

#[test]
fn builtin_result_skin_loads() {
    let path = crate::scene::skinned::default_skin(SkinType::Result, false).unwrap();
    let sk = load(&path, &HashMap::new(), &crate::skin::image_size).unwrap();
    assert_eq!(sk.kind, SkinType::Result);
    assert!(sk.images.iter().all(Option::is_some));
    let graphs = sk
        .objects
        .iter()
        .filter(|o| {
            matches!(
                &o.kind,
                Kind::Graph(Graph::Gauge(GaugeGraph { colors: None, .. }))
                    | Kind::Graph(Graph::Timing(TimingGraph { colors: None, .. }))
            )
        })
        .count();
    // our own skin asks for the built-in graph look
    assert_eq!(graphs, 2);
    assert!(sk
        .objects
        .iter()
        .any(|o| matches!(&o.kind, Kind::ImageSet { srcs, id: 370 } if srcs.len() == 11)));
}

#[test]
fn graph_colors() {
    let g: Value = json5::from_str(r#"{"id": "g", "exHardBGColor": "102030"}"#).unwrap();
    let c = gauge_colors(&g);
    assert_eq!(c.len(), 24);
    // normal gauge: line above the border red, below green (defaults)
    assert_eq!(c[8], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(c[10], [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(c[4 * 4 + 1], hex_color("102030"));
    let list: Vec<String> = (0..24).map(|k| format!("{:02x}000080", k)).collect();
    let g = serde_json::json!({ "color": list });
    assert_eq!(gauge_colors(&g)[5], [5.0 / 255.0, 0.0, 0.0, 128.0 / 255.0]);
}

#[test]
fn builtin_select_skins_load() {
    for portrait in [false, true] {
        let path = crate::scene::skinned::default_skin(SkinType::Select, portrait).unwrap();
        let sk = load(&path, &HashMap::new(), &crate::skin::image_size).unwrap();
        assert_eq!(sk.kind, SkinType::Select);
        let sl = sk.songlist.as_ref().expect("songlist");
        assert!(sl.off.len() > 10 && sl.off.len() == sl.on.len());
        // a body for each bar type, vibeBMS's too
        assert_eq!(
            sl.body.iter().flatten().count(),
            crate::skin::BAR_VIRTUAL + 1
        );
        assert!(sl.text.is_some() && sl.level.len() == 7 && sl.lamp.len() == 11);
        assert!(sl.folder_text.is_some() && sl.count.is_some() && sl.graph.is_some());
        // the IR lamp beside the player's
        assert!(sl.my_lamp.len() == 11 && sl.rival_lamp.len() == 11);
        // the tabs (stepped by halves), back, settings, the song's charts,
        // the buttons, the search box and the panel buttons are clickable
        let acts: std::collections::HashSet<_> = sk.objects.iter().filter_map(|o| o.act).collect();
        for act in [
            9900, 9009, 9910, 9914, 8, 9901, 16, 9902, 9903, 9905, 1, 3, 4, 5, 6, 7,
        ] {
            assert!(acts.contains(&(act, 0)), "{act}");
        }
        for tab in [10, 11, 12] {
            assert!(acts.contains(&(tab, 2)), "{tab}");
        }
        // each panel's page loads when the panel is first drawn
        assert_eq!(sk.lazy_images.len(), 8, "{:?}", sk.lazy_images);
        let path = crate::scene::skinned::default_skin(SkinType::Result, portrait).unwrap();
        assert!(load(&path, &HashMap::new(), &crate::skin::image_size).is_ok());
    }
}

/// No built-in image is over 2048 on a side: a GPU that allows no more
/// (WebGL2's least) would draw it scaled down and blurred.
#[test]
fn builtin_images_fit_small_gpus() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/builtin/skins/default");
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "png") {
            let (w, h) = image::image_dimensions(&p).unwrap();
            assert!(w <= 2048 && h <= 2048, "{}: {w}x{h}", p.display());
        }
    }
}

#[test]
fn builtin_default_skins_load() {
    use crate::scene::play_skin::default_skin;
    use vibebms_core::PlayMode;
    let size = |p: &Path| {
        let b = vfs::read(p).ok()?;
        image::ImageReader::new(std::io::Cursor::new(b))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    };
    for mode in PlayMode::ALL {
        for (p2, portrait, touch) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
            (true, false, true),
        ] {
            let path = default_skin(mode, p2, portrait, touch);
            let sk = load(&path, &HashMap::new(), &size)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(sk.kind.mode(), Some(mode), "{}", path.display());
            assert_eq!(sk.lanes.len(), mode.lane_count());
            assert!(sk
                .lanes
                .iter()
                .all(|l| l.rect.is_some() && l.note.is_some()));
            assert!(sk.images.iter().all(Option::is_some), "{}", path.display());
            assert!(sk.judges[0].judge.iter().all(Option::is_some));
            assert_eq!(sk.bar_lines.len(), mode.players(), "{}", path.display());
            if mode.is_double() {
                assert!(sk.judges[1].judge.iter().all(Option::is_some));
            }
        }
    }
}

#[test]
fn builtin_battle_skins_load() {
    use crate::scene::play_skin::battle_skin;
    use vibebms_core::PlayMode;
    for (mode, kind) in [
        (PlayMode::Beat14K, SkinType::Battle7),
        (PlayMode::Beat10K, SkinType::Battle5),
    ] {
        for touch in [false, true] {
            let path = battle_skin(mode, touch);
            let sk = load(&path, &HashMap::new(), &crate::skin::image_size)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(sk.kind, kind);
            assert_eq!(sk.kind.battle_pair().and_then(|t| t.mode()), Some(mode));
            assert_eq!(sk.lanes.len(), mode.lane_count());
            // a gauge for each side, a judge region for each
            let sides: Vec<usize> = sk
                .objects
                .iter()
                .filter_map(|o| match o.kind {
                    Kind::Gauge { side, .. } => Some(side),
                    _ => None,
                })
                .collect();
            assert_eq!(sides, vec![0, 1], "{}", path.display());
            assert!(sk.judges[1].judge.iter().all(Option::is_some));
        }
    }
}

#[test]
fn builtin_perspective_skins_load_and_reflow() {
    use crate::scene::play_skin::perspective_skin;
    use vibebms_core::PlayMode;
    let size = |p: &Path| {
        let b = vfs::read(p).ok()?;
        image::ImageReader::new(std::io::Cursor::new(b))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
    };
    for mode in PlayMode::ALL {
        for portrait in [false, true] {
            let path = perspective_skin(mode, portrait);
            let mut sk = load(&path, &HashMap::new(), &size)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(sk.perspective, "{}", path.display());
            // the touch zones are hidden unless the TOUCH AREA option shows them
            let area = crate::perspective::touch_area_opacity;
            assert_eq!(area(&sk.options_on, 0.85), 0.0, "{}", path.display());
            let half = load(&path, &HashMap::from([(962, true)]), &size).unwrap();
            assert_eq!(area(&half.options_on, 0.85), 0.5, "{}", path.display());
            assert_eq!(sk.kind.mode(), Some(mode));
            assert_eq!(sk.lanes.len(), mode.lane_count());
            assert_eq!(sk.width > sk.height, !portrait);
            assert!(sk.images.iter().all(Option::is_some), "{}", path.display());
            assert!(sk.objects.iter().any(|o| matches!(o.kind, Kind::Notes)));
            assert!(sk.objects.iter().all(|o| o.dst.anchor.is_some()));
            // START / SELECT's corners are left free
            let (w, h) = (sk.width, sk.height);
            for right in [false, true] {
                for [x, y, bw, bh] in crate::perspective::system_buttons(w, h, right) {
                    let b = Rect::new(x, y, bw, bh);
                    for o in &sk.objects {
                        let Some(k) = o.dst.keys.first() else {
                            continue;
                        };
                        let r = k.rect;
                        let full = r.w >= w && r.h >= h;
                        let apart = r.right() <= b.x
                            || r.x >= b.right()
                            || r.bottom() <= b.y
                            || r.y >= b.bottom();
                        assert!(full || apart, "{}: {r:?}", path.display());
                    }
                }
            }
            // the panels around the lanes stay clear of them on the
            // screens the skin is reflowed to
            let aspects: &[f32] = if portrait {
                &[9.0 / 16.0, 9.0 / 19.5, 3.0 / 4.0]
            } else {
                &[16.0 / 9.0, 19.5 / 9.0, 4.0 / 3.0, 16.0 / 10.0, 2.4]
            };
            for &aspect in aspects {
                let (rw, rh) = crate::perspective::reflowed_size(w, h, aspect);
                let mut re = sk.clone();
                re.reflow(rw, rh);
                // the scratch on either side (1P, 2P)
                let fans: Vec<_> = [false, true]
                    .into_iter()
                    .flat_map(|side2| {
                        crate::perspective::layout(mode, side2, rw, rh, &Default::default())
                    })
                    .collect();
                for o in &re.objects {
                    let (Some(k), Some(a)) = (o.dst.keys.first(), o.dst.anchor) else {
                        continue;
                    };
                    let r = k.rect;
                    // the background, and what is shown over the lanes
                    // (judgements, READY, the hi-speed panel)
                    if r.w >= rw || a[1] == 0.5 || o.dst.timer != 0 {
                        continue;
                    }
                    for f in &fans {
                        let yb = r.bottom().min(f.bottom);
                        let Some(d) = f.depth(yb).filter(|_| yb > f.y(1.0)) else {
                            continue;
                        };
                        let (x0, x1) = (f.point(0.0, d)[0], f.point(1.0, d)[0]);
                        assert!(
                            r.right() <= x0 || r.x >= x1,
                            "{} at {rw}x{rh}: {r:?} over the lanes ({x0}..{x1} at {yb})",
                            path.display()
                        );
                    }
                }
            }
            // a taller screen: the background stretches, the bottom
            // half's objects come down with the lanes
            let bg = sk.objects[0].dst.keys[0].rect;
            let judge = sk.judges[0].judge[0].as_ref().unwrap().1.keys[2].rect;
            sk.reflow(w, h + 200.0);
            assert_eq!(sk.height, h + 200.0);
            let bg2 = sk.objects[0].dst.keys[0].rect;
            assert_eq!((bg2.y, bg2.h), (bg.y, bg.h + 200.0));
            let judge2 = sk.judges[0].judge[0].as_ref().unwrap().1.keys[2].rect;
            assert_eq!(judge2.y, judge.y + 100.0);
        }
    }
}

const JSON: &str = r#"{
  // comments and trailing commas are accepted like beatoraja's parser
  type: 0, name: "JS", w: 1280, h: 720,
  property: [{ name: "Lane", item: [{name: "A", op: 900}, {name: "B", op: 901}], def: "B" }],
  source: [{ id: "s", path: "p*.png" }],
  image: [
    { id: "bg", src: "s", x: 0, y: 0, w: 10, h: 10 },
    { id: "n1", src: "s", x: 0, y: 0, w: 4, h: 2 },
    { id: "j0", src: "s", x: 0, y: 0, w: 8, h: 4 },
  ],
  value: [{ id: "v", src: "s", x: 0, y: 0, w: 100, h: 10, divx: 10, digit: 4, ref: 101 }],
  note: { id: "notes", note: ["n1","n1","n1","n1","n1","n1","n1","n1"], dst: [
    {x:10,y:100,w:20,h:500},{x:30,y:100,w:20,h:500},{x:50,y:100,w:20,h:500},{x:70,y:100,w:20,h:500},
    {x:90,y:100,w:20,h:500},{x:110,y:100,w:20,h:500},{x:130,y:100,w:20,h:500},{x:150,y:100,w:20,h:500}] },
  judge: [{ id: "judge", index: 0, images: [{ id: "j0", loop: -1, timer: 46, dst: [{x: 0, y: 300, w: 80, h: 40}] }] }],
  destination: [
    { if: [900], value: { id: "bg", dst: [{ x: 0, y: 0, w: 1280, h: 720 }] } },
    { if: [901], value: { id: "bg", dst: [{ time: 0, x: 0, y: 0, w: 640, h: 360 }, { time: 500, x: 100 }] } },
    { id: "notes" },
    { id: "v", op: [32], dst: [{ x: 20, y: 20, w: 16, h: 24 }] },
    { id: "judge" },
  ],
}"#;

#[test]
fn loads_json_skin() {
    let dir = std::env::temp_dir().join(format!("vibebms-bskin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("play.json"), JSON).unwrap();
    std::fs::write(dir.join("parts.png"), b"x").unwrap();
    let sz = |_: &Path| Some((256, 256));
    let s = load(&dir.join("play.json"), &HashMap::new(), &sz).unwrap();
    assert_eq!(s.kind, SkinType::Play7);
    assert!(s.images[0].as_ref().unwrap().ends_with("parts.png"));
    // def "B" (901) selected: animated bg, inherits y/w/h in the 2nd key, y flipped
    let bg = &s.objects[0];
    assert_eq!(bg.dst.keys.len(), 2);
    assert_eq!(bg.dst.keys[1].rect, Rect::new(100.0, 360.0, 640.0, 360.0));
    assert!(matches!(s.objects[1].kind, Kind::Notes));
    assert_eq!(
        s.lanes[0].rect.unwrap(),
        Rect::new(10.0, 120.0, 20.0, 500.0)
    );
    let Kind::Number { id, font } = &s.objects[2].kind else {
        panic!("{:?}", s.objects[2].kind)
    };
    assert_eq!((*id, font.style.keta, font.style.digits), (101, 4, 10));
    assert_eq!(s.objects[2].dst.ops, vec![32]);
    assert!(s.judges[0].judge[0].is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bmfont_texts() {
    let dir = std::env::temp_dir().join(format!("vibebms-bmf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fnt = "info face=\"My Font\" size=-32 bold=0\ncommon lineHeight=36 base=29 pages=1\n\
page id=0 file=\"f_0.png\"\nchars count=1\n\
char id=65 x=2 y=3 width=20 height=24 xoffset=1 yoffset=5 xadvance=22 page=0 chnl=15\n";
    std::fs::write(dir.join("f.fnt"), fnt).unwrap();
    let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
        "font": [{"id": 0, "path": "f.fnt"}],
        "text": [{"id": "t", "ref": 10, "font": 0, "size": 24}],
        "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
    std::fs::write(dir.join("r.json"), json).unwrap();
    let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((64, 64))).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let Kind::Text { style, .. } = &s.objects[0].kind else {
        panic!("{:?}", s.objects[0].kind)
    };
    assert_eq!((style.font, style.size), (Some(0), Some(24.0)));
    let f = &s.fonts[0];
    assert_eq!((f.size, f.em), (36.0, 32.0));
    // glyphs scale by the text's size over the font's (beatoraja), not
    // by the destination height over the line height
    use crate::skin::render::bmfont_scale;
    assert_eq!(bmfont_scale(f, 10.0, Some(24.0)), 0.75);
    assert_eq!(bmfont_scale(f, 18.0, None), 0.5);
    let lr2 = BitmapFont {
        size: 20.0,
        ..Default::default()
    };
    assert_eq!(bmfont_scale(&lr2, 10.0, Some(24.0)), 0.5);
    let (img, rect) = f.glyphs[&'A'];
    assert!(s.images[img].as_ref().unwrap().ends_with("f_0.png"));
    assert_eq!(
        (rect, f.metrics[&'A']),
        ([2.0, 3.0, 20.0, 24.0], [1.0, 5.0, 22.0])
    );
}

#[test]
fn text_outline_and_shadow() {
    let dir = std::env::temp_dir().join(format!("vibebms-tfx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fnt = "info face=\"F\" size=32\ncommon lineHeight=32 base=29 pages=1\n\
page id=0 file=\"f_0.png\"\nchar id=65 x=0 y=0 width=20 height=24 xoffset=0 yoffset=0 xadvance=22 page=0\n";
    std::fs::write(dir.join("d.fnt"), fnt).unwrap();
    let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
        "font": [{"id": 0, "path": "f.ttf"}, {"id": 1, "path": "d.fnt", "type": 1}],
        "text": [{"id": "t", "ref": 10, "font": 0, "outlineColor": "000000ff",
                  "outlineWidth": 0.4, "shadowColor": "ff0000", "shadowOffsetX": 4,
                  "shadowOffsetY": -2.5, "shadowSmoothness": 0.5},
                 {"id": "u", "ref": 10, "font": 1}],
        "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]},
                        {"id": "u", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
    std::fs::write(dir.join("r.json"), json).unwrap();
    let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((64, 64))).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let Kind::Text { style, .. } = &s.objects[0].kind else {
        panic!()
    };
    let fx = &style.fx;
    assert_eq!(
        *fx,
        TextFx {
            outline_color: [0.0, 0.0, 0.0, 1.0],
            outline_width: 0.4,
            shadow_color: [1.0, 0.0, 0.0, 1.0],
            shadow_offset: (4.0, -2.5),
            shadow_smoothness: 0.5,
        }
    );
    assert!(fx.has_shadow());
    let st = fx.sdf();
    assert!((st.outline_distance - 0.3).abs() < 1e-6);
    assert_eq!(st.shadow_smoothing, 0.25);
    // nothing given: beatoraja's defaults (transparent white, no shadow)
    let Kind::Text { style, .. } = &s.objects[1].kind else {
        panic!()
    };
    let (fx, font) = (&style.fx, style.font);
    assert_eq!(*fx, TextFx::default());
    assert!(!fx.has_shadow());
    assert_eq!(fx.sdf().outline_distance, 0.5);
    // a font of type 1 is a distance field
    assert!(s.fonts[font.unwrap()].sdf);
}

#[test]
fn constant_texts() {
    let dir = std::env::temp_dir().join(format!("vibebms-ctext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let json = r#"{"type": 7, "name": "T", "w": 100, "h": 100,
        "font": [{"id": 0, "path": "f.TTF"}, {"id": 1, "path": "b.fnt"}, {"id": 2, "path": "big.ttf"}],
        "text": [{"id": "t", "constantText": "A\nB", "ref": 10, "font": 0},
                 {"id": "u", "ref": 10, "font": 1}, {"id": "v", "ref": 10, "font": 2}],
        "destination": [{"id": "t", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]},
                        {"id": "u", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]},
                        {"id": "v", "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]}"#;
    std::fs::write(dir.join("r.json"), json).unwrap();
    // (larger than a font a skin brings is read)
    let big = std::fs::File::create(dir.join("big.ttf")).unwrap();
    big.set_len(crate::gfx::text::FONT_MAX + 1).unwrap();
    let s = load(&dir.join("r.json"), &HashMap::new(), &|_| Some((1, 1))).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let Kind::Text {
        constant, style, ..
    } = &s.objects[0].kind
    else {
        panic!()
    };
    let face = &style.face;
    assert_eq!(constant.as_deref(), Some("A\nB"));
    // TrueType fonts are drawn with, bitmap ones are not, nor one too large
    assert!(face.as_ref().is_some_and(|p| p.ends_with("f.TTF")));
    for o in &s.objects[1..] {
        assert!(matches!(&o.kind, Kind::Text { style, .. } if style.face.is_none()));
    }
}

struct ScriptSt {
    flag: bool,
}
impl SkinState for ScriptSt {
    fn timer(&self, id: i32) -> Option<i64> {
        (id == 41).then_some(500)
    }
    fn option(&self, id: i32) -> bool {
        id == 5 && self.flag
    }
    fn number(&self, id: i32) -> Option<i64> {
        (id == 71).then_some(21)
    }
    fn text(&self, _: i32) -> Option<String> {
        None
    }
    fn rate(&self, _: i32) -> f32 {
        0.0
    }
}

#[test]
fn lua_skin_scripts() {
    use crate::skin::script::Overlay;
    let dir = std::env::temp_dir().join(format!("vibebms-lscript-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lua = r#"
        local main_state = require("main_state")
        local timer_util = require("timer_util")
        local s = { type = 0, name = "S", w = 100, h = 100 }
        if not skin_config then return s end
        s.source = { { id = 1, path = "a.png" } }
        s.image = { { id = "img", src = 1, x = 0, y = 0, w = 10, h = 10, act = function() main_state.event_exec(15) end } }
        -- a stray positional entry must not turn the table into an array
        s.image[2] = { id = "img2", src = 1, x = 0, y = 0, w = 10, h = 10, 12345 }
        s.value = { { id = "num", src = 1, x = 0, y = 0, w = 100, h = 10, divx = 10, digit = 3,
                      value = function() return main_state.number(71) * 2 end } }
        s.text = { { id = "txt", font = 0, size = 10, value = function() return "ex " .. main_state.number(71) end } }
        local on = timer_util.timer_observe_boolean(function() return main_state.option(5) end)
        s.destination = {
          { id = "img", timer = on, dst = { { x = 0, y = 0, w = 10, h = 10 } } },
          { id = "num", draw = function() return main_state.timer_is_on(41) end, dst = { { x = 0, y = 0, w = 10, h = 10 } } },
          { id = "txt", dst = { { x = 0, y = 0, w = 10, h = 10 } } },
        }
        return s
    "#;
    std::fs::write(dir.join("play.luaskin"), lua).unwrap();
    let sz = |_: &Path| Some((100, 20));
    let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
    let raw = read(
        &dir.join("play.luaskin"),
        Some((
            &HashMap::new(),
            &Header::of(&Value::Null),
            &HashMap::new(),
            &HashMap::new(),
        )),
        None,
    )
    .unwrap()
    .0;
    assert_eq!(raw["image"][1]["id"], "img2");
    // what the sandbox keeps out
    let escape = r#"
        local ok = os.execute == nil and os.remove == nil and os.getenv == nil
            and io.popen("ls") == nil and io.open("/etc/passwd") == nil
            and io.open("../../x.txt") == nil and debug.sethook == nil and string.dump == nil
            and package.loadlib == nil and package.searchpath == nil
            and load("\27Lua", "b", "b") == nil
        package.path = "/etc/?"
        return ok and not pcall(require, "passwd") and not pcall(dofile, "../../x.lua")
            and loadfile("/etc/passwd") == nil and loadfile("../../x.lua") == nil
            and loadfile("play.luaskin") ~= nil"#;
    std::fs::write(dir.join("esc.luaskin"), escape).unwrap();
    let (v, _) = read(&dir.join("esc.luaskin"), None, None).unwrap();
    assert_eq!(v, Value::Bool(true));
    // endless loops are stopped
    std::fs::write(dir.join("loop.luaskin"), "while true do end").unwrap();
    assert!(read(&dir.join("loop.luaskin"), None, None).is_err());
    std::fs::remove_dir_all(&dir).ok();
    let rt = s.script.clone().expect("scripts");
    let (img, num, txt) = (&s.objects[0], &s.objects[1], &s.objects[2]);
    let Kind::Number { id: num_id, .. } = num.kind else {
        panic!("number")
    };
    let Kind::Text { id: txt_id, .. } = txt.kind else {
        panic!("text")
    };
    let st = ScriptSt { flag: false };
    rt.update(&st, 100);
    let sc = Overlay {
        st: &st,
        rt: Some(&rt),
        offsets: &HashMap::new(),
    };
    assert_eq!(sc.number(num_id), Some(42));
    assert_eq!(sc.text(txt_id).as_deref(), Some("ex 21"));
    assert!(num.dst.ops.iter().all(|o| sc.option(*o)));
    assert_eq!(sc.timer(img.dst.timer), None);
    // the observed condition turns on at 300 ms: the timer starts there
    let st = ScriptSt { flag: true };
    rt.update(&st, 300);
    rt.update(&st, 400);
    let sc = Overlay {
        st: &st,
        rt: Some(&rt),
        offsets: &HashMap::new(),
    };
    assert_eq!(sc.timer(img.dst.timer), Some(300));
    // click: the script runs on the next frame and asks for event 15
    let (act, _) = img.act.unwrap();
    assert!(rt.click(act));
    rt.update(&st, 500);
    assert_eq!(rt.take_events(), vec![15]);
}

#[test]
fn lua_skin_config_files() {
    let root = std::env::temp_dir().join(format!("vibebms-lcfg-{}", std::process::id()));
    let dir = root.join("play");
    std::fs::create_dir_all(dir.join("judge")).unwrap();
    for f in ["a.png", "b.png"] {
        std::fs::write(dir.join("judge").join(f), b"x").unwrap();
    }
    std::fs::write(dir.join("judge/b.lua"), "return { w = 77 }").unwrap();
    std::fs::write(root.join("outside.txt"), "return 1").unwrap();
    let lua = r#"
        local header = { type = 0, name = "C", w = 100, h = 100,
          filepath = { { name = "Judge", path = "judge/*.png", def = "b" } },
          offset = { { name = "Lift", id = 40, x = false, y = true } } }
        if not skin_config then return header end
        local o = skin_config.offset["Lift"]
        local p = string.match(skin_config.get_path("judge/" .. skin_config.file_path["Judge"]), "(.+)%.png$") .. ".lua"
        local ok, setting = pcall(dofile, p)
        local escaped = pcall(dofile, "../outside.txt")
        header.source = { { id = 1, path = "judge/*.png" } }
        header.image = { { id = "i", src = 1, x = 0, y = 0, w = -1, h = -1 } }
        header.destination = { { id = "i", dst = { { x = o.y, y = 0, w = setting.w, h = escaped and 1 or 2 } } } }
        return header
    "#;
    std::fs::write(dir.join("play.luaskin"), lua).unwrap();
    let sz = |_: &Path| Some((10, 10));
    let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
    let r = s.objects[0].dst.keys[0].rect;
    assert_eq!((r.x, r.w, r.h), (0.0, 77.0, 2.0));
    // the user's offset values reach the script and the skin
    let mut offs = HashMap::new();
    offs.insert("Lift".to_string(), [0, 7, 0, 0, 0, 0]);
    let user = UserSettings {
        offsets: offs,
        ..Default::default()
    };
    let s2 = load_with(&dir.join("play.luaskin"), &dir, &user, None, &sz).unwrap();
    assert_eq!(s2.objects[0].dst.keys[0].rect.x, 7.0);
    assert_eq!(s2.offsets[&40], [0.0, 7.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(custom_offsets(&dir.join("play.luaskin"), &dir).len(), 1);
    std::fs::remove_dir_all(&root).ok();
    // the wildcard source follows the filepath default too
    assert!(s.images[0].as_ref().unwrap().ends_with("judge/b.png"));
}

#[test]
fn a_lua_skin_runs_once_without_settings() {
    let dir = std::env::temp_dir().join(format!("vibebms-lonce-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // a run without settings names its property at random
    let lua = r#"
        local s = { type = 0, name = "O", w = 100, h = 100, property = {
          { name = skin_config and "x" or tostring(math.random()), item = { { name = "a", op = 900 } } } } }
        if not skin_config then return s end
        s.source = { { id = 1, path = "a.png" } }
        s.image = { { id = "i", src = 1, x = 0, y = 0, w = 1, h = 1 } }
        s.destination = { { id = "i", dst = { { x = 0, y = 0, w = 1, h = 1 } } } }
        return s
    "#;
    let p = dir.join("play.luaskin");
    std::fs::write(&p, lua).unwrap();
    let name = custom_options(&p, &dir)[0].name.clone();
    let sz = |_: &Path| Some((1, 1));
    let s = load(&p, &HashMap::new(), &sz).unwrap();
    assert_eq!(s.custom_options[0].name, name);
    assert_eq!(header(&p, &dir), Some((SkinType::Play7, "O".into())));
    assert_eq!(custom_options(&p, &dir)[0].name, name);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn json_skin_scripts() {
    use crate::skin::script::Overlay;
    let dir = std::env::temp_dir().join(format!("vibebms-jscript-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let json = r#"{
        "type": 0, "name": "J", "w": 100, "h": 100,
        "source": [{"id": 1, "path": "a.png"}],
        "value": [{"id": "n", "src": 1, "x": 0, "y": 0, "w": 100, "h": 10, "divx": 10, "digit": 3,
                   "value": "number(71) + 1"}],
        "customTimers": [{"id": 10001, "timer": "timer_observe_boolean(function() return option(5) end)"}],
        "destination": [{"id": "n", "op": [32, "option(5)"], "timer": 10001,
                         "dst": [{"x": 0, "y": 0, "w": 10, "h": 10}]}]
    }"#;
    std::fs::write(dir.join("play.json"), json).unwrap();
    let sz = |_: &Path| Some((100, 20));
    let s = load(&dir.join("play.json"), &HashMap::new(), &sz).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    let rt = s.script.clone().expect("scripts");
    let o = &s.objects[0];
    let Kind::Number { id, .. } = o.kind else {
        panic!("number")
    };
    assert_eq!(o.dst.ops.len(), 2);
    assert_eq!(o.dst.ops[0], 32);
    let st = ScriptSt { flag: true };
    rt.update(&st, 250);
    let sc = Overlay {
        st: &st,
        rt: Some(&rt),
        offsets: &HashMap::new(),
    };
    assert_eq!(sc.number(id), Some(22));
    assert!(sc.option(o.dst.ops[1]));
    assert_eq!(sc.timer(10001), Some(250));
    let st = ScriptSt { flag: false };
    rt.update(&st, 300);
    let sc = Overlay {
        st: &st,
        rt: Some(&rt),
        offsets: &HashMap::new(),
    };
    assert!(!sc.option(o.dst.ops[1]));
    assert_eq!(sc.timer(10001), None);
}

#[test]
fn loads_lua_skin() {
    let dir = std::env::temp_dir().join(format!("vibebms-lskin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lua = r#"
        local main_state = require("main_state")
        local header = { type = 0, name = "LuaSkin", w = 1280, h = 720,
          property = { { name = "Mode", item = { { name = "X", op = 910 }, { name = "Y", op = 911 } } } } }
        if not skin_config then return header end
        header.source = { { id = 1, path = "a.png" } }
        header.image = { { id = "bg", src = 1, x = 0, y = 0, w = -1, h = -1 } }
        local dst = { { x = 0, y = 0, w = 1280, h = 720 } }
        if skin_config.option["Mode"] == 911 then dst[1].w = 10 end
        header.destination = { { id = "bg", dst = dst, draw = function() return main_state.option(1) end } }
        return header
    "#;
    std::fs::write(dir.join("play.luaskin"), lua).unwrap();
    let sz = |_: &Path| Some((64, 32));
    assert_eq!(
        header(&dir.join("play.luaskin"), &dir).unwrap().1,
        "LuaSkin"
    );
    let s = load(&dir.join("play.luaskin"), &HashMap::new(), &sz).unwrap();
    assert_eq!(s.objects.len(), 1);
    assert_eq!(s.objects[0].dst.keys[0].rect.w, 1280.0);
    let mut opt = HashMap::new();
    opt.insert(911, true);
    let s = load(&dir.join("play.luaskin"), &opt, &sz).unwrap();
    assert_eq!(s.objects[0].dst.keys[0].rect.w, 10.0);
    std::fs::remove_dir_all(&dir).ok();
}

/// A data root with two themes in its skins folder and, outside it,
/// files a skin has no business with: (root, skins folder).
fn skins_and_private_files(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("vibebms-{name}-{}", std::process::id()));
    let skins = root.join("skins");
    for f in [
        "skins/Theme/parts/a.png",
        "skins/Theme/play/b.png",
        "skins/Other/o.png",
        "config/x.png",
        "config/y.png",
        "config/secret.txt",
    ] {
        std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
        std::fs::write(root.join(f), b"x").unwrap();
    }
    let object = r#"{"id": "a", "dst": [{"x": 0, "y": 0, "w": 1, "h": 1}]}"#;
    std::fs::write(root.join("config/more.json"), object).unwrap();
    std::fs::write(root.join("skins/Theme/more.json"), object).unwrap();
    (root, skins)
}

/// The files a JSON skin names are files of the skins folder (another
/// theme's too): an image, an include or a choice of files outside it is
/// as if it were missing.
#[test]
fn a_json_skin_reads_the_skins_folder_only() {
    let (root, skins) = skins_and_private_files("jtheme");
    let abs = root.join("config/x.png");
    let json = serde_json::json!({
        "type": 0, "name": "T", "w": 100, "h": 100,
        "filepath": [{"name": "Up", "path": "../../../config/*.png", "def": ""}],
        "source": [
            {"id": 0, "path": "../parts/a.png"},
            {"id": 1, "path": "../../Other/o.png"},
            {"id": 2, "path": "../../../config/x.png"},
            {"id": 3, "path": abs.to_str().unwrap()},
            {"id": 4, "path": "../../../config/*.png"},
            {"id": 5, "path": "\\\\host\\share\\a.png"},
            {"id": 6, "path": "//host/share/a.png"},
        ],
        "image": [{"id": "a", "src": 0, "x": 0, "y": 0, "w": 1, "h": 1}],
        "destination": [
            {"include": "../more.json"},
            {"include": "../../../config/more.json"},
            {"include": root.join("config/more.json").to_str().unwrap()},
            {"include": "\\\\host\\share\\a.json"},
        ],
    });
    let path = skins.join("Theme/play/play.json");
    std::fs::write(&path, json.to_string()).unwrap();
    let user = UserSettings::default();
    let s = load_with(&path, &skins, &user, None, &|_| Some((1, 1))).unwrap();
    let options = custom_options(&path, &skins);
    std::fs::remove_dir_all(&root).ok();
    assert_eq!(s.images[0], Some(skins.join("Theme/parts/a.png")));
    assert_eq!(s.images[1], Some(skins.join("Other/o.png")));
    assert_eq!(s.images.len(), 7);
    assert!(s.images[2..].iter().all(Option::is_none));
    // the include from the skins folder, and no other
    assert_eq!(s.objects.len(), 1);
    assert!(options.is_empty());
}

/// A Lua skin's files, and what its scripts open or list, are files of
/// the skins folder (another theme's too), wherever in it the skin file
/// is: never of the data root above it.
#[test]
fn a_lua_skin_reads_the_skins_folder_only() {
    let (root, skins) = skins_and_private_files("ltheme");
    let lua = r#"
        local t = { type = 0, name = "L", w = 100, h = 100 }
        if not skin_config then return t end
        local listed = skin_config.get_path("../../../config/*.txt")
            .. skin_config.get_path("ROOT/config/*.txt")
            .. skin_config.get_path("\\\\host\\share\\*.txt")
        local opened = io.open("../../../config/secret.txt") or io.open("ROOT/config/secret.txt")
        local shared = io.open("../../Other/o.png") ~= nil and io.open("skin/Theme/parts/a.png") ~= nil
        t.name = listed .. "|" .. tostring(opened) .. "|" .. tostring(shared) .. "|"
            .. skin_config.get_path("../../Other/*.png")
        t.source = { { id = 0, path = "b.png" } }
        t.image = { { id = "a", src = 0, x = 0, y = 0, w = 1, h = 1 } }
        t.destination = { { id = "a", dst = { { x = 0, y = 0, w = 1, h = 1 } } } }
        return t"#
        .replace("ROOT", root.to_str().unwrap());
    let path = skins.join("Theme/play/play.luaskin");
    std::fs::write(&path, lua).unwrap();
    let user = UserSettings::default();
    let s = load_with(&path, &skins, &user, None, &|_| Some((1, 1))).unwrap();
    let other = skins.join("Other/o.png");
    assert_eq!(s.name, format!("|nil|true|{}", other.display()));

    let loose = skins.join("loose.luaskin");
    let lua = r#"return { type = 0, name = tostring(io.open("../config/secret.txt")) .. "|"
        .. tostring(io.open("Theme/parts/a.png") ~= nil) }"#;
    std::fs::write(&loose, lua).unwrap();
    let h = header(&loose, &skins);
    std::fs::remove_dir_all(&root).ok();
    assert_eq!(h, Some((SkinType::Play7, "nil|true".into())));
}

/// A bitmap font's pages are files of the skins folder.
#[test]
fn bmfont_pages_stay_in_the_skins_folder() {
    let (root, skins) = skins_and_private_files("bmtheme");
    let fnt = "info size=32\ncommon lineHeight=36\npage id=0 file=\"../parts/a.png\"\n\
page id=1 file=\"../../../config/x.png\"\npage id=2 file=\"\\\\host\\share\\a.png\"\n\
char id=65 x=0 y=0 width=1 height=1 page=0\n";
    let path = skins.join("Theme/play/f.fnt");
    std::fs::write(&path, fnt).unwrap();
    let (_, pages) = files::load_bmfont(&path, &Home::of(&path, &skins)).unwrap();
    std::fs::remove_dir_all(&root).ok();
    assert_eq!(
        pages,
        [
            skins.join("Theme/parts/a.png"),
            PathBuf::new(),
            PathBuf::new()
        ]
    );
}

/// Text nested deeper than any skin is refused, not read (json5 would
/// run out of stack); brackets in strings and comments open nothing.
#[test]
fn json_nested_too_deep_is_refused() {
    let nested = |n: usize| format!("{}1{}", "[".repeat(n), "]".repeat(n));
    assert!(json::parse(&nested(128)).is_ok());
    assert!(json::parse(&nested(129)).is_err());
    assert!(json::parse(&"[".repeat(200_000)).is_err());
    assert!(json::parse(&"{a:".repeat(200_000)).is_err());
    let quiet = format!(
        "// {0}\n/* {0} */ ['{0}', \"{0}\\\"{0}\", /* ]* */ {1}]",
        "[{".repeat(200),
        nested(127)
    );
    assert!(json::parse(&quiet).is_ok());
    // a comment or a string that only looks like one hides nothing
    for cover in ["'//'", "'/*'", "'\\'//'", "1 // '\r", "1 /* ' */", "'\\\\'"] {
        let hidden = format!("[{cover}, {}]", nested(128));
        assert!(json::parse(&hidden).is_err(), "{cover}");
    }
    // a skin file of such text has no header and does not load
    let dir = std::env::temp_dir().join(format!("vibebms-jdeep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("deep.json");
    std::fs::write(
        &path,
        format!("{{\"type\": 0, \"x\": {}}}", "[".repeat(200_000)),
    )
    .unwrap();
    let h = header(&path, &dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(h, None);
}

/// A Lua skin's code that takes longer than a listing gives it loads: its
/// first run, for its header, has a load's time too.
#[test]
fn a_lua_skin_loads_in_a_loads_time() {
    let dir = std::env::temp_dir().join(format!("vibebms-lload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.luaskin");
    std::fs::write(
        &path,
        "for i = 1, 1e8 do end
         return { type = 7, name = 'S', w = 100, h = 100, font = { { id = 0, path = 'f.ttf' } },
           text = { { id = 't', constantText = 'A', ref = 10, font = 0 } },
           destination = { { id = 't', dst = { { x = 0, y = 0, w = 10, h = 10 } } } } }",
    )
    .unwrap();
    let s = load(&path, &HashMap::new(), &|_| None);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(s.unwrap().name, "S");
}

/// A Lua skin that never ends is given up on in the time a header has
/// (far less than a load's), and a listing of the skins does not wait for
/// every such skin.
#[test]
fn a_lua_skin_that_does_not_end_does_not_hold_the_listing() {
    let dir = std::env::temp_dir().join(format!("vibebms-lslow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skins: Vec<PathBuf> = (0..3).map(|i| dir.join(format!("{i}.luaskin"))).collect();
    for p in &skins {
        std::fs::write(p, "while true do end").unwrap();
    }
    let t = std::time::Instant::now();
    assert!(read(&skins[0], None, None).is_err());
    assert!(t.elapsed() < script::LOAD_TIME);
    let read: fn(&PathBuf) -> bool = |p| crate::skin::header_read(p).is_some();
    assert!(crate::skin::find_skins(&dir).is_empty());
    assert_eq!(
        skins.iter().map(read).collect::<Vec<_>>(),
        [true, false, false]
    );
    for _ in 0..2 {
        crate::skin::find_skins(&dir);
    }
    assert!(skins.iter().all(read));
    std::fs::remove_dir_all(&dir).ok();
}

/// A skin's own numbers for what is drawn every frame, and what it asks
/// for in all (included files, animation frames), are within what a skin
/// uses.
#[test]
fn what_a_skin_asks_for_is_within_limits() {
    let dir = std::env::temp_dir().join(format!("vibebms-blimits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let skin = |more: &str, destinations: &str| {
        format!(
            r#"{{"type": 0, "name": "N", "w": 1280, "h": 720,
              "source": [{{"id": 0, "path": "a.png"}}],
              "image": [{{"id": "cell", "src": 0, "x": 0, "y": 0, "w": 8, "h": 8}},
                        {{"id": "film", "src": 0, "x": 0, "y": 0, "w": 8, "h": 8,
                          "divx": 1024, "divy": 64}}],
              {more}
              "destination": [{destinations}]}}"#
        )
    };
    let at =
        |id: &str| format!(r#"{{"id": "{id}", "dst": [{{"x": 0, "y": 0, "w": 100, "h": 10}}]}}"#);
    let load = |text: String| {
        std::fs::write(dir.join("play.json"), text).unwrap();
        load(&dir.join("play.json"), &HashMap::new(), &|_| Some((64, 64)))
    };
    let numbers = r#""gauge": {"id": "gauge", "nodes": ["cell", "cell", "cell", "cell"],
                              "parts": 4000000000},
                    "timingvisualizer": [{"id": "vis", "judgeWidthMillis": 300000000}],
                    "timingdistributiongraph": [{"id": "dist", "width": 4000000000}],"#;
    let s = load(skin(
        numbers,
        &[at("gauge"), at("vis"), at("dist")].join(","),
    ))
    .unwrap();
    let seen: Vec<String> = (s.objects.iter())
        .filter_map(|o| match &o.kind {
            Kind::Gauge { parts, .. } => Some(format!("gauge {parts}")),
            Kind::Graph(Graph::Visualizer(g)) => Some(format!("visualizer {}", g.range_ms)),
            Kind::Graph(Graph::Timing(g)) => Some(format!("timing {}", g.bins)),
            _ => None,
        })
        .collect();
    assert_eq!(seen, ["gauge 1000", "visualizer 2000", "timing 2001"]);
    // 65,536 frames an object: 16 objects of them, not 17
    let films = |n: usize| vec![at("film"); n].join(",");
    assert!(load(skin("", &films(16))).is_ok());
    let e = load(skin("", &films(17))).unwrap_err();
    assert!(e.contains("too many animation frames"), "{e}");
    // a file that includes itself: twice at each of the 8 levels, not four
    // times
    let include = r#"{"include": "more.json"}"#;
    let more = |n: usize| format!("[{}]", vec![include; n].join(","));
    let with_more = skin(&format!(r#""more": {include},"#), &at("cell"));
    std::fs::write(dir.join("more.json"), more(2)).unwrap();
    assert!(load(with_more.clone()).is_ok());
    std::fs::write(dir.join("more.json"), more(4)).unwrap();
    let e = load(with_more).unwrap_err();
    assert!(e.contains("too many includes"), "{e}");
    std::fs::remove_dir_all(&dir).ok();
}
