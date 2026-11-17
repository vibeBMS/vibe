//! Background work: threads natively, the event loop on the web; and the
//! handle the app keeps of a background job.

use super::Duration;
use bmseeker::progress::Progress;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;

/// Run `f` in the background (a thread natively, the next event-loop tick on the web).
pub fn spawn<F: FnOnce() + Send + 'static>(name: &str, f: F) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        log::error!("spawn {name}: {e}");
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        wasm_bindgen_futures::spawn_local(async move { f() });
    }
}

/// Call `f(i)` for `i in 0..n`, on up to 8 threads natively (each taking
/// the next `i` when it is free).
pub fn parallel_for<F: Fn(usize) + Sync>(n: usize, f: F) {
    let threads = if cfg!(target_arch = "wasm32") {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(4, |t| t.get())
            .clamp(1, 8)
            .min(n)
    };
    if threads <= 1 {
        return (0..n).for_each(f);
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
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

/// Run `f` once `d` has passed (on a thread natively, the page's timer on the web).
pub fn after(d: Duration, f: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    spawn("timer", move || {
        std::thread::sleep(d);
        f()
    });
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let f = wasm_bindgen::closure::Closure::once_into_js(f);
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                f.unchecked_ref(),
                d.as_millis() as i32,
            );
        }
    }
}

/// Call `f` every `period` until it returns false.
pub fn every(period: Duration, mut f: impl FnMut() -> bool + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    spawn("timer", move || loop {
        std::thread::sleep(period);
        if !f() {
            break;
        }
    });
    #[cfg(target_arch = "wasm32")]
    {
        use std::{cell::Cell, rc::Rc};
        use wasm_bindgen::JsCast;
        let Some(w) = web_sys::window() else { return };
        let id = Rc::new(Cell::new(None));
        let own = id.clone();
        let tick = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if !f() {
                if let (Some(id), Some(w)) = (own.get(), web_sys::window()) {
                    w.clear_interval_with_handle(id);
                }
            }
        });
        id.set(
            w.set_interval_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                period.as_millis() as i32,
            )
            .ok(),
        );
        // (cleared when `f` says so; the closure stays for the page's life)
        tick.forget();
    }
}

/// The app's handle of a background job: what its work sends comes here
/// (read each frame), and its progress is shared with the work, which
/// reports to it and stops once it is cancelled. The job is over when the
/// work has dropped its sender and all it sent has been read.
pub struct Job<T> {
    rx: Receiver<T>,
    progress: Arc<Progress>,
    over: bool,
}

impl<T: Send + 'static> Job<T> {
    /// Start `work` with the job's sender and progress.
    pub fn start(work: impl FnOnce(Sender<T>, Arc<Progress>)) -> Job<T> {
        let (tx, rx) = channel();
        let progress = Progress::new();
        work(tx, progress.clone());
        Job {
            rx,
            progress,
            over: false,
        }
    }

    /// `f` in the background ([`spawn`]): its result is what the job sends.
    pub fn spawn(name: &str, f: impl FnOnce(Arc<Progress>) -> T + Send + 'static) -> Job<T> {
        Job::start(|tx, p| {
            spawn(name, move || {
                let _ = tx.send(f(p));
            })
        })
    }

    /// One GET ([`super::http_get`]): what `map` makes of its answer.
    pub fn get(
        url: &str,
        map: impl FnOnce(Result<Vec<u8>, String>) -> T + Send + 'static,
    ) -> Job<T> {
        Job::start(|tx, _| {
            super::http_get(url, move |r| {
                let _ = tx.send(map(r));
            })
        })
    }
}

impl<T> Job<T> {
    /// The next thing the work sent, if one came.
    pub fn poll(&mut self) -> Option<T> {
        match self.rx.try_recv() {
            Ok(t) => Some(t),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.over = true;
                None
            }
        }
    }

    /// Everything the work sent since the last call.
    pub fn drain(&mut self) -> Vec<T> {
        std::iter::from_fn(|| self.poll()).collect()
    }

    /// What the job in `slot` sent since the last call; the slot is
    /// emptied once the job is over.
    pub fn drain_slot(slot: &mut Option<Job<T>>) -> Vec<T> {
        let Some(job) = slot else { return Vec::new() };
        let out = job.drain();
        if job.over {
            *slot = None;
        }
        out
    }

    pub fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    /// Ask the work to stop (it sees [`Progress::cancelled`]).
    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_end_when_their_work_does() {
        let mut slot = Some(Job::start(|tx, _| {
            for i in 0..3 {
                let _ = tx.send(i);
            }
        }));
        assert_eq!(Job::drain_slot(&mut slot), [0, 1, 2]);
        assert!(slot.is_none());
        // a job's work waits for its cancel
        let job = Job::spawn("test", |p| {
            while !p.cancelled() {
                std::thread::yield_now();
            }
            7
        });
        job.cancel();
        let mut slot = Some(job);
        let mut got = Vec::new();
        while slot.is_some() {
            got.extend(Job::drain_slot(&mut slot));
            std::thread::yield_now();
        }
        assert_eq!(got, [7]);
    }
}
