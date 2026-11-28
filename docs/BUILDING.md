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
`vibeBMS Dev.app` (one universal executable from `lipo`, the icon `platform/macos/vibeBMS.icns`, signed ad hoc) and
`vibebms-macos-universal.app.zip` (what the self-update downloads); `tools/package_dmg.sh <app zip> <out dir>`
(Linux, the tools of `tools/build_deps.sh dmg`, see Reproducible builds) makes `vibeBMS-macos-universal.dmg` of
it (the app and a link to Applications). CI does the same for the nightly (`vibeBMS Nightly.app`, `io.github.vibebms.nightly`; the
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
The release APK is built unsigned (`platform/android/app/build/outputs/apk/release/app-release-unsigned.apk`):
`tools/build_release.sh sign <unsigned apk> <out dir>` signs it with apksigner and the key of `VIBEBMS_KEYSTORE` /
`VIBEBMS_KEYSTORE_PASSWORD` / `VIBEBMS_KEY_ALIAS` (see Android signing), and `tools/build_release.sh android <out dir>`
does so after building when they are set. To install one built by hand, sign it
with any key, e.g. the debug key: `apksigner sign --ks ~/.android/debug.keystore --ks-pass pass:android --out
vibeBMS.apk app-release-unsigned.apk`, or build `assembleDebug`.
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
- CI deploys the default branch's web build to GitHub Pages (the `pages` job; Settings › Pages › Source: GitHub
  Actions); other branches' builds are the run's artifact. The `pages` job deploys the `vibeBMS-web` artifact only
  when its files match the `web` job's SHA-256 (Self-update, "The files the nightly publishes").

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
- Third-party licences: `THIRD_PARTY_LICENSES.md` has the licence texts of the crates the releases are built from
  (every target's; not the tests'), each text once with the crates under it, without versions. `tools/gen_licenses.sh`
  makes it with cargo-about (its version and hash are pinned in the script; `about.toml` names the licences accepted,
  and a crate under any other stops it). CI's test job runs `tools/gen_licenses.sh --check`, which fails when the
  committed file is not what Cargo.lock gives: a version bump leaves it as it is, but an update that adds or removes
  a crate or changes a licence text fails CI, Dependabot's too (and is not merged), until someone runs
  `tools/gen_licenses.sh`, reads what changed and commits the file (on a Dependabot pull request that commit is not
  Dependabot's, so it is then merged by hand).
- How the app's code is layered: each module of `crates/vibebms/src` is in a layer of `tools/check_layers.py`'s table
  (lowest first: the file formats and the UI's text, `platform`, the library, the settings, drawing and input, skins,
  sound, the services, the state the scenes share, `ctx/`, the widgets, the in-app browser, the scenes, and the shell,
  `app/`) and uses only the layers below its own. A folder that groups modules of several kinds (`net/`, the
  downloads) has each of its parts named in the table: each part is then checked as a top-level module is, a unit. The
  uses of a higher layer that remain are listed in the script, each with its reason, and so are the units that still
  use each other in a cycle, with the uses that close it. CI's test job runs it: it fails on a use of a higher layer
  or a cycle it does not list, on another use that closes a listed cycle, on a listed use or cycle that is gone or a
  listed cycle that has grown, on an `impl` (not of a trait) of another unit's type, on a glob import of a folder whose
  parts are units of their own, and on a module in no layer, so a change that adds a module gives it a layer, and one
  that removes a listed use or cycle takes it off the list. A folder's `mod.rs` naming its parts (`scoredb::read`) uses
  them, and an item a folder re-exports from one of its parts is that part's. It also counts, per unit outside
  `platform/`, `app/` and `browser/`, the lines of code with a platform condition (a `cfg` naming a target or one of
  `build.rs`'s groups, such as `desktop`), and apart from them the lines naming a constant that stands for one
  (`platform::IS_WEB`, `ctx::IN_APP_BROWSER`): a count above the one the script lists fails (put the difference behind
  `platform/`), and so does one below it, so the listed number is lowered.
- The logo and icons: `tools/gen_logo.py` (fonttools, cairosvg, Pillow) writes `assets/logo/` and the platforms'
  icons.

## Comparison with beatoraja

`tools/e2e/beatoraja` plays the same key logs in vibeBMS and in beatoraja and compares what they make of them; the
e2e beatoraja workflow (`.github/workflows/e2e.yml`) runs it weekly, by hand, and on pushes to `e2e/**` branches,
apart from CI and the releases. beatoraja is a black box: its released program is run, the formats it reads and
writes (replays, its log, its result skins) are learnt from the files it writes, and its source is not read
(nor LR2oraja's or bmz-player's). `versions.env` pins its release (the author's download, as its GitHub release
carries no binaries) and a Java runtime with JavaFX, each by SHA-256.

- **Behaviour** (a test). `gen.py` writes the test charts (`charts/`, committed: windows at each `#RANK` and
  `#DEFEXRANK`, LN / CN / HCN, back-spin scratch, mines, each gauge, `#TOTAL`, BPM changes, stops and a 3/4
  measure) and a key log for each case, also for `tools/lr2-measure`'s charts. vibeBMS plays them with the
  `e2e_play` example of vibebms-core, under its beatoraja rule mode (judge and gauge presets beatoraja, Combo).
  beatoraja plays each as a replay (`-r1`) at three times the speed, a case whose result differs again at normal
  speed (beatoraja reads a replay's keys once a frame, and a busy machine can make it read one late). Compared:
  the counts of each judgement, EX score, max combo, FAST / SLOW, the gauge at the end and the lamp, read from
  beatoraja's log and from a result skin of the comparison's own that prints the result screen's numbers.
  `expected.json` lists the differences that are intended, each with its `docs/RULES.md` section; any other
  fails the run.
- **Rendering** (a report). `render.py` shows `charts/render.bme` in both with beatoraja's default skin at
  1280x720: song select, decide, the play at a few chart times, the result; side-by-side images and a similarity
  score (SSIM of the screens shrunk and blurred) go to the run's artifacts and summary.

To run it locally (Linux, x64): Xorg with the dummy driver, xrandr and xdotool (Ubuntu:
`xserver-xorg-video-dummy x11-xserver-utils xdotool`), Mesa, and for the rendering ffmpeg, Mesa's Vulkan driver,
Pillow and NumPy. The X server needs a mode LWJGL 2 can read: Xvfb's is not one; `xserver.sh` starts the one of
`xorg.conf`.

```sh
d=tools/e2e/beatoraja; dist=/tmp/e2e-dist
$d/fetch.sh $dist                                        # beatoraja and Java, once
cargo build --release -p vibebms-core --example e2e_play
python3 $d/gen.py --cases /tmp/e2e-cases
target/release/examples/e2e_play /tmp/e2e-cases/*.json > /tmp/vibebms.jsonl
export DISPLAY=$($d/xserver.sh 77)
python3 $d/run.py --dist $dist/beatoraja --java $dist/jre/bin/java --work /tmp/e2e-work \
  --cases /tmp/e2e-cases --out /tmp/beatoraja.json --recheck /tmp/vibebms.jsonl   # --only 'judge-*'
python3 $d/compare.py --vibe /tmp/vibebms.jsonl --beatoraja /tmp/beatoraja.json
cargo build -p vibebms
python3 $d/render.py --dist $dist/beatoraja --java $dist/jre/bin/java --vibebms target/debug/vibebms \
  --display $DISPLAY --work /tmp/e2e-render --out /tmp/e2e-rendering
```

Nothing else may use the display while beatoraja plays (it takes the keys of the focused window), and a loaded
machine makes more cases go again at normal speed.

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
in the job that built each, on the Ubuntu 22.04 runner (the executable is built in the pinned container there;
the script keeps to flatpak 1.12 and AppStream 0.15.2), after uploading the executable, installs and caches the runtime alone (without its GL drivers, codecs and translations), checks that
the bundle installs and that every library the executable links resolves in the runtime, and adds the bundle to the
nightly release. Install it with `flatpak install --user vibeBMS-linux-x64.flatpak` (or `-arm64`); a
newer bundle installs over it the same way (it does not update itself: the self-update stays off under `/app`).
Inside the Flatpak the executable's folder (`/app/bin`) is read-only, so the data root is `$XDG_DATA_HOME/vibeBMS`,
which Flatpak makes `~/.var/app/<app id>/data/vibeBMS`. Video BGA needs an `ffmpeg` executable, which the runtime does
not have.

## Android signing
CI signs the release APK with the repository's key when the secrets `ANDROID_KEYSTORE` (the keystore, base64) and
`ANDROID_KEYSTORE_PASSWORD` (its password, also the key's) are set, else with a key made for the run, which installs
but cannot update an APK signed by another. Only a run of the default branch that is not a pull request's signs with
the repository's key; every other run (pull requests, other branches, the runs the Toolchain pins workflow starts on
its branches) signs with a key made for the run.

The key is kept away from the build. CI's `android` job builds the unsigned APK and has no secret: what runs there
(cargo and every crate's build script, cargo-ndk, Gradle and its plugins) could read anything in the job. The
`android-sign` job takes that APK as an artifact, checks it against the SHA-256 the `android` job gave as a job
output (any job of the run can upload an artifact, but only that job sets its outputs), and signs it
(`tools/build_release.sh sign`), with the JDK, the pinned build-tools and apksigcopier alone; the keystore exists for
that one step. It gives the signed APK's SHA-256 as an output in turn: the `nightly` job checks the APK against it
before it uploads the release (Self-update, below, "The files the nightly publishes"), and the `fdroid` job before
it builds the F-Droid index (F-Droid repository, below). On the default branch that job
and the `fdroid` job (for the F-Droid repository's key) run in the environment `release`. Keep the keys there, not in
the repository's secrets: Settings › Environments › `release`, "Deployment branches and tags": selected branches,
the default branch only; then add `ANDROID_KEYSTORE`, `ANDROID_KEYSTORE_PASSWORD` (and `FDROID_KEYSTORE`,
`FDROID_KEYSTORE_PASSWORD`) as the environment's secrets and delete the repository's. The self-update's key,
`UPDATE_SIGNING_KEY`, has an environment of its own, `update-signing` (Self-update, below). A run of any other branch,
whatever its workflow file says, then cannot read them. (Until then the environment is an empty one GitHub makes on
first use, and the repository's secrets are used.) No reviewers or wait timer: each would hold every nightly.

Make the passwords (`ANDROID_KEYSTORE_PASSWORD`, `FDROID_KEYSTORE_PASSWORD`) long, and not of hex digits alone
(`0-9`, `a-f`). GitHub leaves out a job output in which the value of a secret of that job appears, and the jobs give
SHA-256s, 64 hex digits, as outputs: a short password of hex digits is somewhere in one of them now and then (for 4
digits about one run in a thousand). The `nightly` job then refuses the file whose maker "gave no SHA-256", and
nothing is published (or the `fdroid` job refuses the APK, and the F-Droid repository is not updated).

Gradle builds the APK unsigned and `tools/build_release.sh sign` signs it with
the pinned build-tools' apksigner (`BUILD_TOOLS_VERSION`), with APK Signature Scheme v2 alone, as the Android Gradle
plugin signed it before (v1 is needed only before Android 7; the APK's `minSdk` is 26, Android 8.0), keeping the entries as they
are (`--alignment-preserved`): the signed APK is the unsigned one with the APK Signing Block put before its central
directory, so a rebuild with the release's signature copied onto it is the release (Reproducible builds; the signing
checks it with apksigcopier). The APK carries no dependency list in that block (`dependenciesInfo`: it is encrypted
for Google Play). Make the keystore once and keep it: an APK signed by a new key cannot
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

CI's `fdroid` job ("F-Droid repository") publishes the repository with the nightly APK (`tools/fdroid_repo.sh`). It
needs the `nightly` and `pages` jobs and runs only when the `nightly` job published the release (its output
`released`), so it starts once the release is out and the site is deployed: nothing it runs can change what those
two published in this run. A run that does not release (a build failed, a pull request, another branch, a commit
the branch has moved on from) skips it. It takes the APK as the artifact `vibeBMS-android` and checks it against
the SHA-256 `android-sign` gave (before it installs fdroidserver, and again right before the key is written). Then
it:
- installs fdroidserver 2.4.5 from pip, in a step that has no key: it and every package it brings (68 files) are
  pinned by version and SHA-256 in `tools/fdroidserver.txt` and `tools/fdroidserver-build.txt` (`--require-hashes`;
  the four that have no wheel are built with the pinned setuptools, without pip's build isolation, which would fetch
  one unpinned). The files were resolved for Python 3.12 on x86_64 Linux, which the job installs
  (`actions/setup-python`) whatever the runner image's own is: to move to another Python or a newer fdroidserver,
  make them again as the top of `tools/fdroidserver.txt` says;
- runs `fdroid update` on the APK the release has, byte for byte, as its only version
  (named `io.github.vibebms.nightly_<versionCode>.apk`, so each build has its own URL), with the metadata in
  `platform/android/fdroid/metadata/` (name, summary, description, licence, links; the icon is
  `io.github.vibebms.nightly/en-US/icon.png`, a copy of `assets/logo/icon-512.png`);
- signs the index (`entry.jar` for current clients, `index-v1.jar` and `index.jar` for older ones) with
  `FDROID_KEYSTORE` / `FDROID_KEYSTORE_PASSWORD` when set, else with the APK's key (`ANDROID_KEYSTORE`, which that
  step gets only then); the password reaches keytool and fdroidserver through the environment, not a command line
  or `config.yml`;
- replaces the branch `fdroid-repo` with one new commit (no parent), so the older APKs are not kept in its history.
  The job's concurrency group (`fdroid-repo`) and the check that the commit is still the branch's head (or that the
  newer commits skip CI, `[skip ci]`: they start no run, `tools/ci_moved_on.sh`) make the newest run win;
- puts the address, the key's fingerprint and the links that add the repository (`fdroidrepos://...?fingerprint=`)
  in the run's summary and, uploaded to the nightly release, in `fdroid.txt`, the link as a QR code in
  `fdroid-qr.png`.

It skips the repository, saying why in the summary, when the APK was signed with a key made for the run (it could
not update the installed app), or when there is no key for the index (a key made per run would change the
fingerprint the clients pinned). A failure there fails the `fdroid` job alone, so that it is seen: the release and
the site are already published.

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
The secrets go in the environment `release` (Android signing, above; or, until it is set up, in the repository's
Settings › Secrets and variables › Actions). A keystore with one key is expected (its alias is read from it), with
one password for the keystore and the key.

To build the repository locally (Python 3, a JDK, the Android SDK's build-tools; with another Python than 3.12 or
off x86_64 Linux the pinned files do not fit: `pip install fdroidserver==2.4.5` then):
```sh
python3 -m venv /tmp/fdroid
/tmp/fdroid/bin/pip install --require-hashes --only-binary :all: -r tools/fdroidserver-build.txt
/tmp/fdroid/bin/pip install --require-hashes --no-build-isolation -r tools/fdroidserver.txt
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

It has a part per channel: the `update-manifest` job writes `nightly`. It keeps the other parts (a `stable` part, for
the releases to come; nothing reads it yet) from the release's previous `latest.json`, but only when the repository
has `tools/update_key.pub`, that file's signature (`latest.json.sig`) verifies with it (`ssh-keygen -Y verify`, as
`verify-update` checks), and the part has the shape the builds read (a `version` string and `files`, each with a
`version` string, a `sha256` of 64 hex digits and a `size` that is a positive whole number). Otherwise it keeps
none. In a channel, `version` is the release's (the run that published it); `files` has an entry for each desktop
executable in the release (`vibebms-linux-x64`, `-arm64`, `-x64-portable`, `-arm64-portable`, `vibebms-macos-arm64`,
`-x64`, `vibebms-macos-universal.app.zip`, `vibebms-windows-x64.exe`, `-arm64.exe`): the version it was built with,
its SHA-256 and its size in bytes. The version of each file is the run's: every build of the run is given the same
one (`tools/ci_version.sh`, from the commit's `Cargo.toml`, the run number and the commit), and `update-manifest`
makes it from the same three, so it reads nothing for it from an artifact. Each build job gives the SHA-256 of each
file it uploads as a job output, and `update-manifest` takes an executable only when it matches (below, "The files the nightly publishes"). The nightly is released only when every build passed, so every
entry of `nightly` is this run's: nothing of the previous `nightly` part is kept, and the job fails if a build's file
is missing or is not the one its job made. The `nightly` job publishes the `latest.json` that `update-manifest` made
(and `sign-update` signed, below).

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

### The signature (`latest.json.sig`)
`latest.json` names the file each build installs and its SHA-256. Anyone who can change the release's files (a leaked
token, a compromised action in a job that may write the release) could change both. So CI signs `latest.json`, and a
build that has the key takes the list only with a signature that matches.

- The key is an OpenSSH Ed25519 key. Its private half is the secret `UPDATE_SIGNING_KEY` of the environment
  `update-signing`, which only the job `sign-update` names: the jobs that run scripts and downloaded tools (the
  nightly's, and the fdroid job's, in `release`) are never given it. Limit it to the default branch as `release` is
  (Android signing, above). Its public half is `tools/update_key.pub`, in the repository. There is one key and no
  second copy: CI alone has it.
- The job `update-manifest` writes `latest.json`. It has no secret and only reads. It gives the file's SHA-256 as a
  job output. The job `sign-update` runs in the environment `update-signing`: it downloads `latest.json` as an
  artifact, checks it against that SHA-256 (with `sha256sum`, before the key is written), and runs
  `ssh-keygen -Y sign -n vibebms-update` with the key, which exists for that step only. It gives the signature's
  SHA-256 as a job output. It runs nothing from the repository (no checkout) and no action but the artifact ones.
  The job `verify-update`, which has no secret, then checks both files against those SHA-256s, and the signature
  with `ssh-keygen -Y verify` against `tools/update_key.pub`. The `nightly` job (and `pages`) waits for both: if
  signing or the check fails, nothing is published. The `nightly` job checks the two files against the same
  SHA-256s again, so it publishes the very files that were signed and checked (with every other file it publishes:
  below). If `tools/update_key.pub` is in the
  repository but the secret is missing, the run fails and says so. The release then has `latest.json.sig` next to
  `latest.json` (the armoured SSH signature format, OpenSSH's `PROTOCOL.sshsig`).
- build.rs reads `tools/update_key.pub` into the build (the build fails if the file is not one `ssh-ed25519` line).
  A build with the key downloads `latest.json` and `latest.json.sig` (each with a small size limit). It checks that
  the signature is one Ed25519 signature over SHA-512, made for the namespace `vibebms-update`, by the key it has,
  of the very bytes it then reads (`crates/vibebms/src/update/sig.rs`). If the signature is missing, unreadable or
  does not match, the check offers no update and says so in `SETTINGS › SYSTEM` and the log.
- The `nightly` job uploads the other files first, then `latest.json.sig`, then `latest.json`. While it uploads, or
  if the upload is cut off, a build sees one of these:
  - the previous list and signature, while the files change under them: a build that already has the previous
    nightly is offered nothing. An older build is offered the previous nightly; the file it downloads may already
    be the new one, and then its SHA-256 does not match, so nothing is installed ("try later").
  - the new signature beside the previous list: the signature does not match, so nothing is offered.
  - the new list and signature: all of its files are already there.
  A cut-off upload heals with the next good run. No case installs a file whose SHA-256 differs from the signed list.
- Without `tools/update_key.pub` (until the key is made), `sign-update` and `verify-update` do nothing, the release
  has no `latest.json.sig`, and builds check only the SHA-256, as before.
- The stable channel to come is a part of the same `latest.json`: the same file, the same key and signature.

What it does not protect against:
- Someone who can change the default branch: CI builds what is there and signs it.
- An old signed `latest.json` put back, with its signature: a build older than that list is offered the older
  nightly it names. A build newer than it is offered nothing (the run number only grows).

To make the key (once; on any machine with OpenSSH):
```sh
ssh-keygen -t ed25519 -N "" -C vibebms-update -f vibebms-update
```
Paste the whole of the file `vibebms-update` (the private key, `-----BEGIN OPENSSH PRIVATE KEY-----` to the `END`
line) as the secret `UPDATE_SIGNING_KEY` in Settings › Environments › `update-signing` (new; "Deployment branches
and tags": selected branches, the default branch only). Commit `vibebms-update.pub` as
`tools/update_key.pub`. Then delete the private key file. Only builds made from that commit on check the signature:
the builds people already have do not have the key and go on taking `latest.json` as before, until they update to
one that has it.

To replace the key: make a new one the same way, replace the secret, and commit the new public key as
`tools/update_key.pub` in the same push. Builds made from then on check with the new key. Builds made before it
still have the old key: every `latest.json` from then on fails their check, so they stop updating themselves.
Their users must download a new build by hand once. Say so in the release notes.

### The files the nightly publishes
Any job of a run can upload an artifact, under any name, and replace another job's (a crate's build script, a Gradle
plugin or a package that runs in one job could do so). Only a job sets its own outputs. So each file is bound to the
job that made it by its SHA-256, given as a job output, and the jobs after it take an artifact's file only when it
matches:
- Each job that makes a file of the release gives the SHA-256 of the file as it uploads it: `build-linux-x64`,
  `build-linux-arm64` (`sha256`, and `sha256-flatpak` for the Flatpak), `build-linux-x64-portable`,
  `build-linux-arm64-portable`, `build-windows-x64`, `build-windows-arm64`, `macos-dmg`, `ios`, `android-sign`,
  `update-manifest` and `sign-update` (`sha256`), and `macos` (`sha256-macos-arm64`, `-x64`, and
  `sha256-macos-universal` for the app's zip). Each architecture is a job of its own, not a leg of a matrix: the legs
  of a matrix give one set of outputs, which the runner fills at the end of each leg, so a leg whose code took over
  its runner could set its sibling's output and replace that build too. (The jobs share their steps through YAML
  anchors, and each is a matrix of one, so the names in a run and the cache keys are as they were.) The `web` job
  gives the SHA-256 of the list of its build's files (each file's SHA-256 and name, in name order, without the
  hidden files that upload-artifact leaves out).
- `macos-dmg` makes the `.dmg` only of the app zip whose SHA-256 `macos` gave. `update-manifest` takes each
  executable only when it matches its job's output: `latest.json` names only files the build jobs made.
- The `nightly` job zips only the web build whose files match the `web` job's output, and the `pages` job deploys
  only that build: it downloads `vibeBMS-web`, checks it with the same step (a YAML alias), and makes the Pages
  artifact of it itself, named for the attempt (`github-pages-<attempt>`, so a re-run of the job deploys its own).
  Upload-artifact and upload-pages-artifact both leave out the hidden files and follow links, so the site has the
  files of the web build as before. The files it makes itself (the
  web build's zip, `altstore.json`, `builds.json`) are given as the SHA-256 outputs of the steps that made them.
- Its step "check the files" then checks every file in `out/` (what it uploads) against the output of its maker.
  Every one of them must match, and a file that no maker names, or whose maker gave no SHA-256, stops the release:
  nothing is published. It runs before `builds.json` is made (which hashes what is inside the app's zip and the
  APK, so those are the checked files), and again ("check the files again", the same text: a YAML alias in the
  workflow, not a file a step could change) as the last step before the upload, for `builds.json` and anything else
  in `out/` by then. Nothing is installed in that job, and it has no secret.
- The `fdroid` job takes the APK only when it matches `android-sign`'s output (F-Droid repository, above); its
  `fdroid.txt` and `fdroid-qr.png` are not files of the `nightly` job, which refuses them.

What it does not protect against:
- Foreign code in the job that makes a file (a crate's build script in a build job, a Gradle plugin in `android`)
  changing that file before the job takes its SHA-256: the output then names that file.
- The build caches. Each build job restores its Rust cache (the `target` folder and the crates' unpacked sources,
  which Cargo does not check again) before it builds. A job of a run of the default branch can save a cache entry
  under a build job's key, when that key is new (a change of `Cargo.lock`, the toolchain or the pins), and the build
  job then builds from it: its SHA-256 names what it built, so every check passes. The Reproducible builds workflow
  (started by hand) builds the release files again without any cache: a file it does not make to the same bytes
  shows such a difference.
- The `fdroid` job itself. It runs the pinned fdroidserver (each of its files pinned by SHA-256, but third-party
  code, four packages built from their sources) with the F-Droid repository's key and a token that can write the
  repository's contents. Code in it could sign any index with that key, push to `fdroid-repo` and to any other
  branch or tag that no rule protects, and replace or add files of the nightly release after the `nightly` job
  checked them: the executables the self-update installs only together with a `latest.json` that verifies (once
  `tools/update_key.pub` is committed; until then a replaced `latest.json` is taken as it is), every other file as
  it is (an Android client installs an update only with the installed APK's signature). Its steps also name
  `ANDROID_KEYSTORE` and its password (the index's key when `FDROID_KEYSTORE` is not set), so the runner holds them,
  and code with the runner's passwordless `sudo` can read them from its memory. It cannot change this run's site
  (it has no Pages permission, and the `pages` job has ended) or its checks. Nothing it publishes is checked after
  fdroidserver ran: the index, the copy of the APK in it (`tools/fdroid_repo.sh` compares it byte for byte with the
  checked APK, but in the same job), `fdroid.txt` and `fdroid-qr.png`; clients check the index's signature, and the
  APK's own signature, as they install it.
- What is carried from the previous release without a signature: the other apps of `altstore.json` and the older
  entries of `builds.json`, taken from the release as it is (the other channels of `latest.json` are kept only when
  their signature verifies).
- The artifacts as the run's page lists them: the downloads summary links each build's artifact without a check,
  for pull requests too, and any job of the run could have replaced one. The files of the release are the checked
  ones.
- The environments' branch rule ("Deployment branches and tags": the default branch only, for `release` and
  `update-signing`) is a setting of the repository (Settings › Environments), not part of the workflow, which cannot
  check it. Without it, a run of another branch whose workflow file asks for the environment gets its secrets.

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
      "channel": "nightly",
      "run": 200,
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
  `tools/repro.env` the container images by digest (manylinux_2_28 for the portable builds, whose compiler is the
  image's; the packages added there, the ALSA and libudev headers, are installed by `dnf` from AlmaLinux 8's
  repositories as they are that day: by name, with no version or hash; Ubuntu 22.04 for
  `vibebms-linux-x64` / `-arm64`, Ubuntu 26.04 for the web build), the Ubuntu images' packages (gcc, clang, the
  libraries' headers): x64's from the Ubuntu snapshot of a given time, arm64's as `tools/linux-arm64.lock` lists
  them, each by version and SHA-256 (the ports archive has no snapshot and keeps only a package's newest version:
  each comes from it or, once gone from it, from Launchpad, which keeps every package Ubuntu published); the runner
  images (labels) of the macOS, iOS and Windows builds with their Xcode (a major.minor release: an image replaces a
  patch release by the next) and their side-by-side MSVC toolset (an installer component, e.g. `14.44.17.14`, which
  no image update replaces, where Visual Studio's own toolset is replaced by each of its updates) and Windows SDK;
  the Android NDK, cargo-ndk, the JDK, Gradle (the Android Gradle plugin in `platform/android`) and the build-tools
  (apksigner); the `.dmg`'s hfsprogs (with the snapshot) and libdmg-hfsplus by commit; the web build's
  CJK fonts by commit and SHA-256 (`tools/build_release.sh`). `tools/build_release.sh` selects the pinned Xcode
  (`DEVELOPER_DIR`) and the pinned MSVC toolset and SDK (`vcvarsall.bat`, whose `cl.exe`, `link.exe` and libraries
  rustc and the cc crate then take; CI adds the toolset's ARM64 tools with the Visual Studio installer, as the images
  have its x64 tools only) through `tools/toolchain.sh`, and writes exactly what it used into `build.txt`; in CI a pin
  the image lacks stops the build, listing what the image has;
- **the build itself**: `tools/build_release.sh <artifact>`, which CI's jobs run: `Cargo.lock` as committed
  (`--locked`), the checkout, `CARGO_HOME` and the target folder remapped to `/vibe`, `/cargo`, `/target`
  (`--remap-path-prefix`, `-ffile-prefix-map`: no build path in panic messages or C's `__FILE__`), on Windows
  Rust's own linker, rust-lld, with `/Brepro` (no link time in the PE header) and no PDB (the release has none; its
  object paths would go into the `/Brepro` hash; MSVC's `link.exe` writes its and the compiler's build numbers into
  the executable), on macOS and iOS the linker's `-S` (no debug map: its object paths
  would go into the executable's `LC_UUID`), the variables the code reads (`VIBEBMS_THIRD_PARTY`) and the
  deployment targets set, nightlies' thin LTO over 16 codegen units (a stable build keeps the profile's fat LTO);
  Lua's C sources compiled in name order (lua-src 551.1.1 and later does; before, it took them in the file system's
  order, on Linux's ext4 a hash seeded per file system, so two machines linked them differently;
  `tools/build_release.sh` stops if `Cargo.lock` takes an older lua-src). The web build compiles the same
  sources itself and takes only a folder whose files have the SHA-256 in `crates/vibebms-lua-web/build.rs`
  (`LUA_SHA256`): when lua-src is updated, the web build stops and prints the new value to set there;
- **dates**: `SOURCE_DATE_EPOCH` is the commit's time; zips (the web zip, the iOS `.ipa`, the macOS app's zip) are
  made with every file at that time, mode 644/755, in name order, no extra fields, TZ=UTC
  (`tools/build_release.sh zip`); the Flatpak's release date and OSTree commit take it, and the bundle's own
  timestamp is set through `faketime`;
- **checkouts**: `.gitattributes` turns off line-ending conversion, so a Windows checkout has the same bytes as any
  other (the translations and the shader are built in).

Rebuild a Linux executable or the web build, or make the macOS `.dmg` again (needs git, Docker, python3; the build
runs in the pinned image, a few tens of minutes; the arm64 ones on an arm64 machine):
```sh
tools/reproduce.sh <commit> <run number> [nightly] [linux-x64-portable|linux-x64|linux-arm64-portable|linux-arm64|web|macos-dmg]
# e.g. tools/reproduce.sh 9454dfe 300 nightly linux-x64-portable
# VIBEBMS_THIRD_PARTY=1 tools/reproduce.sh <commit> <run> nightly web   (if WEB_THIRD_PARTY was set)
# REPRO_APP_ZIP=vibebms-macos-universal.app.zip tools/reproduce.sh <commit> <run> nightly macos-dmg
```
It prints each file's SHA-256 (`repro-out/<artifact>/SHA256SUMS`) and compares it with `builds.json` (every
nightly executable of the last 90 days) and, while the nightly release is that commit's, with the release's files;
it exits 1 if one differs. `macos-dmg` packages the app's zip (`REPRO_APP_ZIP`; by default the release's, while the
release is that commit's) as CI does. The other builds are made on the runner images `tools/repro.env` names
(`tools/build_release.sh`, after `tools/ci_version.sh`; the Flatpaks and the macOS app are packaged from the
executables by `tools/package_flatpak.sh` and `tools/package_macos.sh`), which the workflow below does.

The **`.dmg`** (`tools/package_dmg.sh`) is made on Linux from the app's zip, in `LINUX_IMAGE_X64` with
`DMG_PACKAGES` from the Ubuntu snapshot (hfsprogs) and libdmg-hfsplus (Mozilla's fork, which makes Firefox's) built at
`LIBDMG_HFSPLUS_COMMIT` (`tools/build_deps.sh dmg`): `mkfs.hfsplus` makes an HFS+ volume named after the app,
libdmg-hfsplus's `hfsplus` adds each folder and file in name order (`mkdir`, `add`, `chmod`; not `addall`, which adds
them in the order the file system lists them, on ext4 a hash seeded per file system, and that order is the catalog
node IDs' and the blocks') and the link to `/Applications` (`symlink`), `tools/hfsplus_pin.py` sets the volume
header's dates (created, modified, checked) and each folder's and file's dates to `SOURCE_DATE_EPOCH` (backup dates:
0), in both copies of the header, and the volume's 64-bit ID (Finder info; `mkfs.hfsplus` hashes the time, the load
average and the CPU time into it) to a hash of the volume, `fsck.hfsplus` checks it, and `dmg build` compresses it
to UDIF (zlib, level 9: UDZO, as hdiutil made it). `dmg build`'s own fields are fixed by the data (the CRC-32s, the
resources' plist) but for the koly block's segment ID, glibc's `rand()` unseeded: the same every run. The volume has
what hdiutil's had (the app, a link to `/Applications`, no `.DS_Store`: the Finder's default window); the image
around it is `dmg build`'s, an Apple partition map with the volume in an `Apple_HFSX` partition (as Firefox's `.dmg`;
hdiutil's had a GUID partition table), and its files are owned by root (macOS mounts a disk image's volume ignoring
owners: they show as the user's). CI's `macos-dmg-check` job checks it on macOS: `hdiutil verify`, mounted, the app's
files are the zip's and its signature holds, and copied out it starts.

The **Reproducible builds** workflow (`.github/workflows/reproducible.yml`, Actions > Run workflow; inputs: the
channel, the commit, default the channel's newest build, its run number, default from `builds.json`, and a box for
each build) builds each
artifact twice in different folders, with different `CARGO_HOME`, parallel jobs and umask (the Linux and web builds
through `tools/reproduce.sh`, the portable and web builds also on two runner images; the others on the runner image
the commit's `tools/repro.env` names), fails if the two differ (diffoscope's report in the summary and as an
artifact) or if either differs from the release, and lists the results and what each was built with in the run's
summary. Each file is compared whole, the `.dmg` too (made from each build's app zip by `tools/reproduce.sh
macos-dmg`); the APK is rebuilt unsigned (a rebuild has no key), its a and b compared whole, and against the release
the release's signature is copied onto it with apksigcopier (`tools/apksigcopier.txt`, pinned by version and
SHA-256), as F-Droid checks a reproducible APK: the result must be the release's `vibeBMS.apk` byte for byte, and
apksigner verifies it. The files in the APK, the app's zip and the `.ipa` are listed too, to show which one differs.
Commits without `tools/build_release.sh` cannot be rebuilt this way; older ones compare the files in their `.dmg`
(made with hdiutil) and in their APK (signed in Gradle).

How far each build is reproducible:

| build | reproducible | what can differ |
|---|---|---|
| `vibebms-linux-x64-portable`, `-arm64-portable` | yes: pinned image and Rust; its three packages (`PORTABLE_PACKAGES`: the ALSA and libudev headers, pkgconf) are named, not pinned: `dnf` installs what AlmaLinux 8's repositories have that day | an update of those packages in AlmaLinux 8 (rare; the compiler and glibc are the image's) |
| `vibebms-linux-x64`, `-arm64` | yes: pinned image and packages (x64: Ubuntu snapshot; arm64: `tools/linux-arm64.lock`), Rust | nothing known |
| web (`vibeBMS-web.zip`) | yes: pinned image and packages (Ubuntu snapshot), Rust, wasm-bindgen, fonts | nothing known |
| the Flatpaks | from the same executable, on the same runner image | packaged on GitHub's Ubuntu 22.04 image: its flatpak, appstream and faketime are the image's, and their updates can change the bytes (rare) |
| `vibebms-windows-*.exe` | yes: pinned runner image, MSVC toolset and Windows SDK, Rust and its linker (rust-lld) | an update of the image can change the toolset's `cl.exe` (which compiles Lua and ring's C) and the SDK's files while their folders keep their versions (`build.txt` names `cl`'s build) |
| macOS (`vibebms-macos-*`, the app's zip) | yes: pinned runner image and Xcode, Rust | Xcode's patch release (`build.txt` names it), and `codesign`, `ditto` and `zip`, which are the image's macOS's: the image's updates move them |
| macOS `.dmg` | yes, from the app's zip: pinned image and packages (Ubuntu snapshot: hfsprogs), libdmg-hfsplus by commit, times and IDs pinned (`tools/package_dmg.sh`) | what the app's zip can |
| iOS `.ipa` | yes: pinned runner image and Xcode, Rust | as macOS; a signed copy (AltStore, the App Store) differs anyway |
| Android `vibeBMS.apk` | yes, with the release's signature copied onto the rebuilt unsigned APK (apksigcopier), on the same runner image: NDK, JDK, Gradle, build-tools pinned | nothing known |

To keep it so: move the pins (`rust-toolchain.toml`, `tools/repro.env`) in commits of their own, and build release
files only through `tools/build_release.sh`. Both are moved for you:
- Dependabot updates `rust-toolchain.toml` weekly, merged once CI passes like a minor update
  (`.github/workflows/dependabot-merge.yml`). It takes a release, of Rust as of any dependency, once it is 7 days old
  (`cooldown` in `.github/dependabot.yml`): what is merged with no person looking has had a week to be withdrawn or
  reported;
- the **Toolchain pins** workflow (`.github/workflows/toolchain-pins.yml`, weekly, or Actions > Run workflow) updates
  `tools/repro.env`: the newest GA macOS and Windows runner labels (from actions/runner-images), the Xcode
  releases, side-by-side MSVC toolsets and Windows SDKs on the pinned and newest images (looked up on the runners,
  the newest checked as `tools/toolchain.sh` selects it), the Android tools (NDK, cargo-ndk, JDK, Gradle,
  build-tools) from their registries, libdmg-hfsplus's commit and, in the first week of a month (or with the
  `images` input), the images' digests, a new `UBUNTU_SNAPSHOT` (the packages of the `*_PACKAGES` lists) and
  `tools/linux-arm64.lock` resolved again in its image. Where the registry gives a time it waits 7 days, as
  Dependabot does: the newest release of cargo-ndk (crates.io), Gradle and the JDK (Adoptium) that is that old, and
  the head libdmg-hfsplus's default branch had 7 days ago (GitHub's record of the pushes, not the dates written in
  the commits); a pin that is already newer stays. The NDK and build-tools (sdkmanager's list), the digests and what
  the runner images have come with no time and are taken as found. A registry that does not answer, or answers
  something else, leaves its pin and fails the `registries` job (the other pins move all the same).
  `tools/repro_pins.py` takes for a pin only a value that looks like one (a version is numbers and dots, an image
  `<repository>[:<tag>]@sha256:<digest>`, a commit 40 hex digits) and splits the changes
  into two pull requests, each one commit that changes only pins, on which the workflow dispatches CI (a pull request
  made with `GITHUB_TOKEN` starts none, and its pull request's CI waits for a person's approval):
  `toolchain-pins/update` (newer tools on the same labels, Xcode and the Android tools within their major versions,
  digests, the snapshot), which the workflow's merge job merges once that CI run passes (it waits for it: such a run
  starts no other workflow), after checking with the default branch's `tools/repro_pins.py` that only pins' values
  changed, each to a value that looks like one; and
  `toolchain-pins/major` (a new runner label with its tools, a new major version), which waits for a person, as a
  Dependabot major update does. Each run makes both again from the default branch, and closes one whose pins are
  current. It takes as its own only an open pull request from this repository's branch that the workflow opened
  (`github-actions`): a fork's pull request with a head branch of the same name is left alone. It needs Settings ›
  Actions › General › "Allow GitHub Actions to create and approve pull requests".

A runner image that no longer has the pinned Xcode, MSVC toolset or SDK stops those builds in CI, saying which pin
and what the image has; the workflow's next run (or one started by hand) moves it. `tools/linux-arm64.lock` names the
image and packages it is for, and the build stops when they are not `tools/repro.env`'s: after changing either, make
it again (`tools/build_deps.sh lock linux-arm64`, as the top of `tools/build_deps.sh` says).
