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

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// The viewer's loopback port.
pub const PORT: u16 = 39177;

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

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// `req` handed to the viewer running at `port`: true when it took it.
pub fn forward_to(port: u16, req: &Request) -> bool {
    let Ok(mut s) = TcpStream::connect_timeout(&addr(port), Duration::from_millis(300)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
    if s.write_all(req.line().as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(s).read_line(&mut answer).is_ok() && answer.trim() == "OK"
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
        std::thread::Builder::new()
            .name("viewer".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    let Ok(mut s) = conn else { continue };
                    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                    let mut line = String::new();
                    let Ok(reader) = s.try_clone() else { continue };
                    if BufReader::new(reader).read_line(&mut line).is_err() {
                        continue;
                    }
                    let Some(req) = Request::parse(&line) else {
                        let _ = s.write_all(b"ERR\n");
                        continue;
                    };
                    if tx.send(req).is_err() {
                        return;
                    }
                    let _ = s.write_all(b"OK\n");
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
        let play = Request::Play {
            path: PathBuf::from("/tmp/a b/___bmse_temp.bms"),
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
    }
}
