//! HTTP: natively ureq, on a thread a request; in browsers `fetch`
//! (ehttp), on the event loop. Callbacks run on that thread natively and
//! on the event loop on the web. Nothing here logs a request: its headers
//! can carry credentials.
//!
//! Every answer has a limit ([`BODY_MAX`], or the size its request names):
//! a body is counted as it comes (as decoded: a little gzip can be any
//! size), and the request ends once it is past it. Natively a server has so
//! long to be found, to accept a connection, to begin its answer and
//! between any two reads ([`native`]), after which the request's thread
//! ends; a browser keeps its own times, and a request there ends for its
//! caller when no data has come for the stall time.
//!
//! Natively every request names vibeBMS ([`user_agent`]) unless it names
//! itself; in browsers the browser's own User-Agent goes, which a page
//! cannot change (nothing here tries).

use super::{Duration, Instant};
use bmseeker::hosts::STALL_MS;
use bmseeker::net::{Answer, NetError};
use bmseeker::progress::Progress;
use ehttp::streaming::Part;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, MutexGuard};

/// vibeBMS's product in a User-Agent: its name, version and where it is
/// from, `vibeBMS/0.1.0 (+https://github.com/vibeBMS/vibe)`. It comes after
/// the HTTP library's on vibeBMS's own requests ([`user_agent`]) and after
/// bmseeker's on bmseeker's downloads (`net::hosts::context`).
pub const PRODUCT: &str = concat!(
    "vibeBMS/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/vibeBMS/vibe)"
);

/// What vibeBMS's own requests are sent with natively (IR, difficulty
/// tables, the updater, skins, the event mirror's lists): the HTTP
/// library's own name and version as it sends it
/// (`bmseeker::config::HTTP_CLIENT`: ehttp's ureq is bmseeker's), kept,
/// then [`PRODUCT`]. Never a browser's.
#[cfg(not(target_arch = "wasm32"))]
pub fn user_agent() -> &'static str {
    static UA: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| bmseeker::config::native_user_agent(PRODUCT));
    &UA
}

/// `req` with vibeBMS's User-Agent, natively, unless it has one.
fn identified(req: ehttp::Request) -> ehttp::Request {
    #[cfg(not(target_arch = "wasm32"))]
    let req = {
        let mut req = req;
        if req.headers.get("user-agent").is_none() {
            req.headers.insert("User-Agent", user_agent());
        }
        req
    };
    req
}

/// A request: `method` is `GET`, `POST`, `PUT` or `DELETE` (anything else
/// is a GET); a JSON body needs its `Content-Type` among `headers`.
pub fn request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> ehttp::Request {
    let mut req = ehttp::Request::get(url);
    req.method = match method {
        "POST" => ehttp::Method::POST,
        "PUT" => ehttp::Method::PUT,
        "DELETE" => ehttp::Method::DELETE,
        _ => ehttp::Method::GET,
    };
    req.body = body;
    for (k, v) in headers {
        req.headers.insert(k, v);
    }
    req
}

/// The largest answer kept (bytes, as decoded) unless its request is for
/// a file and names its size ([`stream`]): pages, lists, tables and IR
/// answers are JSON or HTML of a few megabytes at most (a player's every
/// score on an IR: tens).
pub const BODY_MAX: u64 = 64 << 20;

fn too_large(limit: u64) -> NetError {
    match limit >> 20 {
        0 => NetError::Other(format!("the answer is larger than {limit} bytes")),
        _ => bmseeker::net::too_large(limit as usize),
    }
}

/// What an answer is given to as it comes.
trait Taker: Send + 'static {
    /// Its head, then its body's chunks (none empty). An error ends the
    /// request.
    fn take(&mut self, part: Part) -> Result<(), NetError>;
    /// How it ended (once).
    fn done(self, end: Result<(), NetError>);
}

