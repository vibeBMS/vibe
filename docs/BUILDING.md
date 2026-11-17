# Building vibeBMS

Requires Rust 1.85+ (`rustup`). Release builds use fat LTO; expect a few minutes.

```sh
cargo build --release            # target/release/vibebms(.exe)
cargo test --workspace           # engine/parser/gauge/etc. tests
```

Copy the executable anywhere; it keeps all its data next to itself.

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

## Windows (x64 / ARM64)
MSVC toolchain. ARM64: `rustup target add aarch64-pc-windows-msvc` and
`cargo build --release --target aarch64-pc-windows-msvc` (install the ARM64 build tools in Visual Studio).
ASIO output: install the ASIO SDK, set `CPAL_ASIO_DIR`, build with `--features asio`. The ASIO SDK is under
Steinberg's own licence (proprietary or GPLv3): builds with `asio` can't be redistributed under MIT/Apache without
Steinberg's licence agreement, so release builds leave it out.

## macOS (Apple Silicon / Intel)
`cargo build --release` (native) or `--target x86_64-apple-darwin` / `aarch64-apple-darwin`.
Universal binary: build both and `lipo -create -output vibebms a/vibebms b/vibebms`.
Put the binary in a folder next to your BMS; inside an `.app` bundle the data root is the folder that contains the `.app`.

## Android (experimental)
The APK is a Gradle project (`platform/android`) around the Rust library, which
`cargo-ndk` builds into it. Needs the Android SDK (platform 34) with the NDK, and JDK 17+.
```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=... ANDROID_NDK_HOME=...
cargo ndk -t arm64-v8a -t x86_64 -o platform/android/app/jniLibs build --release -p vibebms-android
gradle -p platform/android assembleRelease    # Gradle 8.9+; or open platform/android in Android Studio
```
The release APK is signed with `VIBEBMS_KEYSTORE` / `VIBEBMS_KEYSTORE_PASSWORD` / `VIBEBMS_KEY_ALIAS`
when set, else with the debug key. Output: `platform/android/app/build/outputs/apk/release/`.
Data root: `Android/data/io.github.vibebms/files` (copy BMS folders there over USB).
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

### Self-update (`latest.json`)
The nightly release has a `latest.json` that the desktop executables read on start (when they are nightlies, and
"Check for updates" in `SETTINGS › SYSTEM` is on):

```json
{
  "version": "0.1.0-nightly.200+abc1234",
  "files": {
    "vibebms-linux-x64": { "version": "0.1.0-nightly.200+abc1234", "sha256": "<64 hex digits>", "size": 31457280 },
    "vibebms-windows-x64.exe": { "version": "0.1.0-nightly.198+0f1e2d3", "sha256": "<64 hex digits>", "size": 28311552 }
  }
}
```

`version` is the release's (the run that published it); `files` has an entry for each desktop executable in the
release (`vibebms-linux-x64`, `-arm64`, `-x64-portable`, `-arm64-portable`, `vibebms-macos-arm64`, `-x64`,
`vibebms-windows-x64.exe`, `-arm64.exe`): the version it was built with, its SHA-256 and its size in bytes. Each
desktop build job writes its `VIBEBMS_VERSION` to a `version.txt` in its artifact, and the `nightly` job reads it. A
build that failed keeps its previous file in the release, so the job starts from the previous `latest.json` (`gh
release download`) and replaces only the entries of the files it uploads: a file's version can be older than the
release's.

An executable picks its own file by its OS and architecture, and on Linux by whether it was built with the
`browser` feature (the `-portable` builds are `--no-default-features`). It offers the update when that file's
commit differs from its own and its run number (`nightly.<run>`) is higher; were the file a release (`0.1.0`, no
`-nightly`), semver order would decide, so a release is newer than its own nightlies.

"Update now" downloads the file next to the executable, checks its size and SHA-256 and puts it in the running
executable's place (the `self-replace` crate: an atomic rename on Unix that keeps the permissions; on Windows the
running executable is renamed away first); "Restart now" starts it with the same arguments. An executable under
`/usr` (or another package manager's folder), inside a `.app` bundle, or in a folder it cannot write to opens the
release page instead. Only nightlies check (a `-nightly.<run>` prerelease whose build metadata is a 7-digit commit):
local builds (`-dev`, no metadata), releases (there is no release channel yet) and the web, Android and iOS builds
never do.
