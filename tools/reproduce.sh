#!/usr/bin/env bash
# Rebuild a released artifact from its commit and compare its SHA-256 with the
# release's (docs/BUILDING.md, "Reproducible builds"):
#
#   tools/reproduce.sh <commit> <run number> [<channel>] [<artifact>]
#
# <run number>  the CI run that built it: the N of 0.1.0-nightly.N+abc1234, the
#               version the build reports (also in latest.json and builds.json)
# <channel>     nightly (the default)
# <artifact>    linux-x64-portable (the default), linux-x64, web, or on an arm64
#               machine linux-arm64-portable, linux-arm64; or macos-dmg: the
#               .dmg of the macOS app's zip (tools/package_dmg.sh), which is
#               REPRO_APP_ZIP or, while the nightly release is this commit's,
#               the release's vibebms-macos-universal.app.zip (the app itself
#               is built on macOS: the Reproducible builds workflow)
#
# It builds the commit's files (git archive, from this clone; fetched from
# origin if missing) in the container image tools/repro.env pins, with that
# commit's pinned packages, Rust (rust-toolchain.toml) and wasm-bindgen
# (Cargo.lock), and tools/build_release.sh, as CI's job does. The web build
# also takes VIBEBMS_THIRD_PARTY (CI: the repository variable WEB_THIRD_PARTY).
# It prints the SHA-256 of each file, then looks them up in the nightly
# release: builds.json (the executables of every nightly of the last 90 days)
# and, when the release is still this commit's, its files. It exits 1 when a
# hash differs. An unsigned APK (vibeBMS-unsigned.apk, from the Reproducible
# builds workflow) is compared whole: the release's signature is copied onto it
# (apksigcopier, tools/apksigcopier.txt: from PATH, else installed in a venv)
# and the result must be the release's vibeBMS.apk byte for byte, which
# apksigner then verifies (when the Android SDK's build-tools are there).
#
# Needs git, Docker (DOCKER=podman for Podman), python3 and the network.
# REPRO_OUT: where the files go (default ./repro-out/<artifact>). These only
# change where and how it builds, never the result (the Reproducible builds
# workflow varies them): REPRO_PATH, the checkout's folder in the container
# (default /vibe); REPRO_HOME, the home there (Rust and Cargo, default
# /root); REPRO_JOBS, cargo's jobs (default: all CPUs). REPRO_COMPARE=0 skips
# the comparison.
#
#   tools/reproduce.sh compare <out dir>    only the comparison, of a build's
#                                           out dir (tools/build_release.sh)
set -euo pipefail
die() { echo "$*" >&2; exit 1; }
here="$(cd "$(dirname "$0")" && pwd)"