/// Send `req` and give its answer to `taker` as it comes. The request
/// ends, for the taker, when the body is whole, when it is larger than
/// `limit` bytes, when the taker fails, when `progress` is cancelled, or
/// when no data has come for `bmseeker::hosts`' stall time (60 s).
fn receive(req: ehttp::Request, limit: u64, progress: Option<Arc<Progress>>, taker: impl Taker) {
    // (the taker until it is done, when data last came, the body so far)
    struct St<T: Taker> {
        taker: Option<T>,
        last: Instant,
        got: u64,
    }
    // a request let go of with nothing said (no thread to send it on)
    impl<T: Taker> Drop for St<T> {
        fn drop(&mut self) {
            if let Some(t) = self.taker.take() {
                t.done(Err(NetError::Other("the request was not sent".into())));
            }
        }
    }
    fn lock<T: Taker>(st: &Mutex<St<T>>) -> MutexGuard<'_, St<T>> {
        st.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn finish<T: Taker>(st: &Mutex<St<T>>, end: Result<(), NetError>) {
        let taker = lock(st).taker.take();
        if let Some(t) = taker {
            t.done(end);
        }
    }
    let state = Arc::new(Mutex::new(St {
        taker: Some(taker),
        last: Instant::now(),
        got: 0,
    }));
    // a watch: cancelled, or nothing for a while (natively the connection
    // itself ends a request that stalls: `native`)
    if progress.is_some() || super::IS_WEB {
        let st = state.clone();
        super::thread::every(Duration::from_millis(500), move || {
            let stalled = {
                let g = lock(&st);
                if g.taker.is_none() {
                    return false;
                }
                g.last.elapsed() > Duration::from_millis(STALL_MS)
            };
            let end = if progress.as_ref().is_some_and(|p| p.cancelled()) {
                NetError::Cancelled
            } else if stalled {
                NetError::Stalled(STALL_MS / 1000)
            } else {
                return true;
            };
            finish(&st, Err(end));
            false
        });
    }
    start(identified(req), move |part| {
        let mut g = lock(&state);
        let st = &mut *g;
        let Some(taker) = st.taker.as_mut() else {
            return ControlFlow::Break(());
        };
        let end = match part {
            Err(e) => Err(NetError::Other(e)),
            Ok(Part::Chunk(c)) if c.is_empty() => Ok(()),
            Ok(part) => {
                // (a body that says it is too large is not begun)
                let size = match &part {
                    Part::Response(r) => (r.headers.get("content-length"))
                        .and_then(|l| l.trim().parse().ok())
                        .unwrap_or(0),
                    Part::Chunk(c) => st.got + c.len() as u64,
                };
                let is_body = matches!(part, Part::Chunk(_));
                match size > limit {
                    true => Err(too_large(limit)),
                    false => match taker.take(part) {
                        Ok(()) => {
                            if is_body {
                                st.got = size;
                            }
                            st.last = Instant::now();
                            return ControlFlow::Continue(());
                        }
                        Err(e) => Err(e),
                    },
                }
            }
        };
        drop(g);
        finish(&state, end);
        ControlFlow::Break(())
    });
}

/// Send `req` in the background and give `on_part` its answer as it
/// comes, until it says to stop: the head, the body in chunks, then an
/// empty chunk at its end; or why there is no more.
fn start(
    req: ehttp::Request,
    on_part: impl Fn(Result<Part, String>) -> ControlFlow<()> + Send + 'static,
) {
    #[cfg(not(target_arch = "wasm32"))]
    super::thread::spawn("http", move || native::run(&native::AGENT, &req, &on_part));
    #[cfg(target_arch = "wasm32")]
    ehttp::streaming::fetch(req, on_part);
}

