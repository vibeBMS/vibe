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
/// that fails is a page (the proxy passes archives only). Natively its
/// requests name the HTTP library, bmseeker, then vibeBMS
/// ([`PRODUCT`](crate::platform::http::PRODUCT)): `ureq/3.5.2
/// BMSeeker/0.1.0 (+…BMSeeker) vibeBMS/0.1.0 (+…vibe)`.
pub fn context(config: &crate::config::Config) -> Context {
    Context {
        failures_are_pages: crate::platform::IS_WEB,
        ..Context::new(bmseeker::config::Config {
            user_agent: bmseeker::config::user_agent_with(crate::platform::http::PRODUCT),
            third_party: config.third_party_mirrors,
            folder_naming: config.name_song_folders,
            ..Default::default()
        })
    }
}

/// What turning third parties' copies on means, as the settings ask it
/// and as a build that has them on says it once.
pub fn third_party_notice() -> &'static str {
    tr!(
        "These are other people's copies and mirrors of songs, not the links their authors give.\n\
         Whether a site allows this, and whether it is up, differs from site to site.\n\
         Using them is your own responsibility: follow each site's terms and respect the rights of \
         the songs' copyright holders."
    )
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

    /// bmseeker's downloads name the HTTP library, bmseeker and vibeBMS, in
    /// that order; a browser is never named.
    #[test]
    fn downloads_name_ureq_bmseeker_and_vibebms() {
        use bmseeker::config::{native_user_agent, HTTP_CLIENT, USER_AGENT};
        let ua = context(&Default::default()).config.user_agent.clone();
        let vibe = crate::platform::http::PRODUCT;
        assert_eq!(ua, format!("{USER_AGENT} {vibe}"));
        assert!(USER_AGENT.starts_with("BMSeeker/"), "{USER_AGENT}");
        assert!(vibe.starts_with(&format!("vibeBMS/{} (+", crate::update::VERSION)));
        // on the wire, natively
        assert_eq!(
            native_user_agent(&ua),
            format!("{HTTP_CLIENT} {USER_AGENT} {vibe}")
        );
        assert!(!ua.contains("Mozilla"), "{ua}");
    }
}
