//! GAMEPLAY › PLAY EXTRAS: what a play shows and sounds beyond the rules
//! (FAST / SLOW threshold, guide sounds, BGA, the end of a play).

use super::row::{Row, Rows};

pub(super) fn rows(b: &mut Rows) {
    let p = &b.ctx.config.play;
    b.group(
        "gameplay.extras",
        "PLAY EXTRAS",
        "FAST / SLOW threshold",
    );
    b.extend([Row::adjust(
        "play.fast_slow_ms",
        "FAST / SLOW threshold",
        match p.fast_slow_ms {
            0 => "ANY".to_string(),
            ms => format!("> {ms} ms"),
        },
        |c, d| {
            let ms = &mut c.config.play.fast_slow_ms;
            *ms = (*ms as i32 + d).clamp(0, 100) as u32
        },
    )
    .desc("FAST / SLOW shows only for a judgement more than this far off (0: any).")
    .keys("fast slow early late timing threshold")]);
}
