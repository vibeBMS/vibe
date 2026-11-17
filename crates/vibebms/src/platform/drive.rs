//! The driver of bmseeker jobs here: their effects done asynchronously, one
//! after another. (Native downloads run on bmseeker's own transport,
//! `native::Native`, on a thread of their own: to disk.) A job asks again
//! after a server's passing failure itself.

use super::http::{fetch, request};
use super::IS_WEB;
use bmseeker::install::job::{Effect, Fed, Job, Served, Step, TaskJob, Unserved};
use bmseeker::net::Request;
use bmseeker::progress::Progress;
use std::sync::{Arc, Mutex};

type Done<O> = Box<dyn FnOnce(O) + Send>;

/// Run a bmseeker job: on the web its requests through the vibeBMS proxy
/// (but to hosts that send CORS headers) and its files through the proxy
/// too (where the redirects ended from the proxy's `X-Final-Url`); its
/// files reporting to `progress`, ended by its cancel or a stall, a MEGA
/// file decrypted here; its waits on a timer. Then `on_done` with its
/// result.
pub fn run_job<J>(
    mut job: J,
    progress: Option<Arc<Progress>>,
    on_done: impl FnOnce(J::Output) + Send + 'static,
) where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    let next = job.start();
    step(job, next, progress, Box::new(on_done));
}

fn step<J>(
    mut job: J,
    next: Step<J::Output>,
    progress: Option<Arc<Progress>>,
    on_done: Done<J::Output>,
) where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    match next {
        Step::Done(out) => on_done(out),
        Step::Do(effect) => perform(effect, progress.clone(), move |fed| {
            let next = job.feed(fed);
            step(job, next, progress, on_done)
        }),
    }
}

/// Do `effect`, then `then` with what came of it.
fn perform(
    effect: Effect,
    progress: Option<Arc<Progress>>,
    then: impl FnOnce(Fed) + Send + 'static,
) {
    let proxied = |u: &str| {
        if IS_WEB {
            crate::hosts::via_proxy(u)
        } else {
            u.to_string()
        }
    };
    match effect {
        Effect::Http(r) => {
            let url = |u: &str| {
                if crate::hosts::sends_cors(u) {
                    u.to_string()
                } else {
                    proxied(u)
                }
            };
            let req = match r {
                Request::Get(u) => ehttp::Request::get(url(&u)),
                Request::Post { url: u, body } => request(
                    "POST",
                    &url(&u),
                    &[("Content-Type".into(), "application/json".into())],
                    body,
                ),
            };
            fetch(req, move |a| then(Fed::Http(a)))
        }
        Effect::Download { target, .. } => {
            let body = Arc::new(Mutex::new(Vec::new()));
            let sink = body.clone();
            super::stream(
                &proxied(&target.url),
                progress,
                move |c| {
                    sink.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend_from_slice(c);
                    Ok(())
                },
                move |r, at| {
                    let fed = match r {
                        Ok(()) => {
                            let mut b = std::mem::take(
                                &mut *body.lock().unwrap_or_else(|e| e.into_inner()),
                            );
                            if let Some(k) = &target.mega_key {
                                bmseeker::hosts::MegaCipher::new(k).apply(&mut b);
                            }
                            Ok(Served::bytes(b, None, at))
                        }
                        Err(error) => Err(Unserved { error, at }),
                    };
                    then(Fed::Downloaded(fed))
                },
            )
        }
        Effect::Sleep(d) => super::thread::after(d, move || then(Fed::Woke)),
    }
}

/// Load a difficulty table (page → header → data, with mirror fallback).
pub fn load_table(
    url: &str,
    on_done: impl FnOnce(Result<bmseeker::table::DifficultyTable, String>) + Send + 'static,
) {
    // web: table servers send no CORS headers; the vibeBMS proxy does
    run_job(
        TaskJob::new(bmseeker::table::TableLoader::new(url)),
        None,
        move |r| on_done(r.map_err(|e| e.to_string())),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmseeker::hosts::Target;
    use bmseeker::net::NetError;
    use std::time::Duration;

    /// A one-shot HTTP server answering `status` and `body`.
    fn serve(status: &'static str, body: &'static [u8]) -> String {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let _ = s.read(&mut [0u8; 4096]);
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(body);
        });
        format!("http://{addr}/f")
    }

    /// A page, a refused page, a file, a wait: what each came to.
    struct Steps {
        urls: Vec<String>,
        got: Vec<String>,
    }

    impl Job for Steps {
        type Output = Vec<String>;
        fn start(&mut self) -> Step<Vec<String>> {
            Step::Do(Effect::Http(Request::Get(self.urls[0].clone())))
        }
        fn feed(&mut self, fed: Fed) -> Step<Vec<String>> {
            self.got.push(match fed {
                Fed::Http(a) => format!("{:?}", a.map(String::from_utf8)),
                Fed::Downloaded(d) => format!("{:?}", d.map(|s| s.head)),
                Fed::Woke => "woke".into(),
            });
            Step::Do(match self.got.len() {
                1 => Effect::Http(Request::Get(self.urls[1].clone())),
                2 => Effect::Download {
                    target: Target::url(&self.urls[2]),
                    slot: Default::default(),
                    any: true,
                },
                3 => Effect::Sleep(Duration::from_millis(10)),
                _ => return Step::Done(std::mem::take(&mut self.got)),
            })
        }
    }

    #[test]
    fn drives_a_job() {
        let urls = vec![
            serve("200 OK", b"page"),
            serve("503 Busy", b""),
            serve("200 OK", b"file"),
        ];
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = Progress::new();
        let job = Steps {
            urls,
            got: Vec::new(),
        };
        run_job(job, Some(progress.clone()), move |got| {
            let _ = tx.send(got);
        });
        let got = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            got,
            [
                r#"Ok(Ok("page"))"#.to_string(),
                format!("{:?}", Err::<(), _>(NetError::Status(503))),
                "Ok([102, 105, 108, 101])".into(),
                "woke".into(),
            ]
        );
        assert_eq!(progress.bytes(), (4, Some(4)));
    }
}
