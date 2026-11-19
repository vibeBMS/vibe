#!/usr/bin/env bash
# Build a release artifact the way CI does, reproducibly: the same commit,
# run number and channel give the same bytes wherever it runs (docs/BUILDING.md,
# "Reproducible builds"). CI's build jobs and tools/reproduce.sh run it.
#
#   tools/build_release.sh <artifact> [<out dir>]
#   tools/build_release.sh zip <zip> <dir>     zip a folder, the same bytes anywhere
#   tools/build_release.sh entries <file>      the SHA-256 of each file in an APK,
#                                              a zip (.ipa, the macOS app's) or a .dmg
#
#   <artifact>            what                          in the release
#   linux-x64             with the in-app browser       vibebms-linux-x64
#   linux-arm64                                         vibebms-linux-arm64
#   linux-x64-portable    no browser, glibc 2.28        vibebms-linux-x64-portable
#   linux-arm64-portable                                vibebms-linux-arm64-portable
#   windows-x64                                         vibebms-windows-x64.exe
#   windows-arm64                                       vibebms-windows-arm64.exe
#   macos                 arm64 and x86_64              vibebms-macos-arm64, -x64
#   ios                   the unsigned app              vibeBMS-unsigned.ipa
#   android               the APK                       vibeBMS.apk
#   web                   web/ (pkg/, the CJK fonts)    vibeBMS-web.zip
#
# Executables stay where cargo puts them (target/<triple>/release/) and the web
# build in web/. With <out dir> (needed for ios and android) each file is also
# copied there under its name in the release, with SHA256SUMS (an APK's and the
# .ipa's files too, as vibeBMS.apk/<path>: an APK's signature differs by key)
# and build.txt.
#
# The version is Cargo.toml's: tools/ci_version.sh stamps a CI build's first.
# What would make two builds of a commit differ is fixed here:
# - paths: the checkout, CARGO_HOME and the target folder are remapped to
#   /vibe, /cargo and /target in Rust's output (panic locations) and in C's
#   (__FILE__), so the folder the build runs in does not show;
# - time: SOURCE_DATE_EPOCH is the commit's time (dates in the zips, the
#   Flatpak's release date) and TZ is UTC (a zip keeps local times);
# - Windows: the linker is Rust's own rust-lld (lld-link), pinned with Rust,
#   with /Brepro (a hash instead of the link time): MSVC's link.exe is the
#   image's (an update of the toolset's binaries keeps its folder's version)
#   and writes their build numbers into the executable (its "Rich" header);
# - Apple: the linker's -S, no debug map (the object files' paths, which the
#   linker's LC_UUID hashes, though the executable is stripped after);
# - the environment: Cargo.lock as committed (--locked), the variables the code
#   reads unset (VIBEBMS_THIRD_PARTY is the web build's, from the caller),
#   RUSTFLAGS, CFLAGS and the deployment targets set here;
# - nightlies (VIBEBMS_CHANNEL other than stable): thin LTO over 16 codegen
#   units, CI's trade of a few percent of speed for much faster links; a
#   stable build and a local one keep Cargo.toml's fat LTO.
# The compilers are pinned outside: Rust by rust-toolchain.toml, wasm-bindgen
# by Cargo.lock (checked here), the containers and packages by tools/repro.env;
# on the runner images, Xcode and the MSVC toolset and Windows SDK of
# tools/repro.env are selected here (tools/toolchain.sh: CI stops without them).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
die() { echo "$*" >&2; exit 1; }
# ("<sha256>  <name>": Git for Windows' sha256sum marks binary files "*<name>")
sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi |
    sed 's/^\([0-9a-f]\{64\}\) \*/\1  /'
}

