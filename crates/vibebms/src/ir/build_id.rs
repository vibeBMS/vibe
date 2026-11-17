//! Which build this is, as an IR asks. IR operators such as BMS-IR and
//! rianIR accept a client by the SHA-256 of its executable file, so CI
//! publishes every build's hashes in the nightly release's `builds.json`
//! (docs/BUILDING.md), and the executable hashes itself to be found there.
//! A build CI did not make (a local or a distribution's build) is not in
//! it. The web, Android and iOS builds have no executable file of their
//! own to hash (docs/BUILDING.md says why).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// The manifest of every published build.
pub const BUILDS_URL: &str =
    "https://github.com/vibeBMS/vibe/releases/download/nightly/builds.json";

/// `builds.json`: the builds CI published, the oldest first.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Builds {
    pub builds: Vec<Build>,
}

/// One CI run's files.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Build {
    /// `CARGO_PKG_VERSION` of its files (`0.1.0-nightly.200+abc1234`).
    pub version: String,
    /// The full commit it was built from.
    pub commit: String,
    /// When it was published (UTC, `2026-11-17T12:34:56Z`).
    pub date: String,
    /// The release's file name (`vibebms-linux-x64`; a file inside a
    /// package after it: `vibeBMS.apk/lib/arm64-v8a/libvibebms_android.so`)
    /// -> what it is.
    pub files: BTreeMap<String, BuildFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BuildFile {
    /// Its SHA-256, hexadecimal.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
}

impl Builds {
    pub fn parse(bytes: &[u8]) -> Result<Builds, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("builds.json: {e}"))
    }

    /// Whether a file whose SHA-256 is `sha256` was published as
    /// `version`: the build is CI's own.
    pub fn official(&self, version: &str, sha256: &str) -> bool {
        self.builds
            .iter()
            .filter(|b| b.version == version)
            .flat_map(|b| b.files.values())
            .any(|f| f.sha256.eq_ignore_ascii_case(sha256))
    }
}

/// The SHA-256 of the running executable (lower-case hexadecimal), on the
/// desktop; `None` elsewhere or when it cannot be read. Hashed on the
/// first call (tens of milliseconds: call it in the background, early: an
/// update can replace the file while the old build runs on), then kept.
pub fn exe_sha256() -> Option<&'static str> {
    static HASH: OnceLock<Option<String>> = OnceLock::new();
    HASH.get_or_init(|| {
        if !cfg!(desktop) {
            return None;
        }
        // Linux keeps the running file here even when an update replaced it
        let exe = if cfg!(target_os = "linux") {
            "/proc/self/exe".into()
        } else {
            std::env::current_exe().ok()?
        };
        file_sha256(&exe)
            .map_err(|e| log::warn!("hash of {}: {e}", exe.display()))
            .ok()
    })
    .as_deref()
}

/// The SHA-256 of the file `p`, lower-case hexadecimal.
pub fn file_sha256(p: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    io::copy(&mut std::fs::File::open(p)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As the nightly job writes it (docs/BUILDING.md).
    const BUILDS: &str = r#"{
      "v": 1,
      "builds": [
        {
          "version": "0.1.0-nightly.199+0123456",
          "commit": "0123456789abcdef0123456789abcdef01234567",
          "date": "2026-11-16T09:00:00Z",
          "files": {
            "vibebms-linux-x64": { "sha256": "1111111111111111111111111111111111111111111111111111111111111111", "size": 4 }
          }
        },
        {
          "version": "0.1.0-nightly.200+abc1234",
          "commit": "abc1234abc1234abc1234abc1234abc1234abc12",
          "date": "2026-11-17T12:34:56Z",
          "files": {
            "vibebms-linux-x64": { "sha256": "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08", "size": 4 },
            "vibebms-windows-x64.exe": { "sha256": "2222222222222222222222222222222222222222222222222222222222222222", "size": 4 },
            "vibebms-macos-universal.app.zip/vibeBMS Nightly.app/Contents/MacOS/vibebms": { "sha256": "3333333333333333333333333333333333333333333333333333333333333333", "size": 8 },
            "vibeBMS.apk/lib/arm64-v8a/libvibebms_android.so": { "sha256": "4444444444444444444444444444444444444444444444444444444444444444", "size": 8 }
          },
          "something": "new"
        }
      ]
    }"#;

    #[test]
    fn builds_json() {
        let b = Builds::parse(BUILDS.as_bytes()).unwrap();
        assert_eq!(b.builds.len(), 2);
        let last = &b.builds[1];
        assert_eq!(last.commit, "abc1234abc1234abc1234abc1234abc1234abc12");
        assert_eq!(last.date, "2026-11-17T12:34:56Z");
        assert_eq!(last.files.len(), 4);
        assert_eq!(last.files["vibebms-windows-x64.exe"].size, 4);
        let test = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert!(b.official("0.1.0-nightly.200+abc1234", test));
        // another build's file, another version's, a hash it does not list
        assert!(!b.official("0.1.0-nightly.199+0123456", test));
        assert!(!b.official("0.1.0-dev", test));
        assert!(!b.official("0.1.0-nightly.200+abc1234", &"5".repeat(64)));
        assert!(Builds::parse(b"not json").is_err());
        assert_eq!(Builds::parse(b"{}").unwrap(), Builds::default());
    }

    #[test]
    fn a_file_is_hashed() {
        let p = std::env::temp_dir().join(format!("vibebms-hash-{}", std::process::id()));
        std::fs::write(&p, b"test").unwrap();
        assert_eq!(
            file_sha256(&p).unwrap(),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
        std::fs::remove_file(&p).ok();
        assert!(file_sha256(&p).is_err());
        // this test program is the running executable
        if cfg!(desktop) {
            let exe = std::env::current_exe().unwrap();
            assert_eq!(exe_sha256(), Some(file_sha256(&exe).unwrap().as_str()));
        }
    }
}
