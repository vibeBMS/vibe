//! Skins drawn into a batch without a GPU, in the bundled font: where
//! their texts and numbers land.

use super::{Hooks, LoadedSkin};
use crate::gfx::text::Text;
use crate::gfx::{Batch, Draw, Gfx, Rect, ATLAS, WHITE_UV};
use crate::skin::*;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

struct St;
impl SkinState for St {
    fn timer(&self, id: i32) -> Option<i64> {
        (id == 0).then_some(0)
    }
    fn option(&self, _: i32) -> bool {
        false
    }
    fn number(&self, id: i32) -> Option<i64> {
        (id == 71).then_some(1234)
    }
    fn text(&self, id: i32) -> Option<Cow<'_, str>> {
        (id == 10).then(|| "ABBA".into())
    }
    fn rate(&self, _: i32) -> f32 {
        0.0
    }
}

struct NoHooks;
impl Hooks for NoHooks {}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibebms-drawn-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The skin written to `dir/name`, loaded.
fn load(dir: &Path, name: &str, text: &str) -> Skin {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    load_file(
        &p,
        dir,
        &Default::default(),
        &|_| None,
        Some(&St),
        &image_size,
    )
    .unwrap()
}

/// `draw` run twice (the glyphs the first asks for are there for the
/// second) into a batch, with `skin` loaded and its images waited for.
fn drawn(skin: Skin, draw: impl Fn(&LoadedSkin, &mut Draw)) -> Batch {
    let mut gfx = Gfx::without_gpu();
    let mut text = Text::built_in();
    let mut sk = LoadedSkin::load(skin, &mut gfx);
    sk.wait(&mut gfx);
    let mut b = Batch::default();
    for _ in 0..2 {
        sk.load_pending(&mut gfx);
        b.clear();
        let mut d = Draw {
            gfx: &gfx,
            text: &mut text,
            b: &mut b,
            pointer: None,
            text_input: false,
        };
        draw(&sk, &mut d);
    }
    b
}

/// The whole skin at 0 ms, on a screen its own size.
fn drawn_all(skin: Skin) -> Batch {
    let view = Rect::new(0.0, 0.0, skin.width, skin.height);
    drawn(skin, |sk, d| sk.draw(d, view, 0, &St, &mut NoHooks))
}

/// Where the glyphs drawn begin and end (x): the quads of the atlas off
/// its white block.
fn glyphs_x(b: &Batch) -> (f32, f32) {
    let white = |uv: [f32; 2]| {
        (WHITE_UV[0]..=WHITE_UV[2]).contains(&uv[0]) && (WHITE_UV[1]..=WHITE_UV[3]).contains(&uv[1])
    };
    let xs: Vec<f32> = (b.draws())
        .filter(|(t, _)| *t == ATLAS)
        .flat_map(|(_, vs)| vs.iter().filter(|v| !white(v.uv)).map(|v| v.pos[0]))
        .collect();
    assert!(!xs.is_empty(), "no glyphs");
    let lo = xs.iter().copied().fold(f32::MAX, f32::min);
    let hi = xs.iter().copied().fold(f32::MIN, f32::max);
    (lo, hi)
}

/// The text ends at `right` (its last glyph within a pixel or two of it)
/// and does not begin at the box's left edge `left`.
fn ends_at(b: &Batch, left: f32, right: f32) {
    let (lo, hi) = glyphs_x(b);
    assert!(
        (right - 3.0..=right + 0.5).contains(&hi),
        "ends at {hi}, not {right}"
    );
    assert!(
        lo > left + 20.0,
        "begins at {lo}: left aligned in {left}..{right}"
    );
}