/// Requests natively: ureq, with the times a server is given.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{ControlFlow, Duration, Part, STALL_MS};
    use std::io::Read;
    use std::sync::LazyLock;
    use ureq::Agent;

    /// How long a host's address may take to be found, and a server to
    /// accept a connection: one that is gone fails in this time, not the
    /// system's (minutes).
    const CONNECT: Duration = Duration::from_secs(15);

    /// How long a server may take to read a request, and to begin its
    /// answer once asked.
    const ANSWER_STARTS: Duration = Duration::from_secs(30);

    /// An agent whose servers have `answer_starts` to take a request and
    /// to begin their answer, and `stall` between two reads (the
    /// handshake's and the body's). Statuses are answers, the system's
    /// certificates are trusted on desktops (corporate proxies, custom
    /// CAs), and each request has a connection of its own.
    pub(super) fn agent(answer_starts: Duration, stall: Duration) -> Agent {
        let tls = ureq::tls::TlsConfig::builder();
        #[cfg(desktop)]
        let tls = tls.root_certs(ureq::tls::RootCerts::PlatformVerifier);
        Agent::config_builder()
            .http_status_as_error(false)
            .tls_config(tls.build())
            .max_idle_connections(0)
            .timeout_resolve(Some(CONNECT))
            .timeout_connect(Some(CONNECT))
            .timeout_send_request(Some(answer_starts))
            .timeout_send_body(Some(answer_starts))
            .timeout_recv_response(Some(answer_starts))
            .timeout_per_read(Some(stall))
            .build()
            .into()
    }

    pub(super) static AGENT: LazyLock<Agent> =
        LazyLock::new(|| agent(ANSWER_STARTS, Duration::from_millis(STALL_MS)));

    /// `req` sent through `agent`, with its headers and its own time for
    /// the whole body (`timeout`; none: the stall time alone ends it).
    fn send(
        agent: &Agent,
        req: &ehttp::Request,
    ) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        fn set<B>(mut b: ureq::RequestBuilder<B>, req: &ehttp::Request) -> ureq::RequestBuilder<B> {
            for (k, v) in &req.headers {
                b = b.header(k, v);
            }
            b.config().timeout_recv_body(req.timeout).build()
        }
        let with_body = |b| match req.body.is_empty() {
            true => set(b, req).send_empty(),
            false => set(b, req).send(&req.body[..]),
        };
        let without = |b| match req.body.is_empty() {
            true => set(b, req).call(),
            false => set(b, req).force_send_body().send(&req.body[..]),
        };
        match req.method {
            ehttp::Method::POST => with_body(agent.post(&req.url)),
            ehttp::Method::PUT => with_body(agent.put(&req.url)),
            ehttp::Method::DELETE => without(agent.delete(&req.url)),
            _ => without(agent.get(&req.url)),
        }
    }

    /// Send `req` (a method [`super::request`] makes) through `agent` on
    /// this thread, its answer given to `on_part` as [`super::start`] says.
    pub(super) fn run(
        agent: &Agent,
        req: &ehttp::Request,
        on_part: &dyn Fn(Result<Part, String>) -> ControlFlow<()>,
    ) {
        let mut res = match send(agent, req) {
            Ok(r) => r,
            Err(e) => {
                let _ = on_part(Err(e.to_string()));
                return;
            }
        };
        let mut headers = ehttp::Headers::default();
        for (k, v) in res.headers() {
            // (a value that is no text: not one read here)
            if let Ok(v) = v.to_str() {
                headers.insert(k, v);
            }
        }
        headers.sort();
        let head = ehttp::PartialResponse {
            url: ureq::ResponseExt::get_uri(&res).to_string(),
            ok: res.status().is_success(),
            status: res.status().as_u16(),
            status_text: res.status().canonical_reason().unwrap_or("").into(),
            headers,
        };
        if on_part(Ok(Part::Response(head))).is_break() {
            return;
        }
        let mut body = res.body_mut().as_reader();
        let mut buf = vec![0; 16 << 10];
        loop {
            let part = match body.read(&mut buf) {
                Ok(n) => Ok(Part::Chunk(buf[..n].to_vec())),
                Err(e) => Err(format!("the answer did not come whole: {e}")),
            };
            let over = !matches!(&part, Ok(Part::Chunk(c)) if !c.is_empty());
            if on_part(part).is_break() || over {
                return;
            }
        }
    }
}

/// An answer, whatever its status.
#[derive(Debug, Default)]
pub struct Response {
    pub status: u16,
    /// Natively all of them; on the web only those CORS lets a page read
    /// (see [`exchange`]).
    pub headers: ehttp::Headers,
    /// The body, a refusal's own words too.
    pub body: Vec<u8>,
}

/// An answer kept whole, and who it is for.
struct Whole<D>(Response, D);

impl<D: FnOnce(Result<Response, NetError>) + Send + 'static> Taker for Whole<D> {
    fn take(&mut self, part: Part) -> Result<(), NetError> {
        match part {
            Part::Response(r) => {
                self.0.status = r.status;
                self.0.headers = r.headers;
            }
            Part::Chunk(c) => self.0.body.extend_from_slice(&c),
        }
        Ok(())
    }
    fn done(self, end: Result<(), NetError>) {
        (self.1)(end.map(|()| self.0))
    }
}

