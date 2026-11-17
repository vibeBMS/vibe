//! `vibebms <chart> [--autoplay] [--ghost=best|last] [--vs]`: a chart
//! played directly at start.

use super::{Ctx, Scene};
use crate::scene::play::{ghost_for, vs_refusal, PlayRequest, PlayScene, Versus};
use std::path::PathBuf;

/// The play the command line asks for, if it names a chart.
pub(super) fn play_scene(ctx: &mut Ctx) -> Option<Box<dyn Scene>> {
    if crate::platform::IS_WEB {
        return None;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = PathBuf::from(args.iter().find(|a| !a.starts_with('-'))?);
    let c = match vibebms_core::load_chart(&path, &Default::default()) {
        Ok(c) => c,
        Err(e) => {
            ctx.toast(format!("{}: {e}", path.display()));
            return None;
        }
    };
    let song = crate::library::song_from_chart(&path, String::new(), &c);
    // --ghost best|last: G-BATTLE against that replay
    let ghost = args
        .iter()
        .find_map(|a| a.strip_prefix("--ghost="))
        .map(|g| {
            let slot = if g == "last" {
                crate::replay::Slot::Last
            } else {
                crate::replay::Slot::Best
            };
            ghost_for(ctx, &song, slot).map(|r| (r, slot))
        });
    let vs = match ghost {
        Some(Ok((r, slot))) => Some(Versus::Ghost(r, slot)),
        Some(Err(e)) => {
            ctx.toast(e);
            None
        }
        // --vs: two players here
        None if args.iter().any(|a| a == "--vs") => match vs_refusal(ctx, &song) {
            Some(why) => {
                ctx.toast(why.to_string());
                None
            }
            None => Some(Versus::Local),
        },
        None => None,
    };
    let req = PlayRequest {
        song,
        autoplay: args.iter().any(|a| a == "--autoplay" || a == "-a"),
        course: None,
        replay: None,
        vs,
    };
    Some(Box::new(PlayScene::new(ctx, req)))
}