# compare <out dir>: its SHA256SUMS against the release's hashes
compare() {
  local out="$1" version commit repo
  { [ -f "$out/SHA256SUMS" ] && [ -f "$out/build.txt" ]; } || die "$out: no SHA256SUMS / build.txt"
  version="$(sed -n 's/^version=//p' "$out/build.txt")"
  commit="$(sed -n 's/^commit=//p' "$out/build.txt")"
  repo="${REPRO_REPO:-$(sed -n 's|^repository = "https://github.com/\(.*\)"$|\1|p' "$here/../Cargo.toml" | head -1)}"
  [ -n "$repo" ] || die "no GitHub repository (REPRO_REPO=owner/name)"
  local tmp auth=()
  tmp="$(mktemp -d)"
  if [ -n "${GH_TOKEN:-${GITHUB_TOKEN:-}}" ]; then
    auth=(-H "Authorization: Bearer ${GH_TOKEN:-$GITHUB_TOKEN}")
  fi
  curl -fsSL --retry 3 -o "$tmp/builds.json" "https://github.com/$repo/releases/download/nightly/builds.json" ||
    echo '{"builds": []}' > "$tmp/builds.json"
  curl -fsSL --retry 3 ${auth[@]+"${auth[@]}"} -o "$tmp/release.json" \
    "https://api.github.com/repos/$repo/releases/tags/nightly" || echo '{}' > "$tmp/release.json"
  local rc=0
  # the unsigned APK with the release's signature (when the release is this
  # commit's): its hash goes in with the others, as vibeBMS.apk
  : > "$tmp/extra.sums"
  if [ -f "$out/vibeBMS-unsigned.apk" ] && [ -n "$commit" ] &&
    [ "$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1])).get("target_commitish", ""))' "$tmp/release.json" 2>/dev/null)" = "$commit" ] &&
    curl -fsSL --retry 3 -o "$tmp/release.apk" "https://github.com/$repo/releases/download/nightly/vibeBMS.apk"; then
    signed_apk "$out/vibeBMS-unsigned.apk" "$tmp/release.apk" "$tmp/vibeBMS.apk" "$tmp" || rc=1
    [ ! -f "$tmp/vibeBMS.apk" ] || (cd "$tmp" && sha256sum vibeBMS.apk) >> "$tmp/extra.sums"
  fi
  if python3 - "$out/SHA256SUMS" "$version" "$commit" "$tmp/builds.json" "$tmp/release.json" "$tmp/extra.sums" <<'PY'
import json, sys
sums, version, commit, builds, release, extra = sys.argv[1:]
ours = {}
for line in [*open(sums), *open(extra)]:
    # ("<sha256>  <name>"; "<sha256> *<name>" from Git for Windows' sha256sum)
    h, name = line.rstrip("\r\n")[:64], line.rstrip("\r\n")[66:]
    ours[name] = h
known = {}
try:
    for b in json.load(open(builds)).get("builds", []):
        if b.get("version") == version:
            known = {n: f["sha256"] for n, f in b.get("files", {}).items()}
except ValueError:
    pass
assets = {}
try:
    rel = json.load(open(release))
    if commit and rel.get("target_commitish") == commit:
        assets = {a["name"]: a["digest"].split(":", 1)[1] for a in rel.get("assets", [])
                  if (a.get("digest") or "").startswith("sha256:")}
except ValueError:
    pass
print(f"\n{version} ({commit})")
print("builds.json: " + ("has this build" if known else "does not have this build"))
print("nightly release: " + ("is this commit" if assets else "is another commit"))
bad = compared = 0
for name, h in ours.items():
    theirs = known.get(name) or assets.get(name)
    if name == "vibeBMS.apk":
        note = "the release's signature copied onto vibeBMS-unsigned.apk"
    elif name.endswith("-unsigned.apk"):
        # (the release has it signed: vibeBMS.apk above, when it is this commit's)
        theirs, note = None, "unsigned: compared as vibeBMS.apk with the release's signature"
    else:
        note = "not published"
    if theirs is None:
        print(f"  {name}: {h}  ({note})")
        continue
    compared += 1
    if theirs == h:
        print(f"  {name}: {h}  same as the release" + (f" ({note})" if name == "vibeBMS.apk" else ""))
    else:
        bad += 1
        print(f"  {name}: {h}  DIFFERS: the release has {theirs}")
if not compared:
    print("nothing to compare: the release has neither this build nor its files")
sys.exit(1 if bad else 0)
PY
  then :; else rc=1; fi
  rm -rf "$tmp"
  return "$rc"
}

# signed_apk <unsigned apk> <signed apk> <out apk> <tmp dir>: the signature of
# <signed apk> copied onto <unsigned apk>, as F-Droid checks a reproducible
# APK (apksigcopier), then verified by apksigner when there is one
signed_apk() {
  local unsigned="$1" signed="$2" result="$3" tmp="$4" copier apksigner
  copier="$(command -v apksigcopier || true)"
  if [ -z "$copier" ]; then
    if ! { python3 -m venv "$tmp/apksigcopier" &&
      "$tmp/apksigcopier/bin/pip" --disable-pip-version-check install -q --require-hashes --only-binary :all: \
        -r "$here/apksigcopier.txt"; }; then
      echo "apksigcopier could not be installed (tools/apksigcopier.txt)" >&2
      return 1
    fi
    copier="$tmp/apksigcopier/bin/apksigcopier"
  fi
  "$copier" copy "$signed" "$unsigned" "$result" ||
    { echo "the release's signature could not be copied onto $unsigned" >&2; return 1; }
  # the pinned build-tools' apksigner, else the newest the SDK has, else PATH's
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}" bt=""
  if [ -n "$sdk" ] && [ -d "$sdk/build-tools" ]; then
    bt="$(. "$here/repro.env"; [ -x "$sdk/build-tools/$BUILD_TOOLS_VERSION/apksigner" ] && echo "$sdk/build-tools/$BUILD_TOOLS_VERSION")" ||
      bt="$(find "$sdk/build-tools" -mindepth 1 -maxdepth 1 | sort -V | tail -1)"
  fi
  apksigner="${bt:+$bt/apksigner}"
  [ -n "$apksigner" ] && [ -x "$apksigner" ] || apksigner="$(command -v apksigner || true)"
  if [ -n "$apksigner" ]; then
    echo "apksigner verify ($apksigner) of vibeBMS.apk, the release's signature on the rebuilt APK:"
    "$apksigner" verify -v --print-certs "$result" 2> /dev/null |
      grep -E '^(Verifies|Verified using v[0-9.]+ scheme .*: true|Number of signers|(Signer #1|V[0-9.]+ Signer): certificate (DN|SHA-256 digest))' ||
      { echo "apksigner: vibeBMS.apk does not verify" >&2; return 1; }
  else
    echo "(no apksigner: the copied signature is not verified; the hash comparison below still holds)"
  fi
}

