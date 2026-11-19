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

/// Call `f(i)` for `i in 0..n`, on up to 8 threads natively.
pub fn parallel_for<F: Fn(usize) + Sync>(n: usize, f: F) {
    vibebms_audio::parallel_for(n, 8, f)
}

/// Run `f` once `d` has passed: natively on a thread of its own then (one
/// thread waits for every timer), on the web on the page's timer.
pub fn after(d: Duration, f: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    timer::add(super::Instant::now() + d, Box::new(f));
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
    after(period, move || {
        if f() {
            every(period, f)
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
mod timer {
    use super::super::Instant;
    use std::cmp::Ordering;
    use std::collections::binary_heap::{BinaryHeap, PeekMut};
    use std::sync::{Condvar, LazyLock, Mutex, PoisonError};

    type Task = Box<dyn FnOnce() + Send>;

    /// A timer: when it is due, what then. The heap's top is the soonest.
    struct Due(Instant, Task);

    impl PartialEq for Due {
        fn eq(&self, o: &Self) -> bool {
            self.0 == o.0
        }
    }
    impl Eq for Due {}
    impl PartialOrd for Due {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Due {
        fn cmp(&self, o: &Self) -> Ordering {
            o.0.cmp(&self.0)
        }
    }

    static TIMERS: LazyLock<(Mutex<BinaryHeap<Due>>, Condvar)> = LazyLock::new(|| {
        super::spawn("timer", wait);
        Default::default()
    });

    pub fn add(at: Instant, f: Task) {
        let (due, woken) = &*TIMERS;
        due.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Due(at, f));
        woken.notify_one();
    }

    /// The timer thread: each timer, once due, on a thread of its own (it
    /// may block, as a request's `on_done` does).
    fn wait() {
        let (due, woken) = &*TIMERS;
        let mut q = due.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let now = Instant::now();
            while let Some(d) = q.peek_mut().filter(|d| d.0 <= now) {
                super::spawn("timer", PeekMut::pop(d).1);
            }
            q = match q.peek().map(|d| d.0 - now) {
                Some(wait) => woken
                    .wait_timeout(q, wait)
                    .map_or_else(|e| e.into_inner().0, |r| r.0),
                None => woken.wait(q).unwrap_or_else(PoisonError::into_inner),
            };
        }
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

    /// What the job in `slot` sent since the last call; the slot is
    /// emptied once the job is over.
    pub fn drain_slot(slot: &mut Option<Job<T>>) -> Vec<T> {
        let Some(job) = slot else { return Vec::new() };
        let out = std::iter::from_fn(|| job.poll()).collect();
        if job.over {
            *slot = None;
        }
        out
    }

    /// What the jobs in `jobs` sent since the last call; the jobs that are
    /// over go.
    pub fn drain_all(jobs: &mut Vec<Job<T>>) -> Vec<T> {
        let mut out = Vec::new();
        jobs.retain_mut(|job| {
            out.extend(std::iter::from_fn(|| job.poll()));
            !job.over
        });
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
