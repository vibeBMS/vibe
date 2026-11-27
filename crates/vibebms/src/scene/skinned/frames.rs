//! Frames of the built-in skins recorded without a GPU or a sound device:
//! the song select, a play and its result, each drawn at its start and
//! 2 s in, into the batch the frame is drawn from. In the bundled font, in
//! English, by the scenes' own clocks: the same frames on every machine.
//!
//! Each frame is checked for what it draws, kind by kind: triangles of the
//! skin's images, of text and of shapes, and how many of the skin's
//! textures. Not where, in what colour or in which order (any edit of a
//! skin would change that): an object that is no longer drawn fails, as
//! does one more, a text that is longer or shorter, or another texture;
//! moving, resizing or recolouring one does not. (An edit of a built-in
//! skin that adds or removes something changes the numbers here too.)

use super::golden::{result, song};
use crate::ctx::Ctx;
use crate::gfx::text::Text;
use crate::gfx::{Batch, Draw, Gfx, ATLAS, WHITE_UV};
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

/// What a frame drew.
#[derive(Debug, PartialEq)]
struct Drawn {
    /// Triangles of the skin's images.
    images: usize,
    /// Triangles of text: of the atlas, off its white block.
    glyphs: usize,
    /// Triangles of shapes: the atlas's white block.
    shapes: usize,
    /// The skin's textures drawn from.
    textures: usize,
}

impl Frames {
    fn new() -> Frames {
        crate::i18n::set(crate::i18n::Lang::En);
        Frames {
            gfx: Gfx::without_gpu(),
            text: Text::built_in(),
            batch: Batch::default(),
        }
    }

    /// `scene` updated and drawn at `t` (twice: what the first frame asks
    /// for, such as a font's sheets, is there for the second), its skin's
    /// images waited for (`wait`); what was drawn, checked.
    fn draw<S: Scene>(
        &mut self,
        ctx: &mut Ctx,
        scene: &mut S,
        t: Instant,
        wait: impl Fn(&mut S, &mut Gfx),
    ) -> Drawn {
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
    /// finite and within two screens' size of the screen. What was drawn.
    fn check(&self, ctx: &Ctx) -> Drawn {
        let (w, h) = (ctx.view.width, ctx.view.height);
        let white = |uv: [f32; 2]| {
            (WHITE_UV[0]..=WHITE_UV[2]).contains(&uv[0])
                && (WHITE_UV[1]..=WHITE_UV[3]).contains(&uv[1])
        };
        let mut n = Drawn {
            images: 0,
            glyphs: 0,
            shapes: 0,
            textures: 0,
        };
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
            for tri in vs.chunks(3) {
                match tex {
                    ATLAS if tri.iter().all(|v| white(v.uv)) => n.shapes += 1,
                    ATLAS => n.glyphs += 1,
                    _ => n.images += 1,
                }
            }
            if tex != ATLAS {
                texs.insert(tex);
            }
        }
        n.textures = texs.len();
        n
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

#[test]
fn the_song_select_is_drawn() {
    let root = temp_root("select");
    let mut ctx = ctx(&root);
    let mut f = Frames::new();
    let mut select = SelectScene::new(&mut ctx);
    select.bars = vec![Bar::Song(0)];
    let t0 = select.host().created;
    let wait = |s: &mut SelectScene, gfx: &mut Gfx| s.host().skin.as_mut().unwrap().wait(gfx);
    let frames = [
        (
            t0,
            Drawn {
                images: 382,
                glyphs: 790,
                shapes: 0,
                textures: 2,
            },
        ),
        (
            t0 + Duration::from_secs(2),
            Drawn {
                images: 382,
                glyphs: 790,
                shapes: 0,
                textures: 2,
            },
        ),
    ];
    for (t, want) in frames {
        // (2 s since the cursor moved, too: the banner, the preview and
        // the ranking are asked for)
        select.set_clock(t);
        let n = f.draw(&mut ctx, &mut select, t, wait);
        assert_eq!(n, want, "select {:?}", t - t0);
    }
    std::fs::remove_dir_all(&root).ok();
}

/// A play of the chart by the bot (as a video export plays one: loaded,
/// then drawn by its own clock), then its result.
#[test]
fn a_play_and_its_result_are_drawn() {
    let root = temp_root("play");
    let mut ctx = ctx(&root);
    let mut f = Frames::new();
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
        assert!(Instant::now() < until, "the chart did not load in 60 s");
        let next = play.update(&mut ctx, &mut f.gfx, Instant::now());
        assert!(matches!(next, Next::Stay), "{:?}", ctx.toasts);
        if let Some(b) = play.began() {
            break b;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    // (the play's skin is waited for as it loads)
    let ready = |_: &mut PlayScene, _: &mut Gfx| {};
    let frames = [
        (
            began,
            Drawn {
                images: 332,
                glyphs: 0,
                shapes: 2,
                textures: 2,
            },
        ),
        (
            began + Duration::from_secs(2),
            Drawn {
                images: 344,
                glyphs: 0,
                shapes: 2,
                textures: 2,
            },
        ),
    ];
    for (t, want) in frames {
        let n = f.draw(&mut ctx, &mut play, t, ready);
        assert_eq!(n, want, "play {:?}", t - began);
    }

    let mut res = result(&mut ctx, &play);
    let t0 = res.host.created;
    let wait = |s: &mut crate::scene::result::ResultScene, gfx: &mut Gfx| {
        s.host.skin.as_mut().unwrap().wait(gfx)
    };
    let frames = [
        (
            t0,
            Drawn {
                images: 196,
                glyphs: 142,
                shapes: 18,
                textures: 2,
            },
        ),
        (
            t0 + Duration::from_secs(2),
            Drawn {
                images: 200,
                glyphs: 142,
                shapes: 18,
                textures: 2,
            },
        ),
    ];
    for (t, want) in frames {
        let n = f.draw(&mut ctx, &mut res, t, wait);
        assert_eq!(n, want, "result {:?}", t - t0);
    }
    std::fs::remove_dir_all(&root).ok();
}
