//! From a link where an author put a BMS archive to the archive's own URL,
//! without a browser where that can be done.
//!
//! Each kind of link is a [`Host`]: Google Drive files and folders,
//! Dropbox, MediaFire, MEGA files and folders, a work's page on an event
//! venue (manbow, BMS SEARCH), hosts that need a browser, and any server
//! that sends the file itself. [`Hosts`] holds them in the order they are
//! asked; the first that accepts a link opens it. A host's answer is a
//! [`Resolved`]: the file ([`Target`]), another link to follow (a venue's
//! work page names the author's link), a page for a person, or a failure.
//!
//! [`Resolver`] walks a link through the hosts as a [`Task`]: it asks for
//! requests and is fed the answers ([`crate::net`]), so it runs natively,
//! in a web page and in tests alike. Downloading the file itself is the
//! caller's ([`download`] natively).

mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod download;
mod drive;
mod dropbox;
mod mediafire;
mod mega;
mod venues;

#[cfg(not(target_arch = "wasm32"))]
pub use download::{download_to, fetch, fetch_named, sniff};
pub(crate) use mega::pack_artist;
pub use mega::MegaCipher;

use crate::config::Config;
use crate::net::{Answer, BoxTask, Next, Request, Task};
use std::sync::Arc;

/// Why a link gave no file.
#[derive(Debug, Clone)]
pub enum Fetch {
    /// The link is a page to use in a browser.
    Page,
    /// The download failed ("HTTP 404"...).
    Failed(String),
}

impl PartialEq for Fetch {
    fn eq(&self, o: &Fetch) -> bool {
        matches!(
            (self, o),
            (Fetch::Page, Fetch::Page) | (Fetch::Failed(_), Fetch::Failed(_))
        )
    }
}

impl From<String> for Fetch {
    fn from(e: String) -> Fetch {
        Fetch::Failed(e)
    }
}

/// A song looked for by its title and artist, as a difficulty table names
/// it: a link may lead to a folder of many songs' archives (a MEGA pack of
/// a table's songs, `[artist] title.zip` each).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Want {
    pub title: String,
    pub artist: String,
}

/// The file a link leads to.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub url: String,
    /// A MEGA file's 32-byte key: the bytes come encrypted ([`MegaCipher`]).
    pub mega_key: Option<Vec<u8>>,
    /// Its name where the host tells it (a folder's listing, MEGA, the
    /// mirror); else the server's or the URL's.
    pub name: Option<String>,
}

impl Target {
    pub fn url(url: impl Into<String>) -> Target {
        Target {
            url: url.into(),
            mega_key: None,
            name: None,
        }
    }

    fn named(url: impl Into<String>, name: Option<String>) -> Target {
        Target {
            name,
            ..Target::url(url)
        }
    }
}

/// What a host makes of a link.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    File(Target),
    /// Another link to go on with (a venue's work page: the author's link).
    Follow(String),
    /// A page for a person in a browser.
    Page,
    Failed(String),
}

/// A link as hosts look at it.
#[derive(Clone, Debug)]
pub struct Link {
    pub url: String,
    pub lower: String,
    /// Its host name, lower case.
    pub host: String,
    /// Links followed to get here.
    pub hops: u8,
}

impl Link {
    /// An http(s) link.
    pub fn parse(url: &str, hops: u8) -> Option<Link> {
        let url = url.trim().to_string();
        let lower = url.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return None;
        }
        let host = lower.split('/').nth(2).unwrap_or("").to_string();
        Some(Link {
            url,
            lower,
            host,
            hops,
        })
    }

    /// Whether its host is `domain` or under it.
    pub fn on(&self, domain: &str) -> bool {
        self.host == domain || self.host.ends_with(&format!(".{domain}"))
    }
}

/// A kind of link: a strategy of [`Resolver`].
pub trait Host: Send + Sync {
    fn name(&self) -> &'static str;
    /// Whether this host handles `link`.
    fn accepts(&self, link: &Link) -> bool;
    /// The way from `link` to its file.
    fn open(&self, link: &Link, want: Option<&Want>, config: &Config) -> BoxTask<Resolved>;
}

/// The hosts, asked in order.
pub struct Hosts(Vec<Box<dyn Host>>);

impl Default for Hosts {
    fn default() -> Hosts {
        Hosts(vec![
            Box::new(drive::Drive),
            Box::new(dropbox::Dropbox),
            Box::new(venues::Manbow),
            Box::new(venues::BmsSearchVenue),
            Box::new(mediafire::MediaFire),
            Box::new(mega::Mega),
            Box::new(browser::BrowserOnly),
            Box::new(browser::Direct),
        ])
    }
}

impl Hosts {
    /// No host: [`Hosts::push`] them.
    pub fn none() -> Hosts {
        Hosts(Vec::new())
    }

