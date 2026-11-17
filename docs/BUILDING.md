# Building vibeBMS

Requires Rust 1.95+ (`rustup`). Release builds use fat LTO; expect a few minutes.

```sh
cargo build --release            # target/release/vibebms(.exe)
cargo test --workspace           # engine/parser/gauge/etc. tests
```

Copy the executable anywhere; it keeps all its data next to itself.

While working on the code, `--no-default-features` (e.g. `cargo test --workspace --no-default-features`) leaves
out the in-app browser (wry and, on Linux, the GTK and WebKitGTK bindings), which a clean build spends a good part
of its time on. Debug builds keep line tables only (`[profile.dev]` in `Cargo.toml`): set `debug = true` there to
see variables in a debugger.

## Linux
Packages: `libasound2-dev libudev-dev libwebkit2gtk-4.1-dev` (Debian/Ubuntu) and a Vulkan or GL driver.
Runtime also loads `libxkbcommon-x11` (X11) or Wayland libraries.
WebKitGTK is for the in-app browser that downloads songs; without it, build with
`--no-default-features` (download pages then open in the system browser).

The release builds: `vibebms-linux-x64` / `-arm64` are built on Ubuntu 22.04 (glibc 2.35, WebKitGTK 4.1);
`vibebms-linux-x64-portable` / `-arm64-portable` are built without the browser in a `manylinux_2_28` (AlmaLinux 8)
container, so they need only glibc 2.28, ALSA and libudev. WebKitGTK cannot be loaded at run time instead (wry
links it), hence the two builds.
Optional JACK output: `cargo build --release --features jack` (needs `libjack-dev`).

### Flatpak
`tools/package_flatpak.sh <vibebms> <out dir>` packages a `browser` build of the machine's architecture as it is (no build from source)
into `<out dir>/vibeBMS-linux-x64.flatpak` (or `-arm64`; a bundle holds one architecture, so CI makes one per architecture), with the manifest, desktop entry and AppStream data in `platform/flatpak`
and the icons from `assets/logo`. It fills in the channel's app id, name and version (`tools/ci_version.sh`; a local
run is `io.github.vibebms.dev`, "vibeBMS Dev", on the branch `dev`), checks the desktop entry and AppStream data
(`desktop-file-validate`, `appstreamcli`, when installed), builds with `flatpak-builder` and bundles with
`flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo`, so `flatpak install` of the bundle
fetches the runtime from Flathub. Needs `flatpak` and `flatpak-builder`; the runtime and SDK (over 1 GB) are installed
for the user when no installation has them. `DRY_RUN=1` stops before the build, with the filled-in sources in
`<out dir>/flatpak-src`.

The runtime is `org.gnome.Platform` (51) because it has WebKitGTK 4.1 (`libwebkit2gtk-4.1`), which the in-app
browser links; the Freedesktop runtime has no WebKitGTK. Its permissions: network, Wayland or X11 (and IPC), the GPU,
PulseAudio, all devices (game controllers) and the home folder (BMS folders added in `SETTINGS › LIBRARY`). CI builds
the nightly's (`io.github.vibebms.nightly`, "vibeBMS Nightly", branch `nightly`) from `vibebms-linux-x64`, caches the
runtime and SDK, checks that the bundle installs and that every library the executable links resolves in the runtime,
and adds the bundle to the nightly release. Install it with `flatpak install --user vibeBMS-linux-x64.flatpak` (or `-arm64`); a
newer bundle installs over it the same way (it does not update itself: the self-update stays off under `/app`).
Inside the Flatpak the executable's folder (`/app/bin`) is read-only, so the data root is `$XDG_DATA_HOME/vibeBMS`,
which Flatpak makes `~/.var/app/<app id>/data/vibeBMS`. Video BGA needs an `ffmpeg` executable, which the runtime does
not have.

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

## Android (experimental)
The APK is a Gradle project (`platform/android`) around the Rust library, which
`cargo-ndk` builds into it. Needs the Android SDK (platform 34) with the NDK, and JDK 17+.
```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cargo ndk -t arm64-v8a -t x86_64 -o platform/android/app/jniLibs build --release -p vibebms-android
gradle -p platform/android assembleRelease    # Gradle 9.6+; or open platform/android in Android Studio
```
The release APK is signed with `VIBEBMS_KEYSTORE` / `VIBEBMS_KEYSTORE_PASSWORD` / `VIBEBMS_KEY_ALIAS`
when set, else with the debug key. Output: `platform/android/app/build/outputs/apk/release/`.
Data root: `Android/data/io.github.vibebms/files` (copy BMS folders there over USB); the nightly APK is its own app,
`io.github.vibebms.nightly`, with its data in `Android/data/io.github.vibebms.nightly/files`.
Touch controls turn on automatically. Songs a difficulty table lists can be downloaded in the
app's browser (`Browser.java`), as on the desktop.

