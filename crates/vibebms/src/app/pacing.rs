//! Frame pacing: the moment a frame being made will be on the screen (what
//! is drawn for it) and, with vsync, when to start making the next.
//!
//! Frames start at uneven moments (the event loop, the OS scheduler, input
//! handled in between) while a vsync display shows them on its own even
//! grid, so a scene drawn for the moment its frame starts moves unevenly.
//! Here the time drawn for advances by the measured frame period and only
//! drifts toward the clock (a part of the difference each frame); after a
//! missed vblank or a stall it jumps by whole periods, as the display does.

use crate::platform::{Duration, Instant};
use std::collections::VecDeque;

/// The share of the difference to the clock taken up each frame.
const DRIFT: f64 = 0.05;
/// The frame period is measured over this many frames.
const WINDOW: usize = 64;
/// With vsync, how long a frame should wait for its swapchain image (µs):
/// enough to keep the queue full, short enough that input that comes
/// meanwhile is handled about when it comes. (Where the display holds
/// frames back elsewhere, as OpenGL does in presenting, no wait is seen:
/// the frames come early and are held back there, as without pacing.)
const WAIT_US: f64 = 1000.0;
/// The share of the wait beyond that by which the next frame starts later
/// (at most a quarter of a period a frame).
const WAIT_GAIN: f64 = 0.5;

/// What is known of the frames, in µs since the first.
#[derive(Default)]
pub struct Pacer {
    epoch: Option<Instant>,
    /// The display's refresh period (0: not known).
    refresh: f64,
    /// The last frames' starts.
    starts: VecDeque<f64>,
    /// When the last frame was to start (its timer wakes it a little late).
    planned: f64,
    /// The moment drawn for the last frame.
    shown: f64,
}

impl Pacer {
    /// The display's refresh rate (Hz; 0: not known).
    pub fn set_refresh(&mut self, hz: f64) {
        self.refresh = if hz > 0.0 { 1e6 / hz } else { 0.0 };
    }

    fn instant(&self, us: f64) -> Instant {
        let epoch = self.epoch.unwrap_or_else(Instant::now);
        epoch + Duration::from_secs_f64(us.max(0.0) / 1e6)
    }

    /// The frame period: the time over the window by the periods in it,
    /// each interval counted in the usual one (the median: a stall or a
    /// missed vblank is many); the refresh period until there are frames.
    fn period(&self) -> f64 {
        let n = self.starts.len().saturating_sub(1);
        if n < 8 {
            return self.refresh;
        }
        let mut d = [0.0; WINDOW];
        for (x, (a, b)) in d
            .iter_mut()
            .zip(self.starts.iter().zip(self.starts.iter().skip(1)))
        {
            *x = b - a;
        }
        let d = &mut d[..n];
        let unit = *d.select_nth_unstable_by(n / 2, f64::total_cmp).1;
        let periods: f64 = d.iter().map(|x| (x / unit).round()).sum();
        let span = self.starts.back().unwrap_or(&0.0) - self.starts.front().unwrap_or(&0.0);
        match periods > 0.0 {
            true => span / periods,
            false => unit,
        }
    }

    /// A frame starting at `start`: the moment to draw it for. With vsync
    /// (`lead`: the periods after its start it is on the screen, the frames
    /// queued before it) it moves by whole periods plus a small part of its
    /// distance to the clock, never goes back and is never before `start`.
    /// Without, a frame is shown at the next vblank, which is not known:
    /// `start`.
    pub fn frame(&mut self, start: Instant, lead: Option<f64>) -> Instant {
        let first = self.epoch.is_none();
        let epoch = *self.epoch.get_or_insert(start);
        let s = start.saturating_duration_since(epoch).as_secs_f64() * 1e6;
        self.starts.push_back(s);
        if self.starts.len() > WINDOW + 1 {
            self.starts.pop_front();
        }
        let Some(lead) = lead else {
            self.shown = s;
            return start;
        };
        let p = self.period();
        let target = s + lead * p;
        let shown = if first || p <= 0.0 {
            target
        } else {
            // the vblanks since the last frame
            let n = ((target - self.shown) / p).round().max(0.0);
            let on_grid = self.shown + n * p;
            on_grid + DRIFT * (target - on_grid)
        };
        self.shown = shown.max(self.shown).max(s);
        self.instant(self.shown)
    }