if [ "${1:-}" = compare ]; then
  [ $# -eq 2 ] || die "usage: $0 compare <out dir>"
  compare "$2"
  exit
fi
{ [ $# -ge 2 ] && [ $# -le 4 ]; } || die "usage: $0 <commit> <run number> [<channel>] [<artifact>]"
commit="$1" run="$2" channel="${3:-nightly}" artifact="${4:-linux-x64-portable}"
[[ "$run" =~ ^[0-9]+$ ]] || die "the run number is not a number: $run"
[[ "$channel" =~ ^[a-z]+$ ]] || die "bad channel: $channel"
docker="${DOCKER:-docker}"

# the commit's files
repo="$(git -C "$here" rev-parse --show-toplevel)"
if ! sha="$(git -C "$repo" rev-parse --verify -q "$commit^{commit}")"; then
  git -C "$repo" fetch -q origin "$commit" || die "no commit $commit here or on origin"
  sha="$(git -C "$repo" rev-parse --verify "FETCH_HEAD^{commit}")"
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
src="$work/src"
mkdir -p "$src"
git -C "$repo" archive "$sha" | tar -x -C "$src"
[ -x "$src/tools/build_release.sh" ] ||
  die "$sha predates reproducible builds (it has no tools/build_release.sh)"
# shellcheck source=tools/repro.env
. "$src/tools/repro.env"
# (a commit from before the WebKitGTK builds moved into containers has no
# LINUX_IMAGE_*: it was built on GitHub's Ubuntu 22.04 runner image)
case "$artifact" in
  linux-x64-portable) image="$PORTABLE_IMAGE_X64" platform=linux/amd64 triple=x86_64-unknown-linux-gnu ;;
  linux-arm64-portable) image="$PORTABLE_IMAGE_ARM64" platform=linux/arm64 triple=aarch64-unknown-linux-gnu ;;
  linux-x64) image="${LINUX_IMAGE_X64:-}" platform=linux/amd64 triple=x86_64-unknown-linux-gnu ;;
  linux-arm64) image="${LINUX_IMAGE_ARM64:-}" platform=linux/arm64 triple=aarch64-unknown-linux-gnu ;;
  web) image="$WEB_IMAGE" platform=linux/amd64 triple=wasm32-unknown-unknown ;;
  macos-dmg)
    [ -x "$src/tools/package_dmg.sh" ] || die "$sha made its .dmg with hdiutil (no tools/package_dmg.sh): it cannot be made again"
    image="$LINUX_IMAGE_X64" platform=linux/amd64 triple=
    ;;
  *) die "$artifact cannot be rebuilt here: linux-x64-portable, linux-arm64-portable, linux-x64, linux-arm64, web or macos-dmg (the others: the Reproducible builds workflow)" ;;
esac
[ -n "$image" ] || die "$sha built $artifact on a runner image, not in a container: the Reproducible builds workflow rebuilds it"
case "$platform:$(uname -m)" in
  linux/amd64:x86_64 | linux/arm64:aarch64 | linux/arm64:arm64) ;;
  *) echo "warning: $artifact on a $(uname -m) machine: the container runs emulated (slow)" >&2 ;;
