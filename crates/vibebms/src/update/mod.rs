//! Self-update of the desktop executables from the nightly release.
//!
//! CI builds are nightlies, `<version>-nightly.<run>+<commit>`
//! (`tools/ci_version.sh`); only they check (local builds are `-dev`,
//! releases have no channel yet). The nightly release has a `latest.json`
//! (docs/BUILDING.md) with a part per channel (`nightly`; `stable` is kept
//! for the releases to come):
//!
//! ```json
//! {
//!   "channels": {
//!     "nightly": {
//!       "version": "0.1.0-nightly.200+abc1234",
//!       "files": {
//!         "vibebms-linux-x64": { "version": "0.1.0-nightly.200+abc1234", "sha256": "…", "size": 31457280 }
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! Each file has the version it was built with: a build that failed keeps
//! its previous file, so a file can be older than the release. On start
//! (when "Check for updates" is on) it is read in the background; when this
//! build's file is from another commit and a later run, the select scene
//! says so and `SETTINGS › SYSTEM` offers to update. The update downloads
//! the file next to the executable, checks its size and SHA-256, and puts it
//! in the running executable's place (`self-replace`: on Windows a running
//! executable cannot be overwritten, but can be renamed); the macOS app
//! (`tools/package_macos.sh`) downloads the app's zip and puts the new app in
//! the bundle's place. "Restart now" then starts it with the same arguments.
//! An executable a package manager installed, or one in a folder that
//! cannot be written to, does nothing: it neither checks nor offers (its
//! package manager updates it).

#[cfg(desktop)]
mod desktop;

use crate::platform::Job;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// What the check reads.
pub const LATEST_URL: &str =
    "https://github.com/vibeBMS/vibe/releases/download/nightly/latest.json";
/// Where the nightly's files are downloaded from (`<base>/<asset>`).
pub const DOWNLOAD_BASE: &str = "https://github.com/vibeBMS/vibe/releases/download/nightly";

/// This build's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The platforms whose executables update themselves (not the web, iOS
/// or Android: their stores and pages update them).
pub const SUPPORTED: bool = cfg!(desktop);

/// A version: `<major>.<minor>.<patch>[-nightly.<run> | -dev][+<build>]`.
/// Nightlies are `0.1.0-nightly.192+e129ac9` (`tools/ci_version.sh`),
/// local builds `0.1.0-dev`, releases `0.1.0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// A nightly's CI run number (it only grows).
    pub nightly: Option<u64>,
    /// A local build (`-dev`).
    pub dev: bool,
    /// The build metadata (a nightly's commit), lower case.
    pub build: String,
}

