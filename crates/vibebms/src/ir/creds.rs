//! The keys of IR accounts (API keys, tokens) and OBS's password, kept
//! apart from `config.toml` so that a configuration shared or attached to a
//! bug report carries none. All of them are one small JSON document, kept
//! where the platform keeps such things:
//! - desktop: `config/credentials.json`, readable by the user alone on
//!   Unix (the install is portable, and a keychain may be missing or ask
//!   again after every unsigned update);
//! - Android: the app's private storage (the data folder is the shared
//!   external one);
//! - iOS: the Keychain, this device only (the data folder is `Documents`,
//!   seen in Files and in backups);
//! - web: the page's `localStorage`, which any script of the site can read.
//!
//! Each secret is kept with the server it was entered for and given out
//! for that server only: the address of a server is a setting, which a
//! configuration taken from someone else (or another program that can
//! write `config.toml`) may point elsewhere.
//!
//! Nothing here logs or formats a key.

use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use vibebms_ir::provider::Creds;
use vibebms_ir::{ProviderId, Secret};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Doc {
    keys: BTreeMap<ProviderId, Stored>,
    /// In the root's document (the OBS settings are app-wide).
    #[serde(skip_serializing_if = "Option::is_none")]
    obs: Option<ObsSecret>,
}

/// An account's key as the document has it.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Stored {
    /// With the origin ([`origin`]) of the server it was entered for.
    Bound { key: String, origin: String },
    /// As versions before kept it: given a server once ([`Keys::bind`]),
    /// never given out before.
    Unbound(String),
}

/// OBS's WebSocket password and the address it was entered for.
#[derive(Serialize, Deserialize)]
struct ObsSecret {
    password: String,
    host: String,
    port: u16,
}

/// `host` is this computer.
pub fn loopback(host: &str) -> bool {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The origin of an IR server's address, as `scheme://host:port`: `https`,
/// or `http` on this computer (a key sent over plain `http` can be read on
/// the way). None for any other address, and for one whose host is not
/// plain to read (a user name in it, characters a URL parser may take
/// another way): no request goes there.
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port: u16 = match scheme.as_str() {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = match authority.rfind(':') {
        Some(i) if !authority[i..].contains(']') => (&authority[..i], Some(&authority[i + 1..])),
        _ => (authority, None),
    };
    let port = match port {
        Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok()?,
        Some(_) => return None,
        None => default_port,
    };
    let named = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    let v6 = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .is_some_and(|h| h.parse::<std::net::Ipv6Addr>().is_ok());
    if !(named || v6) || (scheme == "http" && !loopback(host)) {
        return None;
    }
    Some(format!("{scheme}://{}:{port}", host.to_ascii_lowercase()))
}

/// The accounts' keys.
pub struct Keys {
    at: imp::Place,
    doc: Doc,
}

/// Where the keys are, said to the player.
pub fn kept() -> &'static str {
    imp::kept()
}

fn read(at: &imp::Place) -> Doc {
    match imp::read(at) {
        Ok(Some(t)) => serde_json::from_str(&t).unwrap_or_else(|e| {
            log::warn!("credentials: {e}");
            Doc::default()
        }),
        Ok(None) => Doc::default(),
        Err(e) => {
            log::warn!("credentials: {e}");
            Doc::default()
        }
    }
}

impl Keys {
    /// The keys of the profile in use.
    pub fn open(paths: &Paths) -> Keys {
        let at = imp::place(&paths.private);
        let doc = read(&at);
        Keys { at, doc }
    }

    /// The root's document, whichever profile is in use: OBS's password.
    #[cfg(desktop)]
    pub fn open_root(paths: &Paths) -> Keys {
        let at = imp::place(&paths.private_of(""));
        let doc = read(&at);
        Keys { at, doc }
    }

    /// The credentials of `p`'s account for its requests to the server at
    /// `origin`: the key entered for that server, no other.
    pub fn get(&self, p: ProviderId, origin: &str) -> Option<Creds> {
        match self.doc.keys.get(&p)? {
            Stored::Bound { key, origin: o } if o == origin => {
                Some(Creds::token(Secret::new(key.as_str())))
            }
            _ => None,
        }
    }

    /// A key is kept for `p`'s account (for whichever server).
    pub fn has(&self, p: ProviderId) -> bool {
        self.doc.keys.contains_key(&p)
    }

    /// `p`'s key is kept without a server yet ([`Keys::bind`]).
    pub fn held(&self, p: ProviderId) -> bool {
        matches!(self.doc.keys.get(&p), Some(Stored::Unbound(_)))
    }

