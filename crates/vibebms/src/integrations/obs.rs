//! OBS through obs-websocket 5 (its protocol document, "Connecting to
//! obs-websocket"): the server opens with Hello (op 0), with an
//! authentication challenge and salt when it has a password; the client
//! answers Identify (op 1) and the server Identified (op 2); then Requests
//! (op 6) and their RequestResponses (op 7). Used here:
//! `SetCurrentProgramScene` on each screen whose scene is set, and
//! `StartRecord` / `StopRecord` around a play. No events are asked for.

use super::ws::{Message, Ws};
use super::{Activity, SharedStatus, Status};
use crate::config::ObsConfig;
use serde_json::{json, Value};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// The RPC version spoken.
const RPC_VERSION: u64 = 1;
/// obs-websocket's close code for a wrong password.
const AUTHENTICATION_FAILED: u16 = 4009;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The `authentication` string for `password` and a Hello's `salt` and
/// `challenge`: base64(SHA-256(base64(SHA-256(password + salt)) +
/// challenge)).
pub fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    use super::codec::{base64, sha256};
    let secret = base64(&sha256(format!("{password}{salt}").as_bytes()));
    base64(&sha256(format!("{secret}{challenge}").as_bytes()))
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
    Config(ObsConfig),
    Quit,
}

/// The OBS thread, while this lives.
pub struct Obs {
    tx: Sender<Msg>,
    status: SharedStatus,
    cfg: ObsConfig,
    done: mpsc::Receiver<()>,
}

impl Obs {
    pub fn start(cfg: ObsConfig) -> Obs {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let status = SharedStatus::default();
        *status.lock().unwrap() = Status::Connecting;
        let st = status.clone();
        let c = cfg.clone();
        let spawned = std::thread::Builder::new()
            .name("obs".into())
            .spawn(move || {
                run(c, rx, st);
                let _ = done_tx.send(());
            });
        if let Err(e) = spawned {
            log::error!("obs: {e}");
        }
        Obs {
            tx,
            status,
            cfg,
            done,
        }
    }

    /// New settings (a new address or password: it connects again).
    pub fn configure(&mut self, cfg: &ObsConfig) {
        if *cfg != self.cfg {
            self.cfg = cfg.clone();
            let _ = self.tx.send(Msg::Config(cfg.clone()));
        }
    }

    pub fn set(&mut self, a: Activity) {
        let _ = self.tx.send(Msg::Activity(a));
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Stop (a recording of ours too), waiting for that a moment.
    pub fn stop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        let _ = self.done.recv_timeout(Duration::from_millis(500));
    }
}

/// A connection, identified.
struct Conn {
    ws: Ws,
    next_id: u64,
}

impl Conn {
    fn open(cfg: &ObsConfig) -> Result<Conn, String> {
        let timeout = Duration::from_secs(3);
        let mut ws = Ws::connect(cfg.host.trim(), cfg.port, "obswebsocket.json", timeout)
            .map_err(|e| e.to_string())?;
        let hello = wait_for(&mut ws, 0, timeout)?;
        ws.send_text(&identify(&hello, &cfg.password))
            .map_err(|e| e.to_string())?;
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

fn close_reason(code: u16, reason: &str) -> String {
    match code {
        AUTHENTICATION_FAILED => "wrong password".into(),
        _ if reason.is_empty() => format!("closed ({code})"),
        _ => format!("closed ({code}: {reason})"),
    }
}

/// The first message of opcode `op` (its `d`) within `timeout`.
fn wait_for(ws: &mut Ws, op: u64, timeout: Duration) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        for m in ws
            .poll(Duration::from_millis(100))
            .map_err(|e| e.to_string())?
        {
            match m {
                Message::Text(t) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    if v["op"] == op {
                        return Ok(v["d"].clone());
                    }
                }
                Message::Close(code, reason) => return Err(close_reason(code, &reason)),
            }
        }
    }
    Err("no answer".into())
}

