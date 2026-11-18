//! Discord Rich Presence over the Discord app's local IPC (no network: a
//! Unix socket `discord-ipc-N` in the runtime or temporary folder, or the
//! named pipe `\\.\pipe\discord-ipc-N` on Windows, N from 0 to 9).
//!
//! Each message is a frame: its opcode and its length (two little-endian
//! u32), then that much JSON. The client opens with a HANDSHAKE (`{"v": 1,
//! "client_id": …}`), which Discord answers with a READY dispatch; then
//! FRAMEs carry commands, here `SET_ACTIVITY` (Discord allows about five a
//! 20 seconds: changes are sent at most every 4 s, the last one winning).
//! PING is answered with PONG; CLOSE ends it (a wrong application id).

use super::{Activity, PlayKind, SharedStatus, Status};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;
pub const OP_PING: u32 = 3;
pub const OP_PONG: u32 = 4;

/// The least time between two activities sent.
const MIN_GAP: Duration = Duration::from_secs(4);
/// The longest wait between two tries to reach Discord.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// A frame: opcode, length, payload.
pub fn encode(op: u32, json: &str) -> Vec<u8> {
    let mut f = Vec::with_capacity(8 + json.len());
    f.extend_from_slice(&op.to_le_bytes());
    f.extend_from_slice(&(json.len() as u32).to_le_bytes());
    f.extend_from_slice(json.as_bytes());
    f
}

/// The next frame from `r`: its opcode and JSON.
pub fn read_frame(r: &mut impl Read) -> io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    r.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[..4].try_into().unwrap());
    let len = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
    if len > 1 << 20 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
    Ok((op, v))
}

pub fn handshake(client_id: &str) -> String {
    json!({ "v": 1, "client_id": client_id }).to_string()
}

/// Discord takes 2 to 128 characters.
fn text(s: &str) -> String {
    let mut t: String = s.trim().chars().take(128).collect();
    while t.chars().count() < 2 {
        t.push(' ');
    }
    t
}

/// The activity shown for `a` (Discord's `activity` object), since
/// `start` (Unix seconds).
pub fn activity(a: &Activity, start: i64) -> Value {
    let (details, state) = match a {
        Activity::Select => ("Choosing a song".to_string(), None),
        Activity::Decide(c) => (c.label(), Some("Ready to play".to_string())),
        Activity::Play(c, k) => (
            c.label(),
            Some(
                match k {
                    PlayKind::Play => "Playing",
                    PlayKind::Autoplay => "Watching autoplay",
                    PlayKind::Replay => "Watching a replay",
                    PlayKind::Practice => "Practising",
                }
                .to_string(),
            ),
        ),
        Activity::Result(c, lamp) => (c.label(), Some(format!("Result: {lamp}"))),
    };
    let mut v = json!({
        "details": text(&details),
        "timestamps": { "start": start },
    });
    if let Some(s) = state {
        v["state"] = Value::String(text(&s));
    }
    v
}

/// The SET_ACTIVITY command for process `pid` (`None`: the status cleared).
pub fn set_activity(pid: u32, activity: Option<Value>, nonce: &str) -> String {
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce,
    })
    .to_string()
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

/// The places Discord's socket can be (the first that answers is used).
#[cfg(unix)]
fn candidates() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(Into::into)
        .collect();
    dirs.push("/tmp".into());
    let mut out = Vec::new();
    for d in dirs {
        // the Flatpak and Snap packages of Discord keep it in a folder of
        // their own
        for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
            for n in 0..10 {
                out.push(d.join(sub).join(format!("discord-ipc-{n}")));
            }
        }
    }
    out
}

#[cfg(unix)]
fn open() -> io::Result<Box<dyn Pipe>> {
    for p in candidates() {
        if let Ok(s) = std::os::unix::net::UnixStream::connect(&p) {
            s.set_read_timeout(Some(Duration::from_secs(5)))?;
            s.set_write_timeout(Some(Duration::from_secs(5)))?;
            return Ok(Box::new(s));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Discord is not running",
    ))
}

#[cfg(windows)]
fn open() -> io::Result<Box<dyn Pipe>> {
    for n in 0..10 {
        let p = format!(r"\\.\pipe\discord-ipc-{n}");
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(p) {
            return Ok(Box::new(f));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Discord is not running",
    ))
}