impl Version {
    /// `s` as a version; `None` for anything else (another prerelease).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let (s, build) = match s.split_once('+') {
            Some((v, b)) => (v, Some(b)),
            None => (s, None),
        };
        let build = match build {
            Some(b)
                if b.is_empty()
                    || !b
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') =>
            {
                return None
            }
            b => b.unwrap_or_default().to_ascii_lowercase(),
        };
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        // a number: digits, no leading zero (as semver has it)
        let number = |n: &str| -> Option<u64> {
            let ok = !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && (n == "0" || !n.starts_with('0'));
            ok.then(|| n.parse().ok())?
        };
        let (nightly, dev) = match pre {
            None => (None, false),
            Some("dev") => (None, true),
            Some(p) => (Some(number(p.strip_prefix("nightly.")?)?), false),
        };
        let mut it = core.split('.');
        let mut num = || number(it.next()?);
        let (major, minor, patch) = (num()?, num()?, num()?);
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            nightly,
            dev,
            build,
        })
    }

    /// This build's version.
    pub fn current() -> Option<Version> {
        Version::parse(VERSION)
    }

    /// The commit it was built from (build metadata of 7 hex digits).
    pub fn commit(&self) -> Option<&str> {
        (self.build.len() == 7 && self.build.chars().all(|c| c.is_ascii_hexdigit()))
            .then_some(self.build.as_str())
    }

    /// A nightly (`-nightly.<run>+<commit>`): the builds that look for
    /// updates (local builds are `-dev`; releases have no channel yet).
    pub fn is_nightly(&self) -> bool {
        self.nightly.is_some() && self.commit().is_some()
    }

    /// Semver precedence: `-dev`, then `-nightly.<n>` (`dev` sorts
    /// before `nightly`), then the release.
    fn precedence(&self) -> (u64, u64, u64, u8, u64) {
        let rank = if self.dev {
            0
        } else if self.nightly.is_some() {
            1
        } else {
            2
        };
        let n = self.nightly.unwrap_or(0);
        (self.major, self.minor, self.patch, rank, n)
    }

    /// Whether `self` is an update for `current`: from another commit, and
    /// between nightlies a later run (the run decides, whatever version
    /// they are nightlies of); else later in semver order (a release is
    /// newer than its own nightlies).
    pub fn is_update_for(&self, current: &Version) -> bool {
        let other_commit = match (self.commit(), current.commit()) {
            (Some(a), Some(b)) => a != b,
            _ => true,
        };
        let later = match (self.nightly, current.nightly) {
            (Some(a), Some(b)) => a > b,
            _ => self.precedence() > current.precedence(),
        };
        other_commit && later
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(n) = self.nightly {
            write!(f, "-nightly.{n}")?;
        } else if self.dev {
            write!(f, "-dev")?;
        }
        if !self.build.is_empty() {
            write!(f, "+{}", self.build)?;
        }
        Ok(())
    }
}

/// The nightly release's file built for `os` / `arch` (`std::env::consts`
/// names); `browser`: built with the download browser (the Linux builds
/// without it are the `-portable` ones); `app`: the macOS app, whose zip
/// is the same for both architectures.
pub fn asset_for(os: &str, arch: &str, browser: bool, app: bool) -> Option<String> {
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    Some(match os {
        "linux" if browser => format!("vibebms-linux-{arch}"),
        "linux" => format!("vibebms-linux-{arch}-portable"),
        "macos" if app => "vibebms-macos-universal.app.zip".into(),
        "macos" => format!("vibebms-macos-{arch}"),
        "windows" => format!("vibebms-windows-{arch}.exe"),
        _ => return None,
    })
}

/// This build's file in the nightly release.
pub fn asset_name() -> Option<String> {
    asset_for(
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(feature = "browser"),
        exe_path().is_some_and(|e| app_bundle(&e).is_some()),
    )
}

/// The app `exe` is the executable of, by the macOS bundle layout
/// (`<name>.app/Contents/MacOS/<exe>`).
pub fn app_bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let ok = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    ok.then_some(app)
}

/// The app `exe` runs in, on macOS: the bundle is what an update replaces.
fn own_app(exe: &Path) -> Option<&Path> {
    app_bundle(exe).filter(|_| cfg!(target_os = "macos"))
}

/// One executable in `latest.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileEntry {
    /// The version it was built with.
    pub version: String,
    /// Its SHA-256, hexadecimal.
    pub sha256: String,
    /// Its size in bytes.
    pub size: u64,
}

/// `latest.json`: a part per channel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LatestFile {
    /// `nightly` (and, for the releases to come, `stable`).
    pub channels: BTreeMap<String, Latest>,
}

/// The channel nightlies read.
pub const NIGHTLY: &str = "nightly";

/// One channel of `latest.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Latest {
    /// The channel's version (its newest files').
    pub version: String,
    /// File name -> what it is.
    pub files: BTreeMap<String, FileEntry>,
}

impl Latest {
    /// The `channel` part of a `latest.json` (empty when it has none).
    pub fn parse(bytes: &[u8], channel: &str) -> Result<Latest, String> {
        let f: LatestFile =
            serde_json::from_slice(bytes).map_err(|e| format!("latest.json: {e}"))?;
        Ok(f.channels.get(channel).cloned().unwrap_or_default())
    }

