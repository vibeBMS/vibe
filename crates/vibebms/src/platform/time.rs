//! Clocks: `std`'s natively, the browser's on the web (`web-time`).

pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch.
pub fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// An input event that happened `lag` seconds before `now` (its own time,
/// told by the OS, against that clock's now): that moment, if it was lately
/// (under 250 ms); else `now`.
#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
pub fn event_instant(now: Instant, lag: f64) -> Instant {
    match (0.0..0.25).contains(&lag) {
        true => now.checked_sub(Duration::from_secs_f64(lag)).unwrap_or(now),
        false => now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_instants_are_recent_ones() {
        let now = Instant::now();
        assert_eq!(
            now.duration_since(event_instant(now, 0.012)),
            Duration::from_millis(12)
        );
        assert_eq!(event_instant(now, -0.001), now);
        assert_eq!(event_instant(now, 3.0), now);
    }

    /// Taps around the notes of a chart, stamped when they happen or when
    /// the OS hands them over, at the display's next refresh (UIKit does
    /// so once a frame): the result's 1 ms histogram of the stamped
    /// offsets. Handed-over stamps of a chart whose notes are on the
    /// 120 Hz frame grid (BPM 200: a 16th is 75 ms, 9 frames; BPM 180:
    /// 10) fill one bin in 8.3, spikes a frame apart; of a chart whose
    /// notes are not (BPM 114), or stamped when they happen, all.
    #[test]
    fn stamps_on_the_frame_grid_make_spikes() {
        let frame = 1000.0 / 120.0;
        let bins = |bpm: f64, handed_over: bool| {
            let sixteenth = 15_000.0 / bpm;
            // the song starts somewhere between two refreshes
            let start = 3.1;
            let mut seed = 7u64;
            let mut hit = std::collections::HashSet::new();
            for k in 0..4000 {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let jitter = ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 80.0;
                let note = start + k as f64 * sixteenth;
                let tap = note + jitter;
                let stamp = match handed_over {
                    true => (tap / frame).ceil() * frame,
                    false => tap,
                };
                hit.insert((stamp - note).floor() as i64);
            }
            hit.len()
        };
        for bpm in [200.0, 180.0] {
            let (handed, own) = (bins(bpm, true), bins(bpm, false));
            assert!(handed <= 11, "BPM {bpm}: {handed} bins");
            assert!(own >= 80, "BPM {bpm}: {own} bins");
        }
        assert!(bins(114.0, true) >= 80);
    }
}