    /// Keep `key` for `p`'s account at the server at `origin` (`None`:
    /// forget the account's key).
    pub fn set(&mut self, p: ProviderId, key: Option<(String, String)>) -> io::Result<()> {
        self.change(|doc| {
            match key {
                Some((key, origin)) => doc.keys.insert(p, Stored::Bound { key, origin }),
                None => doc.keys.remove(&p),
            };
        })
    }

    /// Keep `key` for `p`'s account until the account has an address a key
    /// may go to ([`Keys::bind`]).
    pub fn hold(&mut self, p: ProviderId, key: String) -> io::Result<()> {
        self.change(|doc| {
            doc.keys.insert(p, Stored::Unbound(key));
        })
    }

    /// The keys versions before kept without their server get the one
    /// their account has now (`account`: None when there is no such
    /// account, `Some(None)` when its address is not one a key may go to).
    /// Once: a key is kept with its server from then on. One whose account
    /// is gone goes; one whose account's address takes no key is held,
    /// never given out, until the account has such an address or is
    /// removed.
    pub fn bind(
        &mut self,
        account: impl Fn(ProviderId) -> Option<Option<String>>,
    ) -> io::Result<()> {
        let held = |p: &ProviderId, s: &Stored| match s {
            Stored::Unbound(_) => account(*p) == Some(None),
            Stored::Bound { .. } => true,
        };
        if self.doc.keys.iter().all(|(p, s)| held(p, s)) {
            return Ok(());
        }
        self.change(|doc| {
            let old = std::mem::take(&mut doc.keys);
            doc.keys = (old.into_iter())
                .filter_map(|(p, s)| match s {
                    Stored::Unbound(key) => Some((
                        p,
                        match account(p)? {
                            Some(origin) => Stored::Bound { key, origin },
                            None => Stored::Unbound(key),
                        },
                    )),
                    bound => Some((p, bound)),
                })
                .collect();
        })
    }

    /// OBS's password for the server at `host`:`port`: the one entered for
    /// that address.
    pub fn obs(&self, host: &str, port: u16) -> Option<&str> {
        let s = self.doc.obs.as_ref()?;
        (s.host.eq_ignore_ascii_case(host.trim()) && s.port == port).then_some(s.password.as_str())
    }

    /// A password is kept for OBS (at whichever address).
    pub fn has_obs(&self) -> bool {
        self.doc.obs.is_some()
    }

    /// Keep `password` for the OBS at `host`:`port` (empty: forget it).
    pub fn set_obs(&mut self, password: String, host: &str, port: u16) -> io::Result<()> {
        let host = host.trim().to_string();
        self.change(|doc| {
            doc.obs = (!password.is_empty()).then_some(ObsSecret {
                password,
                host,
                port,
            });
        })
    }

    /// Change the document and write it. It is read again first: the
    /// default profile's keys and OBS's password are one file, held twice.
    fn change(&mut self, f: impl FnOnce(&mut Doc)) -> io::Result<()> {
        self.doc = read(&self.at);
        f(&mut self.doc);
        let text = serde_json::to_string(&self.doc).map_err(io::Error::other)?;
        imp::write(&self.at, &text)
    }
}

#[cfg(not(any(target_os = "ios", target_arch = "wasm32")))]
mod imp {
    use super::*;
    use std::path::PathBuf;