    /// Add `host` (after the others).
    pub fn push(&mut self, host: Box<dyn Host>) {
        self.0.push(host);
    }

    /// Add `host` before the others.
    pub fn first(&mut self, host: Box<dyn Host>) {
        self.0.insert(0, host);
    }

    /// Leave out the host named `name`.
    pub fn without(mut self, name: &str) -> Hosts {
        self.0.retain(|h| h.name() != name);
        self
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|h| h.name()).collect()
    }

    /// The host that handles `link`.
    pub fn find(&self, link: &Link) -> Option<&dyn Host> {
        self.0.iter().find(|h| h.accepts(link)).map(|h| h.as_ref())
    }
}

/// What resolving runs with: the settings and the hosts.
#[derive(Clone, Default)]
pub struct Context {
    pub config: Arc<Config>,
    pub hosts: Arc<Hosts>,
}

impl Context {
    pub fn new(config: Config) -> Context {
        Context {
            config: Arc::new(config),
            hosts: Arc::new(Hosts::default()),
        }
    }
}

/// How long a download may go without data before it is given up (ms).
pub const STALL_MS: u64 = 60_000;

/// Links followed at most before giving up.
const MAX_HOPS: u8 = 3;

/// A link walked through the hosts to its file.
pub struct Resolver {
    cx: Context,
    want: Option<Want>,
    link: String,
    hops: u8,
    task: Option<BoxTask<Resolved>>,
    /// The host that answered last.
    host: &'static str,
}

impl Resolver {
    /// For the song `want` (a folder link: its archive).
    pub fn new(link: &str, want: Option<Want>, cx: &Context) -> Resolver {
        Resolver {
            cx: cx.clone(),
            want,
            link: link.trim().to_string(),
            hops: 0,
            task: None,
            host: "",
        }
    }

    /// The host that opened the last link ("drive", "mega"...).
    pub fn host(&self) -> &'static str {
        self.host
    }

    fn open(&mut self) -> Next<Result<Target, Fetch>> {
        let Some(link) = Link::parse(&self.link, self.hops) else {
            return Next::Done(Err(Fetch::Page));
        };
        let Some(host) = self.cx.hosts.find(&link) else {
            return Next::Done(Err(Fetch::Page));
        };
        self.host = host.name();
        let mut task = host.open(&link, self.want.as_ref(), &self.cx.config);
        let next = task.start();
        self.task = Some(task);
        self.step(next)
    }

    fn step(&mut self, next: Next<Resolved>) -> Next<Result<Target, Fetch>> {
        match next {
            Next::Ask(r) => Next::Ask(r),
            Next::Done(Resolved::File(t)) => Next::Done(Ok(t)),
            Next::Done(Resolved::Follow(next)) if self.hops < MAX_HOPS => {
                self.hops += 1;
                self.link = next;
                self.open()
            }
            Next::Done(Resolved::Follow(_) | Resolved::Page) => Next::Done(Err(Fetch::Page)),
            Next::Done(Resolved::Failed(e)) => Next::Done(Err(Fetch::Failed(e))),
        }
    }
}

impl Task for Resolver {
    type Output = Result<Target, Fetch>;
    fn start(&mut self) -> Next<Self::Output> {
        self.hops = 0;
        self.open()
    }
    fn feed(&mut self, answer: Answer) -> Next<Self::Output> {
        match self.task.as_mut() {
            Some(t) => {
                let next = t.feed(answer);
                self.step(next)
            }
            None => Next::Done(Err(Fetch::Failed("nothing asked".into()))),
        }
    }
}

/// A GET whose answer `then` makes into a [`Resolved`] (a failed request:
/// [`Resolved::Failed`]).
pub(crate) fn get_then<F>(url: String, then: F) -> BoxTask<Resolved>
where
    F: FnOnce(Vec<u8>) -> Resolved + Send + 'static,
{
    struct Then<F> {
        request: Option<Request>,
        then: Option<F>,
    }
    impl<F: FnOnce(Vec<u8>) -> Resolved + Send> Task for Then<F> {
        type Output = Resolved;
        fn start(&mut self) -> Next<Resolved> {
            match self.request.take() {
                Some(r) => Next::Ask(r),
                None => Next::Done(Resolved::Failed("asked twice".into())),
            }
        }
        fn feed(&mut self, answer: Answer) -> Next<Resolved> {
            Next::Done(match (answer, self.then.take()) {
                (Ok(body), Some(f)) => f(body),
                (Err(e), _) => Resolved::Failed(e),
                (Ok(_), None) => Resolved::Failed("answered twice".into()),
            })
        }
    }
    Box::new(Then {
        request: Some(Request::Get(url)),
        then: Some(then),
    })
}

