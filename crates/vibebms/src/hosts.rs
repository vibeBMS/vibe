//! Downloading an archive from where BMS authors link to it, without a
//! browser (as vibeBMS/bms `tools/fetch.py` does for the mirror): Google
//! Drive files and folders, Dropbox files and folders, MediaFire, MEGA, and
//! any server that answers with the archive itself. What needs a person on
//! a page (OneDrive, uploaders, a quota page) is [`Fetch::Page`].
//!
//! [`Resolver`] knows the hosts: from a link it asks for the requests to
//! make, one [`Step`] at a time. The app makes them itself ([`fetch`],
//! streaming the archive to disk); the web build makes them through the
//! vibeBMS proxy (no host here sends CORS headers) and keeps the archive in
//! memory.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Why a link gave no archive.
#[derive(Debug)]
pub enum Fetch {
    /// The link is a page to use in a browser.
    Page,
    /// The download failed.
    Failed(String),
}

impl From<String> for Fetch {
    fn from(e: String) -> Fetch {
        Fetch::Failed(e)
    }
}

/// The next request a download needs.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Read this page and [`Resolver::feed`] it back.
    Get(String),
    /// POST this JSON (the MEGA API) and feed back the answer.
    Post(String, Vec<u8>),
    /// The archive: download it, decrypting with `mega_key` (a MEGA file).
    /// What arrives must be an archive; anything else means the link is a
    /// page ([`Fetch::Page`]).
    File {
        url: String,
        mega_key: Option<Vec<u8>>,
    },
    /// No archive from this link.
    Fail(Fetch),
}

impl PartialEq for Fetch {
    fn eq(&self, o: &Fetch) -> bool {
        matches!(
            (self, o),
            (Fetch::Page, Fetch::Page) | (Fetch::Failed(_), Fetch::Failed(_))
        )
    }
}

/// The way from an author's link to its archive.
pub struct Resolver {
    link: String,
    wait: Wait,
}

/// What the last [`Step::Get`] / [`Step::Post`] was for.
enum Wait {
    Nothing,
    DriveFolder,
    MediaFire,
    Mega(Vec<u8>),
}

/// Hosts whose links always need a browser.
const PAGES: &[&str] = &[
    "1drv.ms",
    "onedrive",
    "sharepoint.com",
    "baidu",
    "lanzou",
    "lanzn",
    "gigafile",
    "box.com",
];

impl Resolver {
    /// Start from `link`: the first request.
    pub fn new(link: &str) -> (Resolver, Step) {
        let mut r = Resolver {
            link: link.trim().to_string(),
            wait: Wait::Nothing,
        };
        let step = r.first();
        (r, step)
    }

