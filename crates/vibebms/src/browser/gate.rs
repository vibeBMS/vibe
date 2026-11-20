//! What a page in the in-app browser may save: one file at a time, and each
//! only after the player said yes to it, by its name and the host it comes
//! from. Any page shown there can start a download by itself (a link from a
//! table, an uploader's advertisement, a redirect), and what is saved is
//! unpacked into the library.
//!
//! The web view wants its answer the moment a download starts, so the first
//! try is refused and the player asked; a yes lets the next download of
//! that name from that host through (the page is told to start it again,
//! or the player presses its button once more), once and for a short while.
//! Nothing is remembered beyond that.

use std::time::{Duration, Instant};

/// How long a yes waits for its download to start again.
const ALLOWED_FOR: Duration = Duration::from_secs(120);
/// How long after a refusal its end may be told (a web view tells it at
/// once, where it tells it at all).
const REFUSAL_TOLD: Duration = Duration::from_secs(10);

/// A download a page started.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub url: String,
    /// Where it comes from ([`host`]).
    pub host: String,
    /// Its file name (a safe one).
    pub name: String,
}

/// What becomes of a download a page starts.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// Saved: the player said yes to it.
    Take,
    /// Refused, and the player is asked about it.
    Ask,
    /// Refused: another is asked about or being saved.
    Busy,
}

#[derive(Default)]
pub struct Gate {
    asked: Option<Request>,
    /// What the player said yes to, and until when it may start.
    allowed: Option<(Request, Instant)>,
    /// The address of the download being saved.
    saving: Option<String>,
    /// Downloads refused whose end the web view has not told yet.
    refused: Vec<(String, Instant)>,
}

/// The host a download at `url` comes from (a `blob:` address: its
/// page's), for the player to judge it by; an address without one: its
/// scheme.
pub fn host(url: &str) -> String {
    let inner = url.strip_prefix("blob:").unwrap_or(url);
    let rest = match inner.split_once("://") {
        Some((scheme, rest)) if ["http", "https"].contains(&scheme) => rest,
        _ => return format!("{}:", url.split(':').next().unwrap_or("")),
    };
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = match host.strip_prefix('[').and_then(|h| h.split_once(']')) {
        Some((v6, _)) => v6,
        None => host.split(':').next().unwrap_or(""),
    };
    host.to_lowercase()
}

impl Gate {
    /// A page starts a download of `name` from `url`.
    pub fn started(&mut self, url: &str, name: &str, now: Instant) -> Verdict {
        let host = host(url);
        if self.saving.is_some() || self.asked.is_some() {
            self.refused.push((url.to_string(), now));
            return Verdict::Busy;
        }
        let allowed = self
            .allowed
            .take()
            .is_some_and(|(r, until)| now <= until && r.host == host && r.name == name);
        if allowed {
            self.refused.retain(|(u, _)| u != url);
            self.saving = Some(url.to_string());
            return Verdict::Take;
        }
        self.refused.push((url.to_string(), now));
        self.asked = Some(Request {
            url: url.to_string(),
            host,
            name: name.to_string(),
        });
        Verdict::Ask
    }

    /// The download the player is asked about.
    pub fn asked(&self) -> Option<&Request> {
        self.asked.as_ref()
    }

    /// The player's answer; on a yes, the download to start again.
    pub fn answer(&mut self, yes: bool, now: Instant) -> Option<Request> {
        let r = self.asked.take().filter(|_| yes)?;
        self.allowed = Some((r.clone(), now + ALLOWED_FOR));
        Some(r)
    }

