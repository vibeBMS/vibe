use super::*;

/// [`load_with`] with only options chosen.
fn load(
    path: &Path,
    skins_root: &Path,
    options: &HashMap<i32, bool>,
    static_option: &dyn Fn(i32) -> Option<bool>,
    image_size: &dyn Fn(&Path) -> Option<(u32, u32)>,
) -> Result<Skin, String> {
    let user = UserSettings {
        options: options.clone(),
        ..Default::default()
    };
    load_with(path, skins_root, &user, static_option, image_size)
}

#[test]
fn continue_goes_on_with_the_play_flow_images() {
    let dir = std::env::temp_dir().join(format!("vibebms-lr2cont-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let load = |name: &str, text: &str| {
        let p = dir.join(name);
        let obj = "#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n#DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n";
        std::fs::write(&p, format!("{text}{obj}")).unwrap();
        let user = UserSettings::default();
        load_with(&p, &dir, &user, &|_| None, &|_| Some((1, 1))).unwrap()
    };
    let play = load("play.lr2skin", "#INFORMATION,0,P,me\n#IMAGE,bg.png\n");
    keep_images(&play);
    // a screen outside the flow (key config) in between
    load("keys.lr2skin", "#INFORMATION,8,K,me\n#IMAGE,keys.png\n");
    let result = load("result.lr2skin", "#INFORMATION,7,R,me\n#IMAGE,CONTINUE\n");
    assert_eq!(result.images, vec![Some(dir.join("bg.png"))]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn lr2_fonts() {
    // ASCII, hiragana (two-byte range) and kanji (upper range) codes
    assert_eq!(font_chars(65), vec!['A']);
    assert_eq!(font_chars(0x82a0 - 32832), vec!['あ']);
    assert_eq!(font_chars(0xe040 - 49281), vec!['漾']);
    assert_eq!(font_chars(288).len(), 2);
    let dir = std::env::temp_dir().join(format!("vibebms-lr2font-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("f")).unwrap();
    std::fs::write(
        dir.join("f/a.lr2font"),
        "#S,20,\n#M,!2,\n#T,0,t.png\n#T,1,missing.png\n#R,65,0,0,0,10,20\n#R,66,1,0,0,10,20\n",
    )
    .unwrap();
    let csv = "#LR2FONT,CONTINUE\n#LR2FONT,f/a.lr2font\n#IMAGE,x.png\n\
#SRC_TEXT,0,1,10,1\n#DST_TEXT,0,0,100,50,80,20,0,255,255,255,255\n";
    std::fs::write(dir.join("s.lr2skin"), csv).unwrap();
    let size = |p: &Path| (!p.ends_with("missing.png")).then_some((64, 64));
    let s = load(
        &dir.join("s.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(s.fonts.len(), 1);
    let f = &s.fonts[0];
    assert_eq!((f.size, f.margin, f.glyphs.len()), (20.0, -2.0, 1));
    assert!(s.images[f.glyphs[&'A'].0]
        .as_ref()
        .unwrap()
        .ends_with("f/t.png"));
    // font 1 (0 is a placeholder); centred text is anchored at x
    assert!(matches!(
        s.objects[0].kind,
        Kind::Text {
            font: Some(0),
            align: 1,
            ..
        }
    ));
    assert_eq!(s.objects[0].dst.keys[0].rect.x, 60.0);
}

#[test]
fn parses_select_bars() {
    let dir = std::env::temp_dir().join(format!("vibebms-sel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = "#INFORMATION,5,Sel,me\n#IMAGE,a.png\n\
#SRC_BAR_BODY,0,0,0,0,100,20,1,1,0,0\n#SRC_BAR_BODY,1,0,0,20,100,20,1,1,0,0\n\
#DST_BAR_BODY_OFF,0,0,10,10,100,20,0,255,255,255,255\n#DST_BAR_BODY_OFF,1,0,10,30,100,20,0,255,255,255,255\n\
#DST_BAR_BODY_ON,1,0,0,30,110,20,0,255,255,255,255\n#BAR_CENTER,1\n#BAR_AVAILABLE,0,1\n\
#DST_BAR_TITLE,0,0,20,2,70,16,0,255,255,255,255\n\
#SRC_BAR_LEVEL,2,0,0,40,100,10,10,1,0,0,0,0,2\n#DST_BAR_LEVEL,2,0,80,4,8,10,0,255,255,255,255\n\
#SRC_BAR_LAMP,3,0,0,50,4,20,1,1,0,0\n#DST_BAR_LAMP,3,0,2,0,4,20,0,255,255,255,255\n\
#SRC_BAR_MY_LAMP,4,0,0,50,4,20,1,1,0,0\n#DST_BAR_MY_LAMP,4,0,2,0,2,20,0,255,255,255,255\n\
#SRC_BAR_RIVAL_LAMP,1,0,0,50,4,20,1,1,0,0\n#DST_BAR_RIVAL_LAMP,1,0,4,0,2,20,0,255,255,255,255\n";
    std::fs::write(dir.join("select.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let s = load(
        &dir.join("select.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    assert_eq!(s.kind, SkinType::Select);
    assert!(matches!(s.objects[0].kind, Kind::SongList));
    let sl = s.songlist.unwrap();
    assert_eq!((sl.center, sl.clickable.clone()), (1, vec![0, 1]));
    assert_eq!(sl.off.len(), 2);
    assert!(sl.on[1].is_some() && sl.on[0].is_none());
    assert!(sl.body[BAR_SONG].is_some() && sl.body[BAR_FOLDER].is_some());
    assert!(sl.text.is_some());
    let (font, _) = sl.level[2].as_ref().unwrap();
    assert_eq!((font.digits, font.keta), (10, 2));
    // LR2 lamp 3 (clear) is beatoraja's NORMAL (5)
    assert!(sl.lamp[5].is_some());
    // the player's and the rival's beside each other: hard (6), failed (1)
    assert!(sl.my_lamp[6]
        .as_ref()
        .is_some_and(|(_, d)| d.keys[0].rect.w == 2.0));
    assert!(sl.rival_lamp[1]
        .as_ref()
        .is_some_and(|(_, d)| d.keys[0].rect.x == 4.0));
    assert_eq!((sl.my_lamp.len(), sl.rival_lamp.len()), (7, 2));
}

#[test]
fn parses_buttons() {
    let dir = std::env::temp_dir().join(format!("vibebms-btn-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = "#INFORMATION,5,Btn,me\n#IMAGE,a.png\n\
#SRC_BUTTON,0,0,0,0,100,40,1,4,0,0,42,0,1\n#DST_BUTTON,0,0,10,10,100,10,0,255,255,255,255,0,0,0,0,0,0,32\n\
#SRC_BUTTON,0,0,0,40,30,10,1,1,0,0,15,0,0\n#DST_BUTTON,0,0,10,30,30,10,0,255,255,255,255\n";
    std::fs::write(dir.join("b.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let s = load(
        &dir.join("b.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    let Kind::ImageSet { srcs, id } = &s.objects[0].kind else {
        panic!()
    };
    // one frame per button value, fired as button 42 (RANDOM)
    assert_eq!((srcs.len(), *id), (4, 42));
    assert_eq!(s.objects[0].act, Some((42, 2)));
    // panel 1 adds option 21 to the destination's own conditions
    assert_eq!(s.objects[0].dst.ops, vec![32, 21]);
    assert_eq!(s.objects[1].act, Some((15, 2)));
    assert!(s.objects[1].dst.ops.is_empty());
}

#[test]
fn battle_skins_and_their_gauges() {
    let dir = std::env::temp_dir().join(format!("vibebms-bat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // a 7KEYS BATTLE skin: a gauge for each side
    let csv = "#INFORMATION,12,B,me\n#IMAGE,a.png\n\
#SRC_GROOVEGAUGE,0,0,0,0,40,10,4,1,0,0,5,0\n#DST_GROOVEGAUGE,0,0,10,10,5,10,0,255,255,255,255\n\
#SRC_GROOVEGAUGE,1,0,0,0,40,10,4,1,0,0,5,0\n#DST_GROOVEGAUGE,1,0,300,10,5,10,0,255,255,255,255\n";
    std::fs::write(dir.join("b.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let s = load(
        &dir.join("b.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    assert_eq!(s.kind, SkinType::Battle7);
    let sides: Vec<usize> = s
        .objects
        .iter()
        .filter_map(|o| match o.kind {
            Kind::Gauge { side, .. } => Some(side),
            _ => None,
        })
        .collect();
    assert_eq!(sides, vec![0, 1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn negative_sizes_mirror_images() {
    let dir = std::env::temp_dir().join(format!("vibebms-mir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = "#INFORMATION,0,M,me\n#IMAGE,a.png\n\
#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n#DST_IMAGE,0,0,100,0,-40,10,0,255,255,255,255\n\
#SRC_NUMBER,0,0,0,0,100,10,10,1,0,0,71,0,3\n#DST_NUMBER,0,0,100,0,-10,10,0,255,255,255,255\n";
    std::fs::write(dir.join("m.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let s = load(
        &dir.join("m.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    // the image keeps its negative width (drawn mirrored), a number
    // is turned around
    let (a, b) = (s.objects[0].dst.keys[0].rect, s.objects[1].dst.keys[0].rect);
    assert_eq!((a.x, a.w), (100.0, -40.0));
    assert_eq!((b.x, b.w), (90.0, 10.0));
}

#[test]
fn hover_images_and_image_sets() {
    let dir = std::env::temp_dir().join(format!("vibebms-onm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // a hover image after an image: its keys are its own, it shows
    // over its hover area (in panel 2); beatoraja image sets pick by a
    // value (here 42, RANDOM)
    let csv = "#INFORMATION,5,Hover,me\n#IMAGE,a.png\n\
#SRC_IMAGE,0,0,0,0,23,32,1,1,0,0\n#DST_IMAGE,0,0,10,10,23,32,0,255,255,255,255\n\
#SRC_ONMOUSE,0,0,0,0,258,67,1,1,0,0,2,0,0,129,67\n\
#DST_ONMOUSE,0,500,544,148,258,67,0,200,255,255,255\n\
#IMAGESET,0,0,0,0,10,10,1,1\n#IMAGESET,0,0,10,0,10,10,1,1\n\
#SRC_IMAGESET,0,21,42,2,0,1\n#DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n";
    std::fs::write(dir.join("h.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let s = load(
        &dir.join("h.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    assert_eq!(s.objects[0].dst.keys.len(), 1);
    assert_eq!(s.objects[0].dst.keys[0].rect.w, 23.0);
    let hover = &s.objects[1].dst;
    assert_eq!(hover.hover, Some(Rect::new(0.0, 0.0, 129.0, 67.0)));
    assert_eq!((hover.keys[0].rect.x, hover.ops.clone()), (544.0, vec![22]));
    let Kind::ImageSet { srcs, id } = &s.objects[2].kind else {
        panic!("{:?}", s.objects[2].kind)
    };
    assert_eq!((srcs.len(), *id, srcs[1].timer), (2, 42, 21));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn beatoraja_extensions() {
    let dir = std::env::temp_dir().join(format!("vibebms-ext-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = "#INFORMATION,0,Ext,me\n#CUSTOMOFFSET,Judge,50,1,1,0,0,0,0\n#IMAGE,a.png\n\
#STRETCH,1\n#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n\
#DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255,0,0,0,0,0,0,0,0,0,50\n#STRETCH,0\n\
#SRC_HIDDEN,0,0,0,0,10,10,1,1,0,0,300,0\n#DST_HIDDEN,0,0,0,0,10,10,0,255,255,255,255\n\
#SRC_BARGRAPH_REFNUMBER,0,0,0,0,10,10,1,1,0,0,160,1,0,99\n#DST_BARGRAPH_REFNUMBER,0,0,0,0,10,10,0,255,255,255,255\n\
#DST_NOTE_EXPANSION_RATE,115,112\n#DST_NOTE2,919\n#JUDGETIMER,2\n#PLAYSTART,2000\n#CLOSE,3000\n\
#SRC_NOWJUDGE_3P,5,0,0,0,10,10,1,1,0,0\n#DST_NOWJUDGE_3P,5,0,0,0,10,10,0,255,255,255,255\n";
    std::fs::write(dir.join("e.lr2skin"), csv).unwrap();
    let size = |_: &Path| Some((128, 64));
    let offsets = HashMap::from([("Judge".to_string(), [3, -4, 0, 0, 0, 0])]);
    let user = UserSettings {
        offsets,
        ..Default::default()
    };
    let s = load_with(&dir.join("e.lr2skin"), &dir, &user, &|_| None, &size).unwrap();
    assert_eq!(s.objects[0].dst.stretch, 1);
    assert_eq!(s.objects[0].dst.offsets, vec![50]);
    assert_eq!(s.offsets.get(&50).map(|o| (o[0], o[1])), Some((3.0, -4.0)));
    let Kind::Cover {
        line, link_lift, ..
    } = &s.objects[1].kind
    else {
        panic!("{:?}", s.objects[1].kind)
    };
    assert_eq!((*line, *link_lift), (Some(300.0), false));
    assert_eq!(s.objects[1].dst.stretch, 0);
    assert_eq!(
        s.objects[1].dst.offsets,
        vec![OFFSET_LIFT, OFFSET_HIDDEN_COVER]
    );
    let Kind::Bar { id, minmax, .. } = &s.objects[2].kind else {
        panic!("{:?}", s.objects[2].kind)
    };
    assert_eq!((*id, *minmax), (160, Some((0.0, 99.0))));
    assert_eq!(s.note_expansion, Some((115.0, 112.0)));
    assert_eq!(
        (s.note_fall_y, s.judge_timer, s.play_start_ms, s.close_ms),
        (Some(919.0), Judge::of(2), Some(2000), Some(3000))
    );
    assert!(s.judges[2].judge[0].is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nested_if_levels_decide_by_their_own_condition() {
    let dir = std::env::temp_dir().join(format!("vibebms-skin-if-{}", std::process::id()));
    let theme = dir.join("S");
    std::fs::create_dir_all(&theme).unwrap();
    let img = |x: i32| {
        format!("#SRC_IMAGE,0,0,{x},0,1,1,1,1,0,0\n#DST_IMAGE,0,0,{x},0,1,1,0,255,255,255,255\n")
    };
    // options 901 off, 900 on
    let csv = format!(
        "#INFORMATION,0,T,me\n#CUSTOMOPTION,O,900,A,B\n#IMAGE,a.png\n\
         #IF,901\n{}#IF,900\n{}#ELSE\n{}#ENDIF\n{}#ELSE\n{}#IF,901\n{}#ELSEIF,900\n{}#ENDIF\n#ENDIF\n{}",
        img(1), img(2), img(3), img(4), img(5), img(6), img(7), img(8)
    );
    std::fs::write(theme.join("p.lr2skin"), csv).unwrap();
    let s = load(
        &theme.join("p.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &|_| Some((8, 8)),
    )
    .unwrap();
    let xs: Vec<i32> = s
        .objects
        .iter()
        .filter_map(|o| o.dst.keys.first().map(|k| k.rect.x as i32))
        .collect();
    // 2: inside the skipped 901 level, its own level (900) is on
    assert_eq!(xs, vec![2, 5, 7, 8]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn parses_play_skin() {
    let dir = std::env::temp_dir().join(format!("vibebms-skin-{}", std::process::id()));
    let theme = dir.join("MySkin");
    std::fs::create_dir_all(&theme).unwrap();
    let csv = "#INFORMATION,0,Test,me\n#CUSTOMOPTION,Lane,900,Dark,Light\n#IMAGE,LR2files\\Theme\\MySkin\\a.png\n#IMAGE,missing.png\n#IMAGE,unused.png\n\
#SRC_IMAGE,0,1,0,0,10,10,1,1,0,0\n#IF,901\n#SRC_IMAGE,0,0,0,0,10,10,1,1,0,0\n#DST_IMAGE,0,0,0,0,10,10,0,255,255,255,255\n#ELSE\n#SRC_IMAGE,0,0,0,0,20,20,2,1,100,0\n#DST_IMAGE,0,0,5,5,20,20,0,255,255,255,255,0,0,0,0,-1,41,32\n#DST_IMAGE,0,1000,50,5,20,20\n#ENDIF\n\
#SRC_NUMBER,0,0,0,0,110,10,11,1,0,0,101,0,6\n#DST_NUMBER,0,0,100,100,8,10,0,255,255,255,255\n\
#SRC_NOTE,1,0,0,0,10,4,1,1,0,0\n#SRC_NOTE,0,0,0,0,20,4,1,1,0,0\n#DST_NOTE,0,0,30,0,20,400,0,255,255,255,255\n#DST_NOTE,1,0,50,0,10,400,0,255,255,255,255\n\
#SRC_NOWJUDGE_1P,5,0,0,0,40,10,1,1,0,0\n#DST_NOWJUDGE_1P,5,0,60,200,40,10,0,255,255,255,255,0,0,0,0,-1,46\n";
    std::fs::write(theme.join("play.lr2skin"), csv).unwrap();
    let size = |p: &Path| {
        p.file_name()
            .map(|n| n == "a.png")
            .unwrap_or(false)
            .then_some((128, 64))
    };
    let s = load(
        &theme.join("play.lr2skin"),
        &dir,
        &HashMap::new(),
        &|_| None,
        &size,
    )
    .unwrap();
    assert_eq!(s.name, "Test");
    assert_eq!(s.kind, SkinType::Play7);
    // a missing image is left out; one nothing draws from is not looked at
    assert_eq!(s.images.len(), 3);
    assert!(s.images[1].is_none());
    assert!(s.images[2].is_some());
    // option 900 (Dark) is the default, so the #ELSE branch is used
    let Kind::Image(src) = &s.objects[0].kind else {
        panic!()
    };
    assert_eq!(src.frames.len(), 2);
    assert_eq!(s.objects[0].dst.keys.len(), 2);
    assert_eq!(s.objects[0].dst.timer, 41);
    assert_eq!(s.objects[0].dst.ops, vec![32]);
    assert!(matches!(
        s.objects[1].kind,
        Kind::Number {
            digits: 11,
            id: 101,
            keta: 6,
            ..
        }
    ));
    assert_eq!(s.lanes[7].rect.unwrap().w, 20.0); // scratch
    assert!(s.lanes[0].note.is_some());
    assert!(s.objects.iter().any(|o| matches!(o.kind, Kind::Notes)));
    assert!(s.judges[0].judge[0].is_some()); // LR2 index 5 = PGREAT
    std::fs::remove_dir_all(&dir).ok();
}
