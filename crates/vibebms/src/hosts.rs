//! Downloads from where authors link songs: `bmseeker::hosts`, and the CORS
//! proxy the web build reaches those hosts through.

pub use bmseeker::hosts::*;

/// The CORS proxy the web build reaches download hosts and difficulty
/// tables through (none sends CORS headers): vibeBMS/proxy `worker.js`.
pub const WEB_PROXY: &str = "https://proxy.vibebms.workers.dev/";

/// What the app resolves links with: bmseeker's default settings and hosts.
pub fn context() -> &'static Context {
    static CX: std::sync::LazyLock<Context> = std::sync::LazyLock::new(Context::default);
    &CX
}

/// `url` as the web build fetches it.
pub fn via_proxy(url: &str) -> String {
    format!("{WEB_PROXY}?url={}", url_encode(url))
}