/// Send `req`: its answer, of `limit` bytes at most, or why none came.
pub(super) fn call(
    req: ehttp::Request,
    limit: u64,
    on_done: impl FnOnce(Result<Response, NetError>) + Send + 'static,
) {
    receive(req, limit, None, Whole(Response::default(), on_done))
}

/// Send `req`: the body of its answer, or why there is none (a status as
/// `NetError::Status`, which bmseeker's retries and checks look at).
pub fn fetch(req: ehttp::Request, on_done: impl FnOnce(Answer) + Send + 'static) {
    call(req, BODY_MAX, move |r| {
        on_done(r.and_then(|r| match r.status {
            200..=299 => Ok(r.body),
            n => Err(NetError::Status(n)),
        }))
    });
}

/// How long [`exchange`] waits for a whole answer: a request left hanging
/// would hold the IR outbox's item in "sending".
const EXCHANGE_LIMIT: Duration = Duration::from_secs(30);

/// Send `req` (IR requests) and give its answer whatever its status, with
/// the headers and the body: `Retry-After` and `ETag`, a refusal's JSON.
/// `Err` means no answer came: the connection failed, the browser refused
/// it, the answer is larger than [`BODY_MAX`], or nothing came within 30 s
/// (`NetError::Stalled`).
///
/// On the web this is the browser's `fetch`, in CORS mode and without
/// cookies:
/// - `Authorization`, or a `Content-Type` other than a form's or plain
///   text (JSON), makes the browser ask the server first (a preflight); a
///   server that does not allow them, or sends no CORS headers at all, is
///   a failed connection here, with no status.
/// - A page reads only the safelisted response headers (`Cache-Control`,
///   `Content-Language`, `Content-Length`, `Content-Type`, `Expires`,
///   `Last-Modified`, `Pragma`) and those the server names in
///   `Access-Control-Expose-Headers`: `Retry-After` and `ETag` are missing
///   unless the server exposes them.
/// - An error answer without the CORS headers reaches the page as a
///   failed connection too, its status and body unread.
/// - The browser's cache revalidates by itself: without an `If-None-Match`
///   of the page's own, a 304 comes here as the 200 it confirmed.
pub fn exchange(
    req: ehttp::Request,
    on_done: impl FnOnce(Result<Response, NetError>) + Send + 'static,
) {
    exchange_within(req, EXCHANGE_LIMIT, on_done)
}

fn exchange_within(
    req: ehttp::Request,
    limit: Duration,
    on_done: impl FnOnce(Result<Response, NetError>) + Send + 'static,
) {
    // the answer or the watchdog, whichever is first
    let done = Arc::new(Mutex::new(Some(on_done)));
    let take = |d: &Mutex<Option<_>>| d.lock().unwrap_or_else(|e| e.into_inner()).take();
    let watch = done.clone();
    super::thread::after(limit, move || {
        if let Some(f) = take(&watch) {
            f(Err(NetError::Stalled(limit.as_secs())));
        }
    });
    call(req, BODY_MAX, move |r| {
        if let Some(f) = take(&done) {
            f(r);
        }
    });
}

/// GET `url`: its answer's body ([`BODY_MAX`] at most), or why there is
/// none.
pub fn http_get(url: &str, on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    http_get_within(url, BODY_MAX, on_done)
}

/// GET `url`: its answer's body, of `limit` bytes at most, or why there is
/// none (a status with the URL asked).
pub fn http_get_within(
    url: &str,
    limit: u64,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
    let url = url.to_string();
    call(ehttp::Request::get(&url), limit, move |r| {
        on_done(match r {
            Ok(r) if (200..=299).contains(&r.status) => Ok(r.body),
            Ok(r) => Err(format!("HTTP {} {url}", r.status)),
            Err(e) => Err(e.to_string()),
        })
    });
}