# zip <zip> <dir>: the files under <dir> (hidden ones left out, as
# upload-artifact leaves them out) in name order, each with the time
# SOURCE_DATE_EPOCH and mode 644 (755 if executable), no extra fields: the
# same zip on any machine (Info-ZIP's zip)
repro_zip() {
  local zip="$1" dir="$2" stamp list
  : "${SOURCE_DATE_EPOCH:?SOURCE_DATE_EPOCH (the commit time) is needed for a zip}"
  case "$zip" in /*) ;; *) zip="$PWD/$zip" ;; esac
  stamp="$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y%m%d%H%M.%S 2>/dev/null ||
    date -u -r "$SOURCE_DATE_EPOCH" +%Y%m%d%H%M.%S)"
  list="$(mktemp)"
  rm -f "$zip"
  (
    cd "$dir"
    find . -name '.*' ! -name . -prune -o -type f -print | sed 's|^\./||' | LC_ALL=C sort > "$list"
    while IFS= read -r f; do
      if [ -x "$f" ]; then chmod 755 "$f"; else chmod 644 "$f"; fi
      TZ=UTC touch -t "$stamp" "$f"
    done < "$list"
    TZ=UTC zip -q -X -D "$zip" -@ < "$list"
  )
  rm -f "$list"
}

# entries <file>: "<sha256>  <file name>/<path>" for each file in an APK (but
# the v1 signature's, whose bytes depend on the key), a zip or a .dmg (mounted
# with hdiutil: macOS; its hidden files, the file system's own, left out), in
# name order
entries() {
  local f="$1" mnt rc=0
  case "$f" in
    *.dmg)
      mnt="$(mktemp -d)"
      hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$mnt" "$f"
      (cd "$mnt" && find . -name '.*' ! -name . -prune -o -type f -print | sed 's|^\./||' | LC_ALL=C sort |
        while IFS= read -r file; do
          printf '%s  %s/%s\n' "$(sha256 "$file" | cut -c1-64)" "$(basename "$f")" "$file"
        done) || rc=$?
      hdiutil detach -quiet "$mnt"
      rmdir "$mnt"
      return "$rc"
      ;;
  esac
  python3 - "$f" <<'PY'
import hashlib, os, re, sys, zipfile
f = sys.argv[1]
with zipfile.ZipFile(f) as z:
    for n in sorted(z.namelist()):
        if n.endswith("/") or (f.endswith(".apk") and re.fullmatch(r"META-INF/[^/]+\.(SF|RSA|DSA|EC)", n)):
            continue
        print(f"{hashlib.sha256(z.read(n)).hexdigest()}  {os.path.basename(f)}/{n}")
PY
}

case "${1:-}" in
  zip) [ $# -eq 3 ] || die "usage: $0 zip <zip> <dir>"; repro_zip "$2" "$3"; exit 0 ;;
  entries | apk-entries) [ $# -eq 2 ] || die "usage: $0 entries <file>"; entries "$2"; exit 0 ;;
esac
{ [ $# -ge 1 ] && [ $# -le 2 ]; } || die "usage: $0 <artifact> [<out dir>] (see the top of $0)"
artifact="$1" out="${2:-}"
if [ -n "$out" ]; then mkdir -p "$out" && out="$(cd "$out" && pwd)"; fi
cd "$root"
# shellcheck source=tools/repro.env
. tools/repro.env
# shellcheck source=tools/toolchain.sh
. tools/toolchain.sh

case "$artifact" in
  linux-x64 | linux-x64-portable) triples=(x86_64-unknown-linux-gnu) ;;
  linux-arm64 | linux-arm64-portable) triples=(aarch64-unknown-linux-gnu) ;;
  windows-x64) triples=(x86_64-pc-windows-msvc) ;;
  windows-arm64) triples=(aarch64-pc-windows-msvc) ;;
  macos) triples=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  ios) triples=(aarch64-apple-ios) ;;
  android) triples=(aarch64-linux-android x86_64-linux-android) ;;
  web) triples=(wasm32-unknown-unknown) ;;
  *) die "unknown artifact: $artifact (see the top of $0)" ;;
esac
case "$artifact" in ios | android) [ -n "$out" ] || die "$artifact needs an out dir" ;; esac

# the version: a CI build's is stamped into Cargo.toml (tools/ci_version.sh)
version="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "([^"]+)"/) { print $1; exit }' Cargo.toml)"
if [ -n "${VIBEBMS_VERSION:-}" ] && [ "$VIBEBMS_VERSION" != "$version" ]; then
  die "Cargo.toml has $version, not VIBEBMS_VERSION $VIBEBMS_VERSION: run tools/ci_version.sh first"
fi
commit="${VIBEBMS_COMMIT:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"

# --- the environment ---------------------------------------------------------
windows=""
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) windows=1 ;; esac
# a folder as the compilers see it (and, if different, its real path)
paths_of() {
  if [ -n "$windows" ]; then
    cygpath -m "$1"
  else
    printf '%s\n' "$1"
    if [ -d "$1" ]; then
      local real
      real="$(cd "$1" && pwd -P)"
      [ "$real" = "$1" ] || printf '%s\n' "$real"
    fi
  fi
}
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
target_dir="${CARGO_TARGET_DIR:-$root/target}"
rustflags=() cflags=()
remap() { # <folder> <as>
  local p
  while IFS= read -r p; do
    rustflags+=("--remap-path-prefix=$p=$2")
    cflags+=("-ffile-prefix-map=$p=$2")
  done < <(paths_of "$1")
}
# (the last match wins: the target folder may be in the checkout)
remap "$root" /vibe
remap "$cargo_home" /cargo
remap "$target_dir" /target
case "${triples[0]}" in
  *-windows-msvc) rustflags+=(-Clinker=rust-lld -Clinker-flavor=lld-link -Clink-arg=/Brepro) ;;
  *-apple-*) rustflags+=("-Clink-arg=-Wl,-S") ;;
esac
[ -z "${RUSTFLAGS:-}${CARGO_ENCODED_RUSTFLAGS:-}" ] ||
  echo "note: RUSTFLAGS / CARGO_ENCODED_RUSTFLAGS from the environment are replaced" >&2
unset RUSTFLAGS
CARGO_ENCODED_RUSTFLAGS="$(IFS=$'\x1f'; printf %s "${rustflags[*]}")"
export CARGO_ENCODED_RUSTFLAGS
# C compiled by build scripts (Lua, ring, ...); MSVC's cl takes other options
# (its __FILE__ would keep the path: none of that C uses it in a release build)
if [ -z "$windows" ]; then
  CFLAGS="${cflags[*]}" CXXFLAGS="${cflags[*]}"
  export CFLAGS CXXFLAGS
else
  unset CFLAGS CXXFLAGS
fi
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
  SOURCE_DATE_EPOCH="$(git log -1 --format=%ct 2>/dev/null || true)"
fi
if [ -n "$SOURCE_DATE_EPOCH" ]; then export SOURCE_DATE_EPOCH; else unset SOURCE_DATE_EPOCH; fi
export TZ=UTC ZERO_AR_DATE=1 CARGO_INCREMENTAL=0
unset MACOSX_DEPLOYMENT_TARGET IPHONEOS_DEPLOYMENT_TARGET
[ "$artifact" = web ] || unset VIBEBMS_THIRD_PARTY
if [ -n "${VIBEBMS_CHANNEL:-}" ] && [ "$VIBEBMS_CHANNEL" != stable ]; then
  export CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
else
  unset CARGO_PROFILE_RELEASE_LTO CARGO_PROFILE_RELEASE_CODEGEN_UNITS
fi
# the C compiler and linker: Xcode and MSVC as pinned (tools/toolchain.sh); in
# the containers, the pinned packages' (gcc's version stands for them)
case "$artifact" in
  macos | ios) use_xcode ;;
  windows-x64) use_msvc x64 ;;
  windows-arm64) use_msvc arm64 ;;
  android) TOOLCHAIN="NDK $NDK_VERSION" ;;
  web) TOOLCHAIN="$("${CC_wasm32_unknown_unknown:-$WEB_CC}" --version 2>/dev/null | head -1 || true)" ;;
  *) TOOLCHAIN="$(cc --version 2>/dev/null | head -1 || true)" ;;
esac
# Cargo.lock as committed: a build that would change it stops here
cargo metadata --locked --format-version 1 > /dev/null
# Lua from vendor/lua-src (its sources in name order): the registry's lua-src
# compiles them in the file system's order, which differs between machines
if grep -A2 '^name = "lua-src"$' Cargo.lock | grep -q '^source = '; then
  die "Cargo.lock takes lua-src from the registry, not vendor/lua-src ([patch.crates-io]): see vendor/lua-src/Cargo.toml"
fi

echo "building $artifact $version ($commit) with $(rustc -V)"
echo "  toolchain: ${TOOLCHAIN:-unknown}"
echo "  rustflags: ${rustflags[*]}"
[ -z "${CFLAGS:-}" ] || echo "  cflags: $CFLAGS"
echo "  SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-unset} CARGO_PROFILE_RELEASE_LTO=${CARGO_PROFILE_RELEASE_LTO:-(profile)}"

# --- the build -----------------------------------------------------------------
files=() # "<file>:<name in the release>"
release_dir() { printf '%s/%s/release' "$target_dir" "$1"; }
case "$artifact" in
  linux-* | windows-*)
    t="${triples[0]}"
    features=()
    case "$artifact" in *-portable) features=(--no-default-features) ;; esac
    cargo build --release -p vibebms --target "$t" ${features[@]+"${features[@]}"}
    exe="$(release_dir "$t")/vibebms"
    case "$artifact" in windows-*) exe="$exe.exe" ;; esac
    case "$artifact" in
      *-portable)
        v="$(objdump -T "$exe" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
        echo "newest glibc symbol: $v"
        [ "$(printf '%s\nGLIBC_2.28\n' "$v" | sort -V | tail -1)" = GLIBC_2.28 ] ||
          die "the portable build needs glibc newer than 2.28 ($v)"
        ;;
    esac
    name="vibebms-$artifact"
    case "$artifact" in windows-*) name="$name.exe" ;; esac
    files+=("$exe:$name")
    ;;
  macos)
    cargo build --release -p vibebms --target aarch64-apple-darwin --target x86_64-apple-darwin
    files+=("$(release_dir aarch64-apple-darwin)/vibebms:vibebms-macos-arm64")
    files+=("$(release_dir x86_64-apple-darwin)/vibebms:vibebms-macos-x64")
    ;;
  ios)
    IPHONEOS_DEPLOYMENT_TARGET=14.0 cargo build --release -p vibebms --target aarch64-apple-ios
    pkg="$(mktemp -d)"
    app="$pkg/ipa/Payload/vibeBMS.app"
    mkdir -p "$app"
    cp "$(release_dir aarch64-apple-ios)/vibebms" "$app/"
    cp platform/ios/Info.plist platform/ios/*.png "$app/"
    # each channel is its own app (io.github.vibebms.nightly, "vibeBMS Nightly")
    plutil -replace CFBundleIdentifier -string "${VIBEBMS_APP_ID:-io.github.vibebms.dev}" "$app/Info.plist"
    plutil -replace CFBundleName -string "${VIBEBMS_APP_NAME:-vibeBMS Dev}" "$app/Info.plist"
    plutil -replace CFBundleDisplayName -string "${VIBEBMS_APP_NAME:-vibeBMS Dev}" "$app/Info.plist"
    # iOS takes numbers only: the version (0.1.0) and the run number
    plutil -replace CFBundleShortVersionString -string "${VIBEBMS_BASE_VERSION:-${version%%-*}}" "$app/Info.plist"
    plutil -replace CFBundleVersion -string "${VIBEBMS_BUILD:-0}" "$app/Info.plist"
    repro_zip "$pkg/vibeBMS-unsigned.ipa" "$pkg/ipa"
    files+=("$pkg/vibeBMS-unsigned.ipa:vibeBMS-unsigned.ipa")
    ;;
  android)
    : "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME: the NDK $NDK_VERSION (tools/repro.env)}"
    case "$ANDROID_NDK_HOME" in
      */"$NDK_VERSION") ;;
      *) echo "warning: ANDROID_NDK_HOME is not NDK $NDK_VERSION (tools/repro.env): $ANDROID_NDK_HOME" >&2 ;;
    esac
    have="$(cargo ndk --version 2>/dev/null | awk '{print $2}')"
    [ "$have" = "$CARGO_NDK_VERSION" ] ||
      echo "warning: cargo-ndk $have, not $CARGO_NDK_VERSION (tools/repro.env)" >&2
    # the Rust library into the Gradle project, then the APK
    rm -rf platform/android/app/jniLibs
    cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o platform/android/app/jniLibs \
      build --release -p vibebms-android
    # cargo-ndk copies every cdylib it built; only ours is loaded
    find platform/android/app/jniLibs -name '*.so' ! -name libvibebms_android.so -delete
    # CI builds: the run number and <version>-<channel>.<run>+<commit>
    export VIBEBMS_VERSION_CODE="${VIBEBMS_VERSION_CODE:-${VIBEBMS_BUILD:-1}}"
    export VIBEBMS_VERSION_NAME="${VIBEBMS_VERSION_NAME:-$version}"
    gradle -p platform/android --no-daemon assembleRelease
    files+=("platform/android/app/build/outputs/apk/release/app-release.apk:vibeBMS.apk")
    ;;
  web)
    want="$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')"
    have="$(wasm-bindgen --version | awk '{print $2}')"
    [ "$have" = "$want" ] ||
      die "wasm-bindgen $have, not Cargo.lock's $want: cargo install wasm-bindgen-cli --locked --version $want"
    # Lua is compiled from C: the pinned clang where there is one
    : "${CC_wasm32_unknown_unknown:=$(command -v "$WEB_CC" || command -v clang || true)}"
    : "${AR_wasm32_unknown_unknown:=$(command -v "$WEB_AR" || command -v llvm-ar ||
      find /usr/bin -name 'llvm-ar-*' | sort -V | tail -1)}"
    export CC_wasm32_unknown_unknown AR_wasm32_unknown_unknown
    echo "  C: $CC_wasm32_unknown_unknown ($("$CC_wasm32_unknown_unknown" --version | head -1)), $AR_wasm32_unknown_unknown"
    # no wasm-opt: CI has none, and its output depends on its version
    WASM_OPT=0 PROFILE=web ./tools/build_web.sh
    # the web build has no system fonts: Japanese / Korean titles need these
    # (Noto Sans CJK, SIL OFL 1.1), at a fixed commit of the fonts' repository
    noto=https://raw.githubusercontent.com/notofonts/noto-cjk/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans
    while read -r path file hash; do
      f="web/fonts/$file"
      if ! { [ -f "$f" ] && [ "$(sha256 "$f" | cut -c1-64)" = "$hash" ]; }; then
        curl -fsSL --retry 3 -o "$f" "$noto/$path"
        [ "$(sha256 "$f" | cut -c1-64)" = "$hash" ] || die "$noto/$path: not the expected file"
      fi
    done <<'EOF'
