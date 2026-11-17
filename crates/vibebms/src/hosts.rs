//! Downloads from where authors link songs: `bmseeker::hosts`, and the CORS
//! proxy the web build reaches those hosts through.

pub use bmseeker::hosts::*;
#[cfg(not(target_arch = "wasm32"))]
pub use bmseeker::native::{fetch_files, Native};

/// The CORS proxy the web build reaches download hosts and difficulty
/// tables through (none sends CORS headers): vibeBMS/proxy `worker.js`.
pub const WEB_PROXY: &str = "https://proxy.vibebms.workers.dev/";

/// What the app resolves links and installs songs with: bmseeker's default
/// settings and hosts, with the player's choices (third parties' copies
/// used, a song's folder named after its charts); on the web a download
/// that fails is a page (the proxy passes archives only).
pub fn context(config: &crate::config::Config) -> Context {
    Context {
        failures_are_pages: crate::platform::IS_WEB,
        ..Context::new(bmseeker::config::Config {
            third_party: config.third_party_mirrors,
            folder_naming: config.name_song_folders,
            ..Default::default()
        })
    }
}

/// Whether `url`'s host sends CORS headers (a web page reads it itself):
/// BMS SEARCH's API, GitHub's raw files (the event mirror's lists),
/// Konmai's (a chart's package by its MD5).
pub fn sends_cors(url: &str) -> bool {
    let host = url.split('/').nth(2).unwrap_or("").to_ascii_lowercase();
    matches!(
        host.as_str(),
        "api.bmssearch.net" | "raw.githubusercontent.com" | "bms.alvorna.com"
    )
}

/// `url` as the web build fetches it.
pub fn via_proxy(url: &str) -> String {
    format!("{WEB_PROXY}?url={}", bmseeker::net::url_encode(url))
}