/// For a browser showing a [`Fetch::Page`]: on a OneDrive share (a file,
/// or a folder, which comes as a zip) it presses the page's Download button
/// once, as a person would. OneDrive has no download URL to fetch without a
/// browser any more (its API wants an app's token; its pages are built by
/// script). Run it on every page load (wry: `with_initialization_script`;
/// Android: `evaluateJavascript` when the page has finished).
pub const PRESS_DOWNLOAD_JS: &str = r#"(function () {
  if (window.top !== window || window.__bmseekerPressed) return;
  if (!/(^|\.)(onedrive\.live\.com|1drv\.ms|sharepoint\.com)$/i.test(location.hostname)) return;
  var name = /^\s*(download|ダウンロード|다운로드|下载|下載)\s*$/i;
  var tries = 0;
  var timer = setInterval(function () {
    if (++tries > 120) return clearInterval(timer);
    var all = document.querySelectorAll('button, [role="button"], [role="menuitem"]');
    for (var i = 0; i < all.length; i++) {
      var b = all[i];
      var label = b.getAttribute("aria-label") || b.textContent || "";
      if (name.test(label) && !b.disabled && b.offsetParent !== null) {
        window.__bmseekerPressed = true;
        clearInterval(timer);
        b.click();
        return;
      }
    }
  }, 500);
})();"#;

/// The archive format of a file's first bytes.
pub fn sniff_bytes(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") {
        Some("zip")
    } else if h.starts_with(b"Rar!\x1a\x07") {
        Some("rar")
    } else if h.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some("7z")
    } else {
        None
    }
}

pub use crate::net::url_encode;

/// Whether a file name is an archive's.
fn archive_name(n: &str) -> bool {
    let n = n.to_ascii_lowercase();
    [".zip", ".rar", ".7z"].iter().any(|e| n.ends_with(e))
}

/// The best of (name, item) among a folder's archives: ogg over wav, full
/// over BGA-only and patches, the first of equals.
fn pick_file<'a, T>(files: impl Iterator<Item = (&'a str, T)>) -> Option<T> {
    let score = |n: &str| {
        let n = n.to_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| n.contains(w));
        let mut s = 0;
        if has(&["ogg"]) {
            s += 4;
        }
        if has(&["wav", "flac"]) {
            s -= 4;
        }
        if has(&["lq", "low", "lr2", "wmv", "mpg", "lite", "compact"]) {
            s -= 2;
        }
        if has(&[
            "bga only",
            "bga_only",
            "only bga",
            "bga単体",
            "patch",
            "差分",
            "追加",
        ]) {
            s -= 8;
        }
        s
    };
    let mut best: Option<(i32, T)> = None;
    for (name, item) in files {
        let s = score(name);
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, item));
        }
    }
    best.map(|(_, item)| item)
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{run, Canned};

    /// `link` resolved with canned answers.
    pub(crate) fn resolve(link: &str, answers: &[(&str, &[u8])]) -> Result<Target, Fetch> {
        let http = Canned(
            answers
                .iter()
                .map(|(u, b)| (u.to_string(), Ok(b.to_vec())))
                .collect(),
        );
        run(&mut Resolver::new(link, None, &Context::default()), &http)
    }

    #[test]
    fn hosts_in_order() {
        let h = Hosts::default();
        assert_eq!(
            h.names(),
            [
                "drive",
                "dropbox",
                "manbow",
                "bmssearch-venue",
                "mediafire",
                "mega",
                "browser",
                "direct"
            ]
        );
        let at = |u: &str| h.find(&Link::parse(u, 0).unwrap()).map(|h| h.name());
        assert_eq!(at("https://drive.google.com/file/d/X/view"), Some("drive"));
        assert_eq!(at("https://1drv.ms/u/c/x"), Some("browser"));
        assert_eq!(at("http://example.com/a.zip"), Some("direct"));
        assert!(Link::parse("not a link", 0).is_none());
        assert_eq!(
            Hosts::default()
                .without("direct")
                .find(&Link::parse("http://e/a.zip", 0).unwrap())
                .map(|h| h.name()),
            None
        );
    }

    #[test]
    fn simple_links() {
        assert_eq!(
            resolve("http://example.com/a.zip", &[]),
            Ok(Target::url("http://example.com/a.zip"))
        );
        assert_eq!(resolve("https://1drv.ms/u/c/x", &[]), Err(Fetch::Page));
        assert_eq!(resolve("not a link", &[]), Err(Fetch::Page));
        assert_eq!(
            resolve("https://www.mediafire.com/file/x/a.rar/file", &[]),
            Err(Fetch::Failed(String::new()))
        );
    }

    #[test]
    fn picks_files() {
        let names = ["song_wav.zip", "song_ogg.zip", "bga only.zip"];
        assert_eq!(
            pick_file(names.iter().map(|n| (*n, *n))),
            Some("song_ogg.zip")
        );
        assert!(archive_name("A.RAR") && !archive_name("a.txt"));
    }
}
