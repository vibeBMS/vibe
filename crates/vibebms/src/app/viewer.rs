//! The chart viewer for BMS editors (desktop): uBMplay's command line, as
//! BMSE, iBMSC, BmsONE, woseq and BMS Maker 3 call their viewer.
//!
//! `vibebms -P [-N<measure>] <chart>` autoplays the chart from the measure
//! (0 when not given); `vibebms -S` stops it. The first `-P` stays running
//! as the viewer: it listens on a loopback port, and a later `-P` or `-S`
//! (a short-lived process the editor starts) hands its request over there
//! and exits, so the running viewer reloads the chart and starts at the
//! measure, or stops (on the web and mobile nothing starts it). A line per request: `PLAY\t<measure>\t<absolute
//! path>` or `STOP`, answered `OK`.
//!
//! Any program on the machine can connect to the port, so a request is
//! held to what `-P` sends: a short line, soon, naming a chart file that
//! is there ([`playable`]).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// The viewer's loopback port.
pub const PORT: u16 = 39177;

/// The longest line read (bytes): a path and a few words.
const LINE_MAX: usize = 16 << 10;

/// How long a request, or its answer, may take to come whole.
const PATIENCE: Duration = Duration::from_secs(3);

/// Connections served at once (more are closed unread).
const SERVED_AT_ONCE: usize = 8;

/// A request to the running viewer.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Play { path: PathBuf, measure: usize },
    Stop,
}

impl Request {
    fn line(&self) -> String {
        match self {
            Request::Play { path, measure } => {
                format!("PLAY\t{measure}\t{}\n", path.to_string_lossy())
            }
            Request::Stop => "STOP\n".into(),
        }
    }

    fn parse(line: &str) -> Option<Request> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "STOP" {
            return Some(Request::Stop);
        }
        let mut f = line.splitn(3, '\t');
        (f.next()? == "PLAY").then_some(())?;
        let measure = f.next()?.parse().ok()?;
        let path = PathBuf::from(f.next().filter(|p| !p.is_empty())?);
        Some(Request::Play { path, measure })
    }
}

/// Whether the running viewer plays `path` when asked to: a chart file
/// that is there, by its full path. Not a share's (`\\host\...`: opening
/// one signs this machine in to that host).
fn playable(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    let local = match path.components().next() {
        Some(Component::Prefix(p)) => {
            matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        }
        _ => true,
    };
    local && path.is_absolute() && crate::library::is_chart(path) && path.is_file()
}

/// The line `s` sends within [`PATIENCE`], without its line break; `None`
/// when none came whole in that time, or it is longer than [`LINE_MAX`].
fn read_line(s: &mut TcpStream) -> Option<String> {
    let deadline = Instant::now() + PATIENCE;
    let mut line = Vec::new();
    let mut chunk = [0u8; 1024];
    while line.len() <= LINE_MAX {
        let left = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())?;
        s.set_read_timeout(Some(left)).ok()?;
        let n = s.read(&mut chunk).ok().filter(|n| *n > 0)?;
        line.extend_from_slice(&chunk[..n]);
        if let Some(end) = line.iter().position(|b| *b == b'\n') {
            line.truncate(end);
            return String::from_utf8(line).ok();
        }
    }
    None
}

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// `req` handed to the viewer running at `port`: true when it took it.
pub fn forward_to(port: u16, req: &Request) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&addr(port), Duration::from_millis(300)) else {
        return false;
    };
    let _ = s.set_write_timeout(Some(PATIENCE));
    if s.write_all(req.line().as_bytes()).is_err() {
        return false;
    }
    read_line(&mut s).is_some_and(|answer| answer.trim() == "OK")
}

/// Take the request `s` sends, if it is one the viewer serves, and answer
/// it.
fn serve(mut s: TcpStream, tx: &mpsc::Sender<Request>) {
    let _ = s.set_write_timeout(Some(PATIENCE));
    let Some(line) = read_line(&mut s) else {
        return;
    };
    let req = Request::parse(&line).filter(|r| match r {
        Request::Play { path, .. } => playable(path),
        Request::Stop => true,
    });
    let taken = req.is_some_and(|req| tx.send(req).is_ok());
    let _ = s.write_all(if taken { b"OK\n" } else { b"ERR\n" });
}

