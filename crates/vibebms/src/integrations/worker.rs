//! The thread an integration runs on: it keeps a connection to its program
//! up, tries again after a wait that doubles with each try that failed or
//! did not hold, and tells the settings how that goes; the integration
//! ([`Peer`]) speaks its protocol.

use super::Status;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// An integration, as its thread runs it.
pub trait Peer: Send + 'static {
    /// What the game sends it.
    type Msg: Send + 'static;
    type Conn;
    /// Its thread's name, and its logs'.
    const NAME: &'static str;
    /// The wait after a first failed try, and the longest.
    const BACKOFF: (Duration, Duration);

    /// A connection, and what a new one is told.
    fn open(&mut self) -> Result<Self::Conn, Refusal>;
    /// A message from the game, with the connection if there is one (it
    /// drops a connection that failed: opened again at once).
    fn take(&mut self, m: Self::Msg, c: &mut Option<Self::Conn>) -> Turn;
    /// When [`Peer::tend`] is due next, while connected.
    fn due(&self) -> Instant;
    /// The connection's own work, when no message waits; an error: it is
    /// gone.
    fn tend(&mut self, c: &mut Self::Conn) -> Result<(), String>;
    /// The game is done with it.
    fn quit(&mut self, c: Option<Self::Conn>);
}

/// Why no connection was made.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    /// Tried again after the wait (the program is not running).
    Later(String),
    /// Not tried again until the settings change ([`Turn::Again`]): trying
    /// again would only repeat it (a wrong password).
    Never(String),
}

/// What a message asks of the thread.
pub enum Turn {
    Go,
    /// Connect again at once, the waits from the first (new settings).
    Again,
    Quit,
}

/// An integration's thread, while this lives (dropped: it ends).
pub struct Worker<M> {
    tx: Sender<M>,
    status: Arc<Mutex<Status>>,
    done: Receiver<()>,
}

impl<M: Send + 'static> Worker<M> {
    pub fn start<P: Peer<Msg = M>>(peer: P) -> Worker<M> {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status::Connecting));
        let st = status.clone();
        let spawned = std::thread::Builder::new()
            .name(P::NAME.into())
            .spawn(move || {
                run(peer, rx, &st);
                let _ = done_tx.send(());
            });
        if let Err(e) = spawned {
            log::error!("{}: {e}", P::NAME);
        }
        Worker { tx, status, done }
    }

    pub fn send(&self, m: M) {
        let _ = self.tx.send(m);
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Wait up to `d` for the thread to end (it was told to).
    pub fn wait_end(&self, d: Duration) {
        let _ = self.done.recv_timeout(d);
    }
}

fn run<P: Peer>(mut p: P, rx: Receiver<P::Msg>, status: &Mutex<Status>) {
    let set_status = |s: Status| *status.lock().unwrap() = s;
    let (first, max) = P::BACKOFF;
    let mut backoff = first;
    // when to connect (`None`: not until the settings change)
    let mut next_try = Some(Instant::now());
    let mut conn = None;
    // since when the connection is up
    let mut since = Instant::now();
    loop {
        let due = match conn {
            Some(_) => Some(p.due()),
            None => next_try,
        };
        let got = match due {
            Some(t) => rx.recv_timeout(t.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let now = Instant::now();
        let was_up = conn.is_some();
        match got {
            Ok(m) => match p.take(m, &mut conn) {
                Turn::Go => {}
                Turn::Again => {
                    backoff = first;
                    next_try = Some(now);
                    continue;
                }
                Turn::Quit => return p.quit(conn),
            },
            Err(RecvTimeoutError::Disconnected) => return p.quit(conn),
            Err(RecvTimeoutError::Timeout) => match &mut conn {
                Some(c) => {
                    if let Err(e) = p.tend(c) {
                        log::info!("{}: {e}", P::NAME);
                        set_status(Status::Failed(e));
                        conn = None;
                    }
                }
                None if next_try.is_some_and(|t| now >= t) => match p.open() {
                    Ok(c) => {
                        set_status(Status::Connected);
                        conn = Some(c);
                        since = now;
                    }
                    Err(Refusal::Later(e)) => {
                        log::debug!("{}: {e}", P::NAME);
                        set_status(Status::Failed(e));
                        next_try = Some(now + backoff);
                        backoff = (backoff * 2).min(max);
                    }
                    Err(Refusal::Never(e)) => {
                        log::info!("{}: {e}", P::NAME);
                        set_status(Status::Failed(e));
                        next_try = None;
                    }
                },
                None => {}
            },
        }
        // a connection that held is made again after the first wait; one
        // that is dropped as soon as it is made waits longer each time
        if was_up && conn.is_none() {
            if now.duration_since(since) >= max {
                backoff = first;
            }
            next_try = Some(now + backoff);
            backoff = (backoff * 2).min(max);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A program that takes the connection and drops it at once, or
    /// refuses it for good.
    struct Flaky {
        opens: Arc<AtomicUsize>,
        refuse: bool,
    }

    impl Peer for Flaky {
        type Msg = ();
        type Conn = ();
        const NAME: &'static str = "flaky";
        const BACKOFF: (Duration, Duration) = (Duration::from_millis(5), Duration::from_millis(80));

        fn open(&mut self) -> Result<(), Refusal> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            match self.refuse {
                true => Err(Refusal::Never("wrong password".into())),
                false => Ok(()),
            }
        }
        fn take(&mut self, _: (), _: &mut Option<()>) -> Turn {
            Turn::Again
        }
        fn due(&self) -> Instant {
            Instant::now()
        }
        fn tend(&mut self, _: &mut ()) -> Result<(), String> {
            Err("closed".into())
        }
        fn quit(&mut self, _: Option<()>) {}
    }

    /// A connection dropped as soon as it is made is not made again every
    /// first wait: the waits double up to the longest.
    #[test]
    fn a_connection_that_does_not_hold_backs_off() {
        let opens = Arc::new(AtomicUsize::new(0));
        let w = Worker::start(Flaky {
            opens: opens.clone(),
            refuse: false,
        });
        std::thread::sleep(Duration::from_millis(300));
        // at 0, 5, 15, 35, 75, 155 and 235 ms (every 5 ms would be 60)
        let n = opens.load(Ordering::SeqCst);
        assert!((2..=20).contains(&n), "{n}");
        assert_eq!(w.status(), Status::Failed("closed".into()));
    }

    /// A refusal for good is not tried again until the settings change.
    #[test]
    fn a_refusal_for_good_waits_for_new_settings() {
        let opens = Arc::new(AtomicUsize::new(0));
        let w = Worker::start(Flaky {
            opens: opens.clone(),
            refuse: true,
        });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(opens.load(Ordering::SeqCst), 1);
        assert_eq!(w.status(), Status::Failed("wrong password".into()));
        w.send(());
        let deadline = Instant::now() + Duration::from_secs(10);
        while opens.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