/// One connection to Discord.
struct Conn {
    pipe: Box<dyn Pipe>,
    nonce: u64,
}

impl Conn {
    fn open(client_id: &str) -> io::Result<Conn> {
        let mut c = Conn {
            pipe: open()?,
            nonce: 0,
        };
        c.pipe
            .write_all(&encode(OP_HANDSHAKE, &handshake(client_id)))?;
        match c.read()? {
            Some(v) if v["evt"] == "READY" => Ok(c),
            _ => Err(io::Error::new(io::ErrorKind::InvalidData, "no READY")),
        }
    }

    /// The next FRAME's JSON (PINGs answered on the way); CLOSE: an error
    /// with Discord's message.
    fn read(&mut self) -> io::Result<Option<Value>> {
        loop {
            let (op, v) = read_frame(&mut self.pipe)?;
            match op {
                OP_FRAME => return Ok(Some(v)),
                OP_PING => self.pipe.write_all(&encode(OP_PONG, &v.to_string()))?,
                OP_CLOSE => {
                    let msg = v["message"].as_str().unwrap_or("closed").to_string();
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, msg));
                }
                _ => {}
            }
        }
    }

    /// Send the activity and wait for its answer.
    fn set(&mut self, activity: Option<Value>) -> io::Result<()> {
        self.nonce += 1;
        let nonce = format!("vibebms-{}", self.nonce);
        let cmd = set_activity(std::process::id(), activity, &nonce);
        self.pipe.write_all(&encode(OP_FRAME, &cmd))?;
        loop {
            let Some(v) = self.read()? else { continue };
            if v["nonce"] == nonce.as_str() {
                if v["evt"] == "ERROR" {
                    log::warn!("discord: {}", v["data"]["message"]);
                }
                return Ok(());
            }
        }
    }
}

enum Msg {
    Activity(Activity),
}

/// The Rich Presence thread, while this lives.
pub struct Discord {
    tx: Sender<Msg>,
    status: SharedStatus,
    application_id: String,
}

impl Discord {
    pub fn start(application_id: String) -> Discord {
        let (tx, rx) = mpsc::channel();
        let status = SharedStatus::default();
        *status.lock().unwrap() = Status::Connecting;
        let st = status.clone();
        let id = application_id.clone();
        let spawned = std::thread::Builder::new()
            .name("discord".into())
            .spawn(move || run(id, rx, st));
        if let Err(e) = spawned {
            log::error!("discord: {e}");
        }
        Discord {
            tx,
            status,
            application_id,
        }
    }

    pub fn application_id(&self) -> &str {
        &self.application_id
    }