    pub fn kept() -> &'static str {
        if cfg!(target_os = "android") {
            tr!("in the app's private storage")
        } else {
            tr!("in config/credentials.json, apart from config.toml")
        }
    }

    pub type Place = PathBuf;

    pub fn place(private: &std::path::Path) -> PathBuf {
        private.join("credentials.json")
    }

    pub fn read(p: &PathBuf) -> io::Result<Option<String>> {
        match std::fs::read_to_string(p) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Written whole beside it, then moved over it; on Unix the file is the
    /// user's alone from the start.
    pub fn write(p: &PathBuf, text: &str) -> io::Result<()> {
        use std::io::Write;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp~");
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            o.mode(0o600);
            // made earlier with other permissions
            if tmp.exists() {
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
        }
        let mut f = o.open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, p)
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::*;
    use security_framework::access_control::{ProtectionMode, SecAccessControl};
    use security_framework::passwords::{
        delete_generic_password, generic_password, set_generic_password_options, PasswordOptions,
    };

    pub fn kept() -> &'static str {
        tr!("in the iOS Keychain, on this device only")
    }

    const SERVICE: &str = "vibeBMS";
    const ACCOUNT: &str = "IR credentials";
    /// `errSecItemNotFound`
    const NOT_FOUND: i32 = -25300;

    /// One for the whole app.
    pub struct Place;

    pub fn place(_: &std::path::Path) -> Place {
        Place
    }

    pub fn read(_: &Place) -> io::Result<Option<String>> {
        match generic_password(PasswordOptions::new_generic_password(SERVICE, ACCOUNT)) {
            Ok(b) => String::from_utf8(b).map(Some).map_err(io::Error::other),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(io::Error::other(e)),
        }
    }

    pub fn write(_: &Place, text: &str) -> io::Result<()> {
        // an item's protection is set as it is added
        match delete_generic_password(SERVICE, ACCOUNT) {
            Err(e) if e.code() != NOT_FOUND => return Err(io::Error::other(e)),
            _ => {}
        }
        let mut o = PasswordOptions::new_generic_password(SERVICE, ACCOUNT);
        let mode = ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly;
        o.set_access_control(
            SecAccessControl::create_with_protection(Some(mode), 0).map_err(io::Error::other)?,
        );
        set_generic_password_options(text.as_bytes(), o).map_err(io::Error::other)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::*;

    pub fn kept() -> &'static str {
        tr!("in this browser's storage for the site: any script on the site can read it")
    }

    const KEY: &str = "vibebms-credentials";

    /// One for the whole app.
    pub struct Place;

    pub fn place(_: &std::path::Path) -> Place {
        Place
    }

    fn storage() -> io::Result<web_sys::Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "no storage"))
    }

    pub fn read(_: &Place) -> io::Result<Option<String>> {
        Ok(storage()?.get_item(KEY).ok().flatten())
    }

    pub fn write(_: &Place, text: &str) -> io::Result<()> {
        storage()?
            .set_item(KEY, text)
            .map_err(|_| io::Error::other("the browser's storage is full"))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vibebms-creds-{name}-{}", std::process::id()))
    }

    #[test]
    fn keys_are_kept_apart_and_private() {
        let root = temp("private");
        let paths = Paths::at(root.clone());
        let mut c = Keys::open(&paths);
        let (tachi, mine) = (ProviderId::Bokutachi, ProviderId::VibeJson(2));
        let at = |o: &str| o.to_string();
        assert!(c.get(tachi, "https://a:443").is_none());
        c.set(tachi, Some(("k1".into(), at("https://a:443"))))
            .unwrap();
        c.set(mine, Some(("k2".into(), at("https://b:443"))))
            .unwrap();
        c.set(mine, None).unwrap();
        let again = Keys::open(&paths);
        let k = again.get(tachi, "https://a:443").unwrap();
        assert_eq!(k.token.expose(), "k1");
        assert!(!again.has(mine));
        let file = root.join("config/credentials.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // a broken file: no keys
        std::fs::write(&file, "{").unwrap();
        assert!(!Keys::open(&paths).has(tachi));
        std::fs::remove_dir_all(&root).ok();
    }

    /// A key is given out for the server it was entered for and no other,
    /// whatever the account's address says now.
    #[test]
    fn a_key_goes_to_its_own_server_only() {
        let root = temp("bound");
        let paths = Paths::at(root.clone());
        let p = ProviderId::VibeJson(0);
        let mut c = Keys::open(&paths);
        c.set(p, Some(("k".into(), "https://ir.example:443".into())))
            .unwrap();
        let c = Keys::open(&paths);
        assert!(c.has(p));
        assert!(c.get(p, "https://ir.example:443").is_some());
        for other in [
            "https://evil.example:443",
            "https://ir.example:8443",
            "http://ir.example:443",
            "",
        ] {
            assert!(c.get(p, other).is_none(), "{other}");
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// The keys of a version before, kept without their server, get their
    /// account's once; one without an account goes, one whose account's
    /// address takes no key waits for one, and none is ever given out
    /// unbound.
    #[test]
    fn older_keys_get_their_server_once() {
        let root = temp("older");
        let paths = Paths::at(root.clone());
        let file = root.join("config/credentials.json");
        std::fs::write(
            &file,
            r#"{"keys":{"vibe-json:0":"k0","vibe-json:1":"k1","vibe-json:2":"k2"}}"#,
        )
        .unwrap();
        let (a, b, lan) = (
            ProviderId::VibeJson(0),
            ProviderId::VibeJson(1),
            ProviderId::VibeJson(2),
        );
        let mut c = Keys::open(&paths);
        assert!(c.has(a) && c.get(a, "https://ir.example:443").is_none());
        let accounts = |p| match p {
            p if p == a => Some(Some("https://ir.example:443".to_string())),
            p if p == lan => Some(origin("http://192.168.1.10:8080/")),
            _ => None,
        };
        c.bind(accounts).unwrap();
        assert_eq!(
            c.get(a, "https://ir.example:443").unwrap().token.expose(),
            "k0"
        );
        assert!(!c.has(b));
        // held, not sent anywhere, and kept from one start to the next
        for c in [&c, &Keys::open(&paths)] {
            assert!(c.has(lan) && c.held(lan) && !c.held(a));
            assert!(c.get(lan, "http://192.168.1.10:8080").is_none());
        }
        let mut c = Keys::open(&paths);
        c.bind(accounts).unwrap();
        assert!(c.held(lan));
        // bound for good: another address later does not get it; the held
        // one gets the address its account has now
        let other = "https://lan.example:443";
        c.bind(|_| Some(Some(other.into()))).unwrap();
        assert!(c.get(a, "https://ir.example:443").is_some());
        assert!(c.get(a, other).is_none());
        assert_eq!(c.get(lan, other).unwrap().token.expose(), "k2");
        assert!(!Keys::open(&paths).held(lan));
        std::fs::remove_dir_all(&root).ok();
    }

    /// A held key goes with its account.
    #[test]
    fn a_held_key_goes_with_its_account() {
        let root = temp("held");
        let paths = Paths::at(root.clone());
        let file = root.join("config/credentials.json");
        std::fs::write(&file, r#"{"keys":{"vibe-json:0":"k0"}}"#).unwrap();
        let p = ProviderId::VibeJson(0);
        let mut c = Keys::open(&paths);
        c.bind(|_| Some(None)).unwrap();
        assert!(c.held(p));
        c.set(p, None).unwrap();
        assert!(!Keys::open(&paths).has(p));
        std::fs::write(&file, r#"{"keys":{"vibe-json:0":"k0"}}"#).unwrap();
        let mut c = Keys::open(&paths);
        c.bind(|_| None).unwrap();
        assert!(!Keys::open(&paths).has(p));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn origins() {
        let o = |u: &str| origin(u);
        assert_eq!(
            o("https://IR.example/api/").as_deref(),
            Some("https://ir.example:443")
        );
        assert_eq!(
            o(" HTTPS://ir.example:8443?x#y").as_deref(),
            Some("https://ir.example:8443")
        );
        // plain http: this computer only
        assert_eq!(
            o("http://127.0.0.1:8080/api").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        assert_eq!(
            o("http://localhost").as_deref(),
            Some("http://localhost:80")
        );
        assert_eq!(o("http://[::1]:9/").as_deref(), Some("http://[::1]:9"));
        assert_eq!(
            o("https://[2001:db8::1]").as_deref(),
            Some("https://[2001:db8::1]:443")
        );
        for bad in [
            "http://ir.example/api",
            "http://127.0.0.1.evil.example/",
            "ftp://ir.example",
            "ir.example",
            "",
            // not plainly one host
            "https://user:key@ir.example/",
            "https://ir.example@evil.example/",
            "https://ir.example\\@evil.example/",
            "https://ir.example:443@evil.example",
            "https://ir.example:x/",
            "https://ir.example:/",
            "https://ir.example:99999/",
            "https://ir%2eexample/",
            "https://ir.exa mple/",
            "https://[::1/",
            "https:///api",
        ] {
            assert_eq!(o(bad), None, "{bad}");
        }
    }

    /// OBS's password is in the root's document beside the default
    /// profile's keys (neither store loses the other's), and is given out
    /// for the address it was entered for.
    #[cfg(desktop)]
    #[test]
    fn the_obs_password_is_kept_for_its_address() {
        let root = temp("obs");
        let paths = Paths::at(root.clone());
        let p = ProviderId::Bokutachi;
        let mut keys = Keys::open(&paths);
        let mut app = Keys::open_root(&paths.with_profile("b"));
        keys.set(p, Some(("k".into(), "https://a:443".into())))
            .unwrap();
        app.set_obs("pw".into(), " 127.0.0.1 ", 4455).unwrap();
        keys.set(ProviderId::VibeJson(0), None).unwrap();
        let (keys, app) = (Keys::open(&paths), Keys::open_root(&paths));
        assert!(keys.get(p, "https://a:443").is_some());
        assert_eq!(app.obs("127.0.0.1", 4455), Some("pw"));
        assert!(app.has_obs());
        assert_eq!(app.obs("192.168.0.9", 4455), None);
        assert_eq!(app.obs("127.0.0.1", 4456), None);
        let mut app = app;
        app.set_obs(String::new(), "127.0.0.1", 4455).unwrap();
        assert!(!Keys::open_root(&paths).has_obs());
        assert!(Keys::open(&paths).has(p));
        std::fs::remove_dir_all(&root).ok();
    }
}