    fn first(&mut self) -> Step {
        let u = self.link.clone();
        let lower = u.to_ascii_lowercase();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            return Step::Fail(Fetch::Page);
        }
        let host = lower.split('/').nth(2).unwrap_or("").to_string();
        let file = |url: String| Step::File {
            url,
            mega_key: None,
        };
        if host.ends_with("drive.google.com") || host == "docs.google.com" {
            let Some(id) = drive_id(&u) else {
                return Step::Fail(Fetch::Page);
            };
            if lower.contains("/folders/") {
                self.wait = Wait::DriveFolder;
                return Step::Get(format!(
                    "https://drive.google.com/embeddedfolderview?id={id}"
                ));
            }
            return file(drive_file_url(&id));
        }
        if host.ends_with("dropbox.com") {
            return file(dropbox_url(&u));
        }
        if host.ends_with("mediafire.com") {
            if lower.contains("/folder/") {
                return Step::Fail(Fetch::Page);
            }
            self.wait = Wait::MediaFire;
            return Step::Get(u);
        }
        if host == "mega.nz" || host == "mega.co.nz" || host.ends_with(".mega.nz") {
            let Some((handle, key)) = mega_link(&u) else {
                return Step::Fail(Fetch::Page);
            };
            self.wait = Wait::Mega(key);
            return Step::Post(MEGA_API.into(), mega_request(&handle));
        }
        if PAGES.iter().any(|p| host.contains(p)) {
            return Step::Fail(Fetch::Page);
        }
        file(u)
    }

    /// The answer to the last [`Step::Get`] / [`Step::Post`]: the next step.
    pub fn feed(&mut self, answer: Result<Vec<u8>, String>) -> Step {
        let body = match answer {
            Ok(b) => b,
            Err(e) => return Step::Fail(Fetch::Failed(e)),
        };
        match std::mem::replace(&mut self.wait, Wait::Nothing) {
            Wait::Nothing => Step::Fail(Fetch::Failed("nothing asked".into())),
            Wait::DriveFolder => {
                let page = String::from_utf8_lossy(&body);
                let files = folder_entries(&page);
                match pick_file(
                    files
                        .iter()
                        .filter(|(name, _)| archive_name(name))
                        .map(|(n, i)| (n.as_str(), i.as_str())),
                ) {
                    Some(id) => Step::File {
                        url: drive_file_url(id),
                        mega_key: None,
                    },
                    None => Step::Fail(Fetch::Page),
                }
            }
            // the download button's link
            Wait::MediaFire => {
                let page = String::from_utf8_lossy(&body);
                match page
                    .find("href=\"https://download")
                    .and_then(|at| between(&page[at..], "href=\"", "\""))
                {
                    Some(link) => Step::File {
                        url: unescape(link),
                        mega_key: None,
                    },
                    None => Step::Fail(Fetch::Page),
                }
            }
            Wait::Mega(key) => match mega_answer(&body) {
                Ok(url) => Step::File {
                    url,
                    mega_key: Some(key),
                },
                Err(e) => Step::Fail(Fetch::Failed(e)),
            },
        }
    }
}

/// The file or folder id of a Google Drive link (`/file/d/ID`,
/// `/folders/ID`, `?id=ID`).
pub fn drive_id(u: &str) -> Option<String> {
    let rest = ["/d/", "/folders/", "id="]
        .iter()
        .find_map(|k| u.find(k).map(|i| &u[i + k.len()..]))?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!id.is_empty()).then_some(id)
}

fn drive_file_url(id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t")
}

/// (name, file id) of the files in an `embeddedfolderview` page.
fn folder_entries(page: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in page.split("class=\"flip-entry\"").skip(1) {
        let Some(id) = between(part, "/file/d/", "/") else {
            continue; // a sub-folder
        };
        if let Some(name) = between(part, "flip-entry-title\">", "<") {
            out.push((unescape(name), id.to_string()));
        }
    }
    out
}

fn archive_name(n: &str) -> bool {
    let n = n.to_ascii_lowercase();
    [".zip", ".rar", ".7z"].iter().any(|e| n.ends_with(e))
}

/// The best of (name, id) in a Drive folder (as `fetch.py` `pick_file`):
/// ogg over wav, full over BGA-only and patches, the first of equals.
fn pick_file<'a>(files: impl Iterator<Item = (&'a str, &'a str)>) -> Option<&'a str> {
    let score = |n: &str| {
        let n = n.to_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| n.contains(w));
        let mut s = 0;
        if has(&["ogg"]) {
            s += 4;
        }
        if has(&["wav", "flac"]) {
            s -= 4;
        }
        if has(&["lq", "low", "lr2", "wmv", "mpg", "lite", "compact"]) {
            s -= 2;
        }
        if has(&[
            "bga only",
            "bga_only",
            "only bga",
            "bga単体",
            "patch",
            "差分",
            "追加",
        ]) {
            s -= 8;
        }
        s
    };
    let mut best: Option<(i32, &str)> = None;
    for (name, id) in files {
        let s = score(name);
        if best.is_none_or(|(b, _)| s > b) {
            best = Some((s, id));
        }
    }
    best.map(|(_, id)| id)
}

/// Dropbox files and folders download with `dl=1` (a folder as a zip).
fn dropbox_url(u: &str) -> String {
    let base = u.replace("dl=0", "dl=1");
    if base.contains("dl=1") {
        base
    } else if base.contains('?') {
        format!("{base}&dl=1")
    } else {
        format!("{base}?dl=1")
    }
}

/// MEGA's AES-128-CTR with the 32-byte key of a file link.
pub struct MegaCipher {
    aes: aes::Aes128,
    ctr: u128,
}

