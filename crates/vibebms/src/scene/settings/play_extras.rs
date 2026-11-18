//! GAMEPLAY › PLAY EXTRAS: what a play shows and sounds beyond the rules
//! (FAST / SLOW threshold, guide sounds, BGA, the end of a play).

use super::row::onoff;
use super::row::{Row, Rows};

pub(super) fn rows(b: &mut Rows) {
    let p = &b.ctx.config.play;
    b.group(
        "gameplay.extras",
        tr!("PLAY EXTRAS"),
        tr!("FAST / SLOW threshold, GUIDE SE, notes at the line, RESULT WAIT, miss BGA and keysound"),
    );
    b.extend([
        Row::adjust(
            "play.fast_slow_ms",
            tr!("FAST / SLOW threshold"),
            match p.fast_slow_ms {
                0 => tr!("ANY").to_string(),
                ms => format!("> {ms} ms"),
            },
            |c, d| {
                let ms = &mut c.config.play.fast_slow_ms;
                *ms = (*ms as i32 + d).clamp(0, 100) as u32
            },
        )
        .desc(tr!(
            "FAST / SLOW shows only for a judgement more than this far off (0: any)."
        ))
        .keys("fast slow early late timing threshold"),
        Row::adjust(
            "play.guide_se",
            tr!("GUIDE SE"),
            onoff(p.guide_se),
            |c, _| c.config.play.guide_se ^= true,
        )
        .desc(tr!(
            "A sound for each judgement: the sound set's guide-pg .. guide-ms, else built-in."
        ))
        .keys("guide sound effect judgement se assist"),
        Row::adjust(
            "play.hold_at_line",
            tr!("Notes wait at the line"),
            onoff(p.hold_at_line),
            |c, _| c.config.play.hold_at_line ^= true,
        )
        .desc(tr!(
            "As LR2: a note not yet hit stays on the judge line until it is judged or missed."
        ))
        .keys("lr2 note display judge line hold stop unjudged"),
        Row::adjust(
            "play.result_wait",
            tr!("RESULT WAIT"),
            onoff(p.result_wait),
            |c, _| c.config.play.result_wait ^= true,
        )
        .desc(tr!(
            "Stay in the play until 5 s after the chart's last note, BGM or BGA."
        ))
        .keys("result wait end bgm bga ed-rian"),
        Row::adjust(
            "play.good_clears_poor",
            tr!("GOOD clears the miss BGA"),
            onoff(p.good_clears_poor),
            |c, _| c.config.play.good_clears_poor ^= true,
        )
        .desc(tr!(
            "A PGREAT, GREAT or GOOD takes the POOR image away at once."
        ))
        .keys("miss layer poor bga clear good ed-rian"),
        Row::adjust(
            "play.miss_keysound",
            tr!("Keysound on a missed note"),
            onoff(p.miss_keysound),
            |c, _| c.config.play.miss_keysound ^= true,
        )
        .desc(tr!(
            "A note passed without a press plays its keysound as its POOR comes."
        ))
        .keys("miss keysound poor sound ed-rian"),
    ]);
}
