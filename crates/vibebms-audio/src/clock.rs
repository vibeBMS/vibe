//! The mixer's frames against the judge's clock.
//!
//! The judge times notes with the system clock (`Instant`); the output
//! device plays frames at its own rate, which can be off by a few hundred
//! ppm (12 ms after 5 minutes at 40 ppm). Sounds of the chart are therefore
//! scheduled as deadlines on the system clock and turned into frames in the
//! audio callback, with a mapping fitted from what the callbacks report:
//! each buffer's first frame and when it was asked for (the stream's
//! callback timestamp, which hosts take from a precise clock, put on the
//! system clock by the smallest gap seen lately between the two; without
//! usable timestamps the frames at the nominal rate, put on the system
//! clock the same way, which follows the drift).
//!
//! All times are µs on the system clock since the mixer's epoch.

use std::collections::VecDeque;

/// Fit points are kept at least this far apart (µs).
const SPACING_US: f64 = 100_000.0;
/// The fit uses this many points (about 12 s).
const MAX_POINTS: usize = 120;
/// The stream-to-system offset is the smallest gap of this long (µs).
const OFFSET_WINDOW_US: f64 = 1_000_000.0;

/// What one callback reports: the frame its buffer starts with, the
/// stream's timestamp of the callback (µs, any origin; `None` when the host
/// gives none that can be used) and the system clock in the callback.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Obs {
    pub frame: u64,
    pub frames: usize,
    pub stream_us: Option<f64>,
    pub now_us: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameClock {
    rate: f64,
    /// (frame, stream µs) fit points.
    pts: VecDeque<(f64, f64)>,
    /// (system µs, system − stream) of recent callbacks.
    gaps: VecDeque<(f64, f64)>,
    /// The stream's timestamps are not used (missing or inconsistent).
    no_stream: bool,
    /// The last callback: frame, stream µs.
    last: Option<(f64, f64)>,
    /// t = t0 + (frame - f0) * us_per_frame, on the stream clock.
    fit: Option<(f64, f64, f64)>,
    offset: f64,
    /// Half the last buffer's length (µs): when, on average, a key pressed
    /// at some moment starts sounding after it.
    half_buffer_us: f64,
}

impl FrameClock {
    pub fn new(rate: u32) -> FrameClock {
        FrameClock {
            rate: rate.max(1) as f64,
            pts: VecDeque::new(),
            gaps: VecDeque::new(),
            no_stream: false,
            last: None,
            fit: None,
            offset: 0.0,
            half_buffer_us: 0.0,
        }
    }

    pub fn observe(&mut self, o: Obs) {
        let frame = o.frame as f64;
        self.half_buffer_us = o.frames as f64 * 500_000.0 / self.rate;
        let stream = match o.stream_us {
            Some(s) if !self.no_stream => {
                // a host clock that goes back, or runs far from the frames,
                // is not used from then on
                let sane = self.last.is_none_or(|(f, t)| {
                    let df = (frame - f) * 1_000_000.0 / self.rate;
                    let dt = s - t;
                    dt > 0.0 && (df <= 0.0 || (dt / df > 0.5 && dt / df < 2.0))
                });
                if sane {
                    Some(s)
                } else {
                    log::warn!("audio: the stream's timestamps are not usable");
                    None
                }
            }
            _ => None,
        };
        let stream = match stream {
            Some(s) => s,
            None => {
                if !self.no_stream {
                    self.no_stream = true;
                    self.pts.clear();
                    self.gaps.clear();
                    self.fit = None;
                }
                // without the host's clock the frames are the clock: on
                // the system clock by the smallest gap, which follows the
                // drift
                frame * 1_000_000.0 / self.rate
            }
        };
        self.last = Some((frame, stream));
        // the stream clock on the system clock: the smallest gap lately
        // (a callback runs a little after its timestamp, never before)
        self.gaps.push_back((o.now_us, o.now_us - stream));
        while self
            .gaps
            .front()
            .is_some_and(|g| g.0 < o.now_us - OFFSET_WINDOW_US)
        {
            self.gaps.pop_front();
        }
        self.offset = self.gaps.iter().map(|g| g.1).fold(f64::INFINITY, f64::min);
        if self.pts.back().is_none_or(|p| stream - p.1 >= SPACING_US) {
            self.pts.push_back((frame, stream));
            if self.pts.len() > MAX_POINTS {
                self.pts.pop_front();
            }
            self.refit();
        }
    }

