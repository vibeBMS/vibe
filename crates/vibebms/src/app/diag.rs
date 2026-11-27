//! Latency diagnostics (`SETTINGS › DISPLAY & AUDIO › Latency overlay`): the
//! distributions (median, 95th percentile, maximum) of the audio output's
//! callback interval, the delay from an input event's time to its handling,
//! and the frame time, over the last `KEPT` of each, drawn in a corner.
//! Off, nothing is recorded (a relaxed load per sample). The start's phases
//! are timed in the log (`startup:` lines) always.

use super::Ctx;
use crate::gfx::text::{Align, TextStyle};
use crate::gfx::Draw;
use crate::input::InputEv;
use crate::platform::Instant;
use crate::theme;
use std::sync::{Mutex, OnceLock};
use vibebms_audio::diag::{self, Samples, Stats};

/// From one frame's start to the next's.
pub static FRAME: Samples = Samples::new();
/// From an input event's time (when the system or the device says it
/// happened; else when the window got it) to its handling by the scene.
pub static INPUT: Samples = Samples::new();

/// The overlay is on (the measures recorded), as the config says.
pub fn set(on: bool) {
    if on != diag::enabled() {
        diag::set_enabled(on);
        // a new start for the numbers
        for s in [&FRAME, &INPUT, &diag::AUDIO_CALLBACK] {
            s.clear();
        }
    }
}

/// An input event about to be handled.
pub fn input(ev: &InputEv, now: Instant) {
    if !diag::enabled() {
        return;
    }
    let time = match ev {
        InputEv::Key { time, .. }
        | InputEv::Pad { time, .. }
        | InputEv::Axis { time, .. }
        | InputEv::Pointer { time, .. } => *time,
        _ => return,
    };
    INPUT.record(now.saturating_duration_since(time).as_micros() as u64);
}

/// A frame starts at `now` (`last`: when the one before did).
pub fn frame(last: Option<Instant>, now: Instant) {
    if let Some(l) = last.filter(|_| diag::enabled()) {
        FRAME.record(now.saturating_duration_since(l).as_micros() as u64);
    }
}

/// "p50 5.33  p95 5.40  max 10.67 ms (512)".
fn line(name: &str, s: Option<Stats>) -> String {
    let ms = |us: u32| us as f64 / 1000.0;
    match s {
        Some(s) => format!(
            "{name:<7} p50 {:>6.2}  p95 {:>6.2}  max {:>6.2} ms  ({})",
            ms(s.p50),
            ms(s.p95),
            ms(s.max),
            s.n
        ),
        None => format!("{name:<7} —"),
    }
}

/// The overlay, top left.
pub fn draw(ctx: &Ctx, d: &mut Draw) {
    if !ctx.config.display.show_latency {
        return;
    }
    let lines = [
        line("FRAME", FRAME.stats()),
        line("AUDIO", diag::AUDIO_CALLBACK.stats()),
        line("INPUT", INPUT.stats()),
    ];
    let size = 13.0;
    let w = lines.iter().map(|l| d.measure(l, size)).fold(0.0, f32::max) + 16.0;
    let r = crate::gfx::Rect::new(4.0, 4.0, w, lines.len() as f32 * 17.0 + 10.0);
    d.b.rect(r, [0.0, 0.0, 0.0, 0.7]);
    for (k, l) in lines.iter().enumerate() {
        d.text(
            l,
            r.x + 8.0,
            r.y + 5.0 + k as f32 * 17.0,
            TextStyle::new(size, theme::TEXT, Align::Left),
        );
    }
}

/// The start: when it began, and the last phase's end.
static START: OnceLock<Instant> = OnceLock::new();
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// A phase of the start ended: logged with its time and the time since
/// the start (the first call starts the clock).
pub fn startup(phase: &str) {
    let now = Instant::now();
    let start = *START.get_or_init(|| now);
    let Ok(mut last) = LAST.lock() else { return };
    let since = last.unwrap_or(start);
    *last = Some(now);
    if phase.is_empty() {
        return;
    }
    let ms = |a: Instant| now.saturating_duration_since(a).as_secs_f64() * 1000.0;
    log::info!(
        "startup: {phase} {:.1} ms ({:.1} ms since start)",
        ms(since),
        ms(start)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read() {
        assert_eq!(line("AUDIO", None), "AUDIO   —");
        let s = Stats {
            p50: 5333,
            p95: 5400,
            max: 10667,
            n: 512,
        };
        assert_eq!(
            line("AUDIO", Some(s)),
            "AUDIO   p50   5.33  p95   5.40  max  10.67 ms  (512)"
        );
    }
}
