# Building vibeBMS

How to build vibeBMS for each platform ("Build"), and how CI packages, publishes and reproduces the release builds
("Release engineering", for maintainers).

Requires Rust (`rustup`): `rust-toolchain.toml` names the exact release every build uses (rustup installs it on
the first `cargo` in the folder; add `rustup component add rustfmt clippy` for those). Release builds use fat
LTO; expect a few minutes.

```sh
cargo build --release            # target/release/vibebms(.exe)
cargo test --workspace           # engine/parser/gauge/etc. tests
```

Copy the executable anywhere; it keeps all its data next to itself ([MANUAL.md](MANUAL.md), "Data folder").

# Build

## Linux
Packages: `libasound2-dev libudev-dev libwebkit2gtk-4.1-dev` (Debian/Ubuntu) and a Vulkan or GL driver.
Runtime also loads `libxkbcommon-x11` (X11) or Wayland libraries.
WebKitGTK is for the in-app browser that downloads songs; without it, build with
`--no-default-features` (download pages then open in the system browser).

The release builds: `vibebms-linux-x64` / `-arm64` are built in an Ubuntu 22.04 container (glibc 2.35, WebKitGTK 4.1);
`vibebms-linux-x64-portable` / `-arm64-portable` are built without the browser in a `manylinux_2_28` (AlmaLinux 8)
container, so they need only glibc 2.28, ALSA and libudev. WebKitGTK cannot be loaded at run time instead (wry
links it), hence the two builds.
Optional JACK output: `cargo build --release --features jack` (needs `libjack-dev`).

## Windows (x64 / ARM64)
MSVC toolchain. ARM64: `rustup target add aarch64-pc-windows-msvc` and
`cargo build --release --target aarch64-pc-windows-msvc` (install the ARM64 build tools in Visual Studio).
ASIO output: install the ASIO SDK, set `CPAL_ASIO_DIR`, build with `--features asio`. The ASIO SDK is under
Steinberg's own licence (proprietary or GPLv3): builds with `asio` can't be redistributed under MIT/Apache without
Steinberg's licence agreement, so release builds leave it out.

## macOS (Apple Silicon / Intel)
`cargo build --release` (native) or `--target x86_64-apple-darwin` / `aarch64-apple-darwin`.
The app: build both, then `tools/package_macos.sh <arm64 vibebms> <x86_64 vibebms> <out dir>` makes
`vibeBMS Dev.app` (one universal executable from `lipo`, the icon `platform/macos/vibeBMS.icns`, signed ad hoc),
`vibeBMS-macos-universal.dmg` (the app and a link to Applications) and `vibebms-macos-universal.app.zip` (what the
self-update downloads). CI does the same for the nightly (`vibeBMS Nightly.app`, `io.github.vibebms.nightly`; the
identity comes from `tools/ci_version.sh`). The app is not notarized: its first launch needs right-click → Open, or
System Settings → Privacy & Security → Open Anyway.

A bare executable keeps its data next to itself. The app keeps it in the folder that contains the `.app`, except in an
`Applications` folder or where that folder cannot be written to (a disk image), where it uses
`~/Library/Application Support/vibeBMS`.

## Android
The APK is a Gradle project (`platform/android`) around the Rust library, which
`cargo-ndk` builds into it. Needs the Android SDK (platform 34) with the NDK, and JDK 17+.
```sh
cargo install cargo-ndk               # CI's versions of it, the NDK, JDK and Gradle: tools/repro.env
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o platform/android/app/jniLibs build --release -p vibebms-android
gradle -p platform/android assembleRelease    # Gradle 9.6+; or open platform/android in Android Studio
```
The release APK is signed with `VIBEBMS_KEYSTORE` / `VIBEBMS_KEYSTORE_PASSWORD` / `VIBEBMS_KEY_ALIAS`
when set, else with the debug key. Output: `platform/android/app/build/outputs/apk/release/`.
Data root: `Android/data/io.github.vibebms/files` (copy BMS folders there over USB); the nightly APK is its own app,
`io.github.vibebms.nightly`, with its data in `Android/data/io.github.vibebms.nightly/files`.
Touch controls turn on automatically. Songs a difficulty table lists can be downloaded in the
app's browser (`Browser.java`), as on the desktop.

