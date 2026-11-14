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

/// Send a request built by an IR client.
pub fn ir_send(
    req: vibebms_ir::HttpRequest,
    on_done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
) {
    http(req.method, &req.url, &req.headers, req.body, on_done)
}

/// Load a difficulty table (page → header → data, with mirror fallback).
pub fn load_table(
    url: &str,
    on_done: impl FnOnce(Result<vibebms_tables::DifficultyTable, String>) + Send + 'static,
) {
    fn drive(
        mut loader: vibebms_tables::TableLoader,
        url: String,
        on_done: Box<dyn FnOnce(Result<vibebms_tables::DifficultyTable, String>) + Send>,
    ) {
        http_get(&url, move |body| match loader.feed(body) {
            Ok(vibebms_tables::Step::Get(next)) => drive(loader, next, on_done),
            Ok(vibebms_tables::Step::Done(t)) => on_done(Ok(*t)),
            Err(e) => on_done(Err(e.to_string())),
        });
    }
    let on_done: Box<dyn FnOnce(Result<vibebms_tables::DifficultyTable, String>) + Send> =
        Box::new(on_done);
    // web: table servers send no CORS headers, so the page reads the copies
    // CI puts next to it (`tables/index.json`: table URL -> header copy)
    #[cfg(target_arch = "wasm32")]
    {
        let url = url.to_string();
        http_get("tables/index.json", move |r| {
            let copy = r
                .ok()
                .and_then(|b| {
                    serde_json::from_slice::<std::collections::HashMap<String, String>>(&b).ok()
                })
                .and_then(|m| m.get(&url).cloned())
                .and_then(|rel| {
                    let href = web_sys::window()?.location().href().ok()?;
                    let base = &href[..href.rfind('/')? + 1];
                    Some(format!("{base}{rel}"))
                });
            match copy {
                Some(first) => {
                    let loader = vibebms_tables::TableLoader::starting_at(&url, &first);
                    drive(loader, first, on_done)
                }
                None => {
                    let (loader, first) = vibebms_tables::TableLoader::new(&url);
                    drive(loader, first, on_done)
                }
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (loader, first) = vibebms_tables::TableLoader::new(url);
        drive(loader, first, on_done);
    }
}
