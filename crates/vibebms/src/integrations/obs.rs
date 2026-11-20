//! OBS through obs-websocket 5 (its protocol document, "Connecting to
//! obs-websocket"): the server opens with Hello (op 0), with an
//! authentication challenge and salt when it has a password; the client
//! answers Identify (op 1) and the server Identified (op 2); then Requests
//! (op 6) and their RequestResponses (op 7). Used here:
//! `SetCurrentProgramScene` on each screen whose scene is set, and
//! `StartRecord` / `StopRecord` around a play. No events are asked for.
//!
//! The connection is plain `ws://`, and the authentication string answers
//! a salt and a challenge of the server's choosing: whoever answers at the
//! address gets it, and can try passwords against it. So the password is
//! kept for the address it was entered for (`ir::creds`), a server that
//! refuses it is not asked again until the settings change, and the
//! settings say what an address on another computer means.

use super::worker::{Peer, Refusal, Turn, Worker};
use super::ws::{Message, Ws};
use super::{Activity, Status};
use crate::config::ObsConfig;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// The RPC version spoken.
const RPC_VERSION: u64 = 1;
/// obs-websocket's close code for a wrong password.
const AUTHENTICATION_FAILED: u16 = 4009;

/// The `authentication` string for `password` and a Hello's `salt` and
/// `challenge`: base64(SHA-256(base64(SHA-256(password + salt)) +
/// challenge)).
pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha2::{Digest, Sha256};
    let secret = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}")))
}

/// The Identify answering `hello` (its `d`): with the authentication
/// string when the server asks for one, and no events.
pub fn identify(hello: &Value, password: &str) -> String {
    let mut d = json!({ "rpcVersion": RPC_VERSION, "eventSubscriptions": 0 });
    let auth = &hello["authentication"];
    if let (Some(c), Some(s)) = (auth["challenge"].as_str(), auth["salt"].as_str()) {
        d["authentication"] = Value::String(authentication(password, s, c));
    }
    json!({ "op": 1, "d": d }).to_string()
}

/// A Request message.
pub fn request(kind: &str, id: &str, data: Option<Value>) -> String {
    let mut d = json!({ "requestType": kind, "requestId": id });
    if let Some(data) = data {
        d["requestData"] = data;
    }
    json!({ "op": 6, "d": d }).to_string()
}

