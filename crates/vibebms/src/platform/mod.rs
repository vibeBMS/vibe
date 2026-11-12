//! Platform abstraction: time, background work, file system and HTTP.
//!
//! Native builds map straight to `std`; the web build (wasm32) uses an
//! in-memory file system fed by the browser's folder picker, runs background
//! jobs on the event loop and uses `XMLHttpRequest` for HTTP.

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

/// Blocking HTTP used by tables and IR (from background jobs).
pub struct Http;

impl Http {
    pub fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Read;
            let agent = ureq::AgentBuilder::new()
                .timeout(crate::platform::Duration::from_secs(30))
                .user_agent(concat!("vibeBMS/", env!("CARGO_PKG_VERSION")))
                .build();
            let mut req = agent.request(method, url);
            for (k, v) in headers {
                req = req.set(k, v);
            }
            let resp = match body {
                Some(b) => req.send_bytes(b),
                None => req.call(),
            }
            .map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            resp.into_reader()
                .take(64 << 20)
                .read_to_end(&mut buf)
                .map_err(|e| e.to_string())?;
            Ok(buf)
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_xhr(method, url, headers, body)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn web_xhr(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let e = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let xhr = web_sys::XmlHttpRequest::new().map_err(e)?;
    // synchronous XHR: deprecated but the only blocking option on the main thread.
    xhr.open_with_async(method, url, false).map_err(e)?;
    // binary-safe text: decode as latin1 so bytes round-trip
    xhr.override_mime_type("text/plain; charset=x-user-defined")
        .map_err(e)?;
    for (k, v) in headers {
        xhr.set_request_header(k, v).map_err(e)?;
    }
    match body {
        Some(b) => xhr.send_with_opt_u8_array(Some(b)).map_err(e)?,
        None => xhr.send().map_err(e)?,
    }
    let status = xhr.status().map_err(e)?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status} {url}"));
    }
    let text = xhr.response_text().map_err(e)?.unwrap_or_default();
    Ok(text.chars().map(|c| (c as u32 & 0xff) as u8).collect())
}

impl vibebms_tables::Fetcher for Http {
    fn get(&self, url: &str) -> Result<String, vibebms_tables::TableError> {
        let b = self
            .request("GET", url, &[], None)
            .map_err(vibebms_tables::TableError::Http)?;
        Ok(vibebms_tables::decode_body(&b))
    }
}

impl vibebms_ir::HttpClient for Http {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>, String> {
        Http::request(self, method, url, headers, body)
    }
}