## iOS / iPadOS (experimental)
```sh
rustup target add aarch64-apple-ios
IPHONEOS_DEPLOYMENT_TARGET=14.0 cargo build --release -p vibebms --target aarch64-apple-ios
mkdir -p Payload/vibeBMS.app
cp target/aarch64-apple-ios/release/vibebms platform/ios/Info.plist platform/ios/*.png Payload/vibeBMS.app/
zip -qr vibeBMS-unsigned.ipa Payload
```
CI builds the same unsigned `.ipa` (artifact `vibeBMS-ios-unsigned`). To install it, sign it with your
own Apple ID (AltStore, Sideloadly; a free Apple ID's signature lasts 7 days) or a developer certificate
(`codesign`, then Xcode's Devices window or `ios-deploy`). The app's `Documents` folder is the data root and is
visible in the Files app (`UIFileSharingEnabled`), so BMS folders can be copied in.

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
defined (`node tools/check_wasm_imports.mjs`).

## Versions
`Cargo.toml` names the version being worked towards as a prerelease: `0.1.0-dev` is what local builds report.
CI builds are nightlies of it: `tools/ci_version.sh` rewrites it to `<version>-nightly.<run>+<commit>`,
e.g. `0.1.0-nightly.192+e129ac9`, so `CARGO_PKG_VERSION` names the build. The run number only grows:
Android's `versionCode` and the iOS `CFBundleVersion` are that number (iOS's `CFBundleShortVersionString`
is the plain `0.1.0`, as it takes numbers only). After a release, Cargo.toml moves on to the next version (e.g. `0.1.1-dev`).

Builds belong to a channel (`VIBEBMS_CHANNEL`, `nightly` for now; `preview`, `beta` or `stable` could follow): the
version says it (`-<channel>.<run>`; a stable build is the plain version), and on Android and iOS each channel is its
own app, `io.github.vibebms.<channel>` named "vibeBMS <Channel>" (stable: `io.github.vibebms`, "vibeBMS"), so they
install side by side. `latest.json` and the AltStore source have a part per channel.

### Self-update (`latest.json`)
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
releases to come; nothing reads it yet). In a channel, `version` is the release's (the run that published it); `files` has an entry for each desktop executable in the
release (`vibebms-linux-x64`, `-arm64`, `-x64-portable`, `-arm64-portable`, `vibebms-macos-arm64`, `-x64`,
`vibebms-macos-universal.app.zip`, `vibebms-windows-x64.exe`, `-arm64.exe`): the version it was built with, its SHA-256 and its size in bytes. Each
desktop build job writes its `VIBEBMS_VERSION` to a `version.txt` in its artifact, and the `nightly` job reads it. A
build that failed keeps its previous file in the release, so the job starts from the previous `latest.json` (`gh
release download`) and replaces only the entries of the files it uploads: a file's version can be older than the
release's.

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
bundle that is not vibeBMS's own, an app Homebrew Cask installed, or in a folder it cannot write to does nothing: it neither checks nor
offers updates (its package manager updates it). Only nightlies check (a `-nightly.<run>` prerelease whose build metadata is a 7-digit commit):
local builds (`-dev`, no metadata), releases (there is no release channel yet) and the web, Android and iOS builds
never do.

### Build manifest (`builds.json`)
The nightly release also has a `builds.json`: every build the `nightly` job published, the oldest first, with the
SHA-256 of each executable as it runs. IR operators such as BMS-IR and rianIR accept a client by the hash of its
executable, and this is the list to import (`crates/vibebms/src/ir/build_id.rs` hashes the running desktop
executable to find itself in it). `latest.json` stays as it is, for the self-update.

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
  one. The macOS app's executable carries its ad-hoc signature; the self-update signs the app ad hoc again, which
  is expected to give the same bytes (not yet checked).
- A local or a distribution's build is not in it: an IR that accepts clients by hash does not accept it. A hash says
  which build a client is, but proves nothing about an open-source client's plays.
