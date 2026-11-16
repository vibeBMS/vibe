//! Downloads from where authors link songs: `bmseeker::hosts`, and the CORS
//! proxy the web build reaches those hosts through.

pub use bmseeker::hosts::*;

/// The CORS proxy the web build reaches download hosts and difficulty
/// tables through (none sends CORS headers): vibeBMS/proxy `worker.js`.
pub const WEB_PROXY: &str = "https://proxy.vibebms.workers.dev/";

static THIRD_PARTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether third parties' copies are used (the "Third-party mirrors"
/// setting: bmseeker's `third_party`).
pub fn set_third_party(on: bool) {
    THIRD_PARTY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// What the app resolves links with: bmseeker's default settings and hosts
/// (third parties' copies too, when [`set_third_party`]).
pub fn context() -> &'static Context {
    use std::sync::LazyLock;
    static OWN: LazyLock<Context> = LazyLock::new(|| Context::new(Default::default()));
    static ALL: LazyLock<Context> = LazyLock::new(|| {
        Context::new(bmseeker::config::Config {
            third_party: true,
            ..Default::default()
        })
    });
    if THIRD_PARTY.load(std::sync::atomic::Ordering::Relaxed) {
        &ALL
    } else {
        &OWN
    }
}

/// Whether `url`'s host sends CORS headers (a web page reads it itself):
/// BMS SEARCH's API.
pub fn sends_cors(url: &str) -> bool {
    let host = url.split('/').nth(2).unwrap_or("").to_ascii_lowercase();
    host == "api.bmssearch.net"
}

/// `url` as the web build fetches it.
pub fn via_proxy(url: &str) -> String {
    format!("{WEB_PROXY}?url={}", url_encode(url))
}