/// GET `url` (a file: of `limit` bytes at most, taking as long as data
/// keeps coming), its body given to `sink` as it comes; then `on_done`
/// with how it ended and where its redirects ended when the answer says
/// so (the vibeBMS proxy's `X-Final-Url`, a refusal's too). It reports to
/// `progress`, and ends when that is cancelled, when `sink` fails, when
/// the body is larger than `limit` or when no data has come for
/// `bmseeker::hosts`' stall time (60 s).
pub fn stream(
    url: &str,
    limit: u64,
    progress: Option<Arc<Progress>>,
    sink: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
    on_done: impl FnOnce(Result<(), NetError>, Option<String>) + Send + 'static,
) {
    // (where the redirects ended)
    struct File<S, D> {
        sink: S,
        done: D,
        progress: Option<Arc<Progress>>,
        moved: Option<String>,
    }
    impl<S, D> Taker for File<S, D>
    where
        S: FnMut(&[u8]) -> Result<(), String> + Send + 'static,
        D: FnOnce(Result<(), NetError>, Option<String>) + Send + 'static,
    {
        fn take(&mut self, part: Part) -> Result<(), NetError> {
            match part {
                Part::Response(r) => {
                    self.moved = r.headers.get("x-final-url").map(str::to_string);
                    if !r.ok {
                        return Err(NetError::Status(r.status));
                    }
                    if let Some(p) = &self.progress {
                        p.start_file(r.headers.get("content-length").and_then(|l| l.parse().ok()));
                    }
                }
                Part::Chunk(c) => {
                    (self.sink)(&c).map_err(NetError::Other)?;
                    if let Some(p) = &self.progress {
                        p.add(c.len() as u64);
                    }
                }
            }
            Ok(())
        }
        fn done(self, end: Result<(), NetError>) {
            (self.done)(end, self.moved)
        }
    }
    let mut req = ehttp::Request::get(url);
    // (no time for the whole of it: a slow link's file comes too)
    req.timeout = None;
    let file = File {
        sink,
        done: on_done,
        progress: progress.clone(),
        moved: None,
    };
    receive(req, limit, progress, file)
}

