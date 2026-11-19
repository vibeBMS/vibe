#!/usr/bin/env bash
# Rebuild a released artifact from its commit and compare its SHA-256 with the
# release's (docs/BUILDING.md, "Reproducible builds"):
#
#   tools/reproduce.sh <commit> <run number> [<channel>] [<artifact>]
#
# <run number>  the CI run that built it: the N of 0.1.0-nightly.N+abc1234, the
#               version the build reports (also in latest.json and builds.json)
# <channel>     nightly (the default)
# <artifact>    linux-x64-portable (the default), linux-arm64-portable (on an
#               arm64 machine) or web
#
# It builds the commit's files (git archive, from this clone; fetched from
# origin if missing) in the container image tools/repro.env pins, with that
# commit's pinned packages, Rust (rust-toolchain.toml) and wasm-bindgen
# (Cargo.lock), and tools/build_release.sh, as CI's job does. The web build
# also takes VIBEBMS_THIRD_PARTY (CI: the repository variable WEB_THIRD_PARTY).
# It prints the SHA-256 of each file, then looks them up in the nightly
# release: builds.json (the executables of every nightly of the last 90 days)
# and, when the release is still this commit's, its files. It exits 1 when a
# hash differs.
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
  if python3 - "$out/SHA256SUMS" "$version" "$commit" "$tmp/builds.json" "$tmp/release.json" <<'PY'
import json, sys
sums, version, commit, builds, release = sys.argv[1:]
ours = {}
for line in open(sums):
    h, name = line.rstrip("\n").split("  ", 1)
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
    if name.endswith(".apk"):
        # signed: the key's signature differs; its files are compared
        theirs, note = None, "signed (its files are compared)"
    else:
        note = "not published"
    if theirs is None:
        print(f"  {name}: {h}  ({note})")
        continue
    compared += 1
    if theirs == h:
        print(f"  {name}: {h}  same as the release")
    else:
        bad += 1
        print(f"  {name}: {h}  DIFFERS: the release has {theirs}")
if not compared:
    print("nothing to compare: the release has neither this build nor its files")
sys.exit(1 if bad else 0)
PY
  then :; else rc=$?; fi
  rm -rf "$tmp"
  return "$rc"
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
case "$artifact" in
  linux-x64-portable) image="$PORTABLE_IMAGE_X64" platform=linux/amd64 triple=x86_64-unknown-linux-gnu ;;
  linux-arm64-portable) image="$PORTABLE_IMAGE_ARM64" platform=linux/arm64 triple=aarch64-unknown-linux-gnu ;;
  web) image="$WEB_IMAGE" platform=linux/amd64 triple=wasm32-unknown-unknown ;;
  *) die "$artifact cannot be rebuilt here: linux-x64-portable, linux-arm64-portable or web (the others: the Reproducible builds workflow)" ;;
esac
case "$platform:$(uname -m)" in
  linux/amd64:x86_64 | linux/arm64:aarch64 | linux/arm64:arm64) ;;
  *) echo "warning: $artifact on a $(uname -m) machine: the container runs emulated (slow)" >&2 ;;
esac
out="${REPRO_OUT:-$PWD/repro-out/$artifact}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
rm -f "$out/SHA256SUMS" "$out/build.txt"

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
tools/build_deps.sh "$ARTIFACT"
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

echo
echo "SHA-256 ($out):"
cat "$out/SHA256SUMS"
[ "${REPRO_COMPARE:-1}" = 0 ] || compare "$out"
