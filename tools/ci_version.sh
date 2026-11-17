#!/usr/bin/env bash
# CI builds are versioned <major>.<minor>.<run>+<commit>: the workspace's
# major.minor, the CI run number (it only grows, which Android and the iOS
# sideloading stores need to see an update) and the commit. It is written
# into Cargo.toml, so CARGO_PKG_VERSION carries it; local builds keep the
# committed version and have no "+<commit>".
set -euo pipefail
base="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "(\d+)\.(\d+)\./) { print "$1.$2"; exit }' Cargo.toml)"
[ -n "$base" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
run="${GITHUB_RUN_NUMBER:?}"
version="$base.$run+${GITHUB_SHA:0:7}"
perl -pi -e 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  s/^version = "[^"]*"/version = "'"$version"'"/ if $w' Cargo.toml
echo "VIBEBMS_VERSION=$version" >> "$GITHUB_ENV"
echo "VIBEBMS_BUILD=$run" >> "$GITHUB_ENV"
echo "version $version"
