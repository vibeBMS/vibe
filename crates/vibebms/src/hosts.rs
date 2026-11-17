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

static FOLDER_NAMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a song's folder is named after its charts (the "Name song
/// folders after their charts" setting: bmseeker's `folder_naming`).
pub fn set_folder_naming(on: bool) {
    FOLDER_NAMING.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// What a song's download installs with: [`context`], its folder named as
/// [`set_folder_naming`] says.
pub fn install_context() -> Context {
    let cx = context();
    if !FOLDER_NAMING.load(std::sync::atomic::Ordering::Relaxed) {
        return cx.clone();
    }
    Context {
        config: std::sync::Arc::new(bmseeker::config::Config {
            folder_naming: true,
            ..(*cx.config).clone()
        }),
        ..cx.clone()
    }
}

/// What the app resolves links with: bmseeker's default settings and hosts
/// (third parties' copies too, when [`set_third_party`]); on the web a
/// download that fails is a page (the proxy passes archives only).
pub fn context() -> &'static Context {
    use std::sync::LazyLock;
    fn new(config: bmseeker::config::Config) -> Context {
        Context {
            failures_are_pages: crate::platform::IS_WEB,
            ..Context::new(config)
        }
    }
    static OWN: LazyLock<Context> = LazyLock::new(|| new(Default::default()));
    static ALL: LazyLock<Context> = LazyLock::new(|| {
        new(bmseeker::config::Config {
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
/// BMS SEARCH's API, GitHub's raw files (the event mirror's lists).
pub fn sends_cors(url: &str) -> bool {
    let host = url.split('/').nth(2).unwrap_or("").to_ascii_lowercase();
    host == "api.bmssearch.net" || host == "raw.githubusercontent.com"
}

/// `url` as the web build fetches it.
pub fn via_proxy(url: &str) -> String {
    format!("{WEB_PROXY}?url={}", bmseeker::net::url_encode(url))
}