esac
out="${REPRO_OUT:-$PWD/repro-out/$artifact}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
rm -f "$out/SHA256SUMS" "$out/build.txt"
# the .dmg's input: the app's zip of that build
if [ "$artifact" = macos-dmg ]; then
  if [ -n "${REPRO_APP_ZIP:-}" ]; then
    cp "$REPRO_APP_ZIP" "$out/.app.zip"
  else
    rrepo="${REPRO_REPO:-$(sed -n 's|^repository = "https://github.com/\(.*\)"$|\1|p' "$src/Cargo.toml" | head -1)}"
    target="$(curl -fsSL --retry 3 "https://api.github.com/repos/$rrepo/releases/tags/nightly" |
      python3 -c 'import json, sys; print(json.load(sys.stdin).get("target_commitish", ""))')"
    [ "$target" = "$sha" ] ||
      die "the nightly release is not $sha's: give its app's zip as REPRO_APP_ZIP (the Reproducible builds workflow rebuilds it on macOS)"
    curl -fsSL --retry 3 -o "$out/.app.zip" "https://github.com/$rrepo/releases/download/nightly/vibebms-macos-universal.app.zip"
  fi
fi

# the version CI gave it, and the commit's time
(cd "$src" && VIBEBMS_CHANNEL="$channel" GITHUB_ENV="" tools/ci_version.sh "$run" "$sha" > "$work/version.env")
sde="$(git -C "$repo" show -s --format=%ct "$sha")"
echo "rebuilding $artifact of $sha, run $run ($channel), in $image"

inside="${REPRO_PATH:-/vibe}"
home="${REPRO_HOME:-/root}"
# shellcheck disable=SC2016 # expanded in the container
script='
set -euo pipefail
umask 022
# what the container writes into the mounted folders goes back to the user
trap '"'"'chown -R "$HOST_UID:$HOST_GID" "$REPRO_PATH" /out'"'"' EXIT
mkdir -p "$HOME"
cd "$REPRO_PATH"
. /out/.version.env
tools/build_deps.sh "$( [ "$ARTIFACT" = macos-dmg ] && echo dmg || echo "$ARTIFACT")"
# the .dmg: packaged from the app zip, nothing compiled
if [ "$ARTIFACT" = macos-dmg ]; then
  tools/package_dmg.sh /out/.app.zip /out
  rm -f /out/.app.zip /out/.version.env
  (cd /out && sha256sum vibeBMS-macos-universal.dmg > SHA256SUMS)
  . tools/repro.env
  {
    echo "artifact=$ARTIFACT"
    echo "version=$VIBEBMS_VERSION"
    echo "commit=$VIBEBMS_COMMIT"
    echo "toolchain=hfsprogs $(dpkg-query -W -f "\${Version}" hfsprogs), libdmg-hfsplus $LIBDMG_HFSPLUS_COMMIT"
  } > /out/build.txt
  exit 0
fi
# Rust: rustup, then the toolchain rust-toolchain.toml names
curl --proto "=https" --tlsv1.2 -fsSL --retry 3 https://sh.rustup.rs |
  sh -s -- -y -q --profile minimal --default-toolchain none --no-modify-path
export PATH="$CARGO_HOME/bin:$PATH"
channel="$(sed -n "s/^channel *= *\"\([^\"]*\)\".*/\1/p" rust-toolchain.toml)"
rustup toolchain install "$channel" --profile minimal --target "$TRIPLE"
if [ "$ARTIFACT" = web ]; then
  cargo install -q wasm-bindgen-cli --locked --version \
    "$(grep -A1 "^name = \"wasm-bindgen\"$" Cargo.lock | sed -n "s/^version = \"\(.*\)\"$/\1/p")"
fi
tools/build_release.sh "$ARTIFACT" /out
rm -f /out/.version.env
'
cp "$work/version.env" "$out/.version.env"
jobs=()
[ -z "${REPRO_JOBS:-}" ] || jobs=(-e "CARGO_BUILD_JOBS=$REPRO_JOBS")
"$docker" run --rm --platform "$platform" --hostname rebuild \
  -v "$src:$inside" -v "$out:/out" -w "$inside" \
  -e REPRO_PATH="$inside" -e HOME="$home" -e CARGO_HOME="$home/.cargo" -e RUSTUP_HOME="$home/.rustup" \
  -e CARGO_TARGET_DIR="$home/target" \
  -e ARTIFACT="$artifact" -e TRIPLE="$triple" -e SOURCE_DATE_EPOCH="$sde" \
  -e VIBEBMS_THIRD_PARTY="${VIBEBMS_THIRD_PARTY:-}" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  ${jobs[@]+"${jobs[@]}"} "$image" bash -c "$script"

echo "image=$image" >> "$out/build.txt"
echo
echo "SHA-256 ($out):"
cat "$out/SHA256SUMS"
[ "${REPRO_COMPARE:-1}" = 0 ] || compare "$out"