    /// What it offers a build of `current` whose file is `asset`: its file
    /// when that is an update (and its entry is complete).
    pub fn offer(&self, asset: &str, current: &Version) -> Option<Offer> {
        let f = self.files.get(asset)?;
        let version = Version::parse(&f.version)?;
        let sha_ok = f.sha256.len() == 64 && f.sha256.chars().all(|c| c.is_ascii_hexdigit());
        if !sha_ok || f.size == 0 || !version.is_update_for(current) {
            return None;
        }
        Some(Offer {
            url: format!("{DOWNLOAD_BASE}/{asset}"),
            version,
            sha256: f.sha256.to_ascii_lowercase(),
            size: f.size,
        })
    }
}

/// An update found.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub url: String,
    pub version: Version,
    pub sha256: String,
    pub size: u64,
}

/// Bytes downloaded of an update.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    /// 0..=100.
    pub fn percent(&self) -> u64 {
        let total = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) * 100 / total).min(100)
    }
}

/// Where the updater is.
#[derive(Clone, Debug, Default)]
pub enum Status {
    /// Nothing checked (off, a local build, another platform).
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// The check failed (said in the settings only).
    CheckFailed(String),
    Available(Offer),
    Downloading(Offer, Arc<Progress>),
    /// In place: a restart starts it.
    Installed(Version),
    /// The update failed (the offer can be tried again).
    Failed(Offer, String),
}

enum Msg {
    Checked(Result<Option<Offer>, String>),
    Installed(Result<(), String>),
}

#[derive(Default)]
pub struct Updater {
    pub status: Status,
    /// "Update available" for the select scene to show (once).
    notice: Option<String>,
    /// The executable being replaced (Linux renames a replaced one's
    /// `/proc/self/exe` to "… (deleted)": it is found before).
    exe: Option<PathBuf>,
    job: Option<Job<Msg>>,
}

impl Updater {
    /// Check for an update in the background, if this build updates
    /// itself ([`self_updating`]) and `enabled` (the "Check for updates"
    /// setting).
    pub fn check(&mut self, enabled: bool) {
        if !enabled || self.job.is_some() || !self_updating() {
            return;
        }
        let (Some(current), Some(asset)) = (Version::current(), asset_name()) else {
            return;
        };
        self.status = Status::Checking;
        self.job = Some(Job::get(LATEST_URL, move |r| {
            Msg::Checked(
                r.and_then(|b| Latest::parse(&b, NIGHTLY))
                    .map(|l| l.offer(&asset, &current)),
            )
        }));
    }

    /// What finished. Returns what to tell the player (the update's end).
    pub fn poll(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        let Some(msg) = self.job.as_mut().and_then(Job::poll) else {
            return out;
        };
        self.job = None;
        match msg {
            Msg::Checked(Ok(Some(o))) => {
                log::info!("update available: {} ({})", o.version, o.url);
                self.notice = Some(tr!(
                    "Update available: {version} (SETTINGS › SYSTEM)",
                    version = o.version
                ));
                self.status = Status::Available(o);
            }
            Msg::Checked(Ok(None)) => {
                log::info!("update check: {VERSION} is the latest");
                self.status = Status::UpToDate;
            }
            Msg::Checked(Err(e)) => {
                log::warn!("update check failed: {e}");
                self.status = Status::CheckFailed(e);
            }
            Msg::Installed(r) => {
                let Status::Downloading(o, _) = std::mem::take(&mut self.status) else {
                    return out;
                };
                match r {
                    Ok(()) => {
                        out.push(tr!("Updated to {ver}: restart to use it", ver = o.version));
                        self.status = Status::Installed(o.version);
                    }
                    Err(e) => {
                        log::warn!("update failed: {e}");
                        out.push(tr!("The update failed: {e}", e = e));
                        self.status = Status::Failed(o, e);
                    }
                }
            }
        }
        out
    }

