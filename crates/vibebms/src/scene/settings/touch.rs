use super::{percent, play::flip_side, Act, Row, Screen};
use crate::app::Ctx;
use crate::config::{
    TouchMode, TouchSide, TouchStyle, LENGTH_RANGE, PERSPECTIVE_MAX, TOUCH_HEIGHT_RANGE,
};
use crate::scene::option_panel::{cycle, onoff};

pub(super) fn style_label(s: TouchStyle) -> &'static str {
    match s {
        TouchStyle::Iidx => "BM (turntable + 7)",
        s => s.label(),
    }
}

fn start_select_label(s: TouchSide) -> &'static str {
    match s {
        TouchSide::Auto => "AUTO (scratch side)",
        s => s.label(),
    }
}

/// `v` moved `d` steps of `by` within `range`.
fn step(v: &mut f32, d: i32, by: f32, (lo, hi): (f32, f32)) {
    *v = (*v + d as f32 * by).clamp(lo, hi)
}

pub(super) fn rows(ctx: &Ctx) -> Vec<Row> {
    let c = &ctx.config;
    let t = &c.touch;
    let persp = &c.perspective;
    vec![
        Row::adjust(
            "Touch controls",
            format!("{:?}", t.mode).to_uppercase(),
            |c, d| {
                let all = [TouchMode::Auto, TouchMode::On, TouchMode::Off];
                c.config.touch.mode = cycle(&all, c.config.touch.mode, d)
            },
        ),
        Row::adjust("Layout", style_label(t.style), |c, d| {
            c.config.touch.style = cycle(&TouchStyle::ALL, c.config.touch.style, d)
        }),
        Row::new("Edit the touch layout", "↵", Act::Open(Screen::TouchLayout)),
        Row::adjust(
            "Controls height (landscape)",
            percent(t.landscape_height),
            |c, d| {
                let h = &mut c.config.touch.landscape_height;
                step(h, d, 0.01, crate::touch::CONTROLS_HEIGHT)
            },
        ),
        Row::adjust(
            "Controls height (portrait)",
            percent(t.portrait_height),
            |c, d| {
                let h = &mut c.config.touch.portrait_height;
                step(h, d, 0.01, crate::touch::CONTROLS_HEIGHT)
            },
        ),
        Row::adjust(
            "Screen size (keys at real size)",
            match t.screen_inches {
                0.0 => format!("AUTO ({:.1}\")", ctx.screen.guessed_inches(ctx.scale)),
                n => format!("{n:.1}\""),
            },
            |c, d| {
                const SIZES: [f32; 19] = [
                    0.0, 5.4, 6.1, 6.7, 7.9, 8.3, 10.2, 10.9, 11.0, 12.9, 13.0, 13.3, 14.0, 15.6,
                    17.0, 24.0, 27.0, 32.0, 43.0,
                ];
                c.config.touch.screen_inches = cycle(&SIZES, c.config.touch.screen_inches, d);
            },
        ),
        Row::adjust("Opacity", percent(t.opacity), |c, d| {
            step(&mut c.config.touch.opacity, d, 0.05, (0.1, 1.0))
        }),
        Row::adjust(
            "START / SELECT side",
            start_select_label(t.start_select_side),
            |c, d| {
                let s = &mut c.config.touch.start_select_side;
                *s = cycle(&TouchSide::ALL, *s, d)
            },
        ),
        Row::adjust("Double play in landscape", onoff(t.allow_dp), |c, _| {
            c.config.touch.allow_dp ^= true
        }),
        Row::adjust(
            "Scratch sensitivity (deg)",
            format!("{:.0}", t.scratch_threshold_deg),
            |c, d| {
                let v = &mut c.config.touch.scratch_threshold_deg;
                step(v, d, 1.0, (1.0, 45.0))
            },
        ),
        Row::adjust(
            "Perspective (touch perspective skin)",
            percent(persp.strength),
            |c, d| {
                let v = &mut c.config.perspective.strength;
                step(v, d, 0.05, (0.0, PERSPECTIVE_MAX))
            },
        ),
        Row::adjust("    Lane length", percent(persp.length), |c, d| {
            step(&mut c.config.perspective.length, d, 0.05, LENGTH_RANGE)
        }),
        Row::adjust(
            "    Touch area height",
            percent(persp.touch_height),
            |c, d| {
                let v = &mut c.config.perspective.touch_height;
                step(v, d, 0.01, TOUCH_HEIGHT_RANGE)
            },
        ),
        Row::adjust("Play side (1P/2P)", format!("{:?}", c.play.side), flip_side),
    ]
}
