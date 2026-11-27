//! The drivers of bmseeker jobs here: natively bmseeker's own transport
//! (`native::Native`) on a thread of the job's own, its files to disk; in a
//! web page (and for a table's load everywhere) the page's driver, its
//! effects done asynchronously, one after another. A job asks again after
//! a server's passing failure itself.

use super::http::{call, request};
use super::IS_WEB;
use bmseeker::hosts::Context;
use bmseeker::install::job::{Effect, Fed, Job, Step, TaskJob, Unserved};
use bmseeker::net::{NetError, Request};
use bmseeker::progress::Progress;
use bmseeker::refusal::Refusals;
use bmseeker::web::{proxied, sends_cors, served, unserved, DOWNLOAD_MAX, PROXY};
use std::sync::{Arc, Mutex};

type Done<O> = Box<dyn FnOnce(O) + Send>;

/// What a job's effects are done with: the progress its files report to,
/// the refusals they keep to (none for a table's load: vibeBMS's own).
#[derive(Clone, Default)]
struct Net {
    progress: Option<Arc<Progress>>,
    refused: Option<Arc<Refusals>>,
}

/// Run a bmseeker job kept to `cx`'s refusals (a service that refused is
/// not asked again), its files reporting to `cx`'s progress and ended by
/// its cancel; then `on_done` with its result. Natively on a thread of its
/// own, on `native::Native` (files to their slots on disk; `on_done` on
/// that thread); on the web in the page ([`in_page`]).
pub fn run_job<J>(job: J, cx: &Context, on_done: impl FnOnce(J::Output) + Send + 'static)
where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let cx = cx.clone();
        super::spawn("download", move || {
            let mut job = job;
            let native = bmseeker::native::Native::new(&cx);
            let net = bmseeker::refusal::Guarded::new(&native, &cx);
            on_done(bmseeker::install::job::run_job(&mut job, &net))
        });
    }
    #[cfg(target_arch = "wasm32")]
    in_page(job, cx, on_done)
}

/// [`run_job`] in a web page: its requests and files through the vibeBMS
/// proxy (but to hosts that send CORS headers; where the redirects ended
/// from the proxy's `X-Final-Url`); each answer read to its request's
/// limit (a file to `bmseeker::web::DOWNLOAD_MAX`: it comes into memory);
/// a file ended by a stall too, a MEGA file decrypted and checked here;
/// its waits on a timer.
#[cfg(any(target_arch = "wasm32", test))]
fn in_page<J>(job: J, cx: &Context, on_done: impl FnOnce(J::Output) + Send + 'static)
where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    let net = Net {
        progress: cx.progress.clone(),
        refused: Some(cx.refused.clone()),
    };
    drive(job, net, on_done)
}

fn drive<J>(mut job: J, net: Net, on_done: impl FnOnce(J::Output) + Send + 'static)
where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    let next = job.start();
    step(job, next, net, Box::new(on_done));
}

fn step<J>(mut job: J, next: Step<J::Output>, net: Net, on_done: Done<J::Output>)
where
    J: Job + 'static,
    J::Output: Send + 'static,
{
    match next {
        Step::Done(out) => on_done(out),
        Step::Do(effect) => perform(effect, net.clone(), move |fed| {
            let next = job.feed(fed);
            step(job, next, net, on_done)
        }),
    }
}

