//! Requests and answers as plain data: the crate builds the one and reads
//! the other, the application sends them (`platform::http::exchange`).
//! Nothing here formats a credential: `Secret` and `HttpRequest`'s `Debug`
//! print `<redacted>` in its place.

use std::fmt;

/// A credential (an API key, a token, a password). It prints as
/// `<redacted>` and has no `Serialize`: its value leaves only through
/// [`Secret::expose`], when a request is built.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Secret {
        Secret(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// An HTTP request for the application to send. Its `Debug` hides the
/// credentials it may carry.
#[derive(Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Headers whose value is a credential.
const SECRET_HEADERS: &[&str] = &["authorization", "cookie"];
/// JSON body fields whose value is a credential.
const SECRET_FIELDS: &[&str] = &["token", "pass", "password", "api_token"];

impl HttpRequest {
    pub(crate) fn new(method: &'static str, url: String) -> HttpRequest {
        HttpRequest {
            method,
            url,
            headers: vec![("Accept".into(), "application/json".into())],
            body: Vec::new(),
        }
    }

    pub(crate) fn header(mut self, name: &str, value: impl Into<String>) -> HttpRequest {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub(crate) fn bearer(self, token: &Secret) -> HttpRequest {
        self.header("Authorization", format!("Bearer {}", token.expose()))
    }

    pub(crate) fn json(mut self, content_type: &str, body: &str) -> HttpRequest {
        self.body = body.as_bytes().to_vec();
        self.header("Content-Type", content_type)
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let hide = SECRET_HEADERS.contains(&k.to_ascii_lowercase().as_str());
                (k.as_str(), if hide { "<redacted>" } else { v.as_str() })
            })
            .collect();
        let body = match serde_json::from_slice::<serde_json::Value>(&self.body) {
            Ok(mut v) => {
                redact(&mut v);
                v.to_string()
            }
            Err(_) => format!("<{} bytes>", self.body.len()),
        };
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &format_args!("{body}"))
            .finish()
    }
}

fn redact(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, v) in m.iter_mut() {
                if SECRET_FIELDS.contains(&k.as_str()) {
                    *v = "<redacted>".into();
                } else {
                    redact(v);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(redact),
        _ => {}
    }
}

/// An answer, whatever its status. On the web only the headers CORS lets
/// a page read are here (`Retry-After` and `ETag` only when the server
/// exposes them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn etag(&self) -> Option<String> {
        self.header("ETag").map(str::to_string)
    }

    pub fn retry_after(&self) -> Option<RetryAfter> {
        self.header("Retry-After").and_then(RetryAfter::parse)
    }

    /// The body as text, cut to a line a user can read.
    pub(crate) fn text(&self) -> String {
        let s = String::from_utf8_lossy(&self.body);
        let s = s.trim();
        match s.char_indices().nth(200) {
            Some((i, _)) => format!("{}…", &s[..i]),
            None => s.to_string(),
        }
    }
}

/// `Retry-After`: seconds, or an HTTP date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryAfter {
    Secs(u64),
    /// Unix ms.
    At(i64),
}

impl RetryAfter {
    pub fn parse(v: &str) -> Option<RetryAfter> {
        let v = v.trim();
        match v.parse() {
            Ok(s) => Some(RetryAfter::Secs(s)),
            Err(_) => http_date(v).map(RetryAfter::At),
        }
    }

    /// How long to wait from `now_ms`.
    pub fn delay_ms(self, now_ms: i64) -> u64 {
        match self {
            RetryAfter::Secs(s) => s.saturating_mul(1000),
            RetryAfter::At(at) => at.saturating_sub(now_ms).max(0) as u64,
        }
    }
}

/// An IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) as Unix ms.
fn http_date(v: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut p = v.split_whitespace().skip(1);
    let day = p.next()?.parse().ok()?;
    let month = p.next()?;
    let month = MONTHS.iter().position(|m| *m == month)? as u32 + 1;
    let year = p.next()?.parse().ok()?;
    let mut t = p.next()?.split(':').map(|n| n.parse::<u32>().ok());
    let (h, m, s) = (t.next()??, t.next()??, t.next()??);
    (p.next() == Some("GMT")).then(|| unix_ms(year, month, day, h, m, s))
}

/// Unix ms of a UTC date and time.
pub(crate) fn unix_ms(year: i64, month: u32, day: u32, h: u32, m: u32, s: u32) -> i64 {
    // days from 1970-01-01 (Howard Hinnant's days_from_civil)
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 24 + h as i64) * 60 + m as i64) * 60_000 + s as i64 * 1000
}

/// Percent-encode a query or path value.
pub(crate) fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out += &format!("%{b:02X}"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_print() {
        let s = Secret::new("k3y-value");
        assert_eq!(format!("{s} {s:?}"), "<redacted> <redacted>");
        let req = HttpRequest::new("POST", "https://ir.example/x".into())
            .bearer(&s)
            .json(
                "application/json",
                r#"{"player":"a","api_token":"k3y-value","inner":[{"password":"k3y-value"}]}"#,
            );
        let shown = format!("{req:?}");
        assert!(!shown.contains("k3y-value"), "{shown}");
        assert!(shown.contains("\"player\":\"a\""), "{shown}");
        assert_eq!(req.headers[1].1, "Bearer k3y-value");
    }

    #[test]
    fn retry_after_takes_seconds_and_dates() {
        let now = unix_ms(2026, 11, 17, 12, 0, 0);
        assert_eq!(RetryAfter::parse("120"), Some(RetryAfter::Secs(120)));
        let at = RetryAfter::parse("Tue, 17 Nov 2026 12:01:30 GMT").unwrap();
        assert_eq!(at.delay_ms(now), 90_000);
        assert_eq!(at.delay_ms(now + 100_000), 0);
        assert_eq!(RetryAfter::parse("soon"), None);
        assert_eq!(unix_ms(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(unix_ms(2026, 1, 1, 0, 0, 0), 1_767_225_600_000);
        let r = HttpResponse {
            status: 429,
            headers: vec![("retry-after".into(), " 60 ".into())],
            body: Vec::new(),
        };
        assert_eq!(r.retry_after(), Some(RetryAfter::Secs(60)));
    }

    #[test]
    fn values_are_encoded() {
        assert_eq!(enc("a b/ü"), "a%20b%2F%C3%BC");
    }
}