    pub fn set(&mut self, a: Activity) {
        let _ = self.tx.send(Msg::Activity(a));
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
}

fn run(client_id: String, rx: mpsc::Receiver<Msg>, status: SharedStatus) {
    let set_status = |s: Status| *status.lock().unwrap() = s;
    let mut conn: Option<Conn> = None;
    let mut current: Option<(Activity, i64)> = None;
    let mut dirty = false;
    let mut backoff = Duration::from_secs(2);
    let mut next_try = Instant::now();
    let mut last_sent: Option<Instant> = None;
    loop {
        let now = Instant::now();
        let due = match (&conn, dirty) {
            (None, _) => next_try,
            (Some(_), true) => last_sent.map_or(now, |t| t + MIN_GAP),
            (Some(_), false) => now + Duration::from_secs(3600),
        };
        match rx.recv_timeout(due.saturating_duration_since(now)) {
            Ok(Msg::Activity(a)) => {
                if current.as_ref().map(|c| &c.0) != Some(&a) {
                    current = Some((a, crate::platform::unix_ms() / 1000));
                    dirty = true;
                }
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // turned off: the status goes with it
                if let Some(c) = conn.as_mut() {
                    let _ = c.set(None);
                }
                return;
            }
        }
        let now = Instant::now();
        if conn.is_none() {
            if now < next_try {
                continue;
            }
            match Conn::open(&client_id) {
                Ok(c) => {
                    log::info!("discord: connected");
                    set_status(Status::Connected);
                    conn = Some(c);
                    backoff = Duration::from_secs(2);
                    dirty = current.is_some();
                    last_sent = None;
                }
                Err(e) => {
                    log::debug!("discord: {e}");
                    set_status(Status::Failed(e.to_string()));
                    next_try = now + backoff;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
            continue;
        }
        if dirty && last_sent.is_none_or(|t| now >= t + MIN_GAP) {
            let a = current.as_ref().map(|(a, start)| activity(a, *start));
            let r = conn.as_mut().unwrap().set(a);
            last_sent = Some(now);
            dirty = false;
            if let Err(e) = r {
                log::info!("discord: {e}");
                set_status(Status::Failed(e.to_string()));
                conn = None;
                dirty = true;
                next_try = now + backoff;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::Chart;

    /// Opcode and length little-endian, then the JSON.
    #[test]
    fn frame_encoding() {
        let f = encode(OP_HANDSHAKE, &handshake("123"));
        let body = r#"{"client_id":"123","v":1}"#;
        assert_eq!(&f[..4], &[0, 0, 0, 0]);
        assert_eq!(&f[4..8], &(body.len() as u32).to_le_bytes());
        assert_eq!(
            serde_json::from_slice::<Value>(&f[8..]).unwrap(),
            serde_json::from_str::<Value>(body).unwrap()
        );
        let f = encode(OP_FRAME, "{}");
        assert_eq!(f, [1, 0, 0, 0, 2, 0, 0, 0, b'{', b'}']);
        let (op, v) = read_frame(&mut &f[..]).unwrap();
        assert_eq!((op, v), (OP_FRAME, json!({})));
        // cut short
        assert!(read_frame(&mut &f[..9]).is_err());
    }

    #[test]
    fn activities() {
        let c = Chart {
            title: "Song".into(),
            difficulty: "ANOTHER".into(),
        };
        let v = activity(&Activity::Play(c.clone(), PlayKind::Play), 1700000000);
        assert_eq!(
            v,
            json!({"details": "Song [ANOTHER]", "state": "Playing",
                   "timestamps": {"start": 1700000000}})
        );
        let v = activity(&Activity::Result(c, "HARD CLEAR".into()), 5);
        assert_eq!(v["state"], "Result: HARD CLEAR");
        assert_eq!(activity(&Activity::Select, 5)["details"], "Choosing a song");
        // 2 to 128 characters
        let one = Chart {
            title: "X".into(),
            difficulty: String::new(),
        };
        assert_eq!(activity(&Activity::Decide(one), 0)["details"], "X ");
        let long = Chart {
            title: "あ".repeat(200),
            difficulty: String::new(),
        };
        let d = activity(&Activity::Decide(long), 0)["details"].clone();
        assert_eq!(d.as_str().unwrap().chars().count(), 128);
        let cmd: Value = serde_json::from_str(&set_activity(42, None, "n1")).unwrap();
        assert_eq!(
            cmd,
            json!({"cmd": "SET_ACTIVITY", "args": {"pid": 42, "activity": null}, "nonce": "n1"})
        );
    }

    /// The handshake and an activity against a fake Discord on a socket
    /// pair: READY, a PING on the way, the answer by nonce.
    #[cfg(unix)]
    #[test]
    fn talks_to_discord() {
        let (a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let (op, v) = read_frame(&mut b).unwrap();
            assert_eq!((op, v["client_id"].as_str()), (OP_HANDSHAKE, Some("99")));
            b.write_all(&encode(OP_FRAME, r#"{"cmd":"DISPATCH","evt":"READY"}"#))
                .unwrap();
            let (op, v) = read_frame(&mut b).unwrap();
            assert_eq!(op, OP_FRAME);
            assert_eq!(v["args"]["activity"]["details"], "Choosing a song");
            b.write_all(&encode(OP_PING, r#"{"x":1}"#)).unwrap();
            let (op, _) = read_frame(&mut b).unwrap();
            assert_eq!(op, OP_PONG);
            let answer = json!({"cmd": "SET_ACTIVITY", "nonce": v["nonce"], "evt": null});
            b.write_all(&encode(OP_FRAME, &answer.to_string())).unwrap();
        });
        let mut c = Conn {
            pipe: Box::new(a),
            nonce: 0,
        };
        c.pipe
            .write_all(&encode(OP_HANDSHAKE, &handshake("99")))
            .unwrap();
        assert_eq!(c.read().unwrap().unwrap()["evt"], "READY");
        c.set(Some(activity(&Activity::Select, 0))).unwrap();
        server.join().unwrap();
    }
}