/// Do `effect`, kept to `net`'s refusals, then `then` with what came of
/// it.
fn perform(effect: Effect, net: Net, then: impl FnOnce(Fed) + Send + 'static) {
    let Net { progress, refused } = net;
    if let Some(fed) = refused
        .as_ref()
        .and_then(|r| r.before(&effect, progress.as_deref()))
    {
        return then(fed);
    }
    let kept = {
        let (effect, progress) = (effect.clone(), progress.clone());
        move |fed: Fed| match &refused {
            Some(r) => r.after(&effect, fed, progress.as_deref()),
            None => fed,
        }
    };
    let asked = |u: &str| {
        if IS_WEB {
            proxied(PROXY, u)
        } else {
            u.to_string()
        }
    };
    match effect {
        Effect::Http(r) => {
            let url = r.url().to_string();
            let through = IS_WEB && !sends_cors(&url);
            let limit = r.limit() as u64;
            let req = match r {
                Request::Get(u, _) => ehttp::Request::get(asked(&u)),
                Request::Post { url: u, body, .. } => request(
                    "POST",
                    &asked(&u),
                    &[("Content-Type".into(), "application/json".into())],
                    body,
                ),
            };
            call(req, limit, move |a| {
                let a = a.and_then(|r| match r.status {
                    200..=299 => Ok(r.body),
                    n => Err(unserved(
                        &url,
                        n,
                        through,
                        r.headers.get("x-final-url"),
                        &r.body,
                    )),
                });
                then(kept(Fed::Http(a)))
            })
        }
        Effect::Download { target, .. } => {
            let through = IS_WEB && !sends_cors(&target.url);
            let body = Arc::new(Mutex::new(Vec::new()));
            let sink = body.clone();
            let announced = progress.clone();
            super::stream(
                &asked(&target.url),
                DOWNLOAD_MAX,
                progress,
                move |c| {
                    let mut b = sink.lock().unwrap_or_else(|e| e.into_inner());
                    // (the size the answer announced: `stream` told the
                    // progress)
                    if b.capacity() == 0 {
                        let n = announced.as_ref().and_then(|p| p.bytes().1);
                        if let Some(n) = n.and_then(|n| usize::try_from(n).ok()) {
                            let _ = b.try_reserve_exact(n);
                        }
                    }
                    b.extend_from_slice(c);
                    Ok(())
                },
                move |r, at| {
                    let fed = match r {
                        Ok(()) => {
                            let b = std::mem::take(
                                &mut *body.lock().unwrap_or_else(|e| e.into_inner()),
                            );
                            served(&target, b, None, at.clone())
                                .map_err(|error| Unserved { error, at })
                        }
                        Err(NetError::Status(n)) => Err(Unserved {
                            error: unserved(&target.url, n, through, at.as_deref(), &[]),
                            at,
                        }),
                        Err(error) => Err(Unserved { error, at }),
                    };
                    then(kept(Fed::Downloaded(fed)))
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
    drive(
        TaskJob::new(bmseeker::table::TableLoader::new(url)),
        Net::default(),
        move |r| on_done(r.map_err(|e| e.to_string())),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bmseeker::hosts::Target;
    use bmseeker::net::{Kind, NetError};
    use std::time::Duration;

    /// A one-shot HTTP server giving `answer`, at `/f`.
    fn serve(answer: &'static str) -> String {
        format!("{}/f", super::super::http::tests::serve(vec![answer]).0)
    }

    /// A page, a refused page, a file, a wait: what each came to.
    struct Steps {
        urls: Vec<String>,
        got: Vec<String>,
    }

    impl Job for Steps {
        type Output = Vec<String>;
        fn start(&mut self) -> Step<Vec<String>> {
            Step::Do(Effect::Http(Request::Get(self.urls[0].clone(), Kind::Page)))
        }
        fn feed(&mut self, fed: Fed) -> Step<Vec<String>> {
            self.got.push(match fed {
                Fed::Http(a) => format!("{:?}", a.map(String::from_utf8)),
                Fed::Downloaded(d) => format!("{:?}", d.map(|s| s.head)),
                Fed::Woke => "woke".into(),
            });
            Step::Do(match self.got.len() {
                1 => Effect::Http(Request::Get(self.urls[1].clone(), Kind::Page)),
                2 => Effect::Download {
                    target: Target::url(&self.urls[2]),
                    slot: Default::default(),
                },
                3 => Effect::Sleep(Duration::from_millis(10)),
                _ => return Step::Done(std::mem::take(&mut self.got)),
            })
        }
    }

    #[test]
    fn drives_a_job() {
        let urls = vec![
            serve("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npage"),
            serve("HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            serve("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfile"),
        ];
        let (tx, rx) = std::sync::mpsc::channel();
        let progress = Progress::new();
        let job = Steps {
            urls,
            got: Vec::new(),
        };
        let cx = Context::default().with_progress(progress.clone());
        in_page(job, &cx, move |got| {
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

    /// A job of one effect: what it came to.
    struct One(Option<Effect>);

    impl Job for One {
        type Output = Fed;
        fn start(&mut self) -> Step<Fed> {
            Step::Do(self.0.take().unwrap())
        }
        fn feed(&mut self, fed: Fed) -> Step<Fed> {
            Step::Done(fed)
        }
    }

    fn one(effect: Effect) -> Fed {
        let (tx, rx) = std::sync::mpsc::channel();
        in_page(One(Some(effect)), &Context::default(), move |fed| {
            let _ = tx.send(fed);
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    /// Natively a job runs on a thread of its own, on bmseeker's
    /// transport.
    #[test]
    fn runs_a_job_on_a_thread_of_its_own() {
        let (tx, rx) = std::sync::mpsc::channel();
        let here = std::thread::current().id();
        let job = One(Some(Effect::Sleep(Duration::from_millis(1))));
        run_job(job, &Context::default(), move |fed| {
            let _ = tx.send((fed, std::thread::current().id()));
        });
        let (fed, on) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(fed, Fed::Woke), "{fed:?}");
        assert_ne!(on, here);
    }

    /// An answer is read no further than its request's kind allows: a
    /// table's header of more than 1 MiB is no header.
    #[test]
    fn an_answer_is_kept_to_its_requests_limit() {
        let url = serve("HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n");
        let got = one(Effect::Http(Request::Get(url, Kind::Header)));
        let limit = Kind::Header.limit();
        assert!(
            matches!(&got, Fed::Http(Err(e)) if *e == bmseeker::net::too_large(limit)),
            "{got:?}"
        );
        let url = serve("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nhead");
        let got = one(Effect::Http(Request::Get(url, Kind::Header)));
        assert!(matches!(&got, Fed::Http(Ok(b)) if b == b"head"), "{got:?}");
    }

    /// A MEGA file that is not the one its link's key was made for does
    /// not come.
    #[test]
    fn a_mega_file_is_checked() {
        let url = serve("HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfile");
        let target = Target {
            mega_key: Some(vec![7; 32]),
            ..Target::url(&url)
        };
        let got = one(Effect::Download {
            target,
            slot: Default::default(),
        });
        assert!(
            matches!(&got, Fed::Downloaded(Err(u)) if u.error.to_string().contains("check failed")),
            "{got:?}"
        );
    }

    /// A service that refused is not asked again: not its other pages, not
    /// its files.
    #[test]
    fn keeps_to_refusals() {
        let refusing =
            serve("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let other = refusing.replace("/f", "/g");
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Steps {
            urls: vec![refusing, other.clone(), other],
            got: Vec::new(),
        };
        let cx = Context::default();
        in_page(job, &cx, move |got| {
            let _ = tx.send(got);
        });
        let got = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(got[..3].iter().all(|g| g.contains("Refused")), "{got:?}");
        assert_eq!(got[3], "woke");
    }
}
