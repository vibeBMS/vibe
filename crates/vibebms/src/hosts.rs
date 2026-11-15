//! Downloading an archive from where BMS authors link to it, without a
//! browser (as vibeBMS/bms `tools/fetch.py` does for the mirror): Google
//! Drive files and folders, Dropbox files and folders, MediaFire, MEGA, and
//! any server that answers with the archive itself. What needs a person on
//! a page (OneDrive, uploaders, a quota page) is [`Fetch::Page`].
//!
//! The web build reaches only hosts that let other pages read them (CORS):
//! Dropbox files and MEGA ([`web_source`]). Google Drive answers a
//! browser's cross-site request with 403, whatever its headers say.

#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36";

#[cfg(not(target_arch = "wasm32"))]
/// Why a link gave no archive.
#[derive(Debug)]
pub enum Fetch {
    /// The link is a page to use in a browser.
    Page,
    /// The download failed.
    Failed(String),
}

#[cfg(not(target_arch = "wasm32"))]
impl From<String> for Fetch {
    fn from(e: String) -> Fetch {
        Fetch::Failed(e)
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Download the archive `link` leads to into `out`.
pub fn fetch(link: &str, out: &Path) -> Result<(), Fetch> {
    let u = link.trim();
    if !(u.starts_with("https://") || u.starts_with("http://")) {
        return Err(Fetch::Page);
    }
    let lower = u.to_ascii_lowercase();
    let host = lower.split('/').nth(2).unwrap_or("").to_string();
    let url = if host.ends_with("drive.google.com") || host == "docs.google.com" {
        let id = drive_id(u).ok_or(Fetch::Page)?;
        if lower.contains("/folders/") {
            drive_folder_file(&id)?
        } else {
            drive_file_url(&id)
        }
    } else if host.ends_with("dropbox.com") {
        dropbox_url(u)
    } else if host.ends_with("mediafire.com") {
        if lower.contains("/folder/") {
            return Err(Fetch::Page);
        }
        mediafire_url(u)?
    } else if host == "mega.nz" || host == "mega.co.nz" || host.ends_with(".mega.nz") {
        return mega(u, out);
    } else if PAGES.iter().any(|p| host.contains(p)) {
        return Err(Fetch::Page);
    } else {
        u.to_string()
    };
    download_to(&url, out)?;
    if sniff(out).is_none() {
        let _ = std::fs::remove_file(out);
        return Err(Fetch::Page);
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
/// The archive format of a file from its first bytes.
pub fn sniff(p: &Path) -> Option<&'static str> {
    let mut head = [0u8; 8];
    let n = std::fs::File::open(p).ok()?.read(&mut head).ok()?;
    sniff_bytes(&head[..n])
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn drive_file_url(id: &str) -> String {
    format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t")
}

#[cfg(not(target_arch = "wasm32"))]
/// The archive to take from a shared Drive folder (as `fetch.py`
/// `pick_file`: ogg over wav, full over BGA-only and patches).
fn drive_folder_file(id: &str) -> Result<String, Fetch> {
    let page = get_text(&format!(
        "https://drive.google.com/embeddedfolderview?id={id}"
    ))?;
    let files = folder_entries(&page);
    let best = pick_file(
        files
            .iter()
            .filter(|(name, _)| archive_name(name))
            .map(|(n, i)| (n.as_str(), i.as_str())),
    )
    .ok_or(Fetch::Page)?;
    Ok(drive_file_url(best))
}

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
fn archive_name(n: &str) -> bool {
    let n = n.to_ascii_lowercase();
    [".zip", ".rar", ".7z"].iter().any(|e| n.ends_with(e))
}

#[cfg(not(target_arch = "wasm32"))]
/// The best of (name, id): ogg over wav, full over BGA-only and patches,
/// the first of equals.
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

#[cfg(not(target_arch = "wasm32"))]
/// The download button's link on a MediaFire page.
fn mediafire_url(u: &str) -> Result<String, Fetch> {
    let page = get_text(u)?;
    let at = page.find("href=\"https://download").ok_or(Fetch::Page)?;
    let link = between(&page[at..], "href=\"", "\"").ok_or(Fetch::Page)?;
    Ok(unescape(link))
}

#[cfg(not(target_arch = "wasm32"))]
/// A MEGA file: its URL from the API, then AES-128-CTR decrypted with the
/// key in the link (`mega.nz/file/H#K`, `mega.nz/#!H!K`).
fn mega(u: &str, out: &Path) -> Result<(), Fetch> {
    let (handle, key) = mega_link(u).ok_or(Fetch::Page)?;
    let mut req = ehttp::Request::post(MEGA_API, mega_request(&handle));
    req.headers.insert("Content-Type", "application/json");
    let r = ehttp::fetch_blocking(&req).map_err(Fetch::Failed)?;
    let g = mega_answer(&r.bytes)?;
    let g = g.as_str();
    let enc = out.with_extension("mega");
    download_to(g, &enc)?;
    let r = mega_decrypt(&enc, out, &key);
    let _ = std::fs::remove_file(&enc);
    r.map_err(Fetch::Failed)?;
    if sniff(out).is_none() {
        return Err(Fetch::Failed("MEGA: not an archive".into()));
    }
    Ok(())
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

#[cfg(not(target_arch = "wasm32"))]
/// Fill `buf` (short only at the end), so chunks stay on 16-byte blocks.
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

#[cfg(not(target_arch = "wasm32"))]
fn between<'a>(s: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = s.find(a)? + a.len();
    let j = s[i..].find(b)? + i;
    Some(&s[i..j])
}

#[cfg(not(target_arch = "wasm32"))]
fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(not(target_arch = "wasm32"))]
fn get_text(url: &str) -> Result<String, Fetch> {
    let mut req = ehttp::Request::get(url);
    req.headers.insert("User-Agent", UA);
    let r = ehttp::fetch_blocking(&req).map_err(Fetch::Failed)?;
    if !r.ok {
        return Err(Fetch::Failed(format!("HTTP {}", r.status)));
    }
    Ok(String::from_utf8_lossy(&r.bytes).into_owned())
}

#[cfg(not(target_arch = "wasm32"))]
/// Stream `url` into `out`.
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

/// A MEGA file link: (handle, 32-byte key).
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

/// The MEGA API request for a file's download URL.
pub const MEGA_API: &str = "https://g.api.mega.co.nz/cs?id=0";

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

/// Where the web build can get a link's archive.
pub enum Web {
    Url(String),
    /// A MEGA file: the API, then its encrypted bytes.
    Mega {
        handle: String,
        key: Vec<u8>,
    },
}

/// A link the web build can download (its host lets other pages read it):
/// a Dropbox file, a MEGA file.
pub fn web_source(link: &str) -> Option<Web> {
    let u = link.trim();
    let lower = u.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return None;
    }
    let host = lower.split('/').nth(2).unwrap_or("");
    if host.ends_with("dropbox.com") || host.ends_with("dropboxusercontent.com") {
        if lower.contains("/scl/fo/") || lower.contains("/sh/") {
            return None;
        }
        // www.dropbox.com redirects without CORS headers; this host has them
        let path = &u[lower.find(host).map(|i| i + host.len())?..];
        return Some(Web::Url(dropbox_url(&format!(
            "https://dl.dropboxusercontent.com{path}"
        ))));
    }
    if host == "mega.nz" || host == "mega.co.nz" {
        return mega_link(u).map(|(handle, key)| Web::Mega { handle, key });
    }
    None
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

    #[test]
    fn web_sources() {
        let url = |l: &str| match web_source(l) {
            Some(Web::Url(u)) => u,
            _ => String::new(),
        };
        assert_eq!(
            url("https://www.dropbox.com/scl/fi/a/b.zip?rlkey=k&dl=0"),
            "https://dl.dropboxusercontent.com/scl/fi/a/b.zip?rlkey=k&dl=1"
        );
        assert!(web_source("https://www.dropbox.com/scl/fo/a/b?rlkey=k").is_none());
        // Drive refuses browsers' cross-site requests
        assert!(web_source("https://drive.google.com/file/d/X/view").is_none());
        assert!(matches!(
            web_source("https://mega.nz/file/bzQy3bYS#Og0LBoWkfbKwxUceGvzLHDl2exyI0l1DiQ3XNw79zMw"),
            Some(Web::Mega { .. })
        ));
        assert!(web_source("https://www.mediafire.com/file/x/y.zip/file").is_none());
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
