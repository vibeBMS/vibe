# Building vibeBMS

Requires Rust 1.85+ (`rustup`). Release builds use fat LTO; expect a few minutes.

```sh
cargo build --release            # target/release/vibebms(.exe)
cargo test --workspace           # engine/parser/gauge/etc. tests
```

Copy the executable anywhere; it keeps all its data next to itself.

## Linux
Packages: `libasound2-dev libudev-dev` (Debian/Ubuntu) and a Vulkan or GL driver.
Runtime also loads `libxkbcommon-x11` (X11) or Wayland libraries.
Optional JACK output: `cargo build --release --features jack` (needs `libjack-dev`).

## Windows (x64 / ARM64)
MSVC toolchain. ARM64: `rustup target add aarch64-pc-windows-msvc` and
`cargo build --release --target aarch64-pc-windows-msvc` (install the ARM64 build tools in Visual Studio).
ASIO output: install the ASIO SDK, set `CPAL_ASIO_DIR`, build with `--features asio`.

## macOS (Apple Silicon / Intel)
`cargo build --release` (native) or `--target x86_64-apple-darwin` / `aarch64-apple-darwin`.
Universal binary: build both and `lipo -create -output vibebms a/vibebms b/vibebms`.
Put the binary in a folder next to your BMS; inside an `.app` bundle the data root is the folder that contains the `.app`.

## Android (experimental)
```sh
cargo install cargo-apk
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=... ANDROID_NDK_ROOT=...
cargo apk build -p vibebms-android --release
```
Data root: `Android/data/io.github.vibebms/files` (copy BMS folders there over USB).
Touch controls turn on automatically.

## iOS / iPadOS (experimental)
```sh
rustup target add aarch64-apple-ios
cargo build --release -p vibebms --target aarch64-apple-ios
mkdir -p vibeBMS.app && cp target/aarch64-apple-ios/release/vibebms vibeBMS.app/ && cp platform/ios/Info.plist vibeBMS.app/
codesign -s "<your identity>" --entitlements <entitlements> vibeBMS.app
```
Install with Xcode (Devices window) or `ios-deploy`. The app's `Documents` folder is the data root and is
visible in the Files app (`UIFileSharingEnabled`), so BMS folders can be copied in.

## Test data
`python3 tools/gen_demo.py bms/demo` writes a small self-made 7-key chart with synthesized keysounds.

## Web (wasm32)
```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
./tools/build_web.sh                 # -> web/pkg
python3 -m http.server -d web 8080   # open http://localhost:8080
```