/// A text's `align` past 2 is right-aligned whatever font draws it: in
/// the UI's font too (a beatoraja text without a `.fnt` font, the song
/// list's title, an LR2 text or bar title whose `#LR2FONT` is not there)
/// it ends at the right edge of its box.
#[test]
fn a_text_aligned_past_right_ends_at_its_box() {
    let dir = temp("align");
    // beatoraja: the box is x - w .. x = 120..320 (y up)
    let json = r#"{"type": 6, "name": "T", "w": 640, "h": 480,
        "text": [{"id": "t", "ref": 10, "align": 3}],
        "destination": [{"id": "t", "dst": [{"x": 320, "y": 100, "w": 200, "h": 20}]}]}"#;
    let skin = load(&dir, "t.json", json);
    let Kind::Text { style, .. } = &skin.objects[0].kind else {
        panic!("{:?}", skin.objects[0].kind)
    };
    assert_eq!((style.align, style.font), (Align::Right, None));
    assert_eq!(skin.objects[0].dst.keys[0].rect.x, 120.0);
    ends_at(&drawn_all(skin), 120.0, 320.0);

    // the song list's title: its box relative to the bar, 260..460
    let json = r#"{"type": 5, "name": "S", "w": 1280, "h": 720,
        "text": [{"id": "title", "ref": 10, "align": 3}],
        "songlist": {"id": "songlist", "center": 0,
            "listoff": [{"id": "bar", "dst": [{"x": 600, "y": 600, "w": 500, "h": 40}]}],
            "liston": [{"id": "bar", "dst": [{"x": 580, "y": 600, "w": 520, "h": 40}]}],
            "text": [{"id": "title", "dst": [{"x": 460, "y": 10, "w": 200, "h": 24}]}]},
        "destination": [{"id": "songlist", "dst": [{"x": 0, "y": 0, "w": 1280, "h": 720}]}]}"#;
    let skin = load(&dir, "s.json", json);
    let title = skin.songlist.as_ref().unwrap().text.clone().unwrap();
    assert_eq!(title.style.align, Align::Right);
    let fr = title.dst.eval(0, &St).unwrap();
    assert_eq!((fr.rect.x, fr.rect.w), (260.0, 200.0));
    let b = drawn(skin, |sk, d| {
        sk.text(d, "ABBA", &title.style, &fr, None, 1.0)
    });
    ends_at(&b, 260.0, 460.0);

    // LR2: font 0 is not there; anchored at x, the box is 120..320
    let csv = "#INFORMATION,6,T,me\n#SRC_TEXT,0,0,10,3\n\
#DST_TEXT,0,0,320,340,200,20,0,255,255,255,255\n";
    let skin = load(&dir, "t.lr2skin", csv);
    let Kind::Text { style, .. } = &skin.objects[0].kind else {
        panic!("{:?}", skin.objects[0].kind)
    };
    assert_eq!((style.align, style.font), (Align::Right, None));
    assert_eq!(skin.objects[0].dst.keys[0].rect.x, 120.0);
    ends_at(&drawn_all(skin), 120.0, 320.0);

    // an LR2 bar title likewise: 60..260 of the bar
    let csv = "#INFORMATION,5,S,me\n#LR2FONT,missing.lr2font\n#SRC_BAR_TITLE,0,0,0,3\n\
#DST_BAR_TITLE,0,0,260,10,200,24,0,255,255,255,255\n#SRC_TEXT,0,0,10,0\n\
#DST_TEXT,0,0,0,0,10,10,0,255,255,255,255\n";
    let skin = load(&dir, "s.lr2skin", csv);
    let title = skin.songlist.as_ref().unwrap().text.clone().unwrap();
    assert_eq!((title.style.align, title.style.font), (Align::Right, None));
    let fr = title.dst.eval(0, &St).unwrap();
    assert_eq!((fr.rect.x, fr.rect.w), (60.0, 200.0));
    let b = drawn(skin, |sk, d| {
        sk.text(d, "ABBA", &title.style, &fr, None, 1.0)
    });
    ends_at(&b, 60.0, 260.0);
    std::fs::remove_dir_all(&dir).ok();
}

/// A number 12 px a place from x = 10, its digits drawn where (x of each).
fn digits_x(align: i32, keta: usize) -> Vec<f32> {
    let dir = temp(&format!("number{align}-{keta}"));
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::copy(
        here.join("assets/builtin/skins/default/parts.png"),
        dir.join("parts.png"),
    )
    .unwrap();
    let csv = format!(
        "#INFORMATION,6,N,me\n#IMAGE,parts.png\n#SRC_NUMBER,0,0,0,0,100,20,10,1,0,0,71,{align},6\n\
#DST_NUMBER,0,0,10,20,12,20,0,255,255,255,255\n"
    );
    let mut skin = load(&dir, "n.lr2skin", &csv);
    let Kind::Number { font, .. } = &mut skin.objects[0].kind else {
        panic!("{:?}", skin.objects[0].kind)
    };
    // (the loaders keep it within the places there are)
    font.style.keta = keta;
    let b = drawn_all(skin);
    std::fs::remove_dir_all(&dir).ok();
    let mut xs: Vec<f32> = (b.draws())
        .filter(|(t, _)| *t != ATLAS)
        .flat_map(|(_, vs)| vs.chunks(6).map(|q| q[0].pos[0]))
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

/// A number's `align` 0 sets its digits in the last of its places (they
/// end at its right), 1 in the first, 2 in the middle: as the names say.
#[test]
fn a_number_sits_where_its_align_says() {
    let x = |i: usize| 10.0 + 12.0 * i as f32;
    let of = |align| Align::of_number(align);
    assert_eq!(
        (of(0), of(1), of(2)),
        (Align::Right, Align::Left, Align::Center)
    );
    // 1234 in six places
    assert_eq!(digits_x(0, 6), [x(2), x(3), x(4), x(5)]);
    assert_eq!(digits_x(1, 6), [x(0), x(1), x(2), x(3)]);
    assert_eq!(digits_x(2, 6), [x(1), x(2), x(3), x(4)]);
}

/// More places than a number may have: as many as it may, drawn every
/// frame without a panic.
#[test]
fn a_number_of_too_many_places_has_the_most_it_may() {
    let n = NumberStyle::PLACES;
    let x = |i: usize| 10.0 + 12.0 * i as f32;
    assert_eq!(digits_x(0, n + 8), [x(n - 4), x(n - 3), x(n - 2), x(n - 1)]);
    assert_eq!(digits_x(1, n + 8), [x(0), x(1), x(2), x(3)]);
}
