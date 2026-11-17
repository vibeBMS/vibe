//! Which play skin a mode uses: the built-in ones and the configured
//! paths.

use crate::app::Ctx;
use crate::skin::SkinType;
use std::path::PathBuf;
use vibebms_core::PlayMode;

pub fn skin_type(mode: PlayMode) -> SkinType {
    match mode {
        PlayMode::Beat5K => SkinType::Play5,
        PlayMode::Beat7K => SkinType::Play7,
        PlayMode::Beat10K => SkinType::Play10,
        PlayMode::Beat14K => SkinType::Play14,
        PlayMode::Popn9K => SkinType::Play9,
    }
}

/// The built-in default skin for a mode, side and screen orientation
/// (`touch`: the landscape variant above on-screen controls).
pub fn default_skin(mode: PlayMode, p2: bool, portrait: bool, touch: bool) -> PathBuf {
    let variant = if portrait {
        "_portrait"
    } else if touch {
        "_touch"
    } else {
        ""
    };
    let name = match mode {
        // double play has one landscape layout (portrait shows it too)
        PlayMode::Beat14K | PlayMode::Beat10K => format!(
            "play{}{}",
            mode.keys_per_player() * 2,
            if touch && !portrait { "_touch" } else { "" }
        ),
        // 9 buttons have no scratch, so one centred layout serves both sides
        m => format!(
            "play{}{variant}{}",
            m.keys_per_player(),
            if p2 && m.has_scratch() { "_2p" } else { "" }
        ),
    };
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name + ".json")
}

/// The built-in battle skin (both sides' gauges and scores) for a double
/// play mode.
pub fn battle_skin(mode: PlayMode, touch: bool) -> PathBuf {
    let name = format!(
        "play{}_battle{}.json",
        mode.keys_per_player() * 2,
        if touch { "_touch" } else { "" }
    );
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name)
}

/// The battle skin type of a double play mode.
pub fn battle_type(mode: PlayMode) -> Option<SkinType> {
    skin_type(mode).battle_pair()
}

/// `config.skins` value of the built-in touch perspective skin.
pub const PERSPECTIVE: &str = "builtin:perspective";

/// The built-in touch perspective skin for a mode and orientation (the
/// play side is the game's: it draws the lanes).
pub fn perspective_skin(mode: PlayMode, portrait: bool) -> PathBuf {
    let keys = mode.keys_per_player() * mode.players();
    let name = format!(
        "persp{keys}{}.json",
        if portrait { "_portrait" } else { "" }
    );
    PathBuf::from(crate::platform::vfs::BUILTIN)
        .join("skins/default")
        .join(name)
}

/// Resolve a `config.skins` value: empty = the built-in default; anything
/// else is a skin file relative to the data root (or absolute).
pub fn configured_path(ctx: &Ctx, value: &str) -> Option<PathBuf> {
    // "classic" was the removed code-drawn view: use the default
    if value.is_empty() || value == "classic" || value == PERSPECTIVE {
        return None;
    }
    let p = PathBuf::from(value);
    Some(if p.is_absolute() {
        p
    } else {
        ctx.paths.root.join(p)
    })
}

/// The play skin's width / height as far as it is known without loading
/// it (the touch layout editor): a JSON skin's own (the default skins
/// are), else 16:9.
pub fn aspect(ctx: &Ctx, mode: PlayMode) -> f32 {
    let value = ctx
        .config
        .skins
        .get(mode.label())
        .cloned()
        .unwrap_or_default();
    // the perspective skin takes the screen's shape
    if value == PERSPECTIVE {
        return ctx.width / ctx.height.max(1.0);
    }
    let path = configured_path(ctx, &value).unwrap_or_else(|| {
        let p2 = ctx.config.play.side == crate::config::Side::P2;
        default_skin(mode, p2, ctx.portrait(), true)
    });
    let json = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    json.then(|| crate::skin::beatoraja::size(&path))
        .flatten()
        .map_or(16.0 / 9.0, |(w, h)| w / h.max(1.0))
}
