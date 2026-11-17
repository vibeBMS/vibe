#!/usr/bin/env bash
# CI builds are nightlies of the version Cargo.toml names:
# <version>-nightly.<run>+<commit>, e.g. 0.1.0-nightly.192+e129ac9. The run
# number only grows (Android's versionCode and the iOS CFBundleVersion take
# it); the commit says which source it is. Written into Cargo.toml, so
# CARGO_PKG_VERSION carries it. Local builds keep the committed
# <version>-dev, which never looks for updates.
set -euo pipefail
base="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "(\d+\.\d+\.\d+)/) { print $1; exit }' Cargo.toml)"
[ -n "$base" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
run="${GITHUB_RUN_NUMBER:?}"
version="$base-nightly.$run+${GITHUB_SHA:0:7}"
perl -pi -e 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  s/^version = "[^"]*"/version = "'"$version"'"/ if $w' Cargo.toml
{
  echo "VIBEBMS_VERSION=$version"
  echo "VIBEBMS_BASE_VERSION=$base"
  echo "VIBEBMS_BUILD=$run"
} >> "$GITHUB_ENV"
echo "version $version"