/// `host` is this computer.
pub fn loopback(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The origin of a server's address (an IR server's, the event mirror's),
/// as `scheme://host:port`: `https`, or `http` on this computer (a key sent
/// over plain `http` can be read on the way). None for any other address,
/// and for one whose host is not plain to read (a user name in it,
/// characters a URL parser may take another way): no request goes there.
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port: u16 = match scheme.as_str() {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => (&authority[..i], Some(&authority[i + 1..])),
        _ => (authority, None),
    };
    let port = match port {
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok()?,
        Some(_) => return None,
        None => default_port,
    };
    let named = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let v6 = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .is_some_and(|h| h.parse::<std::net::Ipv6Addr>().is_ok());
    if !(named || v6) || (scheme == "http" && !loopback(host)) {
        return None;
    }
    Some(format!("{scheme}://{}:{port}", host.to_ascii_lowercase()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::mpsc;

    /// A server giving `answers` in turn, one per connection; each request
    /// as it came (head and body) is sent back.
    pub(crate) fn serve(answers: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for a in answers {
                let (mut s, _) = l.accept().unwrap();
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                // the head, then as much body as it announces
                loop {
                    let n = s.read(&mut buf).unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).to_lowercase();
                    let whole = text.find("\r\n\r\n").is_some_and(|end| {
                        let len = text
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .map_or(0, |n| n.trim().parse().unwrap());
                        got.len() >= end + 4 + len
                    });
                    if n == 0 || whole {
                        break;
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&got).into_owned());
                let _ = s.write_all(a.as_bytes());
            }
        });
        (url, rx)
    }

    fn send(req: ehttp::Request) -> Result<Response, NetError> {
        let (tx, rx) = mpsc::channel();
        exchange(req, move |r| {
            let _ = tx.send(r);
        });
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap()
    }

    #[test]
    fn an_exchange_keeps_the_status_headers_and_body() {
        let (url, seen) = serve(vec![
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 120\r\nContent-Length: 4\r\nConnection: close\r\n\r\nslow",
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 304 Not Modified\r\nETag: \"v2\"\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 37\r\nConnection: close\r\n\r\n{\"success\":false,\"description\":\"key\"}",
        ]);
        let r = send(request("GET", &format!("{url}/a"), &[], Vec::new())).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (429, &b"slow"[..]));
        assert_eq!(r.headers.get("retry-after"), Some("120"));
        let got = seen.recv().unwrap();
        assert!(got.starts_with("GET /a HTTP/1.1"), "{got}");
        // vibeBMS names itself, as itself, after the HTTP library
        let ua = format!("user-agent: {}\r\n", user_agent().to_lowercase());
        assert!(got.to_lowercase().contains(&ua), "{got}");
        assert_eq!(
            user_agent(),
            format!("{} {PRODUCT}", bmseeker::config::HTTP_CLIENT)
        );
        assert!(PRODUCT.starts_with("vibeBMS/") && !user_agent().contains("Mozilla"));

        // a request that names itself keeps its own
        let own = vec![("User-Agent".to_string(), "tester/1".to_string())];
        let r = send(request("GET", &format!("{url}/b"), &own, Vec::new())).unwrap();
        assert_eq!(r.status, 429);
        assert_eq!(r.headers.get("Retry-After"), None);
        let got = seen.recv().unwrap().to_lowercase();
        assert!(got.contains("user-agent: tester/1\r\n"), "{got}");
        assert!(!got.contains("vibebms/"), "{got}");

        let tag = vec![("If-None-Match".to_string(), "\"v2\"".to_string())];
        let r = send(request("GET", &format!("{url}/c"), &tag, Vec::new())).unwrap();
        assert_eq!(r.status, 304);
        assert_eq!(r.headers.get("etag"), Some("\"v2\""));
        assert!(r.body.is_empty());
        assert!(seen
            .recv()
            .unwrap()
            .to_lowercase()
            .contains("if-none-match: \"v2\""));

        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), "Bearer k3y".to_string()),
        ];
        let body = br#"{"scores":[]}"#.to_vec();
        let r = send(request("POST", &format!("{url}/d"), &headers, body)).unwrap();
        assert_eq!(r.status, 401);
        assert_eq!(r.body, br#"{"success":false,"description":"key"}"#);
        let got = seen.recv().unwrap();
        assert!(got.starts_with("POST /d HTTP/1.1"), "{got}");
        assert!(got.to_lowercase().contains("authorization: bearer k3y"));
        assert!(got.ends_with(r#"{"scores":[]}"#));
    }

    #[test]
    fn an_exchange_without_an_answer_ends() {
        // a server that never answers: the watchdog ends it
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let req = request("POST", &url, &[], b"{}".to_vec());
        exchange_within(req, Duration::from_millis(300), move |r| {
            let _ = tx.send(r);
        });
        let r = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(r.unwrap_err(), NetError::Stalled(0));
        drop(l);
        // nothing listens: the failure does not say the credentials
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        drop(l);
        let auth = vec![("Authorization".to_string(), "Bearer s3cret".to_string())];
        match send(request("GET", &url, &auth, Vec::new())) {
            Err(NetError::Other(e)) => assert!(!e.contains("s3cret"), "{e}"),
            r => panic!("{r:?}"),
        }
    }

    #[test]
    fn a_stream_ends_when_cancelled() {
        // 2 bytes of 10, then nothing
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/f", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let _ = s.read(&mut [0u8; 4096]);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab");
            std::thread::sleep(std::time::Duration::from_secs(5));
        });
        let p = Progress::new();
        let (tx, rx) = std::sync::mpsc::channel();
        stream(
            &url,
            BODY_MAX,
            Some(p.clone()),
            |_| Ok(()),
            move |r, _| {
                let _ = tx.send(r);
            },
        );
        let start = Instant::now();
        while p.bytes().0 < 2 && start.elapsed() < Duration::from_secs(5) {
            std::thread::yield_now();
        }
        assert_eq!(p.bytes(), (2, Some(10)));
        p.cancel();
        let r = rx.recv_timeout(std::time::Duration::from_secs(3));
        assert_eq!(r, Ok(Err(NetError::Cancelled)));
    }

    /// A server writing `head` and then `body` over and over until the
    /// client is gone: how many bytes it wrote is sent back.
    fn pour(head: &'static str, body: Vec<u8>) -> (String, mpsc::Receiver<usize>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/f", l.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let _ = s.read(&mut [0u8; 4096]);
            let _ = s.write_all(head.as_bytes());
            let mut n = 0;
            while n < 1 << 30 && s.write_all(&body).is_ok() {
                n += body.len();
            }
            let _ = tx.send(n);
        });
        (url, rx)
    }

    /// [`call`], but for the limit.
    fn call_within(url: &str, limit: u64) -> Result<Response, NetError> {
        let (tx, rx) = mpsc::channel();
        let whole = Whole(Response::default(), move |r| {
            let _ = tx.send(r);
        });
        receive(ehttp::Request::get(url), limit, None, whole);
        rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap()
    }

    fn larger(r: Result<Response, NetError>) -> bool {
        matches!(&r, Err(NetError::Other(e)) if e.contains("larger than"))
    }

    /// An answer is read as far as its request allows, whatever the server
    /// says of its size or sends: one that would not end is let go of.
    #[test]
    fn an_answer_is_read_within_its_limit() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789";
        let (url, _) = serve(vec![ok, ok, ok]);
        let r = call_within(&url, 10).unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"0123456789"[..]));
        // a size said that is too large: nothing of the body is read
        assert!(larger(call_within(&url, 9)));
        assert!(larger(call_within(&url, 0)));
        // no size said, and no end
        let (url, wrote) = pour(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            vec![b'x'; 64 << 10],
        );
        assert!(larger(call_within(&url, 1 << 20)));
        let wrote = wrote
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert!(wrote < 64 << 20, "{wrote}");
        // a refusal's words are bounded as an answer's are
        let (url, _) = pour(
            "HTTP/1.1 500 No\r\nConnection: close\r\n\r\n",
            vec![b'x'; 64 << 10],
        );
        assert!(larger(call_within(&url, 1 << 20)));
    }

    /// A body is counted as decoded: a little gzip is not a little answer.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_compressed_answer_is_counted_as_it_decodes() {
        let gzip = |n: usize| {
            let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            z.write_all(&vec![0u8; n]).unwrap();
            z.finish().unwrap()
        };
        let serve_gzip = |body: Vec<u8>| {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/f", l.local_addr().unwrap());
            std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let _ = s.read(&mut [0u8; 4096]);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            });
            url
        };
        let small = gzip(1 << 20);
        assert!(small.len() < 4096);
        let r = call_within(&serve_gzip(small), 1 << 20).unwrap();
        assert_eq!(r.body.len(), 1 << 20);
        let bomb = gzip(256 << 20);
        assert!(bomb.len() < 1 << 20);
        assert!(larger(call_within(&serve_gzip(bomb), 1 << 20)));
    }

    /// A file's download is held to its size too.
    #[test]
    fn a_stream_is_read_within_its_limit() {
        let (url, wrote) = pour(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n",
            vec![b'x'; 64 << 10],
        );
        let (tx, rx) = mpsc::channel();
        let got = Arc::new(Mutex::new(0u64));
        let sink = got.clone();
        stream(
            &url,
            1 << 20,
            None,
            move |c| {
                *sink.lock().unwrap() += c.len() as u64;
                Ok(())
            },
            move |r, _| {
                let _ = tx.send(r);
            },
        );
        let r = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
        assert!(
            matches!(&r, Err(NetError::Other(e)) if e.contains("larger than 1 MiB")),
            "{r:?}"
        );
        assert!(*got.lock().unwrap() <= 1 << 20);
        let wrote = wrote
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap();
        assert!(wrote < 64 << 20, "{wrote}");
    }

    /// What `native::run` gives of `req` through an agent with these
    /// times: the body, or why it ended; and how long it took.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_with(
        req: ehttp::Request,
        answer_starts: Duration,
        stall: Duration,
    ) -> (Result<Vec<u8>, String>, Duration) {
        let agent = native::agent(answer_starts, stall);
        let got = Mutex::new((Vec::new(), None));
        let start = Instant::now();
        native::run(&agent, &req, &|part| {
            let mut g = got.lock().unwrap();
            match part {
                Ok(Part::Chunk(c)) => g.0.extend_from_slice(&c),
                Ok(Part::Response(_)) => {}
                Err(e) => g.1 = Some(e),
            }
            ControlFlow::Continue(())
        });
        let (body, failed) = got.into_inner().unwrap();
        (failed.map_or(Ok(body), Err), start.elapsed())
    }

    /// A server that sends `parts` of an answer, waiting `pause` before
    /// each, then keeps the connection open for a while.
    #[cfg(not(target_arch = "wasm32"))]
    fn drip(parts: Vec<&'static str>, pause: Duration) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/f", l.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let _ = s.read(&mut [0u8; 4096]);
            for p in parts {
                std::thread::sleep(pause);
                let _ = s.write_all(p.as_bytes());
            }
            std::thread::sleep(Duration::from_secs(20));
        });
        url
    }

    /// Natively a request's thread ends by itself: when the server does
    /// not begin its answer, and when the answer stops coming. A body may
    /// take any time while it keeps coming, unless its request has a time
    /// for the whole of it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_server_has_so_long() {
        let ms = Duration::from_millis;
        let soon = Duration::from_secs(10);
        // never answers
        let (r, took) = run_with(ehttp::Request::get(drip(vec![], ms(0))), ms(300), ms(5000));
        assert!(r.unwrap_err().contains("timeout"));
        assert!(took < soon, "{took:?}");
        // begins, then nothing
        let head = "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n";
        let (r, took) = run_with(
            ehttp::Request::get(drip(vec![head, "ab"], ms(0))),
            ms(5000),
            ms(300),
        );
        assert!(r.unwrap_err().contains("timeout"));
        assert!(took < soon, "{took:?}");
        // slowly, but it comes: longer in all than a stall, and than the
        // time a whole body has where a request sets one
        let parts = vec![head, "ab", "cd", "ef"];
        let mut slow = ehttp::Request::get(drip(parts.clone(), ms(200)));
        slow.timeout = None;
        let (r, took) = run_with(slow, ms(5000), ms(500));
        assert_eq!(r.unwrap(), b"abcdef");
        assert!(took >= ms(600), "{took:?}");
        let mut timed = ehttp::Request::get(drip(parts, ms(200)));
        timed.timeout = Some(ms(300));
        let (r, _) = run_with(timed, ms(5000), ms(500));
        assert!(r.unwrap_err().contains("timeout"));
    }

    /// The HTTP library's name kept first is the one this build's ureq
    /// sends by itself (vibeBMS's lockfile's version, which ehttp and
    /// bmseeker share): bmseeker's `HTTP_CLIENT` must say it.
    #[test]
    fn the_http_library_named_is_the_one_built() {
        let (url, seen) = serve(vec!["HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"]);
        let (tx, rx) = mpsc::channel();
        // (not through `identified`: ureq's own User-Agent)
        ehttp::fetch(ehttp::Request::get(format!("{url}/a")), move |r| {
            let _ = tx.send(r.map(|r| r.status));
        });
        let got = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(got, Ok(204));
        let head = seen.recv().unwrap();
        let ua = head.lines().find_map(|l| {
            l.to_ascii_lowercase()
                .starts_with("user-agent:")
                .then(|| l[11..].trim().to_string())
        });
        assert_eq!(
            ua.as_deref(),
            Some(bmseeker::config::HTTP_CLIENT),
            "{head}: bmseeker's config::HTTP_CLIENT is not this build's ureq"
        );
    }

    #[test]
    fn origins() {
        let o = |u: &str| origin(u);
        assert_eq!(
            o("https://IR.example/api/").as_deref(),
            Some("https://ir.example:443")
        );
        assert_eq!(
            o(" HTTPS://ir.example:8443?x#y").as_deref(),
            Some("https://ir.example:8443")
        );
        // plain http: this computer only
        assert_eq!(
            o("http://127.0.0.1:8080/api").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            o("http://localhost").as_deref(),
            Some("http://localhost:80")
        );
        assert_eq!(o("http://[::1]:9/").as_deref(), Some("http://[::1]:9"));
        assert_eq!(
            o("https://[2001:db8::1]").as_deref(),
            Some("https://[2001:db8::1]:443")
        );
        for bad in [
            "http://ir.example/api",
            "http://127.0.0.1.evil.example/",
            "ftp://ir.example",
            "ir.example",
            "",
            // not plainly one host
            "https://user:key@ir.example/",
            "https://ir.example@evil.example/",
            "https://ir.example\\@evil.example/",
            "https://ir.example:443@evil.example",
            "https://ir.example:x/",
            "https://ir.example:/",
            "https://ir.example:99999/",
            "https://ir%2eexample/",
            "https://ir.exa mple/",
            "https://[::1/",
            "https:///api",
        ] {
            assert_eq!(o(bad), None, "{bad}");
        }
    }
}
