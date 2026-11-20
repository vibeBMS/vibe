//! A small WebSocket client (RFC 6455) for obs-websocket: the opening
//! handshake, masked text frames out, text / ping / close frames in. No
//! TLS and no extensions (obs-websocket offers neither).

use super::codec::sha1;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// RFC 6455 1.3: the accept key is derived from the client's key with this.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// The `Sec-WebSocket-Accept` a server answers `key` with.
pub fn accept_key(key: &str) -> String {
    STANDARD.encode(sha1(format!("{key}{GUID}").as_bytes()))
}

/// Bytes not to be guessed in advance (a key, a mask: RFC 6455 asks no
/// more of them).
fn random<const N: usize>() -> io::Result<[u8; N]> {
    let mut b = [0; N];
    getrandom::fill(&mut b).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(b)
}

/// What came from the server.
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    /// The server closes the connection: its status code (0 for none) and
    /// reason.
    Close(u16, String),
}

/// A frame read: FIN, opcode, payload.
#[derive(Debug, PartialEq)]
pub struct Frame {
    pub fin: bool,
    pub op: u8,
    pub payload: Vec<u8>,
}

pub const OP_CONT: u8 = 0;
pub const OP_TEXT: u8 = 1;
pub const OP_BINARY: u8 = 2;
pub const OP_CLOSE: u8 = 8;
pub const OP_PING: u8 = 9;
pub const OP_PONG: u8 = 10;

