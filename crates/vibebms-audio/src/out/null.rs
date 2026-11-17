//! No output device: the mixer goes on by the wall clock, silently, so
//! sounds start and end as if heard and a play does not stall.

use super::cpal::Mix;
use super::us_since;
use crate::clock::{FrameClock, Obs};
use web_time::Instant;

/// The rate and buffer of the silent output when no device opens.
pub(super) const SILENT_RATE: u32 = 44_100;
pub(super) const SILENT_BUFFER: u32 = 512;

/// Frames advance with the wall clock from `since` and frame `base`.
pub(super) struct Silent {
    pub since: Instant,
    pub base: u64,
    pub rate: u32,
}

impl Silent {
    /// From now and the mixer's frame, on a clock fitted anew.
    pub fn start(mix: &mut Mix, rate: u32) -> Silent {
        mix.state.clock = FrameClock::new(rate);
        Silent {
            since: Instant::now(),
            base: mix.state.frame,
            rate,
        }
    }

    /// Render up to now: after a long time unasked (a hidden window) only
    /// the last second, the sounds pause over the rest.
    pub fn render(&self, mix: &mut Mix, epoch: Instant) {
        let rate = self.rate as u64;
        let target = self.base + (self.since.elapsed().as_secs_f64() * rate as f64) as u64;
        mix.state.frame = mix.state.frame.max(target.saturating_sub(rate));
        let since_us = us_since(epoch, self.since);
        while mix.state.frame < target {
            let frame = mix.state.frame;
            let frames = ((target - frame) as usize).min(SILENT_BUFFER as usize);
            // the frames are the clock here
            let t = since_us + (frame - self.base) as f64 * 1e6 / rate as f64;
            let obs = Obs {
                frame,
                frames,
                stream_us: Some(t),
                now_us: t,
            };
            mix.render(frames, Some(obs));
        }
    }
}