    /// With vsync, the refresh rate known: when to start the next frame,
    /// given how long this one waited for its image. A refresh period
    /// after this one was to start (after it started, when it was half a
    /// period late or more), later by part of the wait beyond [`WAIT_US`],
    /// earlier when it did not wait (the queue is not full, or the frame
    /// was late, or the rate told is a little slow). Not the measured
    /// period: these frames' starts are the timer's.
    pub fn next_start(&mut self, waited: Duration) -> Instant {
        let w = waited.as_secs_f64() * 1e6;
        let p = self.refresh;
        let shift = (WAIT_GAIN * (w - WAIT_US)).clamp(-0.25 * p, 0.25 * p);
        let start = self.starts.back().copied().unwrap_or(0.0);
        let from = match (start - self.planned).abs() < 0.5 * p {
            true => self.planned,
            false => start,
        };
        self.planned = from + p + shift;
        self.instant(self.planned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A random number in [0, 1) (a fixed sequence).
    fn rng(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    fn us(t: Instant, t0: Instant) -> f64 {
        t.duration_since(t0).as_secs_f64() * 1e6
    }

    /// Frames shown on a 120 Hz grid but started up to 3 ms late at random
    /// (over a third of a frame): what is drawn for them advances by the
    /// period within 2 % (the start times' steps vary by up to 36 %).
    #[test]
    fn jittery_starts_give_even_steps() {
        let t0 = Instant::now();
        let period = 1e6 / 120.0;
        let mut p = Pacer::default();
        p.set_refresh(120.0);
        let mut seed = 3;
        let mut shown = Vec::new();
        for k in 0..2000 {
            let start = k as f64 * period + rng(&mut seed) * 3000.0;
            let at = t0 + Duration::from_secs_f64(start / 1e6);
            shown.push(us(p.frame(at, Some(2.0)), t0));
        }
        let steps: Vec<f64> = shown[100..].windows(2).map(|w| w[1] - w[0]).collect();
        let worst = steps.iter().map(|s| (s - period).abs()).fold(0.0, f64::max);
        eprintln!("worst step error {worst:.1} µs");
        assert!(worst < period * 0.02, "worst step error {worst} µs");
        // and it stays with the clock: two periods after the start
        let k = shown.len() - 1;
        let lag = shown[k] - (k as f64 * period + 1500.0);
        assert!((lag - 2.0 * period).abs() < 1600.0, "{lag}");
    }

    /// A missed vblank (a frame two periods late) is a step of two periods,
    /// then even steps again on the same grid.
    #[test]
    fn a_missed_vblank_skips_a_whole_period() {
        let t0 = Instant::now();
        let period = 1e6 / 60.0;
        let mut p = Pacer::default();
        p.set_refresh(60.0);
        let mut shown = Vec::new();
        for k in (0..200).filter(|&k| k != 100) {
            let at = t0 + Duration::from_secs_f64(k as f64 * period / 1e6);
            shown.push(us(p.frame(at, Some(1.0)), t0));
        }
        for (i, w) in shown.windows(2).enumerate().skip(1) {
            let want = if i == 99 { 2.0 * period } else { period };
            assert!(
                (w[1] - w[0] - want).abs() < 1.0,
                "step {i}: {}",
                w[1] - w[0]
            );
        }
    }

    /// The display runs at half the rate expected (120 Hz expected, 60
    /// shown, as on an iPhone without the ProMotion key): two periods a frame.
    #[test]
    fn half_the_rate_expected_steps_two_periods() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        p.set_refresh(120.0);
        let mut t = 0.0;
        let mut last = 0.0;
        let mut step = 0.0;
        for _ in 0..40 {
            t += 1e6 / 60.0;
            let s = us(
                p.frame(t0 + Duration::from_secs_f64(t / 1e6), Some(2.0)),
                t0,
            );
            step = s - last;
            last = s;
        }
        assert!((step - 1e6 / 60.0).abs() < 50.0, "{step}");
    }

    /// No refresh rate known, a long stall after the first frame (loading),
    /// then frames every 10 ms: steps of 10 ms (within 1 %) from the 40th.
    #[test]
    fn a_stall_does_not_set_the_period() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        let mut shown = Vec::new();
        for t in std::iter::once(0.0).chain((0..100).map(|k| 400_000.0 + k as f64 * 10_000.0)) {
            shown.push(us(
                p.frame(t0 + Duration::from_secs_f64(t / 1e6), Some(0.5)),
                t0,
            ));
        }
        for w in shown[40..].windows(2) {
            assert!((w[1] - w[0] - 10_000.0).abs() < 100.0, "{}", w[1] - w[0]);
        }
    }

    /// With vsync the next frame starts a period after this one, later when
    /// this one waited long for its image.
    #[test]
    fn the_next_start_follows_the_wait() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        p.set_refresh(100.0);
        p.frame(t0, Some(2.0));
        let mut next = |ms| us(p.next_start(Duration::from_millis(ms)), t0);
        assert_eq!(next(0), 9_500.0);
        assert_eq!(next(5), 12_000.0);
        assert_eq!(next(1), 10_000.0);
    }

    /// A FIFO display with three images (a frame gets its image when the
    /// one two before it is on the screen), frames that start 1 ms after
    /// they were to (a timer's wake-up) and take 2 ms to their image: after
    /// two seconds every vblank shows a new frame, and frames wait for
    /// their image under 1.5 ms (without pacing: most of a period). Also
    /// with the rate told a little wrong (Windows tells 59 Hz for 59.94).
    #[test]
    fn vsync_pacing_keeps_the_queue_full_without_long_waits() {
        for (hz, told) in [(60.0, 60.0), (59.94, 59.0), (59.94, 60.0), (144.0, 144.0)] {
            let t0 = Instant::now();
            let period = 1e6 / hz;
            let mut p = Pacer::default();
            p.set_refresh(told);
            let mut shown_at: Vec<f64> = Vec::new();
            let mut start = 0.0;
            let mut waits = Vec::new();
            for k in 0..(hz as usize * 20) {
                p.frame(t0 + Duration::from_secs_f64(start / 1e6), Some(2.0));
                let call = start + 2000.0;
                let free = match k {
                    0..=1 => call,
                    _ => shown_at[k - 2],
                };
                let got = call.max(free);
                let vblank = ((got + 1000.0) / period).ceil() * period;
                shown_at.push(vblank.max(shown_at.last().map_or(0.0, |s| s + period)));
                waits.push(got - call);
                let next = us(
                    p.next_start(Duration::from_secs_f64((got - call) / 1e6)),
                    t0,
                );
                start = next.max(got + 1000.0) + 1000.0;
            }
            let settled = hz as usize * 2;
            for w in shown_at[settled..].windows(2) {
                assert!(
                    (w[1] - w[0] - period).abs() < 1.0,
                    "{hz} Hz told {told}: a vblank without a new frame"
                );
            }
            let worst = waits[settled..].iter().copied().fold(0.0, f64::max);
            assert!(worst < 1500.0, "{hz} Hz told {told}: waited {worst} µs");
        }
    }
}
