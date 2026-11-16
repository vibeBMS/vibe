//! Platform abstraction: time, background work, file system and HTTP.
//!
//! Native builds map straight to `std`; the web build (wasm32) uses an
//! in-memory file system fed by the browser's folder picker, runs background
//! jobs on the event loop and uses `XMLHttpRequest` for HTTP.

pub mod dxa;
pub mod screen;
pub mod vfs;

pub use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const IS_WEB: bool = cfg!(target_arch = "wasm32");

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Open `url` in the system browser (a new tab on the web).
pub fn open_url(url: &str) -> bool {
    match webbrowser::open(url) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("open {url}: {e}");
            false
        }
    }
}

/// Run `f` in the background (a thread natively, the next event-loop tick on the web).
pub fn spawn<F: FnOnce() + Send + 'static>(name: &str, f: F) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
            log::error!("spawn {name}: {e}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        wasm_bindgen_futures::spawn_local(async move { f() });
    }
}

/// Number of worker threads for data-parallel jobs (1 on the web).
pub fn workers() -> usize {
    if IS_WEB {
        1
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 8)
    }
}

/// Call `f(i)` for `i in 0..n`, in parallel natively.
pub fn parallel_for<F: Fn(usize) + Sync>(n: usize, f: F) {
    let w = workers().min(n.max(1));
    if w <= 1 {
        (0..n).for_each(f);
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let next = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..w {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    f(i);
                });
            }
        });
    }
}

/// Asynchronous HTTP (ehttp: ureq natively, `fetch` in browsers). `on_done`
/// runs on a worker thread natively and on the event loop on the web.
pub fn http(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
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
    ehttp::fetch(req, move |r| {
        on_done(match r {
            Ok(r) if r.ok => Ok(r.bytes),
            Ok(r) => Err(format!("HTTP {} {}", r.status, r.url)),
            Err(e) => Err(e),
        })
    });
}

pub fn http_get(url: &str, on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
    http("GET", url, &[], Vec::new(), on_done)
}

/// [`http_get`] that reports the bytes coming in to `progress` (a song's
/// download on the web), and ends when it is cancelled or no data comes for
/// `bmseeker::hosts`' stall time (60 s).
#[cfg(target_arch = "wasm32")]
pub fn http_get_progress(
    url: &str,
    progress: Option<std::sync::Arc<bmseeker::progress::Progress>>,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
    use ehttp::streaming::Part;
    use std::ops::ControlFlow;
    use std::sync::{Arc, Mutex};
    use wasm_bindgen::JsCast;
    type Done = Box<dyn FnOnce(Result<Vec<u8>, String>) + Send>;
    // (the body so far, `on_done` until it is called, when data last came)
    struct St {
        body: Vec<u8>,
        done: Option<Done>,
        last: f64,
        watch: Option<i32>,
    }
    let now = || js_sys::Date::now();
    let state = Arc::new(Mutex::new(St {
        body: Vec::new(),
        done: Some(Box::new(on_done)),
        last: now(),
        watch: None,
    }));
    let finish = |st: &Arc<Mutex<St>>, r: Result<(), String>| {
        let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
        let body = std::mem::take(&mut g.body);
        if let (Some(id), Some(w)) = (g.watch.take(), web_sys::window()) {
            w.clear_interval_with_handle(id);
        }
        if let Some(f) = g.done.take() {
            drop(g);
            f(r.map(|_| body));
        }
    };
    // a watch: cancelled, or nothing for a while
    {
        let (st, p) = (state.clone(), progress.clone());
        let tick = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let stalled = {
                let g = st.lock().unwrap_or_else(|e| e.into_inner());
                now() - g.last > bmseeker::hosts::STALL_MS as f64
            };
            if p.as_ref().is_some_and(|p| p.cancelled()) {
                finish(&st, Err(bmseeker::progress::CANCELLED.into()));
            } else if stalled {
                let s = bmseeker::hosts::STALL_MS / 1000;
                finish(&st, Err(format!("no data for {s} s")));
            }
        });
        if let Some(w) = web_sys::window() {
            if let Ok(id) = w.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                500,
            ) {
                state.lock().unwrap_or_else(|e| e.into_inner()).watch = Some(id);
            }
        }
        // (cleared by `finish`; the closure stays for the page's life)
        tick.forget();
    }
    ehttp::streaming::fetch(
        ehttp::Request::get(url),
        Box::new(move |part| {
            let over = state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .done
                .is_none();
            if over {
                return ControlFlow::Break(());
            }
            match part {
                Err(e) => {
                    finish(&state, Err(e));
                    ControlFlow::Break(())
                }
                Ok(Part::Response(r)) if !r.ok => {
                    finish(&state, Err(format!("HTTP {} {}", r.status, r.url)));
                    ControlFlow::Break(())
                }
                Ok(Part::Response(r)) => {
                    if let Some(p) = &progress {
                        p.start_file(r.headers.get("content-length").and_then(|l| l.parse().ok()));
                    }
                    state.lock().unwrap_or_else(|e| e.into_inner()).last = now();
                    ControlFlow::Continue(())
                }
                Ok(Part::Chunk(c)) if c.is_empty() => {
                    finish(&state, Ok(()));
                    ControlFlow::Break(())
                }
                Ok(Part::Chunk(c)) => {
                    if let Some(p) = &progress {
                        p.add(c.len() as u64);
                    }
                    let mut g = state.lock().unwrap_or_else(|e| e.into_inner());
                    g.body.extend_from_slice(&c);
                    g.last = now();
                    ControlFlow::Continue(())
                }
            }
        }),
    );
}

/// Send a request built by an IR client.
pub fn ir_send(
    req: vibebms_ir::HttpRequest,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
    http(req.method, &req.url, &req.headers, req.body, on_done)
}

/// Run a bmseeker task: its requests made here, one after another
/// (`proxy`: on the web through the vibeBMS proxy, for hosts that send no
/// CORS headers), then `on_done` with its result.
pub fn run_task<T>(task: T, proxy: bool, on_done: impl FnOnce(T::Output) + Send + 'static)
where
    T: bmseeker::net::Task + 'static,
    T::Output: Send + 'static,
{
    use bmseeker::net::{Next, Request};
    type Done<O> = Box<dyn FnOnce(O) + Send>;
    fn step<T>(mut task: T, next: Next<T::Output>, proxy: bool, on_done: Done<T::Output>)
    where
        T: bmseeker::net::Task + 'static,
        T::Output: Send + 'static,
    {
        let url = |u: &str| {
            if proxy && IS_WEB {
                crate::hosts::via_proxy(u)
            } else {
                u.to_string()
            }
        };
        match next {
            Next::Done(out) => on_done(out),
            Next::Ask(Request::Get(u)) => http_get(&url(&u), move |a| {
                let next = task.feed(a);
                step(task, next, proxy, on_done)
            }),
            Next::Ask(Request::Post { url: u, body }) => http(
                "POST",
                &url(&u),
                &[("Content-Type".into(), "application/json".into())],
                body,
                move |a| {
                    let next = task.feed(a);
                    step(task, next, proxy, on_done)
                },
            ),
        }
    }
    let mut task = task;
    let next = task.start();
    step(task, next, proxy, Box::new(on_done));
}

/// Load a difficulty table (page → header → data, with mirror fallback).
pub fn load_table(
    url: &str,
    on_done: impl FnOnce(Result<bmseeker::table::DifficultyTable, String>) + Send + 'static,
) {
    // web: table servers send no CORS headers; the vibeBMS proxy does
    run_task(bmseeker::table::TableLoader::new(url), true, move |r| {
        on_done(r.map_err(|e| e.to_string()))
    });
}