/// A request to make: its type and data.
#[derive(Debug, PartialEq)]
pub struct Req(pub &'static str, pub Option<Value>);

/// What moving from `from` to `to` asks of OBS under `cfg`: the scene of
/// the new screen (when it is set and the screen changed), and with
/// `record_play` a recording for each play: started as the play starts
/// (the last one stopped first), stopped when a screen other than play or
/// result comes. `recording` is whether one of ours runs.
pub fn requests(
    cfg: &ObsConfig,
    from: Option<&Activity>,
    to: &Activity,
    recording: &mut bool,
) -> Vec<Req> {
    let mut out = Vec::new();
    let scene = cfg.scenes.get(to.screen()).trim();
    if !scene.is_empty() && from.map(Activity::screen) != Some(to.screen()) {
        out.push(Req(
            "SetCurrentProgramScene",
            Some(json!({ "sceneName": scene })),
        ));
    }
    let new_play = matches!(to, Activity::Play(..)) && from != Some(to);
    if new_play && cfg.record_play {
        if *recording {
            out.push(Req("StopRecord", None));
        }
        out.push(Req("StartRecord", None));
        *recording = true;
    } else if *recording && matches!(to, Activity::Select | Activity::Decide(_)) {
        out.push(Req("StopRecord", None));
        *recording = false;
    }
    out
}

enum Msg {
    Activity(Activity),
    Config(ObsConfig, String),
    Quit,
}

/// The OBS thread, while this lives.
pub struct Obs {
    worker: Worker<Msg>,
    cfg: ObsConfig,
    password: String,
}

impl Obs {
    /// With the settings and the password kept for their address (empty:
    /// none).
    pub fn start(cfg: ObsConfig, password: String) -> Obs {
        let link = Link {
            cfg: cfg.clone(),
            password: password.clone(),
            current: None,
            recording: false,
        };
        Obs {
            worker: Worker::start(link),
            cfg,
            password,
        }
    }

    /// New settings (a new address or password: it connects again).
    pub fn configure(&mut self, cfg: &ObsConfig, password: &str) {
        if *cfg != self.cfg || password != self.password {
            self.cfg = cfg.clone();
            self.password = password.to_string();
            self.worker
                .send(Msg::Config(cfg.clone(), password.to_string()));
        }
    }

    pub fn set(&mut self, a: Activity) {
        self.worker.send(Msg::Activity(a));
    }

    pub fn status(&self) -> Status {
        self.worker.status()
    }

    /// Stop (a recording of ours too), waiting for that a moment.
    pub fn stop(&mut self) {
        self.worker.send(Msg::Quit);
        self.worker.wait_end(Duration::from_millis(500));
    }
}

/// A connection, identified.
struct Conn {
    ws: Ws,
    next_id: u64,
}

impl Conn {
    fn open(cfg: &ObsConfig, password: &str) -> Result<Conn, Refusal> {
        let timeout = Duration::from_secs(3);
        let later = |e: std::io::Error| Refusal::Later(e.to_string());
        let mut ws =
            Ws::connect(cfg.host.trim(), cfg.port, "obswebsocket.json", timeout).map_err(|e| {
                match e.kind() {
                    std::io::ErrorKind::ConnectionRefused => Refusal::Later(
                        "OBS is not running, or its WebSocket server is off".to_string(),
                    ),
                    _ => later(e),
                }
            })?;
        let hello = wait_for(&mut ws, 0, timeout)?;
        // (nothing made from an empty password is sent either)
        if password.is_empty() && hello["authentication"].is_object() {
            ws.close();
            return Err(Refusal::Never("OBS asks for a password".into()));
        }
        ws.send_text(&identify(&hello, password)).map_err(later)?;
        wait_for(&mut ws, 2, timeout)?;
        Ok(Conn { ws, next_id: 0 })
    }

    fn send(&mut self, r: &Req) -> std::io::Result<()> {
        self.next_id += 1;
        let id = format!("vibebms-{}", self.next_id);
        log::info!("obs: {}", r.0);
        self.ws.send_text(&request(r.0, &id, r.1.clone()))
    }

    /// What came: failed requests are logged. An error: the connection is
    /// gone.
    fn poll(&mut self, wait: Duration) -> Result<(), String> {
        for m in self.ws.poll(wait).map_err(|e| e.to_string())? {
            match m {
                Message::Text(t) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    let st = &v["d"]["requestStatus"];
                    if v["op"] == 7 && st["result"] == false {
                        log::warn!(
                            "obs: {} failed ({}): {}",
                            v["d"]["requestType"],
                            st["code"],
                            st["comment"]
                        );
                    }
                }
                Message::Close(code, reason) => return Err(close_reason(code, &reason)),
            }
        }
        Ok(())
    }
}

/// The server closed while a connection was being made: a wrong password
/// is not tried again.
fn closed(code: u16, reason: &str) -> Refusal {
    let why = close_reason(code, reason);
    match code {
        AUTHENTICATION_FAILED => Refusal::Never(why),
        _ => Refusal::Later(why),
    }
}

fn close_reason(code: u16, reason: &str) -> String {
    match code {
        AUTHENTICATION_FAILED => "wrong password".into(),
        _ if reason.is_empty() => format!("closed ({code})"),
        _ => format!("closed ({code}: {reason})"),
    }
}

/// The first message of opcode `op` (its `d`) within `timeout`.
fn wait_for(ws: &mut Ws, op: u64, timeout: Duration) -> Result<Value, Refusal> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for m in ws
            .poll(Duration::from_millis(100))
            .map_err(|e| Refusal::Later(e.to_string()))?
        {
            match m {
                Message::Text(t) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    if v["op"] == op {
                        return Ok(v["d"].clone());
                    }
                }
                Message::Close(code, reason) => return Err(closed(code, &reason)),
            }
        }
    }
    Err(Refusal::Later("no answer".into()))
}