    /// The web view says the download from `url` ended. True when that is
    /// the one being saved (it is free for the next), not a refused one.
    pub fn ended(&mut self, url: &str, now: Instant) -> bool {
        self.refused.retain(|(_, at)| now < *at + REFUSAL_TOLD);
        if let Some(i) = self.refused.iter().position(|(u, _)| u == url) {
            self.refused.swap_remove(i);
            return false;
        }
        self.saving.take_if(|u| u == url).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://files.example/get/Song.zip?t=1";

    /// Nothing is saved before the player's yes, and the yes is for that
    /// one file: its next start, from its host, under its name.
    #[test]
    fn a_download_waits_for_a_yes() {
        let t = Instant::now();
        let mut g = Gate::default();
        assert_eq!(g.started(URL, "Song.zip", t), Verdict::Ask);
        let asked = g.asked().unwrap().clone();
        assert_eq!(
            (asked.host.as_str(), asked.name.as_str()),
            ("files.example", "Song.zip")
        );
        // (the web view tells the refused one's end)
        assert!(!g.ended(URL, t));
        // no: nothing is allowed, and the same again is asked again
        assert_eq!(g.answer(false, t), None);
        assert_eq!(g.started(URL, "Song.zip", t), Verdict::Ask);
        assert!(!g.ended(URL, t));
        // yes: the one to start again; another address of the host will do
        // (a link good for one use)
        assert_eq!(g.answer(true, t).unwrap().url, URL);
        let again = "https://files.example/get/Song.zip?t=2";
        assert_eq!(g.started(again, "Song.zip", t), Verdict::Take);
        assert!(g.ended(again, t));
        // once: the next is asked about
        assert_eq!(g.started(again, "Song.zip", t), Verdict::Ask);
    }

    /// A yes does not pass another file, another host, or a start long
    /// after it.
    #[test]
    fn a_yes_is_for_one_file() {
        let t = Instant::now();
        for (url, name, at) in [
            ("https://files.example/x", "Other.zip", t),
            ("https://evil.example/get/Song.zip", "Song.zip", t),
            (URL, "Song.zip", t + ALLOWED_FOR + Duration::from_secs(1)),
        ] {
            let mut g = Gate::default();
            g.started(URL, "Song.zip", t);
            g.answer(true, t);
            assert_eq!(g.started(url, name, at), Verdict::Ask, "{url}");
            assert_eq!(g.asked().unwrap().name, name);
        }
    }

    /// One at a time: while one is asked about or being saved, a page's
    /// other downloads are refused, and their ends do not end the one
    /// being saved.
    #[test]
    fn one_download_at_a_time() {
        let t = Instant::now();
        let mut g = Gate::default();
        g.started(URL, "Song.zip", t);
        assert_eq!(
            g.started("https://ad.example/a.exe", "a.exe", t),
            Verdict::Busy
        );
        assert_eq!(g.asked().unwrap().name, "Song.zip");
        assert!(!g.ended("https://ad.example/a.exe", t));
        assert!(!g.ended(URL, t));
        g.answer(true, t);
        assert_eq!(g.started(URL, "Song.zip", t), Verdict::Take);
        // pressed twice: the second is refused, the first goes on
        assert_eq!(g.started(URL, "Song.zip", t), Verdict::Busy);
        assert!(!g.ended(URL, t));
        assert_eq!(g.started("https://ad.example/b", "b.zip", t), Verdict::Busy);
        assert!(g.ended(URL, t));
        assert_eq!(g.started("https://ad.example/b", "b.zip", t), Verdict::Ask);
        // a web view that never told the refusals' ends: the one saved
        // still ends
        let mut g = Gate::default();
        g.started(URL, "Song.zip", t);
        g.answer(true, t);
        g.started(URL, "Song.zip", t);
        g.started(URL, "Song.zip", t);
        assert!(g.ended(URL, t + REFUSAL_TOLD));
    }

    #[test]
    fn hosts() {
        assert_eq!(host("https://Files.Example:8443/a/b.zip"), "files.example");
        assert_eq!(host("blob:https://mega.nz/1b2c"), "mega.nz");
        // who a download is from is the host, whatever is put before it
        assert_eq!(host("https://good.example@evil.example/x"), "evil.example");
        assert_eq!(
            host("https://evil.example\\@good.example/x"),
            "evil.example"
        );
        assert_eq!(host("https://evil.example#@good.example"), "evil.example");
        assert_eq!(host("http://[::1]:8080/x"), "::1");
        assert_eq!(host("data:application/zip;base64,AAAA"), "data:");
        assert_eq!(host("data:text/plain,https://good.example/x"), "data:");
    }
}
