//! The keys of IR accounts (API keys, tokens), kept apart from
//! `config.toml` so that a configuration shared or attached to a bug report
//! carries none. All of them are one small JSON document, kept where the
//! platform keeps such things:
//! - desktop: `config/credentials.json`, readable by the user alone on
//!   Unix (the install is portable, and a keychain may be missing or ask
//!   again after every unsigned update);
//! - Android: the app's private storage (the data folder is the shared
//!   external one);
//! - iOS: the Keychain, this device only (the data folder is `Documents`,
//!   seen in Files and in backups);
//! - web: the page's `localStorage`, which any script of the site can read.
//!
//! Nothing here logs or formats a key.

use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io;
use vibebms_ir::provider::Creds;
use vibebms_ir::{ProviderId, Secret};

#[derive(Default, Serialize, Deserialize)]
struct Doc {
    keys: BTreeMap<ProviderId, String>,
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

impl Keys {
    pub fn open(paths: &Paths) -> Keys {
        let at = imp::place(paths);
        let doc = match imp::read(&at) {
            Ok(Some(t)) => serde_json::from_str(&t).unwrap_or_else(|e| {
                log::warn!("credentials: {e}");
                Doc::default()
            }),
            Ok(None) => Doc::default(),
            Err(e) => {
                log::warn!("credentials: {e}");
                Doc::default()
            }
        };
        Keys { at, doc }
    }

    /// The credentials of `p`'s account, for its requests.
    pub fn get(&self, p: ProviderId) -> Option<Creds> {
        let k = self.doc.keys.get(&p)?;
        Some(Creds::token(Secret::new(k.as_str())))
    }

    pub fn has(&self, p: ProviderId) -> bool {
        self.doc.keys.contains_key(&p)
    }

    /// Keep `key` for `p`'s account (`None`: forget it).
    pub fn set(&mut self, p: ProviderId, key: Option<String>) -> io::Result<()> {
        match key {
            Some(k) => self.doc.keys.insert(p, k),
            None => self.doc.keys.remove(&p),
        };
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

    pub fn place(paths: &Paths) -> PathBuf {
        paths.private.join("credentials.json")
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

    pub fn place(_: &Paths) -> Place {
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

    pub fn place(_: &Paths) -> Place {
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

    #[test]
    fn keys_are_kept_apart_and_private() {
        let root = std::env::temp_dir().join(format!("vibebms-creds-{}", std::process::id()));
        let paths = Paths::at(root.clone());
        let mut c = Keys::open(&paths);
        let (tachi, mine) = (ProviderId::Bokutachi, ProviderId::VibeJson(2));
        assert!(c.get(tachi).is_none());
        c.set(tachi, Some("k1".into())).unwrap();
        c.set(mine, Some("k2".into())).unwrap();
        c.set(mine, None).unwrap();
        let again = Keys::open(&paths);
        assert_eq!(again.get(tachi).unwrap().token.expose(), "k1");
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
}