    /// The notice for the select scene, once.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// Download and put in place the update found. Returns what to tell
    /// the player.
    pub fn install(&mut self) -> Option<String> {
        let o = match &self.status {
            Status::Available(o) | Status::Failed(o, _) => o.clone(),
            _ => return None,
        };
        let Some(exe) = exe_path() else {
            return Some(tr!("The update failed: the executable's location is not known").into());
        };
        let progress = Arc::new(Progress::default());
        progress.total.store(o.size, Ordering::Relaxed);
        self.exe = Some(exe.clone());
        self.status = Status::Downloading(o.clone(), progress.clone());
        self.job = Some(Job::spawn("update", move |_| {
            Msg::Installed(desktop::install(&o, &exe, &progress))
        }));
        None
    }

    /// Start the new executable when this one exits (after "Restart now").
    pub fn relaunch_on_exit(&self) {
        if let (Status::Installed(_), Some(exe)) = (&self.status, &self.exe) {
            *RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(exe.clone());
        }
    }
}

/// The executable to start as this one exits.
static RELAUNCH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Start the updated executable, with this one's arguments, if
/// [`Updater::relaunch_on_exit`] asked for it (called as the app exits,
/// after the settings are saved).
pub fn relaunch_if_asked() {
    let exe = RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(exe) = exe {
        match std::process::Command::new(&exe)
            .args(std::env::args_os().skip(1))
            .spawn()
        {
            Ok(_) => log::info!("restarting: {}", exe.display()),
            Err(e) => log::error!("restart {}: {e}", exe.display()),
        }
    }
}

/// The running executable, its links followed.
pub fn exe_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
}

/// Why `exe` is a package manager's or system's (to be updated by it), if
/// it is.
pub fn managed_location(exe: &Path) -> Option<&'static str> {
    let s = exe.to_string_lossy().replace('\\', "/");
    // an app bundle other than vibeBMS's own (`tools/package_macos.sh`)
    let in_app = exe
        .components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"));
    if in_app && app_bundle(exe).is_none() {
        return Some("vibeBMS is inside an app bundle");
    }
    const UNIX: &[&str] = &[
        "/usr/",
        "/nix/store/",
        "/gnu/store/",
        "/snap/",
        "/var/lib/flatpak/",
        // inside a Flatpak (platform/flatpak): `flatpak install` updates it
        "/app/",
        "/opt/homebrew/",
        "/home/linuxbrew/",
    ];
    const MACOS: &[&str] = &[
        "/Cellar/",
        "/Caskroom/",
        "/Applications/Setapp/",
        "/Applications/MacPorts/",
    ];
    if UNIX.iter().any(|p| s.starts_with(p)) || MACOS.iter().any(|p| s.contains(p)) {
        return Some("vibeBMS was installed by a package manager");
    }
    if s.contains("/WindowsApps/") || s.contains("/scoop/apps/") {
        return Some("vibeBMS was installed by a package manager");
    }
    None
}

/// Whether Homebrew Cask installed vibeBMS (it moves the app to
/// /Applications and keeps its record in a Caskroom).
fn homebrew_cask() -> bool {
    ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"]
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("vibebms")
        })
}

/// Whether this build updates itself: a nightly, on the desktop, whose
/// executable is its own (not a package manager's) in a folder it can
/// write to. Others neither check nor offer. (Looked at once.)
pub fn self_updating() -> bool {
    static IT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *IT.get_or_init(|| {
        SUPPORTED
            && Version::current().is_some_and(|v| v.is_nightly())
            && asset_name().is_some()
            && exe_path().is_some_and(|e| install_refusal(&e).is_none())
    })
}

