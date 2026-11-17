//! HTTP (ehttp: ureq natively, `fetch` in browsers). Callbacks run on a
//! worker thread natively and on the event loop on the web.

use super::{Duration, Instant};
use bmseeker::hosts::STALL_MS;
use bmseeker::net::{Answer, NetError};
use bmseeker::progress::Progress;
use std::sync::{Arc, Mutex, MutexGuard};

pub(super) fn request(
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

/// Send `req`: the body of its answer, or why there is none (a status as
/// `NetError::Status`, which bmseeker's retries and checks look at).
pub fn fetch(req: ehttp::Request, on_done: impl FnOnce(Answer) + Send + 'static) {
    ehttp::fetch(req, move |r| {
        on_done(match r {
            Ok(r) if r.ok => Ok(r.bytes),
            Ok(r) => Err(NetError::Status(r.status)),
            Err(e) => Err(NetError::Other(e)),
        })
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

/// Send a request built by an IR client.
pub fn ir_send(
    req: vibebms_ir::HttpRequest,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
    send(
        request(req.method, &req.url, &req.headers, req.body),
        on_done,
    )
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
    ehttp::streaming::fetch(ehttp::Request::get(url), move |part| {
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
mod tests {
    use super::*;

    #[test]
    fn a_stream_ends_when_cancelled() {
        use std::io::{Read, Write};
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
}