## iOS / iPadOS
```sh
rustup target add aarch64-apple-ios
IPHONEOS_DEPLOYMENT_TARGET=14.0 cargo build --release -p vibebms --target aarch64-apple-ios
mkdir -p Payload/vibeBMS.app
cp target/aarch64-apple-ios/release/vibebms platform/ios/Info.plist platform/ios/*.png Payload/vibeBMS.app/
zip -qr vibeBMS-unsigned.ipa Payload
```
To install it, sign it with your own Apple ID (AltStore, Sideloadly; a free Apple ID's signature lasts 7 days) or a
developer certificate (`codesign`, then Xcode's Devices window or `ios-deploy`). The app's `Documents` folder is the
data root and is visible in the Files app (`UIFileSharingEnabled`), so BMS folders can be copied in.

## Web (wasm32)
```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
./tools/build_web.sh                 # -> web/pkg
python3 -m http.server -d web 8080   # open http://localhost:8080
```
The web build compiles Lua (for Lua skins) from C, so it also needs **clang with the WebAssembly target** and
`llvm-ar` (Debian/Ubuntu: `clang llvm`; macOS: Apple's clang has no WebAssembly target, use Homebrew's `llvm` and
`export CC_wasm32_unknown_unknown=$(brew --prefix llvm)/bin/clang AR_wasm32_unknown_unknown=$(brew --prefix
llvm)/bin/llvm-ar`). Lua raises errors as WebAssembly exceptions, so `wasm-opt`, if installed, runs with
`--enable-exception-handling`, and the page needs a browser with them (Chrome 95, Firefox 100, Safari 15.2 or
later). `build_web.sh` ends by checking (with Node, if installed) that the module imports no C function nothing
defined (`node tools/check_wasm_imports.mjs`). The release's web build is `tools/build_release.sh web`: the same
without wasm-opt, plus the CJK fonts (see Reproducible builds).

- Fonts: browsers give a page no system fonts, so CI puts Noto Sans CJK (JP/KR, SIL OFL) into `web/fonts/`, and SC /
  TC loaded only while the UI is in Simplified / Traditional Chinese (a line `file lang` in `fonts.txt`). When
  building yourself, add CJK fonts there and list them in `fonts.txt`.
- Third-party sources: `VIBEBMS_THIRD_PARTY=1` at build time (the repository variable `WEB_THIRD_PARTY=1` in CI)
  turns them on by default in that build; the page then says once, at its first start, what they are (OK keeps them
  on, Turn off turns them off).
- CI publishes a web build to the `gh-pages` branch on every push (the default branch at the site's root, other
  branches under `preview/<branch>/`). Turn on GitHub Pages (Settings › Pages › `gh-pages` / root) to open it.

# Development

- While working on the code, `--no-default-features` (e.g. `cargo test --workspace --no-default-features`) leaves
  out the in-app browser (wry and, on Linux, the GTK and WebKitGTK bindings), which a clean build spends a good part
  of its time on. Debug builds keep line tables only (`[profile.dev]` in `Cargo.toml`): set `debug = true` there to
  see variables in a debugger.
- Translations: text is looked up by its English in `crates/vibebms/assets/i18n/<lang>.toml` (embedded). `en.toml`
  lists every text the code has, kept so by a test; `I18N_BLESS=1 cargo test -p vibebms i18n` rewrites it.
  `python3 tools/i18n_hant.py <zh-Hans.toml> <zh-Hant.toml>` makes the Traditional Chinese table from the Simplified
  one.
- Built-in skins: `tools/gen_default_skins.py` draws them and writes `crates/vibebms/assets/builtin/skins/default/`.
  Edit the script and run it to make them again; CI checks with `--check` that the committed files are what it
  makes. The committed files are made with Pillow 12.3.0 and Inter 4.0, as CI's check uses: other versions draw other
  pixels.
- The logo and icons: `tools/gen_logo.py` (fonttools, cairosvg, Pillow) writes `assets/logo/` and the platforms'
  icons.

# Release engineering

## Versions
`Cargo.toml` names the version being worked towards as a prerelease: `0.1.0-dev` is what local builds report.
CI builds are nightlies of it: `tools/ci_version.sh` rewrites it to `<version>-nightly.<run>+<commit>`,
e.g. `0.1.0-nightly.192+e129ac9`, so `CARGO_PKG_VERSION` names the build. The run number only grows:
Android's `versionCode` and the iOS `CFBundleVersion` are that number (iOS's `CFBundleShortVersionString`
is the plain `0.1.0`, as it takes numbers only). After a release, Cargo.toml moves on to the next version (e.g. `0.1.1-dev`).

Builds belong to a channel (`VIBEBMS_CHANNEL`, `nightly`; `preview`, `beta` or `stable` could follow): the
version says it (`-<channel>.<run>`; a stable build is the plain version), and on Android and iOS each channel is its
own app, `io.github.vibebms.<channel>` named "vibeBMS <Channel>" (stable: `io.github.vibebms`, "vibeBMS"), so they
install side by side. `latest.json` and the AltStore source have a part per channel.

## Flatpak
`tools/package_flatpak.sh <vibebms> <out dir>` packages a `browser` build of the machine's architecture as it is
into `<out dir>/vibeBMS-linux-x64.flatpak` (or `-arm64`; a bundle holds one architecture, so CI makes one per
architecture). Nothing is compiled, so it uses neither `flatpak-builder` nor the SDK. It:

1. fills in the channel's app id, name and version (`tools/ci_version.sh`; a local run is `io.github.vibebms.dev`,
   "vibeBMS Dev", on the branch `dev`) in the manifest, desktop entry and AppStream data of `platform/flatpak`, with
   the icons from `assets/logo`;
