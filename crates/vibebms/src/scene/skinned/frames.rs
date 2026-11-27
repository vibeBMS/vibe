//! Frames of the built-in skins recorded without a GPU or a sound device:
//! the song select, a play and its result, each drawn at its start and
//! 2 s in, into the batch the frame is drawn from. Not the frames' every
//! vertex (any edit of a skin would change them): that drawing runs, and
//! that what it records can be drawn.

use super::golden::{result, song};
use crate::ctx::Ctx;
use crate::gfx::text::Text;
use crate::gfx::{Batch, Draw, Gfx, ATLAS};
use crate::platform::{Duration, Instant};
use crate::scene::play::{PlayRequest, PlayScene};
use crate::scene::select::{Bar, SelectScene};
use crate::scene::{Next, Scene};
use std::path::PathBuf;
use std::sync::Arc;

struct Frames {
    gfx: Gfx,
    text: Text,
    batch: Batch,
}

impl Frames {
    fn new(ctx: &Ctx) -> Frames {
        Frames {
            gfx: Gfx::without_gpu(),
            text: Text::new(&[ctx.paths.fonts.as_path()]),
            batch: Batch::default(),
        }
    }

    /// `scene` updated and drawn at `t` (twice: what the first frame asks
    /// for, such as a font's sheets, is there for the second), its skin's
    /// images waited for (`wait`); the triangles drawn, checked.
    fn draw<S: Scene>(
        &mut self,
        ctx: &mut Ctx,
        scene: &mut S,
        t: Instant,
        wait: impl Fn(&mut S, &mut Gfx),
    ) -> usize {
        for _ in 0..2 {
            assert!(matches!(scene.update(ctx, &mut self.gfx, t), Next::Stay));
            wait(scene, &mut self.gfx);
            self.batch.clear();
            let mut d = Draw {
                gfx: &self.gfx,
                text: &mut self.text,
                b: &mut self.batch,
                pointer: None,
                text_input: false,
            };
            scene.draw(ctx, &mut d, t);
        }
        self.check(ctx)
    }

    /// Every draw of the batch is of a texture there is, every vertex
    /// finite and within two screens' size of the screen; text and images
    /// are drawn. How many triangles.
    fn check(&self, ctx: &Ctx) -> usize {
        let (w, h) = (ctx.view.width, ctx.view.height);
        let mut verts = 0;
        let mut texs = std::collections::HashSet::new();
        for (tex, vs) in self.batch.draws() {
            assert!(self.gfx.texture_size(tex).is_some(), "texture {tex}");
            assert_eq!(vs.len() % 3, 0);
            for v in vs {
                let [x, y] = v.pos;
                assert!((-2.0 * w..=3.0 * w).contains(&x), "{:?}", v.pos);
                assert!((-2.0 * h..=3.0 * h).contains(&y), "{:?}", v.pos);
                assert!(v.uv.iter().chain(&v.color).all(|c| c.is_finite()), "{v:?}");
            }
            verts += vs.len();
            texs.insert(tex);
        }
        assert!(texs.contains(&ATLAS), "no text, no shapes");
        assert!(texs.iter().any(|&t| t != ATLAS), "no image of the skin");
        verts / 3
    }
}

/// A context with one chart in its library, at `root/song/chart.bme`.
fn ctx(root: &std::path::Path) -> Ctx {
    let mut ctx = Ctx::for_test(root.to_path_buf());
    let chart = root.join("song/chart.bme");
    std::fs::create_dir_all(chart.parent().unwrap()).unwrap();
    std::fs::write(&chart, vibebms_core::bot::chart_text(7, 21)).unwrap();
    let s = crate::library::Song {
        path: chart.to_string_lossy().into_owned(),
        folder: root.join("song").to_string_lossy().into_owned(),
        ..song()
    };
    ctx.lib.songs = Arc::new(vec![s]);
    ctx
}

fn temp_root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("vibebms-frames-{name}-{}", std::process::id()))
}

/// `n` triangles drawn, in the range about what was recorded for a frame
/// (the select 1172 at both times, the play 334 then 346, the result 356
/// then 360: the text's glyphs depend a little on the fonts there are).
fn assert_within(n: usize, range: std::ops::RangeInclusive<usize>, frame: &str) {
    assert!(range.contains(&n), "{frame}: {n} triangles");
}

#[test]
fn the_song_select_is_drawn() {
    let root = temp_root("select");
    let mut ctx = ctx(&root);
    let mut f = Frames::new(&ctx);
    let mut select = SelectScene::new(&mut ctx);
    select.bars = vec![Bar::Song(0)];
    let t0 = select.host.created;
    let wait = |s: &mut SelectScene, gfx: &mut Gfx| s.host.skin.as_mut().unwrap().wait(gfx);
    for (t, range) in [(t0, 800..=1600), (t0 + Duration::from_secs(2), 800..=1600)] {
        let n = f.draw(&mut ctx, &mut select, t, wait);
        assert_within(n, range, &format!("select {:?}", t - t0));
    }
    std::fs::remove_dir_all(&root).ok();
}

/// A play of the chart by the bot (as a video export plays one: loaded,
/// then drawn by its own clock), then its result.
#[test]
fn a_play_and_its_result_are_drawn() {
    let root = temp_root("play");
    let mut ctx = ctx(&root);
    let mut f = Frames::new(&ctx);
    let req = PlayRequest {
        song: ctx.lib.songs[0].clone(),
        autoplay: true,
        course: None,
        replay: None,
        vs: None,
        practice: None,
    };
    let mut play = PlayScene::new(&mut ctx, req);
    let until = Instant::now() + Duration::from_secs(60);
    let began = loop {
        assert!(Instant::now() < until, "the chart did not load");
        let next = play.update(&mut ctx, &mut f.gfx, Instant::now());
        assert!(matches!(next, Next::Stay), "{:?}", ctx.toasts);
        if let Some(b) = play.began() {
            break b;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // (the play's skin is waited for as it loads)
    let ready = |_: &mut PlayScene, _: &mut Gfx| {};
    for (t, range) in [
        (began, 230..=480),
        (began + Duration::from_secs(2), 230..=480),
    ] {
        let n = f.draw(&mut ctx, &mut play, t, ready);
        assert_within(n, range, &format!("play {:?}", t - began));
    }

    let mut res = result(&mut ctx, &play);
    let t0 = res.host.created;
    let wait = |s: &mut crate::scene::result::ResultScene, gfx: &mut Gfx| {
        s.host.skin.as_mut().unwrap().wait(gfx)
    };
    for (t, range) in [(t0, 250..=500), (t0 + Duration::from_secs(2), 250..=500)] {
        let n = f.draw(&mut ctx, &mut res, t, wait);
        assert_within(n, range, &format!("result {:?}", t - t0));
    }
    std::fs::remove_dir_all(&root).ok();
}