    /// Least squares over the points (the nominal rate until there are two
    /// a second apart).
    fn refit(&mut self) {
        let n = self.pts.len() as f64;
        let (fm, tm) = self
            .pts
            .iter()
            .fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
        let span = self.pts.back().map_or(0.0, |b| b.1) - self.pts.front().map_or(0.0, |f| f.1);
        let (mut sxy, mut sxx) = (0.0, 0.0);
        for (f, t) in &self.pts {
            sxy += (f - fm) * (t - tm);
            sxx += (f - fm) * (f - fm);
        }
        let nominal = 1_000_000.0 / self.rate;
        let slope = if span >= 1_000_000.0 && sxx > 0.0 {
            // a rate more than 1 % off is not a clock error: keep nominal
            (sxy / sxx).clamp(nominal * 0.99, nominal * 1.01)
        } else {
            nominal
        };
        self.fit = Some((fm, tm, slope));
    }

    /// The frame to start a sound on that should sound together with a key
    /// pressed at `us` (a key's sound starts with the next buffer: half a
    /// buffer later on average).
    pub fn frame_at(&self, us: f64) -> f64 {
        let us = us + self.half_buffer_us;
        match (self.fit, self.last) {
            (Some((f0, t0, slope)), Some((fl, tl))) => {
                // anchored at the last callback, so a fit a little off
                // in offset does not move sounds near now
                let fitted_last = t0 + (fl - f0) * slope;
                let anchor = 0.5 * (fitted_last + tl);
                fl + (us - self.offset - anchor) / slope
            }
            _ => us * self.rate / 1_000_000.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device off by `ppm` from the system clock, with callbacks of
    /// `buf` frames that run up to 2 ms late; its timestamps on another
    /// origin.
    fn simulate(ppm: f64, buf: usize, stream: bool, check: &mut dyn FnMut(&FrameClock, f64)) {
        let rate = 48_000.0;
        let true_rate = rate * (1.0 + ppm * 1e-6);
        let mut c = FrameClock::new(rate as u32);
        let mut seed = 7u64;
        let mut frame = 0u64;
        while (frame as f64) < true_rate * 600.0 {
            let t = frame as f64 * 1_000_000.0 / true_rate;
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let late = (seed >> 33) as f64 / (1u64 << 31) as f64 * 2_000.0;
            c.observe(Obs {
                frame,
                frames: buf,
                stream_us: stream.then_some(t + 5e9),
                now_us: t + late,
            });
            check(&c, t);
            frame += buf as u64;
        }
    }

    #[test]
    fn deadlines_follow_a_drifting_device_for_ten_minutes() {
        for ppm in [-200.0, -40.0, 0.0, 40.0, 200.0] {
            for stream in [true, false] {
                let true_rate = 48_000.0 * (1.0 + ppm * 1e-6);
                let mut worst: f64 = 0.0;
                let mut n = 0;
                simulate(ppm, 256, stream, &mut |c, t| {
                    n += 1;
                    if n % 50 != 0 {
                        return;
                    }
                    // a sound due 150 ms from now
                    let d = t + 150_000.0;
                    let f = c.frame_at(d);
                    let heard = f * 1_000_000.0 / true_rate;
                    let want = d + 256.0 * 500_000.0 / 48_000.0;
                    if t > 1_000_000.0 {
                        worst = worst.max((heard - want).abs());
                    }
                });
                // the nominal-rate schedule would be off by up to 120 ms
                let limit = if stream { 300.0 } else { 400.0 };
                assert!(worst < limit, "{ppm} ppm, stream {stream}: {worst} us");
            }
        }
    }

    #[test]
    fn a_host_clock_that_jumps_back_is_dropped() {
        let mut c = FrameClock::new(48_000);
        for (i, s) in [1000.0, 6333.0, 500.0, 11666.0].iter().enumerate() {
            c.observe(Obs {
                frame: i as u64 * 256,
                frames: 256,
                stream_us: Some(*s),
                now_us: i as f64 * 5333.0,
            });
        }
        assert!(c.no_stream);
        // the frames on the system clock: frame 0 at 0
        let f = c.frame_at(100_000.0 - c.half_buffer_us);
        assert!((f - 4_800.0).abs() < 2.0, "{f}");
    }

    #[test]
    fn nominal_before_any_callback() {
        let c = FrameClock::new(44_100);
        assert_eq!(c.frame_at(1_000_000.0), 44_100.0);
    }
}