2. checks the desktop entry and AppStream data (`desktop-file-validate` when installed, `appstreamcli`);
3. lays out the app directory itself: the executable in `/app/bin`, the desktop entry, metainfo and icons under
   `/app/share`, and the `metadata` with the runtime and command from `platform/flatpak/vibebms.yml`, which keeps
   flatpak-builder's manifest format;
4. composes the AppStream catalog and its cached icons with `appstreamcli compose` as flatpak-builder does (the
   bundle carries them for software centres);
5. adds the manifest's `finish-args` with `flatpak build-finish`;
6. exports to a local repository with `flatpak build-export` and bundles with
   `flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo`, so `flatpak install` of the
   bundle fetches the runtime from Flathub.

It needs `flatpak` and `appstreamcli` with its compose plugin (`appstream-compose` on Debian and Ubuntu); neither the
runtime nor the SDK has to be installed. `DRY_RUN=1` stops before the build, with the filled-in sources in
`<out dir>/flatpak-src`.

The runtime is `org.gnome.Platform` (51) because it has WebKitGTK 4.1 (`libwebkit2gtk-4.1`), which the in-app
browser links; the Freedesktop runtime has no WebKitGTK. Its permissions: network, Wayland or X11 (and IPC), the GPU,
PulseAudio, all devices (game controllers) and the home folder (BMS folders added in `SETTINGS › LIBRARY`). CI builds
the nightly's (`io.github.vibebms.nightly`, "vibeBMS Nightly", branch `nightly`) from `vibebms-linux-x64` and `-arm64`
as built, in a job per architecture on the Ubuntu 22.04 runner (so the script keeps to flatpak 1.12 and AppStream
0.15.2), installs and caches the runtime alone (without its GL drivers, codecs and translations), checks that
the bundle installs and that every library the executable links resolves in the runtime, and adds the bundle to the
nightly release. Install it with `flatpak install --user vibeBMS-linux-x64.flatpak` (or `-arm64`); a
newer bundle installs over it the same way (it does not update itself: the self-update stays off under `/app`).
Inside the Flatpak the executable's folder (`/app/bin`) is read-only, so the data root is `$XDG_DATA_HOME/vibeBMS`,
which Flatpak makes `~/.var/app/<app id>/data/vibeBMS`. Video BGA needs an `ffmpeg` executable, which the runtime does
not have.