/// A frame as a client sends it: final, masked with `mask`.
pub fn encode_frame(op: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 14);
    f.push(0x80 | op);
    match payload.len() {
        n @ 0..=125 => f.push(0x80 | n as u8),
        n @ 126..=0xffff => {
            f.push(0x80 | 126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// The largest frame, and the largest message of several frames, taken
/// (bytes; obs-websocket's are a few kilobytes).
const MESSAGE_MAX: usize = 16 << 20;

/// The first whole frame in `buf` and how many bytes it took; `None` while
/// it has not all come. (A masked frame is unmasked, though servers send
/// none.) Too large a frame is an error.
pub fn parse_frame(buf: &[u8]) -> io::Result<Option<(Frame, usize)>> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let fin = buf[0] & 0x80 != 0;
    let op = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let (len, mut at) = match buf[1] & 0x7f {
        126 if buf.len() >= 4 => (u16::from_be_bytes([buf[2], buf[3]]) as u64, 4),
        127 if buf.len() >= 10 => (u64::from_be_bytes(buf[2..10].try_into().unwrap()), 10),
        126 | 127 => return Ok(None),
        n => (n as u64, 2),
    };
    if len > MESSAGE_MAX as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mask = if masked {
        if buf.len() < at + 4 {
            return Ok(None);
        }
        at += 4;
        Some([buf[at - 4], buf[at - 3], buf[at - 2], buf[at - 1]])
    } else {
        None
    };
    let end = at + len as usize;
    if buf.len() < end {
        return Ok(None);
    }
    let mut payload = buf[at..end].to_vec();
    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }
    Ok(Some((Frame { fin, op, payload }, end)))
}

/// The opening request for `host:port` (path `/`), with `key` and the
/// subprotocol asked for.
pub fn handshake_request(host: &str, port: u16, key: &str, protocol: &str) -> String {
    format!(
        "GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Protocol: {protocol}\r\n\r\n"
    )
}

/// Whether the server's answer `head` (its status line and headers)
/// accepts the connection opened with `key`.
pub fn check_response(head: &str, key: &str) -> Result<(), String> {
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    if status.split_whitespace().nth(1) != Some("101") {
        return Err(format!("not a WebSocket server ({status})"));
    }
    let want = accept_key(key);
    let ok = lines
        .filter_map(|l| l.split_once(':'))
        .any(|(k, v)| k.trim().eq_ignore_ascii_case("sec-websocket-accept") && v.trim() == want);
    if ok {
        Ok(())
    } else {
        Err("the server's accept key is wrong".into())
    }
}

pub struct Ws {
    stream: TcpStream,
    buf: Vec<u8>,
    /// A fragmented text message's parts so far.
    partial: Option<Vec<u8>>,
}

impl Ws {
    /// Connect to `ws://host:port/` with `protocol`, within `timeout`.
    pub fn connect(host: &str, port: u16, protocol: &str, timeout: Duration) -> io::Result<Ws> {
        let addr = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address"))?;
        let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let key = STANDARD.encode(random::<16>()?);
        stream.write_all(handshake_request(host, port, &key, protocol).as_bytes())?;
        let mut buf = Vec::new();
        let deadline = Instant::now() + timeout;
        let head_end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            if buf.len() > 16 << 10 || Instant::now() > deadline {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "no handshake"));
            }
            let mut chunk = [0u8; 1024];
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        check_response(&head, &key).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // (the server's first message may have come with its answer)
        buf.drain(..head_end);
        Ok(Ws {
            stream,
            buf,
            partial: None,
        })
    }

    fn send(&mut self, op: u8, payload: &[u8]) -> io::Result<()> {
        let f = encode_frame(op, payload, random()?);
        self.stream.write_all(&f)
    }

    pub fn send_text(&mut self, s: &str) -> io::Result<()> {
        self.send(OP_TEXT, s.as_bytes())
    }

    /// Close the connection (normal closure), not waiting for the answer.
    pub fn close(&mut self) {
        let _ = self.send(OP_CLOSE, &1000u16.to_be_bytes());
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    /// The messages that came within `wait` (none if nothing did); pings
    /// are answered. An error: the connection is gone, or what came is
    /// not taken (a message larger than [`MESSAGE_MAX`]).
    pub fn poll(&mut self, wait: Duration) -> io::Result<Vec<Message>> {
        self.stream
            .set_read_timeout(Some(wait.max(Duration::from_millis(1))))?;
        let mut chunk = [0u8; 4096];
        match self.stream.read(&mut chunk) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e),
        }
        let mut out = Vec::new();
        while let Some((f, used)) = parse_frame(&self.buf)? {
            self.buf.drain(..used);
            match f.op {
                OP_PING => self.send(OP_PONG, &f.payload)?,
                OP_PONG => {}
                OP_CLOSE => {
                    let code = match f.payload.get(..2) {
                        Some(c) => u16::from_be_bytes([c[0], c[1]]),
                        None => 0,
                    };
                    let reason = String::from_utf8_lossy(f.payload.get(2..).unwrap_or(&[]));
                    out.push(Message::Close(code, reason.into_owned()));
                    let _ = self.send(OP_CLOSE, &f.payload[..f.payload.len().min(2)]);
                    return Ok(out);
                }
                OP_TEXT | OP_BINARY | OP_CONT => {
                    let mut data = match f.op {
                        OP_CONT => self.partial.take().unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    if data.len() + f.payload.len() > MESSAGE_MAX {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "message too large",
                        ));
                    }
                    data.extend_from_slice(&f.payload);
                    if !f.fin {
                        self.partial = Some(data);
                    } else if f.op != OP_BINARY {
                        out.push(Message::Text(String::from_utf8_lossy(&data).into_owned()));
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6455 1.3's example key and accept value.
    #[test]
    fn accept_key_of_the_rfc() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        let head = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                    Connection: Upgrade\r\nsec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n";
        assert_eq!(check_response(head, "dGhlIHNhbXBsZSBub25jZQ=="), Ok(()));
        assert!(check_response(head, "AAAAAAAAAAAAAAAAAAAAAA==").is_err());
        assert!(check_response("HTTP/1.1 404 Not Found\r\n\r\n", "x").is_err());
    }

    /// RFC 6455 5.7's examples: a masked "Hello", and lengths of 126 and
    /// 65536 bytes.
    #[test]
    fn frames_of_the_rfc() {
        let f = encode_frame(OP_TEXT, b"Hello", [0x37, 0xfa, 0x21, 0x3d]);
        assert_eq!(
            f,
            [0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58]
        );
        let (frame, used) = parse_frame(&f).unwrap().unwrap();
        assert_eq!(used, f.len());
        assert_eq!(
            frame,
            Frame {
                fin: true,
                op: OP_TEXT,
                payload: b"Hello".to_vec()
            }
        );
        // an unmasked server frame
        let s = [0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f];
        assert_eq!(parse_frame(&s).unwrap().unwrap().0.payload, b"Hello");
        // not all of it yet
        assert!(parse_frame(&s[..4]).unwrap().is_none());
        let big = vec![7u8; 256];
        let f = encode_frame(OP_BINARY, &big, [0; 4]);
        assert_eq!(&f[..4], &[0x82, 0xfe, 0x01, 0x00]);
        assert_eq!(parse_frame(&f).unwrap().unwrap().0.payload, big);
        let huge = vec![1u8; 65536];
        let f = encode_frame(OP_BINARY, &huge, [1, 2, 3, 4]);
        assert_eq!(&f[..10], &[0x82, 0xff, 0, 0, 0, 0, 0, 1, 0, 0]);
        assert_eq!(parse_frame(&f).unwrap().unwrap().0.payload, huge);
    }

    /// The server's side of the handshake on `l`'s next connection: the
    /// connection, and the answer to write (before the first frames).
    fn accept(l: &std::net::TcpListener) -> (TcpStream, Vec<u8>) {
        let (mut s, _) = l.accept().unwrap();
        let mut req = Vec::new();
        let mut b = [0u8; 1];
        while !req.ends_with(b"\r\n\r\n") {
            s.read_exact(&mut b).unwrap();
            req.push(b[0]);
        }
        let req = String::from_utf8(req).unwrap();
        assert!(req.contains("Sec-WebSocket-Protocol: obswebsocket.json\r\n"));
        let key = req
            .lines()
            .find_map(|l| l.strip_prefix("Sec-WebSocket-Key: "))
            .unwrap();
        let answer = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
             Connection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
            accept_key(key)
        );
        (s, answer.into_bytes())
    }

    /// Against a server on a local port: the handshake, a message each way,
    /// a ping answered, the server's close.
    #[test]
    fn talks_to_a_server() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            // the answer and the first message in one write
            let (mut s, mut out) = accept(&l);
            out.extend_from_slice(&[0x81, 0x02, b'h', b'i', 0x89, 0x01, b'p']);
            s.write_all(&out).unwrap();
            // the pong, then the client's text
            let mut got = Vec::new();
            let mut frames = Vec::new();
            while frames.len() < 2 {
                let mut c = [0u8; 256];
                let n = s.read(&mut c).unwrap();
                got.extend_from_slice(&c[..n]);
                while let Some((f, used)) = parse_frame(&got).unwrap() {
                    got.drain(..used);
                    frames.push(f);
                }
            }
            assert_eq!((frames[0].op, &frames[0].payload[..]), (OP_PONG, &b"p"[..]));
            assert_eq!(
                (frames[1].op, &frames[1].payload[..]),
                (OP_TEXT, &b"yo"[..])
            );
            let mut close = vec![0x88, 0x06];
            close.extend_from_slice(&4009u16.to_be_bytes());
            close.extend_from_slice(b"auth");
            s.write_all(&close).unwrap();
        });
        let mut ws = Ws::connect(
            "127.0.0.1",
            port,
            "obswebsocket.json",
            Duration::from_secs(5),
        )
        .unwrap();
        let mut msgs = Vec::new();
        while msgs.is_empty() {
            msgs = ws.poll(Duration::from_millis(100)).unwrap();
        }
        assert_eq!(msgs, [Message::Text("hi".into())]);
        ws.send_text("yo").unwrap();
        let mut msgs = Vec::new();
        while msgs.is_empty() {
            msgs = ws.poll(Duration::from_millis(100)).unwrap();
        }
        assert_eq!(msgs, [Message::Close(4009, "auth".into())]);
        server.join().unwrap();
    }

    /// A message in parts is taken up to the size of one frame, however
    /// many parts a server sends.
    #[test]
    fn a_message_in_parts_is_bounded() {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let part = MESSAGE_MAX / 2 + 1;
        let server = std::thread::spawn(move || {
            let (mut s, mut out) = accept(&l);
            // "ab" in two parts, then a text that never ends
            out.extend_from_slice(&[0x01, 0x01, b'a', 0x80, 0x01, b'b']);
            s.write_all(&out).unwrap();
            let mut frame = vec![0x01, 127];
            frame.extend_from_slice(&(part as u64).to_be_bytes());
            frame.resize(10 + part, b'x');
            for op in [OP_TEXT, OP_CONT, OP_CONT] {
                frame[0] = op;
                // (the client leaves once it has too much)
                if s.write_all(&frame).is_err() {
                    break;
                }
            }
        });
        let mut ws = Ws::connect(
            "127.0.0.1",
            port,
            "obswebsocket.json",
            Duration::from_secs(5),
        )
        .unwrap();
        let mut msgs = Vec::new();
        let end = loop {
            match ws.poll(Duration::from_millis(100)) {
                Ok(m) => msgs.extend(m),
                Err(e) => break e,
            }
        };
        assert_eq!(msgs, [Message::Text("ab".into())]);
        assert_eq!(end.kind(), io::ErrorKind::InvalidData, "{end}");
        drop(ws);
        server.join().unwrap();
    }
}