fn run(mut cfg: ObsConfig, rx: mpsc::Receiver<Msg>, status: SharedStatus) {
    let set_status = |s: Status| *status.lock().unwrap() = s;
    let mut conn: Option<Conn> = None;
    let mut current: Option<Activity> = None;
    let mut recording = false;
    let mut backoff = Duration::from_secs(1);
    let mut next_try = Instant::now();
    loop {
        // while connected the socket is read between messages (pings,
        // answers); else the next try is waited for
        let wait = match &conn {
            Some(_) => Duration::from_millis(50),
            None => next_try.saturating_duration_since(Instant::now()),
        };
        let msg = match rx.recv_timeout(wait) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Msg::Quit),
        };
        match msg {
            Some(Msg::Activity(a)) => {
                // (not connected: nothing was started or stopped)
                let mut rec = recording;
                let reqs = requests(&cfg, current.as_ref(), &a, &mut rec);
                if let Some(c) = conn.as_mut() {
                    recording = rec;
                    for r in &reqs {
                        if let Err(e) = c.send(r) {
                            log::info!("obs: {e}");
                            conn = None;
                            next_try = Instant::now();
                            break;
                        }
                    }
                }
                current = Some(a);
            }
            Some(Msg::Config(c)) => {
                let reconnect =
                    (&c.host, c.port, &c.password) != (&cfg.host, cfg.port, &cfg.password);
                cfg = c;
                if !cfg.record_play && recording {
                    if let Some(c) = conn.as_mut() {
                        let _ = c.send(&Req("StopRecord", None));
                    }
                    recording = false;
                }
                if reconnect {
                    if let Some(mut c) = conn.take() {
                        c.ws.close();
                    }
                    backoff = Duration::from_secs(1);
                    next_try = Instant::now();
                }
            }
            Some(Msg::Quit) => {
                if let Some(mut c) = conn.take() {
                    if recording {
                        let _ = c.send(&Req("StopRecord", None));
                    }
                    c.ws.close();
                }
                return;
            }
            None => {}
        }
        match conn.as_mut() {
            Some(c) => {
                if let Err(e) = c.poll(Duration::from_millis(1)) {
                    log::info!("obs: {e}");
                    set_status(Status::Failed(e));
                    conn = None;
                    next_try = Instant::now() + backoff;
                }
            }
            None if Instant::now() >= next_try => match Conn::open(&cfg) {
                Ok(mut c) => {
                    log::info!("obs: connected to {}:{}", cfg.host, cfg.port);
                    set_status(Status::Connected);
                    backoff = Duration::from_secs(1);
                    // the scene of the screen the player is on
                    if let Some(a) = &current {
                        let mut rec = recording;
                        for r in requests(&cfg, None, a, &mut rec) {
                            if r.0 == "SetCurrentProgramScene" {
                                let _ = c.send(&r);
                            }
                        }
                    }
                    conn = Some(c);
                }
                Err(e) => {
                    log::debug!("obs: {e}");
                    set_status(Status::Failed(e));
                    next_try = Instant::now() + backoff;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            },
            None => {}
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

    /// The thread against a fake obs-websocket on a local port: the
    /// password challenge, then the play's scene and its recording.
    #[test]
    fn talks_to_obs() {
        use crate::integrations::ws::{accept_key, parse_frame, OP_TEXT};
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
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
            let text = |v: Value| {
                let p = v.to_string().into_bytes();
                let mut f = vec![0x81, 126];
                f.extend_from_slice(&(p.len() as u16).to_be_bytes());
                f.extend(p);
                f
            };
            let mut out = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                 Connection: Upgrade\r\nSec-WebSocket-Protocol: obswebsocket.json\r\n\
                 Sec-WebSocket-Accept: {}\r\n\r\n",
                accept_key(key)
            )
            .into_bytes();
            out.extend(text(hello));
            s.write_all(&out).unwrap();
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
            password: "supersecretpassword".into(),
            scenes: ObsScenes {
                play: "Game".into(),
                ..Default::default()
            },
            record_play: true,
            ..Default::default()
        };
        let mut obs = Obs::start(cfg);
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
