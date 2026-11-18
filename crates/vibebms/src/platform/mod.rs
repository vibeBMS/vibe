//! Platform abstraction: time, background work, file system and HTTP.
//!
//! Native builds map straight to `std`; the web build (wasm32) uses an
//! in-memory file system fed by the browser's folder picker, runs background
//! jobs on the event loop and uses the browser's `fetch` for HTTP.

#[cfg(target_os = "android")]
pub mod android;
mod drive;
pub mod http;
#[cfg(target_os = "ios")]
pub mod ios;
pub mod open;
#[cfg(target_arch = "wasm32")]
pub mod opfs;
pub mod screen;
pub mod store;
mod thread;
mod time;
pub mod vfs;

pub use drive::{load_table, run_job};
pub use http::{http_get, stream};
pub use thread::{after, parallel_for, spawn, Job};
#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
pub use time::event_instant;
pub use time::{unix_ms, unix_now, Duration, Instant, SystemTime, UNIX_EPOCH};

pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

/// Open `url` in the system browser (a new tab on the web).
pub fn open_url(url: &str) -> bool {
    match webbrowser::open(url) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("open {url}: {e}");
            false
        }
    }
}