## Android signing
CI signs the release APK with the repository's key when the secrets `ANDROID_KEYSTORE` (the keystore, base64) and
`ANDROID_KEYSTORE_PASSWORD` (its password, also the key's) are set, else with a key made for the run, which installs
but cannot update an APK signed by another. Make the keystore once and keep it: an APK signed by a new key cannot
update the installed app (it has to be uninstalled, with its data).
```sh
keytool -genkeypair -keystore vibebms-android.p12 -storetype PKCS12 -alias vibebms \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=vibeBMS"
base64 -w0 vibebms-android.p12    # the value of ANDROID_KEYSTORE (macOS: base64 -i vibebms-android.p12)
```

## F-Droid repository
For users: add `https://raw.githubusercontent.com/vibeBMS/vibe/fdroid-repo/repo` once in
[F-Droid](https://f-droid.org/), [Droid-ify](https://github.com/Droid-ify/client) or
[Neo Store](https://github.com/NeoApplications/Neo-Store) (in its repositories settings), with the fingerprint of
its key so the client checks that the index is the project's, and each new nightly is offered as an update of
"vibeBMS Nightly" (`io.github.vibebms.nightly`). Both are in the nightly's
[`fdroid.txt`](https://github.com/vibeBMS/vibe/releases/download/nightly/fdroid.txt), with a link that adds the
repository in one tap (and as a QR code,
[`fdroid-qr.png`](https://github.com/vibeBMS/vibe/releases/download/nightly/fdroid-qr.png)). An APK installed from
the release by hand updates the same way, as it is signed with the same key.

The nightly job publishes the repository with the nightly APK (`tools/fdroid_repo.sh`). On each push to the default
branch it:
- runs `fdroid update` (fdroidserver 2.4.5, from pip) on the APK the release has, byte for byte, as its only version
  (named `io.github.vibebms.nightly_<versionCode>.apk`, so each build has its own URL), with the metadata in
  `platform/android/fdroid/metadata/` (name, summary, description, licence, links; the icon is
  `io.github.vibebms.nightly/en-US/icon.png`, a copy of `assets/logo/icon-512.png`);
- signs the index (`entry.jar` for current clients, `index-v1.jar` and `index.jar` for older ones) with
  `FDROID_KEYSTORE` / `FDROID_KEYSTORE_PASSWORD` when set, else with the APK's key (`ANDROID_KEYSTORE`);
- after the release is published, replaces the branch `fdroid-repo` with one new commit (no parent), so the older
  APKs are not kept in its history. The release's concurrency group and the check that the commit is still the
  branch's head make the newest run win;
- puts the address, the key's fingerprint and the links that add the repository (`fdroidrepos://...?fingerprint=`)
  in the run's summary and in the release's `fdroid.txt`, the link as a QR code in `fdroid-qr.png`.

It skips the repository, saying why in the summary, when the run built no APK, when the APK was signed with a key
made for the run (it could not update the installed app), or when there is no key for the index (a key made per run
would change the fingerprint the clients pinned). A failure there holds back no other file.

The index's key is what clients pin: changing it later means every user adds the repository again. So whether the
index keeps the APK's key or gets its own (F-Droid advises one; `FDROID_KEYSTORE`) is decided once, before users add
the repository. It must be an RSA key: fdroidserver signs the index older clients read with SHA1withRSA, and with an
EC key that index would be left unsigned (CI then skips the repository and says so). An APK key made as an EC key
(Android Studio's and `keytool`'s defaults differ) therefore needs a key of its own here. A key of its own:
```sh
keytool -genkeypair -keystore vibebms-fdroid.p12 -storetype PKCS12 -alias vibebms-fdroid \
  -keyalg RSA -keysize 4096 -validity 10000 -dname "CN=vibeBMS F-Droid repository"
base64 -w0 vibebms-fdroid.p12    # the value of FDROID_KEYSTORE; FDROID_KEYSTORE_PASSWORD is the password asked for
```
The secrets go in the repository's Settings › Secrets and variables › Actions. A keystore with one key is expected
(its alias is read from it), with one password for the keystore and the key.

To build the repository locally (Python 3, a JDK, the Android SDK's build-tools):
```sh
python3 -m venv /tmp/fdroid && /tmp/fdroid/bin/pip install fdroidserver==2.4.5
PATH=/tmp/fdroid/bin:$PATH ANDROID_HOME=... FDROID_KEYSTORE_FILE=vibebms-fdroid.p12 FDROID_KEYSTORE_PASS=... \
  FDROID_REPO_URL=http://192.168.1.2:8000/repo tools/fdroid_repo.sh build vibeBMS.apk /tmp/fdroid-work
python3 -m http.server -d /tmp/fdroid-work/site 8000    # serves that address (this machine's, on the LAN)
```
`fdroid-repo` is a branch of this repository, so a plain `git clone` also fetches it (the APK, tens of MB);
`git clone --single-branch https://github.com/vibeBMS/vibe` fetches only the default branch. Do not commit to it: the
next nightly replaces it.

## iOS distribution
CI builds the same unsigned `.ipa` as above (artifact `vibeBMS-ios-unsigned`) and adds it to the nightly release as
`vibeBMS-unsigned.ipa`. The release also has `altstore.json`, an AltStore / SideStore source: added once in the app
store, it offers each new nightly as an update of "vibeBMS Nightly" (`io.github.vibebms.nightly`). It has an app per
channel: the nightly job writes the nightly's and keeps the others.

## Self-update (`latest.json`)
The nightly release has a `latest.json` that the desktop executables read on start (when they are nightlies, and
"Check for updates" in `SETTINGS › SYSTEM` is on):

```json
{
  "channels": {
    "nightly": {
      "version": "0.1.0-nightly.200+abc1234",
      "files": {
        "vibebms-linux-x64": { "version": "0.1.0-nightly.200+abc1234", "sha256": "<64 hex digits>", "size": 31457280 },
        "vibebms-windows-x64.exe": { "version": "0.1.0-nightly.198+0f1e2d3", "sha256": "<64 hex digits>", "size": 28311552 }
      }
    }
  }
}
```

It has a part per channel: the `nightly` job writes `nightly` and keeps any other (a `stable` part is kept for the
releases to come; nothing reads it). In a channel, `version` is the release's (the run that published it); `files`
has an entry for each desktop executable in the release (`vibebms-linux-x64`, `-arm64`, `-x64-portable`,
`-arm64-portable`, `vibebms-macos-arm64`, `-x64`, `vibebms-macos-universal.app.zip`, `vibebms-windows-x64.exe`,
`-arm64.exe`): the version it was built with, its SHA-256 and its size in bytes. Each desktop build job writes its
`VIBEBMS_VERSION` to a `version.txt` in its artifact, and the `nightly` job reads it. A build that failed keeps its
previous file in the release, so the job starts from the previous `latest.json` (`gh release download`) and replaces
only the entries of the files it uploads: a file's version can be older than the release's.

An executable picks its own file by its OS and architecture, on Linux by whether it was built with the
`browser` feature (the `-portable` builds are `--no-default-features`), and on macOS by whether it runs in the app
(`<name>.app/Contents/MacOS/vibebms`: the app's zip, the same for both architectures). It offers the update when that file's
commit differs from its own and its run number (`nightly.<run>`) is higher; were the file a release (`0.1.0`, no
`-nightly`), semver order would decide, so a release is newer than its own nightlies.

"Update now" downloads the file next to the executable, checks its size and SHA-256 and puts it in the running
executable's place (the `self-replace` crate: an atomic rename on Unix that keeps the permissions; on Windows the
running executable is renamed away first). The macOS app downloads its zip next to the `.app`, checks it the same
way, unpacks it with `ditto` (which keeps the signature and modes), moves the old app aside, puts the new one in its
place (the old one back if that fails), deletes the old one and signs the app ad hoc again (`codesign --force --deep
-s -`, when `codesign` is there). "Restart now" starts it with the same arguments. An executable under `/usr` (or
another package manager's folder: Homebrew's Cellar and Caskroom, Setapp, MacPorts, a Flatpak's `/app`), in an app
bundle that is not vibeBMS's own, an app Homebrew Cask installed, or in a folder it cannot write to does nothing: it
neither checks nor offers updates (its package manager updates it). Only nightlies check (a `-nightly.<run>`
prerelease whose build metadata is a 7-digit commit): local builds (`-dev`, no metadata), releases (there is no
release channel) and the web, Android and iOS builds never do.

## Build manifest (`builds.json`)
The nightly release also has a `builds.json`: every build the `nightly` job published, the oldest first, with the
SHA-256 of each executable as it runs. An IR operator that accepts a client by the hash of its executable imports
this list (`crates/vibebms/src/ir/build_id.rs` hashes the running desktop executable to find itself in it).
`latest.json` stays as it is, for the self-update.

```json
{
  "v": 1,
  "builds": [
    {
      "version": "0.1.0-nightly.200+abc1234",
      "commit": "<40 hex digits>",
      "date": "2026-11-17T12:34:56Z",
      "files": {
        "vibebms-linux-x64": { "sha256": "<64 hex digits>", "size": 31457280 },
        "vibebms-windows-x64.exe": { "sha256": "<64 hex digits>", "size": 28311552 },
        "vibebms-macos-universal.app.zip/vibeBMS Nightly.app/Contents/MacOS/vibebms": { "sha256": "<64 hex digits>", "size": 62914560 },
        "vibeBMS.apk/lib/arm64-v8a/libvibebms_android.so": { "sha256": "<64 hex digits>", "size": 33554432 }
      }
    }
  ]
}
```

- One entry per run: its version, the full commit, when it was published (UTC) and its files. Only the files this run
  built are in it (a build that failed is in an earlier entry). A run done again replaces its own entry.
- `files` has the desktop executables of the release (`vibebms-linux-*`, `vibebms-macos-arm64`, `-x64`,
  `vibebms-windows-*.exe`) and, named `<file in the release>/<path inside it>`, the macOS app's universal executable
  (from the app's zip; the `.dmg` has the same app) and the Android library for each ABI (`lib/<abi>/` in the APK).
  The Flatpaks run `vibebms-linux-x64` / `-arm64` unchanged, so those entries are their hashes too.
- Not in it: the web build (no executable: any page can claim any hash) and the iOS app (signing it, by AltStore or
  the App Store, rewrites the executable, so it has no hash of its own).
- The job downloads the previous `builds.json` (`gh release download`), appends, and keeps nightlies for 90 days (the
  newest 500 at most) and releases (a version without `-`) for good.
- The hash is of the file people run: the executables are published as built, and the self-update writes the same
  bytes. A step that changed a file after it was hashed (code signing, notarization) would have to run before this
  one. The macOS app's executable carries its ad-hoc signature, and the self-update signs the app ad hoc again;
  whether that re-signing gives the same bytes is not established.
- A local or a distribution's build is not in it: an IR that accepts clients by hash does not accept it. A hash says
  which build a client is, but proves nothing about an open-source client's plays.

## Reproducible builds
A release build can be made again from its commit, to the same bytes: anyone can check that a published executable
is that commit's source (and an IR operator that accepts clients by SHA-256, `builds.json`, need not trust CI).
What a build depends on is the commit plus these inputs:
- the run number of the CI run that built it, the `N` in `0.1.0-nightly.N+abc1234` (the version the build reports,
  and in `latest.json` and `builds.json`): `tools/ci_version.sh <run> <commit>` stamps it, as CI does with its own;
- the channel (`nightly`);
- for the web build, the repository variable `WEB_THIRD_PARTY` (`VIBEBMS_THIRD_PARTY`), which is built in.

Everything else is fixed by the commit:
- **compilers and tools**: Rust by `rust-toolchain.toml`; wasm-bindgen by `Cargo.lock` (checked); in
  `tools/repro.env` the container images by digest (manylinux_2_28 for the portable builds, Ubuntu 22.04 for
  `vibebms-linux-x64` / `-arm64`, Ubuntu 26.04 for the web build), the Ubuntu images' packages (gcc, clang, the
  libraries' headers): x64's from the Ubuntu snapshot of a given time, arm64's as `tools/linux-arm64.lock` lists
  them, each by version and SHA-256 (the ports archive has no snapshot and keeps only a package's newest version:
  each comes from it or, once gone from it, from Launchpad, which keeps every package Ubuntu published); the runner
  images (labels) of the macOS, iOS and Windows builds with their Xcode (a major.minor release: an image replaces a
  patch release by the next) and their side-by-side MSVC toolset (an installer component, e.g. `14.44.17.14`, which
  no image update replaces, where Visual Studio's own toolset is replaced by each of its updates) and Windows SDK;
  the Android NDK, cargo-ndk, the JDK and Gradle (the Android Gradle plugin in `platform/android`); the web build's
  CJK fonts by commit and SHA-256 (`tools/build_release.sh`). `tools/build_release.sh` selects the pinned Xcode
  (`DEVELOPER_DIR`) and the pinned MSVC toolset and SDK (`vcvarsall.bat`, whose `cl.exe`, `link.exe` and libraries
  rustc and the cc crate then take; CI adds the toolset's ARM64 tools with the Visual Studio installer, as the images
  have its x64 tools only) through `tools/toolchain.sh`, and writes exactly what it used into `build.txt`; in CI a pin
  the image lacks stops the build, listing what the image has;
- **the build itself**: `tools/build_release.sh <artifact>`, which CI's jobs run: `Cargo.lock` as committed
  (`--locked`), the checkout, `CARGO_HOME` and the target folder remapped to `/vibe`, `/cargo`, `/target`
  (`--remap-path-prefix`, `-ffile-prefix-map`: no build path in panic messages or C's `__FILE__`), Windows' linker
  `/Brepro` (no link time in the PE header), the variables the code reads (`VIBEBMS_THIRD_PARTY`) and the
  deployment targets set, nightlies' thin LTO over 16 codegen units (a stable build keeps the profile's fat LTO);
- **dates**: `SOURCE_DATE_EPOCH` is the commit's time; zips (the web zip, the iOS `.ipa`, the macOS app's zip) are
  made with every file at that time, mode 644/755, in name order, no extra fields, TZ=UTC
  (`tools/build_release.sh zip`); the Flatpak's release date and OSTree commit take it, and the bundle's own
  timestamp is set through `faketime`;
- **checkouts**: `.gitattributes` turns off line-ending conversion, so a Windows checkout has the same bytes as any
  other (the translations and the shader are built in).

Rebuild a Linux executable or the web build (needs git, Docker, python3; the build runs in the pinned image, a few
tens of minutes; the arm64 ones on an arm64 machine):
```sh
tools/reproduce.sh <commit> <run number> [nightly] [linux-x64-portable|linux-x64|linux-arm64-portable|linux-arm64|web]
# e.g. tools/reproduce.sh 9454dfe 300 nightly linux-x64-portable
# VIBEBMS_THIRD_PARTY=1 tools/reproduce.sh <commit> <run> nightly web   (if WEB_THIRD_PARTY was set)
```
It prints each file's SHA-256 (`repro-out/<artifact>/SHA256SUMS`) and compares it with `builds.json` (every
nightly executable of the last 90 days) and, while the nightly release is that commit's, with the release's files;
it exits 1 if one differs. The other builds are made on the runner images `tools/repro.env` names
(`tools/build_release.sh`, after `tools/ci_version.sh`; the Flatpaks and the macOS app are packaged from the
executables by `tools/package_flatpak.sh` and `tools/package_macos.sh`), which the workflow below does.

The **Reproducible builds** workflow (`.github/workflows/reproducible.yml`, Actions > Run workflow; inputs: the
commit, default the nightly's, its run number, default from `builds.json`, the channel and what to build) builds each
artifact twice in different folders, with different `CARGO_HOME`, parallel jobs and umask (the Linux and web builds
through `tools/reproduce.sh`, the portable and web builds also on two runner images; the others on the runner image
the commit's `tools/repro.env` names), fails if the two differ (diffoscope's report in the summary and as an
artifact) or if either differs from the release, and lists the results and what each was built with in the run's
summary. Packages are compared by their files too: the APK's (its signature is the key's), the `.dmg`'s (not
reproducible itself), the macOS app zip's and the `.ipa`'s. Commits without `tools/build_release.sh` cannot be rebuilt
this way.

How far each build is reproducible:

| build | reproducible | what can differ |
|---|---|---|
| `vibebms-linux-x64-portable`, `-arm64-portable` | yes: pinned image, packages, Rust | nothing known |
| `vibebms-linux-x64`, `-arm64` | yes: pinned image and packages (x64: Ubuntu snapshot; arm64: `tools/linux-arm64.lock`), Rust | nothing known |
| web (`vibeBMS-web.zip`) | yes: pinned image and packages (Ubuntu snapshot), Rust, wasm-bindgen, fonts | nothing known |
| the Flatpaks | from the same executable, on the same runner image | packaged on GitHub's Ubuntu 22.04 image: its flatpak, appstream and faketime are the image's, and their updates can change the bytes (rare) |
| `vibebms-windows-*.exe` | yes: pinned runner image, MSVC toolset and Windows SDK, Rust | a servicing update of the SDK keeps its version (`10.0.26100.0`), so the pin does not see it |
| macOS (`vibebms-macos-*`, the app's zip) | yes: pinned runner image and Xcode, Rust | Xcode's patch release (`build.txt` names it), and `codesign`, `ditto` and `zip`, which are the image's macOS's: the image's updates move them; the `.dmg` (hdiutil writes times and IDs) is not reproducible: its files are compared |
| iOS `.ipa` | yes: pinned runner image and Xcode, Rust | as macOS; a signed copy (AltStore, the App Store) differs anyway |
| Android: the library, the APK's files | on the same runner image (NDK, JDK, Gradle pinned) | the APK's signature: a rebuild is signed by another key, so its files are compared (`tools/build_release.sh entries`), not the APK |

To keep it so: move the pins (`rust-toolchain.toml`, `tools/repro.env`) in commits of their own, and build release
files only through `tools/build_release.sh`. Both are moved for you:
- Dependabot updates `rust-toolchain.toml` weekly, merged once CI passes like a minor update
  (`.github/workflows/dependabot-merge.yml`);
- the **Toolchain pins** workflow (`.github/workflows/toolchain-pins.yml`, weekly, or Actions > Run workflow) updates
  `tools/repro.env`: the newest GA macOS and Windows runner labels (from actions/runner-images), the Xcode
  releases, side-by-side MSVC toolsets and Windows SDKs on the pinned and newest images (looked up on the runners,
  the newest checked as `tools/toolchain.sh` selects it), the Android tools from their registries and, in the first
  week of a month (or with the `images` input), the images' digests, a new `UBUNTU_SNAPSHOT` and
  `tools/linux-arm64.lock` resolved again in its image. `tools/repro_pins.py` splits the changes
  into two pull requests, each one commit that changes only pins, on which the workflow dispatches CI (a pull request
  made with `GITHUB_TOKEN` starts none): `toolchain-pins/update` (newer tools on the same labels, Xcode and the
  Android tools within their major versions, digests, the snapshot), which the merge workflow merges once that CI run
  passes, after checking with the default branch's `tools/repro_pins.py` that only pins' values changed; and
  `toolchain-pins/major` (a new runner label with its tools, a new major version), which waits for a person, as a
  Dependabot major update does. Each run makes both again from the default branch, and closes one whose pins are
  current. It needs Settings › Actions › General › "Allow GitHub Actions to create and approve pull requests".

A runner image that no longer has the pinned Xcode, MSVC toolset or SDK stops those builds in CI, saying which pin
and what the image has; the workflow's next run (or one started by hand) moves it. `tools/linux-arm64.lock` names the
image and packages it is for, and the build stops when they are not `tools/repro.env`'s: after changing either, make
it again (`tools/build_deps.sh lock linux-arm64`, as the top of `tools/build_deps.sh` says).
