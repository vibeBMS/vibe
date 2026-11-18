//! Latency diagnostics: the last few hundred samples of a time (µs), kept
//! lock-free so that the audio callback can record them, and their
//! distribution. Nothing is recorded while they are off ([`set_enabled`]):
//! one relaxed load per sample.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Samples kept per measure.
pub const KEPT: usize = 512;

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Record the measures (the latency overlay is shown).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The last `KEPT` samples of one measure (µs).
pub struct Samples {
    slots: [AtomicU32; KEPT],
    next: AtomicUsize,
}

impl Default for Samples {
    fn default() -> Self {
        Self::new()
    }
}

impl Samples {
    pub const fn new() -> Samples {
        Samples {
            slots: [const { AtomicU32::new(0) }; KEPT],
            next: AtomicUsize::new(0),
        }
    }

    /// A sample (µs; kept when the measures are on).
    pub fn record(&self, us: u64) {
        if !enabled() {
            return;
        }
        let k = self.next.fetch_add(1, Ordering::Relaxed);
        self.slots[k % KEPT].store(us.min(u32::MAX as u64) as u32, Ordering::Relaxed);
    }

    /// Their distribution (`None`: none yet).
    pub fn stats(&self) -> Option<Stats> {
        let n = self.next.load(Ordering::Relaxed).min(KEPT);
        let mut v: Vec<u32> = self.slots[..n]
            .iter()
            .map(|s| s.load(Ordering::Relaxed))
            .collect();
        Stats::of(&mut v)
    }

    /// Forget them (the overlay turned on again).
    pub fn clear(&self) {
        self.next.store(0, Ordering::Relaxed);
    }
}

/// A distribution: median, 95th percentile, maximum (µs), and how many.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub p50: u32,
    pub p95: u32,
    pub max: u32,
    pub n: usize,
}

impl Stats {
    /// Of `v` (sorted here).
    pub fn of(v: &mut [u32]) -> Option<Stats> {
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        Some(Stats {
            p50: at(0.5),
            p95: at(0.95),
            max: v[v.len() - 1],
            n: v.len(),
        })
    }
}

/// The time between the audio output's callbacks (each asks for the next
/// buffer).
pub static AUDIO_CALLBACK: Samples = Samples::new();

/// When the last callback came (µs on the output's clock; 0: none yet).
static LAST_CALLBACK: AtomicU64 = AtomicU64::new(0);

/// A callback at `now_us`: the time since the one before.
pub(crate) fn audio_callback(now_us: f64) {
    if !enabled() {
        LAST_CALLBACK.store(0, Ordering::Relaxed);
        return;
    }
    let now = now_us.max(1.0) as u64;
    let last = LAST_CALLBACK.swap(now, Ordering::Relaxed);
    if last > 0 && now > last {
        AUDIO_CALLBACK.record(now - last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions() {
        let mut v: Vec<u32> = (1..=100).rev().collect();
        let s = Stats::of(&mut v).unwrap();
        assert_eq!((s.p50, s.p95, s.max, s.n), (51, 95, 100, 100));
        assert_eq!(Stats::of(&mut []), None);
        // off: nothing kept; on: the last KEPT
        let m = Samples::new();
        set_enabled(false);
        m.record(5);
        assert_eq!(m.stats(), None);
        set_enabled(true);
        for k in 0..(KEPT as u64 + 10) {
            m.record(k);
        }
        let s = m.stats().unwrap();
        assert_eq!((s.n, s.max), (KEPT, KEPT as u32 + 9));
        set_enabled(false);
    }
}
