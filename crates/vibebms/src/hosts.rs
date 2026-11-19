//! Downloads from where authors link songs: `bmseeker::hosts` (the CORS
//! proxy the web build reaches those hosts through: `bmseeker::web`).

pub use bmseeker::hosts::{Context, Fetch};
#[cfg(not(target_arch = "wasm32"))]
pub use bmseeker::native::{fetch_files, Native};

/// What the app resolves links and installs songs with: bmseeker's default
/// settings and hosts, with the player's choices (third parties' copies
/// used, a song's folder named after its charts); on the web a download
/// that fails is a page (the proxy passes archives only). Natively its
/// requests name the HTTP library (its own name and version), bmseeker,
/// then vibeBMS ([`PRODUCT`](crate::platform::http::PRODUCT)).
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
