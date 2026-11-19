//! The thread an integration runs on: it keeps a connection to its program
//! up, tries again after a wait that doubles with each failed try, and
//! tells the settings how that goes; the integration ([`Peer`]) speaks its
//! protocol.

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
    fn open(&mut self) -> Result<Self::Conn, String>;
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
    let mut next_try = Instant::now();
    let mut conn = None;
    loop {
        let due = match conn {
            Some(_) => p.due(),
            None => next_try,
        };
        match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
            Ok(m) => {
                match p.take(m, &mut conn) {
                    Turn::Go => {}
                    Turn::Again => {
                        backoff = first;
                        next_try = Instant::now();
                    }
                    Turn::Quit => return p.quit(conn),
                }
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return p.quit(conn),
        }
        let now = Instant::now();
        match &mut conn {
            Some(c) => {
                if let Err(e) = p.tend(c) {
                    log::info!("{}: {e}", P::NAME);
                    set_status(Status::Failed(e));
                    conn = None;
                    next_try = now + backoff;
                }
            }
            None if now >= next_try => match p.open() {
                Ok(c) => {
                    set_status(Status::Connected);
                    backoff = first;
                    conn = Some(c);
                }
                Err(e) => {
                    log::debug!("{}: {e}", P::NAME);
                    set_status(Status::Failed(e));
                    next_try = now + backoff;
                    backoff = (backoff * 2).min(max);
                }
            },
            None => {}
        }
    }
}