/// The OBS thread's side: the settings, the screen the player is on and
/// whether a recording of ours runs.
struct Link {
    cfg: ObsConfig,
    password: String,
    current: Option<Activity>,
    recording: bool,
}

impl Peer for Link {
    type Msg = Msg;
    type Conn = Conn;
    const NAME: &'static str = "obs";
    const BACKOFF: (Duration, Duration) = (Duration::from_secs(1), Duration::from_secs(30));

    fn open(&mut self) -> Result<Conn, Refusal> {
        let mut c = Conn::open(&self.cfg, &self.password)?;
        log::info!("obs: connected to {}:{}", self.cfg.host, self.cfg.port);
        // the scene of the screen the player is on
        if let Some(a) = &self.current {
            let mut rec = self.recording;
            for r in requests(&self.cfg, None, a, &mut rec) {
                if r.0 == "SetCurrentProgramScene" {
                    let _ = c.send(&r);
                }
            }
        }
        Ok(c)
    }

    fn take(&mut self, m: Msg, conn: &mut Option<Conn>) -> Turn {
        match m {
            Msg::Activity(a) => {
                // (not connected: nothing was started or stopped)
                let mut rec = self.recording;
                let reqs = requests(&self.cfg, self.current.as_ref(), &a, &mut rec);
                if let Some(c) = conn.as_mut() {
                    self.recording = rec;
                    if let Err(e) = reqs.iter().try_for_each(|r| c.send(r)) {
                        log::info!("obs: {e}");
                        *conn = None;
                    }
                }
                self.current = Some(a);
            }
            Msg::Config(c, password) => {
                let cfg = &self.cfg;
                let reconnect =
                    (&c.host, c.port, &password) != (&cfg.host, cfg.port, &self.password);
                self.cfg = c;
                self.password = password;
                if !self.cfg.record_play && self.recording {
                    if let Some(c) = conn.as_mut() {
                        let _ = c.send(&Req("StopRecord", None));
                    }
                    self.recording = false;
                }
                if reconnect {
                    if let Some(mut c) = conn.take() {
                        c.ws.close();
                    }
                    return Turn::Again;
                }
            }
            Msg::Quit => return Turn::Quit,
        }
        Turn::Go
    }

    /// While connected the socket is read between messages (pings,
    /// answers).
    fn due(&self) -> Instant {
        Instant::now() + Duration::from_millis(50)
    }

    fn tend(&mut self, c: &mut Conn) -> Result<(), String> {
        c.poll(Duration::from_millis(1))
    }

