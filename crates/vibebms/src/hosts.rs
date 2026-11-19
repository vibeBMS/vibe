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
            user_agent: user_agent(&bmseeker::config::Config::default().user_agent),
            third_party: config.third_party_mirrors,
            folder_naming: config.name_song_folders,
            ..Default::default()
        })
    }
}

/// What bmseeker's own requests (native downloads) are sent with:
/// vibeBMS's name and version before bmseeker's default, `vibeBMS/0.1.0
/// BMSeeker/0.1.0 (+…)`. A default that is a browser's (`Mozilla/…`) is
/// never sent: vibeBMS's own User-Agent instead. On the web the browser
/// sends its own, whatever this says.
pub fn user_agent(bmseeker_default: &str) -> String {
    if bmseeker_default.is_empty() || bmseeker_default.starts_with("Mozilla/") {
        crate::platform::http::USER_AGENT.to_string()
    } else {
        format!("vibeBMS/{} {bmseeker_default}", crate::update::VERSION)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// bmseeker's downloads name vibeBMS before bmseeker; a browser's
    /// User-Agent is never sent.
    #[test]
    fn downloads_name_vibebms_and_bmseeker() {
        let v = crate::update::VERSION;
        let named = "BMSeeker/0.2.0 (+https://github.com/vibeBMS/BMSeeker)";
        assert_eq!(
            user_agent(named),
            format!("vibeBMS/{v} BMSeeker/0.2.0 (+https://github.com/vibeBMS/BMSeeker)")
        );
        let browser = "Mozilla/5.0 (X11; Linux x86_64) Chrome/126 Safari/537.36";
        assert_eq!(user_agent(browser), crate::platform::http::USER_AGENT);
        assert_eq!(user_agent(""), crate::platform::http::USER_AGENT);
        // what the app's downloads are sent with, from bmseeker's default
        let ua = context(&Default::default()).config.user_agent.clone();
        assert!(ua.starts_with(&format!("vibeBMS/{v}")), "{ua}");
        assert!(!ua.contains("Mozilla"), "{ua}");
    }
}