SubsetOTF/JP/NotoSansJP-Regular.otf NotoSansJP-Regular.otf dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073
SubsetOTF/KR/NotoSansKR-Regular.otf NotoSansKR-Regular.otf 69975a0ac8472717870aefeab0a4d52739308d90856b9955313b2ad5e0148d68
SubsetOTF/SC/NotoSansSC-Regular.otf NotoSansSC-Regular.otf faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9
SubsetOTF/TC/NotoSansTC-Regular.otf NotoSansTC-Regular.otf 5bab0cb3c1cf89dde07c4a95a4054b195afbcfe784d69d75c340780712237537
LICENSE NotoSansCJK-LICENSE.txt 6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2
EOF
    # listed in fonts.txt (the Chinese ones are loaded only when the UI is in
    # Chinese: a line's second word); once, should this run again
    if ! grep -q '^NotoSansJP-Regular\.otf$' web/fonts/fonts.txt; then
      printf "NotoSansJP-Regular.otf\nNotoSansKR-Regular.otf\n" >> web/fonts/fonts.txt
      printf "NotoSansSC-Regular.otf zh-Hans\nNotoSansTC-Regular.otf zh-Hant\n" >> web/fonts/fonts.txt
    fi
    if [ -n "$out" ]; then
      zipdir="$(mktemp -d)"
      repro_zip "$zipdir/vibeBMS-web.zip" web
      files+=("$zipdir/vibeBMS-web.zip:vibeBMS-web.zip")
    fi
    ;;
esac

[ ${#files[@]} -gt 0 ] || exit 0
for f in "${files[@]}"; do sha256 "${f%%:*}"; done
[ -n "$out" ] || exit 0
mkdir -p "$out"
for f in "${files[@]}"; do cp "${f%%:*}" "$out/${f#*:}"; done
(
  cd "$out"
  names=()
  for f in "${files[@]}"; do names+=("${f#*:}"); done
  sha256 "${names[@]}"
  for n in "${names[@]}"; do
    case "$n" in *.apk | *.ipa) entries "$n" ;; esac
  done
) > "$out/SHA256SUMS"
{
  echo "artifact=$artifact"
  echo "version=$version"
  echo "commit=$commit"
  echo "rustc=$(rustc -V)"
  echo "toolchain=${TOOLCHAIN:-unknown}"
} > "$out/build.txt"
cat "$out/SHA256SUMS"
