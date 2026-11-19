#!/usr/bin/env bash
# The version and identity of a CI build, by channel (VIBEBMS_CHANNEL,
# default "nightly"; later perhaps "preview", "beta" or "stable"):
# - version: <version>-<channel>.<run>+<commit>, e.g. 0.1.0-nightly.192+e129ac9
#   (a stable build is the plain <version>). The run number only grows
#   (Android's versionCode and the iOS CFBundleVersion take it); the commit
#   says which source it is. Written into Cargo.toml, so CARGO_PKG_VERSION
#   carries it. Local builds keep the committed <version>-dev, which never
#   looks for updates.
# - app id: io.github.vibebms.<channel> and the name "vibeBMS <Channel>" for
#   the Android and iOS apps (a stable build is io.github.vibebms, "vibeBMS"),
#   so each channel installs beside the others.
#
#   tools/ci_version.sh [<run number> [<commit>]]
#
# The run number and the commit default to CI's (GITHUB_RUN_NUMBER,
# GITHUB_SHA; the commit else to the checkout's HEAD). The run number is not in
# the commit: rebuilding a release (tools/reproduce.sh) gives it here. In CI
# the variables go to GITHUB_ENV; elsewhere they are printed as `export` lines
# (eval "$(tools/ci_version.sh 300 9454dfe...)").
set -euo pipefail
cd "$(dirname "$0")/.."
channel="${VIBEBMS_CHANNEL:-nightly}"
[[ "$channel" =~ ^[a-z]+$ ]] || { echo "bad channel: $channel" >&2; exit 1; }
base="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "(\d+\.\d+\.\d+)/) { print $1; exit }' Cargo.toml)"
[ -n "$base" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
run="${1:-${GITHUB_RUN_NUMBER:-}}"
[[ "$run" =~ ^[0-9]+$ ]] || { echo "the run number is missing or not a number: '$run'" >&2; exit 1; }
commit="${2:-${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || true)}}"
[[ "$commit" =~ ^[0-9a-f]{7,40}$ ]] || { echo "the commit is missing or not a hex SHA: '$commit'" >&2; exit 1; }
if [ "$channel" = stable ]; then
  version="$base"
  suffix=""
  name="vibeBMS"
else
  version="$base-$channel.$run+${commit:0:7}"
  suffix=".$channel"
  name="vibeBMS $(printf %s "$channel" | perl -pe 's/^(.)/\u$1/')"
fi
old="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "([^"]+)"/) { print $1; exit }' Cargo.toml)"
perl -pi -e 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  s/^version = "[^"]*"/version = "'"$version"'"/ if $w' Cargo.toml
# and the workspace's packages in Cargo.lock (those without a source), so a
# build with --locked takes the lock file as committed otherwise
OLD="$old" NEW="$version" perl -00 -pi -e 'if (/^\[\[package\]\]$/m && !/^source = /m) {
  s/^version = "\Q$ENV{OLD}\E"$/version = "$ENV{NEW}"/m }' Cargo.lock
vars=(
  "VIBEBMS_CHANNEL=$channel"
  "VIBEBMS_VERSION=$version"
  "VIBEBMS_BASE_VERSION=$base"
  "VIBEBMS_BUILD=$run"
  "VIBEBMS_COMMIT=$commit"
  "VIBEBMS_APP_ID=io.github.vibebms$suffix"
  "VIBEBMS_APP_NAME=$name"
)
if [ -n "${GITHUB_ENV:-}" ]; then
  printf '%s\n' "${vars[@]}" >> "$GITHUB_ENV"
else
  for v in "${vars[@]}"; do printf 'export %s=%q\n' "${v%%=*}" "${v#*=}"; done
fi
echo "version $version ($name, io.github.vibebms$suffix)" >&2
