//! Platform abstraction: time, background work, file system and HTTP.
//!
//! Native builds map straight to `std`; the web build (wasm32) uses an
//! in-memory file system fed by the browser's folder picker, runs background
//! jobs on the event loop and uses the browser's `fetch` for HTTP.

#[cfg(target_os = "android")]
pub mod android;
pub mod arcfs;
mod drive;
mod files;
#[cfg(any(target_os = "ios", test))]
pub mod hid;
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

#[cfg(test)]
pub(crate) use drive::in_page;
pub use drive::{load_table, run_job, Launch};
pub use files::Vfs;
pub use http::{http_get, stream};
pub use thread::{after, parallel_for, spawn, Job};
#[cfg(any(target_os = "ios", target_arch = "wasm32", test))]
pub use time::event_instant;
pub use time::{unix_ms, unix_now, Duration, Instant, SystemTime, UNIX_EPOCH};

pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

/// Whether `url` is a link for a browser: `http://` or `https://`, a
/// host, and written as every URL parser reads it (BMSeeker's rule for a
/// download's links): no user name or password before the host, no
/// backslash, no white space or control character. Links come from
/// charts, tables, indexes and catalogs: a browser or a web view given
/// another scheme (`file:`, `javascript:`, an application's) would act on
/// it, and one given `https://trusted.example@evil.example/` or a
/// backslash goes to a host other than the one the link seems to name.
pub fn is_web_url(url: &str) -> bool {
    let rest = ["http://", "https://"].iter().find_map(|scheme| {
        let head = url.get(..scheme.len())?;
        head.eq_ignore_ascii_case(scheme)
            .then(|| &url[scheme.len()..])
    });
    let Some(rest) = rest else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty()
        && !authority.contains('@')
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

/// Open `url` (a web link: [`is_web_url`]) in the system browser (a new
/// tab on the web).
pub fn open_url(url: &str) -> bool {
    if !is_web_url(url) {
        log::warn!("open {url:?}: not a web link");
        return false;
    }
    match webbrowser::open(url) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("open {url}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_are_opened() {
        for ok in [
            "http://a",
            "https://example.com/a?b=c#d",
            "HTTPS://Example.com/",
            "http://127.0.0.1:8080/x",
            // what honest links hold after the host
            "https://host/a@b.zip",
            "https://host/dl?mail=a@b",
            "https://host/x#a@b",
            "https://host:8443/x",
            "http://[::1]:8080/x",
            "https://[2001:db8::1]/x",
            "https://host/%E6%97%A5%20x.zip",
            "https://host/日本語.zip",
            "https://日本.jp/x",
        ] {
            assert!(is_web_url(ok), "{ok}");
        }
        for bad in [
            "",
            "https://",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "file:///etc/passwd",
            "file://host/share/x.html",
            "content://media/x",
            "intent://x#Intent;end",
            "data:text/html,x",
            "about:blank",
            "ftp://host/x",
            "httpx://a",
            "http:/a",
            "C:\\x.html",
            "\\\\host\\share\\x.html",
            "/etc/passwd",
            "x.html",
            " https://a",
            "https://a b",
            "https://a\nb",
            "https://a\u{7f}",
            // not plainly written: another host than it seems
            "https://user@host/x",
            "https://user:pw@host/x",
            "https://trusted.example@evil.example/x",
            "https://@host/x",
            "https://host\\x",
            "https://host/x\\y",
            "https://host/x\u{7}",
            "https://host/x y",
            "https:///x",
            "https://?x",
        ] {
            assert!(!is_web_url(bad), "{bad}");
            // (refused before a browser is looked for)
            assert!(!open_url(bad), "{bad}");
        }
    }
}