    fn quit(&mut self, c: Option<Conn>) {
        if let Some(mut c) = c {
            if self.recording {
                let _ = c.send(&Req("StopRecord", None));
            }
            c.ws.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ObsScenes;
    use crate::integrations::{Chart, PlayKind};

    /// The protocol document's example: password `supersecretpassword`
    /// and its Hello's challenge and salt (the steps of "Creating an
    /// authentication string" done by hand give this string; the
    /// document's Identify example is of another challenge).
    #[test]
    fn authentication_of_the_protocol_example() {
        assert_eq!(
            authentication(
                "supersecretpassword",
                "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="
            ),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
    }

    #[test]
    fn messages() {
        let hello = json!({
            "obsWebSocketVersion": "5.1.0",
            "rpcVersion": 1,
            "authentication": {
                "challenge": "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
                "salt": "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI="
            }
        });
        let v: Value = serde_json::from_str(&identify(&hello, "supersecretpassword")).unwrap();
        assert_eq!(
            v,
            json!({"op": 1, "d": {"rpcVersion": 1, "eventSubscriptions": 0,
                   "authentication": "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="}})
        );
        // no password asked for: none sent
        let v: Value = serde_json::from_str(&identify(&json!({"rpcVersion": 1}), "x")).unwrap();
        assert_eq!(
            v,
            json!({"op": 1, "d": {"rpcVersion": 1, "eventSubscriptions": 0}})
        );
        // the document's Request example
        let r = request(
            "SetCurrentProgramScene",
            "f819dcf0-89cc-11eb-8f0e-382c4ac93b9c",
            Some(json!({"sceneName": "Scene 12"})),
        );
        assert_eq!(
            serde_json::from_str::<Value>(&r).unwrap(),
            json!({"op": 6, "d": {"requestType": "SetCurrentProgramScene",
                   "requestId": "f819dcf0-89cc-11eb-8f0e-382c4ac93b9c",
                   "requestData": {"sceneName": "Scene 12"}}})
        );
        let r: Value = serde_json::from_str(&request("StartRecord", "1", None)).unwrap();
        assert_eq!(
            r,
            json!({"op": 6, "d": {"requestType": "StartRecord", "requestId": "1"}})
        );
    }

    /// A fake obs-websocket's side of a connection on `l`: the WebSocket
    /// handshake answered, then its Hello asking for a password.
    fn hello(l: &std::net::TcpListener) -> std::net::TcpStream {
        use crate::integrations::ws::accept_key;
        use std::io::{Read, Write};
        let (mut s, _) = l.accept().unwrap();
        let mut req = Vec::new();
        let mut b = [0u8; 1];
        while !req.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            req.push(b[0]);
        }
        let req = String::from_utf8(req).unwrap();
        let key = req
            .lines()
            .find_map(|l| l.strip_prefix("Sec-WebSocket-Key: "))
            .unwrap();
        let hello = json!({"op": 0, "d": {"obsWebSocketVersion": "5.1.0", "rpcVersion": 1,
            "authentication": {
                "challenge": "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
                "salt": "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI="}}});
        let mut out = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Protocol: obswebsocket.json\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(key)
        )
        .into_bytes();
        out.extend(text(hello));
        s.write_all(&out).unwrap();
        s
    }

    /// A text frame from the server.
    fn text(v: Value) -> Vec<u8> {
        let p = v.to_string().into_bytes();
        let mut f = vec![0x81, 126];
        f.extend_from_slice(&(p.len() as u16).to_be_bytes());
        f.extend(p);
        f
    }

    /// The thread against a fake obs-websocket on a local port: the
    /// password challenge, then the play's scene and its recording.
    #[test]
    fn talks_to_obs() {
        use crate::integrations::ws::{parse_frame, OP_TEXT};
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut s = hello(&l);
            let mut got = Vec::new();
            let mut msgs: Vec<Value> = Vec::new();
            let mut next = |s: &mut std::net::TcpStream, msgs: &mut Vec<Value>| loop {
                if let Some((f, used)) = parse_frame(&got).unwrap() {
                    got.drain(..used);
                    assert_eq!(f.op, OP_TEXT);
                    msgs.push(serde_json::from_slice(&f.payload).unwrap());
                    return;
                }
                let mut c = [0u8; 1024];
                let n = s.read(&mut c).unwrap();
                assert!(n > 0);
                got.extend_from_slice(&c[..n]);
            };
            next(&mut s, &mut msgs);
            assert_eq!(msgs[0]["op"], 1);
            assert_eq!(
                msgs[0]["d"]["authentication"],
                "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
            );
            s.write_all(&text(json!({"op": 2, "d": {"negotiatedRpcVersion": 1}})))
                .unwrap();
            next(&mut s, &mut msgs);
            next(&mut s, &mut msgs);
            let kinds: Vec<_> = msgs[1..]
                .iter()
                .map(|m| m["d"]["requestType"].clone())
                .collect();
            assert_eq!(
                kinds,
                [json!("SetCurrentProgramScene"), json!("StartRecord")]
            );
            assert_eq!(msgs[1]["d"]["requestData"]["sceneName"], "Game");
        });
        let cfg = ObsConfig {
            enabled: true,
            port,
            scenes: ObsScenes {
                play: "Game".into(),
                ..Default::default()
            },
            record_play: true,
            ..Default::default()
        };
        let mut obs = Obs::start(cfg, "supersecretpassword".into());
        let deadline = Instant::now() + Duration::from_secs(10);
        while obs.status() != Status::Connected {
            assert!(Instant::now() < deadline, "{:?}", obs.status());
            std::thread::sleep(Duration::from_millis(10));
        }
        let chart = Chart {
            title: "a".into(),
            difficulty: String::new(),
        };
        obs.set(Activity::Play(chart, PlayKind::Play));
        server.join().unwrap();
        obs.stop();
    }

    /// A server that asks for a password gets nothing made from one when
    /// none is kept for its address, and one that refuses the password is
    /// not asked again: both end the tries until the settings change.
    #[test]
    fn a_refused_password_is_not_tried_again() {
        use crate::integrations::ws::parse_frame;
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let cfg = ObsConfig {
            port: l.local_addr().unwrap().port(),
            ..Default::default()
        };
        let server = std::thread::spawn(move || {
            // (the client's close frame at most: an Identify is longer)
            let mut s = hello(&l);
            let mut sent = Vec::new();
            s.read_to_end(&mut sent).unwrap();
            assert!(sent.len() < 16, "{}", sent.len());
            // a wrong password: closed with 4009
            let mut s = hello(&l);
            let mut identify = Vec::new();
            while parse_frame(&identify).unwrap().is_none() {
                let mut c = [0u8; 1024];
                let n = s.read(&mut c).unwrap();
                identify.extend_from_slice(&c[..n]);
            }
            s.write_all(&[0x88, 2, 0x0f, 0xa9]).unwrap();
        });
        assert_eq!(
            Conn::open(&cfg, "").err(),
            Some(Refusal::Never("OBS asks for a password".into()))
        );
        assert_eq!(
            Conn::open(&cfg, "not it").err(),
            Some(Refusal::Never("wrong password".into()))
        );
        server.join().unwrap();
        // another close is tried again
        assert_eq!(closed(1001, ""), Refusal::Later("closed (1001)".into()));
    }

    #[test]
    fn scenes_and_recording() {
        let chart = |t: &str| Chart {
            title: t.into(),
            difficulty: String::new(),
        };
        let play = |t| Activity::Play(chart(t), PlayKind::Play);
        let result = |t| Activity::Result(chart(t), "CLEAR".into());
        let mut cfg = ObsConfig {
            scenes: ObsScenes {
                select: "Menu".into(),
                decide: String::new(),
                play: "Game".into(),
                result: "Game".into(),
            },
            ..Default::default()
        };
        let scene = |n: &str| Req("SetCurrentProgramScene", Some(json!({ "sceneName": n })));
        let mut rec = false;
        assert_eq!(
            requests(&cfg, None, &Activity::Select, &mut rec),
            [scene("Menu")]
        );
        // decide has none
        assert!(requests(
            &cfg,
            Some(&Activity::Select),
            &Activity::Decide(chart("a")),
            &mut rec
        )
        .is_empty());
        assert_eq!(
            requests(
                &cfg,
                Some(&Activity::Decide(chart("a"))),
                &play("a"),
                &mut rec
            ),
            [scene("Game")]
        );
        assert!(!rec);
        // recording: a play each
        cfg.record_play = true;
        let r = requests(&cfg, Some(&Activity::Select), &play("a"), &mut rec);
        assert_eq!(r, [scene("Game"), Req("StartRecord", None)]);
        assert!(rec);
        // the result is in the same recording
        assert_eq!(
            requests(&cfg, Some(&play("a")), &result("a"), &mut rec),
            [scene("Game")]
        );
        // retried: the last one stopped, a new one
        assert_eq!(
            requests(&cfg, Some(&result("a")), &play("a"), &mut rec),
            [
                scene("Game"),
                Req("StopRecord", None),
                Req("StartRecord", None)
            ]
        );
        // the next course stage: another
        assert_eq!(
            requests(&cfg, Some(&play("a")), &play("b"), &mut rec),
            [Req("StopRecord", None), Req("StartRecord", None)]
        );
        assert_eq!(
            requests(&cfg, Some(&result("b")), &Activity::Select, &mut rec),
            [scene("Menu"), Req("StopRecord", None)]
        );
        assert!(!rec);
        // the same activity again asks nothing
        assert!(requests(&cfg, Some(&Activity::Select), &Activity::Select, &mut rec).is_empty());
    }
}
