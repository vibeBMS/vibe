//! Discord Rich Presence over the Discord app's local IPC (no network: a
//! Unix socket `discord-ipc-N` in the user's runtime folder, or the named
//! pipe `\\.\pipe\discord-ipc-N` on Windows, N from 0 to 9).
//!
//! What is sent says what the player plays, so the socket has to be the
//! player's own Discord's: it is looked for only in the folder the system
//! keeps for the user alone (not in `/tmp`, where anyone may put one), and
//! the pipe is opened so that its server cannot act as the player.
//!
//! Each message is a frame: its opcode and its length (two little-endian
//! u32), then that much JSON. The client opens with a HANDSHAKE (`{"v": 1,
//! "client_id": …}`), which Discord answers with a READY dispatch; then
//! FRAMEs carry commands, here `SET_ACTIVITY` (Discord allows about five a
//! 20 seconds: changes are sent at most every 4 s, the last one winning).
//! PING is answered with PONG; CLOSE ends it (a wrong application id).

use super::worker::{Peer, Refusal, Turn, Worker};
use super::{Activity, PlayKind, Status};
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;
pub const OP_PING: u32 = 3;
pub const OP_PONG: u32 = 4;

/// The least time between two activities sent.
const MIN_GAP: Duration = Duration::from_secs(4);

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

/// The folder Discord keeps its socket in: the user's runtime folder
/// (macOS has none: the user's own temporary folder).
#[cfg(unix)]
fn runtime_dir() -> Option<std::path::PathBuf> {
    let var = if cfg!(target_os = "macos") {
        "TMPDIR"
    } else {
        "XDG_RUNTIME_DIR"
    };
    std::env::var_os(var)
        .filter(|d| !d.is_empty())
        .map(Into::into)
}

/// Discord's socket in `dir`, when `dir` is the user's alone: no one else
/// may enter it (so whoever can is its owner), and the socket is its
/// owner's.
#[cfg(unix)]
fn open_in(dir: &std::path::Path) -> Option<std::os::unix::net::UnixStream> {
    use std::os::unix::fs::MetadataExt;
    let owner = std::fs::metadata(dir)
        .ok()
        .filter(|m| m.mode() & 0o077 == 0)?;
    // the Flatpak and Snap packages of Discord keep it in a folder of
    // their own
    for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
        for n in 0..10 {
            let p = dir.join(sub).join(format!("discord-ipc-{n}"));
            if !std::fs::metadata(&p).is_ok_and(|m| m.uid() == owner.uid()) {
                continue;
            }
            if let Ok(s) = std::os::unix::net::UnixStream::connect(&p) {
                return Some(s);
            }
        }
    }
    None
}

#[cfg(unix)]
fn open() -> io::Result<Box<dyn Pipe>> {
    if let Some(s) = runtime_dir().and_then(|d| open_in(&d)) {
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        s.set_write_timeout(Some(Duration::from_secs(5)))?;
        return Ok(Box::new(s));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Discord is not running",
    ))
}

#[cfg(windows)]
fn open() -> io::Result<Box<dyn Pipe>> {
    use std::os::windows::fs::OpenOptionsExt;
    /// The pipe's server may learn who the client is, not act as them.
    const SECURITY_IDENTIFICATION: u32 = 0x0001_0000;
    for n in 0..10 {
        let p = format!(r"\\.\pipe\discord-ipc-{n}");
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(p);
        if let Ok(f) = opened {
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

/// The Rich Presence thread's side.
#[derive(Default)]
struct Presence {
    client_id: String,
    /// The activity and since when (Unix seconds).
    current: Option<(Activity, i64)>,
    /// `current` is not shown yet.
    dirty: bool,
    last_sent: Option<Instant>,
}

impl Peer for Presence {
    type Msg = Activity;
    type Conn = Conn;
    const NAME: &'static str = "discord";
    const BACKOFF: (Duration, Duration) = (Duration::from_secs(2), Duration::from_secs(60));

    fn open(&mut self) -> Result<Conn, Refusal> {
        let c = Conn::open(&self.client_id).map_err(|e| Refusal::Later(e.to_string()))?;
        log::info!("discord: connected");
        self.dirty = self.current.is_some();
        self.last_sent = None;
        Ok(c)
    }

    fn take(&mut self, a: Activity, _: &mut Option<Conn>) -> Turn {
        if self.current.as_ref().map(|c| &c.0) != Some(&a) {
            self.current = Some((a, crate::platform::unix_ms() / 1000));
            self.dirty = true;
        }
        Turn::Go
    }

    fn due(&self) -> Instant {
        let now = Instant::now();
        match self.dirty {
            true => self.last_sent.map_or(now, |t| t + MIN_GAP),
            false => now + Duration::from_secs(3600),
        }
    }

    fn tend(&mut self, c: &mut Conn) -> Result<(), String> {
        let now = Instant::now();
        if !self.dirty || self.last_sent.is_some_and(|t| now < t + MIN_GAP) {
            return Ok(());
        }
        let a = self.current.as_ref().map(|(a, start)| activity(a, *start));
        self.last_sent = Some(now);
        // (one that failed is sent again once connected)
        self.dirty = false;
        c.set(a).map_err(|e| e.to_string())
    }

    fn quit(&mut self, c: Option<Conn>) {
        // turned off: the status goes with it
        if let Some(mut c) = c {
            let _ = c.set(None);
        }
    }
}

/// The Rich Presence thread, while this lives.
pub struct Discord {
    worker: Worker<Activity>,
    application_id: String,
}

impl Discord {
    pub fn start(application_id: String) -> Discord {
        Discord {
            worker: Worker::start(Presence {
                client_id: application_id.clone(),
                ..Default::default()
            }),
            application_id,
        }
    }

    pub fn application_id(&self) -> &str {
        &self.application_id
    }

    pub fn set(&mut self, a: Activity) {
        self.worker.send(a);
    }

    pub fn status(&self) -> Status {
        self.worker.status()
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

    /// Discord's socket is taken from a folder that is the user's alone,
    /// and from no other: one anyone may enter could hold another user's.
    #[cfg(unix)]
    #[test]
    fn the_socket_is_the_users_own() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("vibebms-discord-{}", std::process::id()));
        let sub = dir.join("app/com.discordapp.Discord");
        std::fs::create_dir_all(&sub).unwrap();
        let mode = |m| std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(m)).unwrap();
        let _l = std::os::unix::net::UnixListener::bind(sub.join("discord-ipc-3")).unwrap();
        mode(0o700);
        assert!(open_in(&dir).is_some());
        for open_to_others in [0o755, 0o750, 0o1777] {
            mode(open_to_others);
            assert!(open_in(&dir).is_none(), "{open_to_others:o}");
        }
        mode(0o700);
        assert!(open_in(&dir.join("none")).is_none());
        std::fs::remove_dir_all(&dir).ok();
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
