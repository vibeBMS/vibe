//! HTTP (ehttp: ureq natively, `fetch` in browsers). Callbacks run on a
//! worker thread natively and on the event loop on the web. Nothing here
//! logs a request: its headers can carry credentials.
//!
//! Natively every request names vibeBMS ([`user_agent`]) unless it names
//! itself; in browsers the browser's own User-Agent goes, which a page
//! cannot change (nothing here tries).

use super::{Duration, Instant};
use bmseeker::hosts::STALL_MS;
use bmseeker::net::{Answer, NetError};
use bmseeker::progress::Progress;
use std::sync::{Arc, Mutex, MutexGuard};

/// vibeBMS's product in a User-Agent: its name, version and where it is
/// from, `vibeBMS/0.1.0 (+https://github.com/vibeBMS/vibe)`. It comes after
/// the HTTP library's on vibeBMS's own requests ([`user_agent`]) and after
/// bmseeker's on bmseeker's downloads (`hosts::context`).
pub const PRODUCT: &str = concat!(
    "vibeBMS/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/vibeBMS/vibe)"
);

/// What vibeBMS's own requests are sent with natively (IR, difficulty
/// tables, the updater, skins, the event mirror's lists): the HTTP
/// library's own name as it sends it (`bmseeker::config::HTTP_CLIENT`:
/// ehttp's ureq is bmseeker's), kept, then [`PRODUCT`]:
/// `ureq/3.5.2 vibeBMS/0.1.0 (+https://github.com/vibeBMS/vibe)`. Never a
/// browser's.
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

/// An answer, whatever its status.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    /// Natively all of them; on the web only those CORS lets a page read
    /// (see [`exchange`]).
    pub headers: ehttp::Headers,
    /// The body, a refusal's own words too.
    pub body: Vec<u8>,
}

/// Send `req`: its answer, or why none came.
fn call(req: ehttp::Request, on_done: impl FnOnce(Result<Response, NetError>) + Send + 'static) {
    ehttp::fetch(identified(req), move |r| {
        on_done(match r {
            Ok(r) => Ok(Response {
                status: r.status,
                headers: r.headers,
                body: r.bytes,
            }),
            Err(e) => Err(NetError::Other(e)),
        })
    });
}

/// Send `req`: the body of its answer, or why there is none (a status as
/// `NetError::Status`, which bmseeker's retries and checks look at).
pub fn fetch(req: ehttp::Request, on_done: impl FnOnce(Answer) + Send + 'static) {
    call(req, move |r| {
        on_done(r.and_then(|r| match r.status {
            200..=299 => Ok(r.body),
            n => Err(NetError::Status(n)),
        }))
    });
}

/// How long [`exchange`] waits for a whole answer. Natively ehttp bounds
/// only the reading of a GET's body, and a request left hanging would hold
/// the IR outbox's item in "sending".
const EXCHANGE_LIMIT: Duration = Duration::from_secs(30);

/// Send `req` (IR requests) and give its answer whatever its status, with
/// the headers and the body: `Retry-After` and `ETag`, a refusal's JSON.
/// `Err` means no answer came: the connection failed, the browser refused
/// it, or nothing came within 30 s (`NetError::Stalled`).
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
    call(req, move |r| {
        if let Some(f) = take(&done) {
            f(r);
        }
    });
}

/// [`fetch`], its failure as text (a status with the URL asked).
fn send(req: ehttp::Request, on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    let url = req.url.clone();
    fetch(req, move |a| {
        on_done(a.map_err(|e| match e {
            NetError::Status(n) => format!("HTTP {n} {url}"),
            e => e.to_string(),
        }))
    });
}

pub fn http_get(url: &str, on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    send(ehttp::Request::get(url), on_done)
}

/// GET `url`, its body given to `sink` as it comes; then `on_done` with how
/// it ended and where its redirects ended when the answer says so (the
/// vibeBMS proxy's `X-Final-Url`, a refusal's too). It reports to
/// `progress`, and ends when that is cancelled, when `sink` fails or when
/// no data has come for `bmseeker::hosts`' stall time (60 s).
pub fn stream(
    url: &str,
    progress: Option<Arc<Progress>>,
    sink: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
    on_done: impl FnOnce(Result<(), NetError>, Option<String>) + Send + 'static,
) {
    use ehttp::streaming::Part;
    use std::ops::ControlFlow;
    type Done = Box<dyn FnOnce(Result<(), NetError>, Option<String>) + Send>;
    // (`on_done` until it is called, when data last came, where the
    // redirects ended)
    struct St<S> {
        sink: S,
        done: Option<Done>,
        last: Instant,
        moved: Option<String>,
    }
    fn lock<S>(st: &Mutex<St<S>>) -> MutexGuard<'_, St<S>> {
        st.lock().unwrap_or_else(|e| e.into_inner())
    }
    let state = Arc::new(Mutex::new(St {
        sink,
        done: Some(Box::new(on_done)),
        last: Instant::now(),
        moved: None,
    }));
    let finish = |st: &Mutex<St<_>>, r: Result<(), NetError>| {
        let mut g = lock(st);
        let moved = g.moved.take();
        if let Some(f) = g.done.take() {
            drop(g);
            f(r, moved);
        }
    };
    // a watch: cancelled, or nothing for a while
    {
        let (st, p) = (state.clone(), progress.clone());
        super::thread::every(Duration::from_millis(500), move || {
            let stalled = {
                let g = lock(&st);
                if g.done.is_none() {
                    return false;
                }
                g.last.elapsed() > Duration::from_millis(STALL_MS)
            };
            let end = if p.as_ref().is_some_and(|p| p.cancelled()) {
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
    ehttp::streaming::fetch(identified(ehttp::Request::get(url)), move |part| {
        let mut g = lock(&state);
        if g.done.is_none() {
            return ControlFlow::Break(());
        }
        let end = match part {
            Err(e) => Err(NetError::Other(e)),
            Ok(Part::Response(r)) => {
                g.moved = r.headers.get("x-final-url").map(str::to_string);
                if !r.ok {
                    Err(NetError::Status(r.status))
                } else {
                    if let Some(p) = &progress {
                        p.start_file(r.headers.get("content-length").and_then(|l| l.parse().ok()));
                    }
                    g.last = Instant::now();
                    return ControlFlow::Continue(());
                }
            }
            Ok(Part::Chunk(c)) if c.is_empty() => Ok(()),
            Ok(Part::Chunk(c)) => match (g.sink)(&c) {
                Ok(()) => {
                    if let Some(p) = &progress {
                        p.add(c.len() as u64);
                    }
                    g.last = Instant::now();
                    return ControlFlow::Continue(());
                }
                Err(e) => Err(NetError::Other(e)),
            },
        };
        drop(g);
        finish(&state, end);
        ControlFlow::Break(())
    });
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
}
