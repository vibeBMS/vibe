#!/usr/bin/env bash
# THIRD_PARTY_LICENSES.md: the licences of the crates the releases are built
# from (every target of tools/build_release.sh), made with cargo-about from
# Cargo.lock and about.toml (docs/BUILDING.md, "Development").
#
#   tools/gen_licenses.sh           write it
#   tools/gen_licenses.sh --check   whether the committed one is current (CI)
#
# cargo-about is the version below: one on PATH if it is that version (on
# other machines: cargo install cargo-about --locked --version <it>), else on
# x86_64 Linux its release download, checked against the hash below. It
# builds nothing: it reads `cargo metadata` and the crates' sources, which
# `cargo fetch` downloads for every target first; it then reads no network
# (--frozen), so the file is the same on every machine.
set -euo pipefail
version=0.9.3
# cargo-about-<version>-x86_64-unknown-linux-musl.tar.gz
sha256=3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4

root="$(cd "$(dirname "$0")/.." && pwd)"
check=
case "${1:-}" in
  --check) check=1 ;;
  "") ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

about="$(command -v cargo-about || true)"
if [ -z "$about" ] || [ "$("$about" --version)" != "cargo-about $version" ]; then
  [ "$(uname -sm)" = "Linux x86_64" ] ||
    { echo "cargo-about $version is needed: cargo install cargo-about --locked --version $version" >&2; exit 1; }
  name="cargo-about-$version-x86_64-unknown-linux-musl"
  curl -fsSL --retry 3 -o "$tmp/$name.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-about/releases/download/$version/$name.tar.gz"
  echo "$sha256  $tmp/$name.tar.gz" | sha256sum -c --quiet -
  tar -xzf "$tmp/$name.tar.gz" -C "$tmp" "$name/cargo-about"
  about="$tmp/$name/cargo-about"
fi

cd "$root"
cargo fetch --locked --quiet
"$about" generate --frozen --workspace --fail --format json -c about.toml -o "$tmp/about.json" 2> "$tmp/about.log" ||
  { cat "$tmp/about.log" >&2; exit 1; }
python3 -I tools/gen_licenses.py "$tmp/about.json" > "$tmp/THIRD_PARTY_LICENSES.md"

if [ -z "$check" ]; then
  cp "$tmp/THIRD_PARTY_LICENSES.md" THIRD_PARTY_LICENSES.md
  exit 0
fi
if ! diff -u THIRD_PARTY_LICENSES.md "$tmp/THIRD_PARTY_LICENSES.md"; then
  echo "THIRD_PARTY_LICENSES.md is not what Cargo.lock gives: run tools/gen_licenses.sh and commit it (read any new licence first)" >&2
  exit 1
fi
