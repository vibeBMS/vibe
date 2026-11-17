//! Clocks: `std`'s natively, the browser's on the web (`web-time`).

pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