/// The running viewer's end: requests as they come.
pub struct Server {
    rx: mpsc::Receiver<Request>,
}

impl Server {
    /// Listen at `port` (`None`: another program has it).
    pub fn start(port: u16) -> Option<Server> {
        let listener = match TcpListener::bind(addr(port)) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("viewer: cannot listen on port {port}: {e}");
                return None;
            }
        };
        let (tx, rx) = mpsc::channel();
        let serving = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("viewer".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    let Ok(s) = conn else { continue };
                    // each on a thread of its own (one that sends nothing
                    // holds up no other), a few at a time
                    if serving.fetch_add(1, Ordering::SeqCst) >= SERVED_AT_ONCE {
                        serving.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let (tx, done) = (tx.clone(), serving.clone());
                    let served = std::thread::Builder::new()
                        .name("viewer request".into())
                        .spawn(move || {
                            serve(s, &tx);
                            done.fetch_sub(1, Ordering::SeqCst);
                        });
                    if served.is_err() {
                        serving.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            })
            .ok()?;
        log::info!("viewer: waiting for requests on port {port}");
        Some(Server { rx })
    }

    /// The newest request since the last call (older ones are superseded).
    pub fn poll(&self) -> Option<Request> {
        self.rx.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_reach_the_running_viewer() {
        let dir = std::env::temp_dir().join(format!("vibebms-viewer-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a b")).unwrap();
        let chart = dir.join("a b/___bmse_temp.bms");
        std::fs::write(&chart, "#TITLE a").unwrap();
        let play = Request::Play {
            path: chart.clone(),
            measure: 12,
        };
        assert_eq!(Request::parse(&play.line()), Some(play.clone()));
        assert_eq!(Request::parse("STOP\r\n"), Some(Request::Stop));
        assert_eq!(Request::parse("PLAY\tx\t/a.bms"), None);
        assert_eq!(Request::parse("HELLO"), None);
        // over the loopback port (one the test picks)
        let port = 39177 + 7 + (std::process::id() % 1000) as u16;
        assert!(!forward_to(port, &Request::Stop));
        let server = Server::start(port).unwrap();
        assert!(forward_to(port, &play));
        assert!(forward_to(port, &Request::Stop));
        // the newest wins
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.poll(), Some(Request::Stop));
        assert_eq!(server.poll(), None);

        // only a chart file that is there is played
        let other = dir.join("notes.txt");
        std::fs::write(&other, "x").unwrap();
        let mut refused = vec![
            other,
            dir.join("missing.bms"),
            dir.join("a b"),
            PathBuf::from("a b/___bmse_temp.bms"),
        ];
        if cfg!(windows) {
            refused.push(PathBuf::from(r"\\127.0.0.1\share\a.bms"));
            refused.push(PathBuf::from(r"\\?\UNC\127.0.0.1\share\a.bms"));
            refused.push(PathBuf::from(r"\\.\pipe\a.bms"));
        }
        for path in refused {
            assert!(!playable(&path), "{}", path.display());
            assert!(!forward_to(port, &Request::Play { path, measure: 0 }));
        }
        assert_eq!(server.poll(), None);

        // a line that never ends is not read to its end
        let mut s = TcpStream::connect(addr(port)).unwrap();
        let _ = s.write_all(&vec![b'x'; LINE_MAX + 4096]);
        assert_eq!(read_line(&mut s), None);
        // connections that send nothing hold up no request
        let idle: Vec<TcpStream> = (0..SERVED_AT_ONCE / 2)
            .map(|_| TcpStream::connect(addr(port)).unwrap())
            .collect();
        let start = Instant::now();
        assert!(forward_to(port, &play));
        assert!(start.elapsed() < PATIENCE / 2);
        assert_eq!(server.poll(), Some(play));
        drop(idle);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