impl MegaCipher {
    pub fn new(k: &[u8]) -> MegaCipher {
        use aes::cipher::KeyInit;
        let mut key = [0u8; 16];
        for i in 0..16 {
            key[i] = k[i] ^ k[i + 16];
        }
        // the counter starts at the key's nonce (bytes 16..24), then 0
        let mut nonce = [0u8; 16];
        nonce[..8].copy_from_slice(&k[16..24]);
        MegaCipher {
            aes: aes::Aes128::new(&key.into()),
            ctr: u128::from_be_bytes(nonce),
        }
    }

    /// Decrypt the next part of the file (whole 16-byte blocks but at the
    /// end).
    pub fn apply(&mut self, data: &mut [u8]) {
        use aes::cipher::BlockCipherEncrypt;
        for chunk in data.chunks_mut(16) {
            let mut block = self.ctr.to_be_bytes().into();
            self.aes.encrypt_block(&mut block);
            for (b, s) in chunk.iter_mut().zip(block.iter()) {
                *b ^= s;
            }
            self.ctr = self.ctr.wrapping_add(1);
        }
    }
}

/// A MEGA file link (`mega.nz/file/H#K`, `mega.nz/#!H!K`): (handle,
/// 32-byte key).
pub fn mega_link(u: &str) -> Option<(String, Vec<u8>)> {
    if u.contains("/folder/") || u.contains("#F!") {
        return None;
    }
    let (h, k) = if let Some(i) = u.find("/file/") {
        u[i + 6..].split_once('#')?
    } else {
        u[u.find("#!")? + 2..].split_once('!')?
    };
    let key = b64url(k).filter(|k| k.len() == 32)?;
    Some((h.to_string(), key))
}

/// The MEGA API.
pub const MEGA_API: &str = "https://g.api.mega.co.nz/cs?id=0";

/// The API request for a file's download URL.
pub fn mega_request(handle: &str) -> Vec<u8> {
    // ssl: an https URL (a page served over https may not read http)
    format!(r#"[{{"a":"g","g":1,"ssl":1,"p":"{handle}"}}]"#).into_bytes()
}

/// The download URL in the MEGA API's answer.
pub fn mega_answer(body: &[u8]) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("MEGA: {e}"))?;
    v.get(0)
        .and_then(|f| f.get("g"))
        .and_then(|g| g.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("MEGA refused ({v})"))
}