/// Why the executable at `exe` is not replaced in place, if it is not.
pub fn install_refusal(exe: &Path) -> Option<String> {
    if let Some(why) = managed_location(exe) {
        return Some(why.into());
    }
    let app = own_app(exe);
    if app.is_some() && homebrew_cask() {
        return Some("vibeBMS was installed by Homebrew".into());
    }
    // the app is replaced whole, in the folder it is in
    match app.unwrap_or(exe).parent() {
        Some(dir) if crate::platform::vfs::writable(dir) => None,
        _ => Some("vibeBMS's folder cannot be written to".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_parse() {
        assert_eq!(
            Version::parse("0.1.0-nightly.192+e129ac9"),
            Some(Version {
                major: 0,
                minor: 1,
                patch: 0,
                nightly: Some(192),
                dev: false,
                build: "e129ac9".into()
            })
        );
        // this build's own (Cargo.toml's `-dev`, or a nightly's)
        assert!(Version::current().is_some(), "{VERSION}");
        let n = v(" 1.2.3-nightly.7+ABCDEF0\n");
        assert_eq!(n.commit(), Some("abcdef0"));
        assert!(n.is_nightly());
        for s in [
            "0.1.0-nightly.192+e129ac9",
            "0.1.0-dev",
            "0.1.0",
            "10.20.30+abc1234",
        ] {
            assert_eq!(v(s).to_string(), s);
        }
        // local builds and releases are not nightlies
        let dev = v("0.1.0-dev");
        assert!(dev.dev);
        // (even with a commit as its metadata)
        assert!(!v("0.1.0-dev+abc1234").is_nightly());
        assert_eq!(
            (dev.nightly, dev.commit(), dev.is_nightly()),
            (None, None, false)
        );
        let rel = v("0.1.0");
        assert_eq!(
            (rel.nightly, rel.commit(), rel.is_nightly()),
            (None, None, false)
        );
        // a nightly without its commit, or with something else
        assert!(!v("0.1.0-nightly.5").is_nightly());
        assert!(!v("0.1.0-nightly.5+dev").is_nightly());
        assert!(!v("0.1.0-nightly.5+abc12345").is_nightly());
        // nonsense, other prereleases, the old scheme's local build is fine
        for s in [
            "",
            "0.1",
            "0.1.2.3",
            "0.1.x",
            "01.1.0",
            "0.1.0+",
            "0.1.0+a b",
            "0.1.0-beta.1",
            "0.1.0-dev.1",
            "0.1.0-nightly.5-dev",
            "0.1.0-nightly",
            "0.1.0-nightly.",
            "0.1.0-nightly.x+abc1234",
            "0.1.0-nightly.+5+abc1234",
        ] {
            assert_eq!(Version::parse(s), None, "{s}");
        }
    }

    #[test]
    fn updates_are_later_runs_of_other_commits() {
        let cur = v("0.1.0-nightly.192+e129ac9");
        assert!(v("0.1.0-nightly.200+abc1234").is_update_for(&cur));
        // the same commit (built again), an older or the same run
        assert!(!v("0.1.0-nightly.200+e129ac9").is_update_for(&cur));
        assert!(!v("0.1.0-nightly.191+abc1234").is_update_for(&cur));
        assert!(!v("0.1.0-nightly.192+abc1234").is_update_for(&cur));
        // between nightlies the run decides, not the version
        assert!(v("0.2.0-nightly.193+abc1234").is_update_for(&cur));
        assert!(!v("0.2.0-nightly.190+abc1234").is_update_for(&cur));
        // a release is newer than its own nightlies, older than the next's
        assert!(v("0.1.0").is_update_for(&cur));
        assert!(v("0.1.0+abc1234").is_update_for(&cur));
        assert!(!v("0.1.0+e129ac9").is_update_for(&cur));
        assert!(!v("0.0.9").is_update_for(&cur));
        let rel = v("0.1.0");
        assert!(!v("0.1.0-nightly.300+abc1234").is_update_for(&rel));
        assert!(v("0.2.0-nightly.300+abc1234").is_update_for(&rel));
        assert!(v("0.1.1").is_update_for(&rel));
        assert!(!v("0.1.0").is_update_for(&rel));
        // a local build is older than the nightlies and the release of
        // its version (it never checks, but the order holds)
        let dev = v("0.1.0-dev");
        assert!(v("0.1.0-nightly.1+abc1234").is_update_for(&dev));
        assert!(v("0.1.0").is_update_for(&dev));
        assert!(!v("0.0.9").is_update_for(&dev));
        assert!(!dev.is_update_for(&cur));
    }

    #[test]
    fn asset_names() {
        let a = |os, arch, b| asset_for(os, arch, b, false);
        assert_eq!(
            a("linux", "x86_64", true).as_deref(),
            Some("vibebms-linux-x64")
        );
        assert_eq!(
            a("linux", "aarch64", true).as_deref(),
            Some("vibebms-linux-arm64")
        );
        assert_eq!(
            a("linux", "x86_64", false).as_deref(),
            Some("vibebms-linux-x64-portable")
        );
        assert_eq!(
            a("linux", "aarch64", false).as_deref(),
            Some("vibebms-linux-arm64-portable")
        );
        assert_eq!(
            a("macos", "aarch64", true).as_deref(),
            Some("vibebms-macos-arm64")
        );
        assert_eq!(
            a("macos", "x86_64", true).as_deref(),
            Some("vibebms-macos-x64")
        );
        assert_eq!(
            a("windows", "x86_64", true).as_deref(),
            Some("vibebms-windows-x64.exe")
        );
        assert_eq!(
            a("windows", "aarch64", false).as_deref(),
            Some("vibebms-windows-arm64.exe")
        );
        // the macOS app: one zip for both
        for arch in ["aarch64", "x86_64"] {
            assert_eq!(
                asset_for("macos", arch, true, true).as_deref(),
                Some("vibebms-macos-universal.app.zip")
            );
        }
        assert_eq!(
            asset_for("linux", "x86_64", true, true).as_deref(),
            Some("vibebms-linux-x64")
        );
        assert_eq!(a("android", "aarch64", true), None);
        assert_eq!(a("ios", "aarch64", true), None);
        assert_eq!(a("linux", "riscv64", true), None);
        // this build's
        assert!(asset_name().is_some());
    }

    const LATEST: &str = r#"{ "channels": {
      "stable": { "version": "0.1.0", "files": {} },
      "nightly": {
      "version": "0.1.0-nightly.200+abc1234",
      "files": {
        "vibebms-linux-x64": {
          "version": "0.1.0-nightly.200+abc1234",
          "sha256": "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08",
          "size": 4
        },
        "vibebms-windows-x64.exe": {
          "version": "0.1.0-nightly.180+0123456",
          "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
          "size": 4
        },
        "vibebms-macos-arm64": { "version": "0.1.0-nightly.200+abc1234", "sha256": "xyz", "size": 4 },
        "vibebms-macos-x64": { "version": "0.1.0-nightly.201+abc1234", "sha256": "", "size": 0 },
        "vibebms-linux-arm64-portable": { "version": "0.1.0-beta", "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08", "size": 4 }
      },
      "something": "new"
    } } }"#;

    #[test]
    fn latest_json() {
        let l = Latest::parse(LATEST.as_bytes(), NIGHTLY).unwrap();
        assert_eq!(l.version, "0.1.0-nightly.200+abc1234");
        assert_eq!(l.files.len(), 5);
        let cur = v("0.1.0-nightly.192+e129ac9");
        let o = l.offer("vibebms-linux-x64", &cur).unwrap();
        assert_eq!(o.version, v("0.1.0-nightly.200+abc1234"));
        assert_eq!(o.size, 4);
        assert_eq!(
            o.sha256,
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
        assert_eq!(
            o.url,
            "https://github.com/vibeBMS/vibe/releases/download/nightly/vibebms-linux-x64"
        );
        // its file is older than this build (a failed build kept it)
        assert_eq!(l.offer("vibebms-windows-x64.exe", &cur), None);
        // an entry that is not complete or readable, and a file it does
        // not list
        assert_eq!(l.offer("vibebms-macos-arm64", &cur), None);
        assert_eq!(l.offer("vibebms-linux-arm64-portable", &cur), None);
        assert_eq!(l.offer("vibebms-macos-x64", &cur), None);
        assert_eq!(l.offer("vibebms-linux-arm64", &cur), None);
        // already this commit
        assert_eq!(
            l.offer("vibebms-linux-x64", &v("0.1.0-nightly.200+abc1234")),
            None
        );
        // the other channels are not read
        let stable = Latest::parse(LATEST.as_bytes(), "stable").unwrap();
        assert_eq!(stable.version, "0.1.0");
        assert_eq!(stable.offer("vibebms-linux-x64", &cur), None);
        assert!(Latest::parse(b"not json", NIGHTLY).is_err());
        assert_eq!(Latest::parse(b"{}", NIGHTLY).unwrap(), Latest::default());
    }

    #[test]
    fn package_manager_locations() {
        for p in [
            "/usr/bin/vibebms",
            "/usr/local/bin/vibebms",
            "/nix/store/abc-vibebms/bin/vibebms",
            "/snap/vibebms/1/vibebms",
            "/app/bin/vibebms",
            "/opt/homebrew/bin/vibebms",
            "/usr/local/Cellar/vibebms/1/bin/vibebms",
            "/opt/homebrew/Caskroom/vibebms/1/vibeBMS.app/Contents/MacOS/vibebms",
            "/Users/me/Caskroom/vibebms/vibeBMS.app/Contents/MacOS/vibebms",
            "/Applications/Setapp/vibeBMS.app/Contents/MacOS/vibebms",
            "/Applications/Other.app/Contents/Resources/vibebms",
            "/Applications/Other.app/Contents/MacOS/bin/vibebms",
            "C:\\Program Files\\WindowsApps\\vibeBMS\\vibebms.exe",
        ] {
            assert!(managed_location(Path::new(p)).is_some(), "{p}");
        }
        for p in [
            "/home/me/games/vibeBMS/vibebms",
            "/home/me/app/vibebms",
            "/opt/vibeBMS/vibebms",
            "/Applications/vibeBMS.app/Contents/MacOS/vibebms",
            "/Users/me/Games/vibeBMS Nightly.app/Contents/MacOS/vibebms",
            "C:\\Games\\vibeBMS\\vibebms.exe",
        ] {
            assert_eq!(managed_location(Path::new(p)), None, "{p}");
        }
    }

    #[test]
    fn app_bundles() {
        let exe = Path::new("/Applications/vibeBMS Nightly.app/Contents/MacOS/vibebms");
        assert_eq!(
            app_bundle(exe),
            Some(Path::new("/Applications/vibeBMS Nightly.app"))
        );
        for p in [
            "/Applications/vibeBMS.app/Contents/Resources/vibebms",
            "/Applications/vibeBMS/Contents/MacOS/vibebms",
            "/home/me/vibeBMS/vibebms",
            "vibebms",
        ] {
            assert_eq!(app_bundle(Path::new(p)), None, "{p}");
        }
    }

    #[test]
    fn unwritable_folders_are_refused() {
        let dir = std::env::temp_dir().join(format!("vibebms-update-w-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(install_refusal(&dir.join("vibebms")), None);
        let gone = dir.join("missing").join("vibebms");
        assert!(install_refusal(&gone).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Other platforms do not update themselves ([`SUPPORTED`]: no update is
/// ever offered there).
#[cfg(not(desktop))]
mod desktop {
    use super::{Offer, Progress};
    use std::{path::Path, sync::Arc};

    pub fn install(_: &Offer, _: &Path, _: &Arc<Progress>) -> Result<(), String> {
        Err("this platform does not update itself".into())
    }
}