/// The archive format of a file's first bytes.
pub fn sniff_bytes(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") {
        Some("zip")
    } else if h.starts_with(b"Rar!\x1a\x07") {
        Some("rar")
    } else if h.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some("7z")
    } else {
        None
    }
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.trim_end_matches('=').bytes() {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// `s` for a URL's query.
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ---- the app: requests made directly, the archive streamed to disk ----

#[cfg(not(target_arch = "wasm32"))]
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

/// Download the archive `link` leads to into `out`.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch(link: &str, out: &Path) -> Result<(), Fetch> {
    let (mut r, mut step) = Resolver::new(link);
    loop {
        step = match step {
            Step::Get(url) => {
                let mut req = ehttp::Request::get(&url);
                req.headers.insert("User-Agent", UA);
                r.feed(answer(ehttp::fetch_blocking(&req)))
            }
            Step::Post(url, body) => {
                let mut req = ehttp::Request::post(&url, body);
                req.headers.insert("Content-Type", "application/json");
                r.feed(answer(ehttp::fetch_blocking(&req)))
            }
            Step::File { url, mega_key } => {
                match mega_key {
                    None => download_to(&url, out)?,
                    Some(key) => {
                        let enc = out.with_extension("mega");
                        download_to(&url, &enc)?;
                        let d = mega_decrypt(&enc, out, &key);
                        let _ = std::fs::remove_file(&enc);
                        d?;
                    }
                }
                if sniff(out).is_none() {
                    let _ = std::fs::remove_file(out);
                    return Err(Fetch::Page);
                }
                return Ok(());
            }
            Step::Fail(f) => return Err(f),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn answer(r: Result<ehttp::Response, String>) -> Result<Vec<u8>, String> {
    match r {
        Ok(r) if r.ok => Ok(r.bytes),
        Ok(r) => Err(format!("HTTP {}", r.status)),
        Err(e) => Err(e),
    }
}

/// The archive format of a file from its first bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn sniff(p: &Path) -> Option<&'static str> {
    let mut head = [0u8; 8];
    let n = std::fs::File::open(p).ok()?.read(&mut head).ok()?;
    sniff_bytes(&head[..n])
}

#[cfg(not(target_arch = "wasm32"))]
fn mega_decrypt(from: &Path, to: &Path, k: &[u8]) -> Result<(), String> {
    let mut c = MegaCipher::new(k);
    let mut src = std::io::BufReader::new(std::fs::File::open(from).map_err(|e| e.to_string())?);
    let mut dst = std::io::BufWriter::new(std::fs::File::create(to).map_err(|e| e.to_string())?);
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = read_full(&mut src, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        c.apply(&mut buf[..n]);
        dst.write_all(&buf[..n]).map_err(|e| e.to_string())?;
    }
    dst.flush().map_err(|e| e.to_string())
}

/// Fill `buf` (short only at the end), so chunks stay on 16-byte blocks.
#[cfg(not(target_arch = "wasm32"))]
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Stream `url` into `out`.
#[cfg(not(target_arch = "wasm32"))]
pub fn download_to(url: &str, out: &Path) -> Result<(), String> {
    use std::ops::ControlFlow;
    use std::sync::{Arc, Mutex};
    let file = std::fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    // the file being written, then how the download ended
    type State = (
        Option<std::io::BufWriter<std::fs::File>>,
        Option<Result<(), String>>,
    );
    let state: Arc<Mutex<State>> =
        Arc::new(Mutex::new((Some(std::io::BufWriter::new(file)), None)));
    let st = state.clone();
    let mut req = ehttp::Request::get(url);
    // Dropbox answers a browser with its page instead of the file
    if !url.contains("dropbox") {
        req.headers.insert("User-Agent", UA);
    }
    ehttp::streaming::fetch_streaming_blocking(
        req,
        Box::new(move |part| {
            let mut g = st.lock().expect("download state");
            match part {
                Err(e) => {
                    g.1 = Some(Err(e));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Response(r)) if !r.ok => {
                    g.1 = Some(Err(format!("HTTP {}", r.status)));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Response(_)) => ControlFlow::Continue(()),
                Ok(ehttp::streaming::Part::Chunk(c)) if c.is_empty() => {
                    let r = g.0.take().map(|mut w| w.flush()).transpose();
                    g.1 = Some(r.map(|_| ()).map_err(|e| e.to_string()));
                    ControlFlow::Break(())
                }
                Ok(ehttp::streaming::Part::Chunk(c)) => {
                    if let Some(Err(e)) = g.0.as_mut().map(|w| w.write_all(&c)) {
                        g.1 = Some(Err(e.to_string()));
                        return ControlFlow::Break(());
                    }
                    ControlFlow::Continue(())
                }
            }
        }),
    );
    let r = state.lock().expect("download state").1.take();
    r.unwrap_or_else(|| Err("the download ended early".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert_eq!(
            drive_id("https://drive.google.com/file/d/1AbC-d_9/view?usp=sharing").as_deref(),
            Some("1AbC-d_9")
        );
        assert_eq!(
            drive_id("https://drive.google.com/drive/folders/1UX?usp=x").as_deref(),
            Some("1UX")
        );
        assert_eq!(
            drive_id("https://drive.google.com/open?id=XYZ").as_deref(),
            Some("XYZ")
        );
        assert_eq!(
            dropbox_url("https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=0"),
            "https://www.dropbox.com/scl/fo/a/b?rlkey=k&dl=1"
        );
        assert_eq!(b64url("AQID").unwrap(), [1, 2, 3]);
        assert_eq!(b64url("-_8").unwrap(), [0xfb, 0xff]);
        assert_eq!(
            url_encode("https://a.b/c?d=1&e"),
            "https%3A%2F%2Fa.b%2Fc%3Fd%3D1%26e"
        );
    }

    #[test]
    fn steps() {
        let file = |s: Step| match s {
            Step::File { url, .. } => url,
            s => panic!("{s:?}"),
        };
        assert!(file(Resolver::new("https://drive.google.com/file/d/X/view").1).contains("id=X"));
        assert!(file(Resolver::new("http://example.com/a.zip").1).ends_with("a.zip"));
        assert_eq!(
            Resolver::new("https://1drv.ms/u/c/x").1,
            Step::Fail(Fetch::Page)
        );
        assert_eq!(Resolver::new("not a link").1, Step::Fail(Fetch::Page));
        // a Drive folder: its page, then its best archive
        let (mut r, s) = Resolver::new("https://drive.google.com/drive/folders/F");
        assert!(matches!(s, Step::Get(u) if u.ends_with("id=F")));
        let page = r#"<div class="flip-entry" id="entry-B"><a href="https://drive.google.com/file/d/B/view"><div class="flip-entry-title">s_ogg.zip</div></a></div>"#;
        assert!(file(r.feed(Ok(page.into()))).contains("id=B"));
        // MediaFire: the download button
        let (mut r, s) = Resolver::new("https://www.mediafire.com/file/x/a.rar/file");
        assert!(matches!(s, Step::Get(_)));
        let page = r#"<a class="input" href="https://download1.mediafire.com/x/a.rar">"#;
        assert_eq!(
            file(r.feed(Ok(page.into()))),
            "https://download1.mediafire.com/x/a.rar"
        );
        // MEGA: the API, then the file and its key
        let (mut r, s) = Resolver::new(
            "https://mega.nz/file/bzQy3bYS#Og0LBoWkfbKwxUceGvzLHDl2exyI0l1DiQ3XNw79zMw",
        );
        assert!(matches!(s, Step::Post(..)));
        match r.feed(Ok(br#"[{"s":3,"g":"https://gfs.example/dl/x"}]"#.to_vec())) {
            Step::File { url, mega_key } => {
                assert_eq!(url, "https://gfs.example/dl/x");
                assert_eq!(mega_key.map(|k| k.len()), Some(32));
            }
            s => panic!("{s:?}"),
        }
    }

    #[test]
    fn drive_folders() {
        let page = r#"<div class="flip-entry" id="entry-A"><a href="https://drive.google.com/file/d/A/view?usp=drive_web"><div class="flip-entry-title">song.zip</div></a></div>
            <div class="flip-entry" id="entry-S"><a href="https://drive.google.com/drive/folders/S"><div class="flip-entry-title">sub</div></a></div>
            <div class="flip-entry" id="entry-B"><a href="https://drive.google.com/file/d/B/view?usp=drive_web"><div class="flip-entry-title">song_ogg.zip</div></a></div>
            <div class="flip-entry" id="entry-C"><a href="https://drive.google.com/file/d/C/view?usp=drive_web"><div class="flip-entry-title">song_wav.zip</div></a></div>"#;
        let e = folder_entries(page);
        assert_eq!(e.len(), 3);
        let best = pick_file(e.iter().map(|(n, i)| (n.as_str(), i.as_str())));
        assert_eq!(best, Some("B"));
    }

    /// `VIBEBMS_FETCH=URL cargo test -p vibebms fetch_url -- --ignored`
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "network"]
    fn fetch_url() {
        let url = std::env::var("VIBEBMS_FETCH").expect("VIBEBMS_FETCH");
        let out = std::env::temp_dir().join("vibebms-fetch");
        match fetch(&url, &out) {
            Ok(()) => println!(
                "OK {:?} {} bytes",
                sniff(&out),
                std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0)
            ),
            Err(e) => println!("ERR {e:?}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mega_ctr() {
        // AES-128 of a zero key and zero counter block, xored with zeros
        let dir = std::env::temp_dir().join(format!("vibebms-mega-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        std::fs::write(&a, [0u8; 20]).unwrap();
        mega_decrypt(&a, &b, &[0u8; 32]).unwrap();
        let out = std::fs::read(&b).unwrap();
        assert_eq!(out.len(), 20);
        assert_eq!(&out[..4], &[0x66, 0xe9, 0x4b, 0xd4]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
