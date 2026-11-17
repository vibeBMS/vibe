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
set -euo pipefail
channel="${VIBEBMS_CHANNEL:-nightly}"
[[ "$channel" =~ ^[a-z]+$ ]] || { echo "bad channel: $channel" >&2; exit 1; }
base="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "(\d+\.\d+\.\d+)/) { print $1; exit }' Cargo.toml)"
[ -n "$base" ] || { echo "no [workspace.package] version in Cargo.toml" >&2; exit 1; }
run="${GITHUB_RUN_NUMBER:?}"
if [ "$channel" = stable ]; then
  version="$base"
  suffix=""
  name="vibeBMS"
else
  version="$base-$channel.$run+${GITHUB_SHA:0:7}"
  suffix=".$channel"
  name="vibeBMS $(printf %s "$channel" | perl -pe 's/^(.)/\u$1/')"
fi
perl -pi -e 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  s/^version = "[^"]*"/version = "'"$version"'"/ if $w' Cargo.toml
{
  echo "VIBEBMS_CHANNEL=$channel"
  echo "VIBEBMS_VERSION=$version"
  echo "VIBEBMS_BASE_VERSION=$base"
  echo "VIBEBMS_BUILD=$run"
  echo "VIBEBMS_APP_ID=io.github.vibebms$suffix"
  echo "VIBEBMS_APP_NAME=$name"
} >> "$GITHUB_ENV"
echo "version $version ($name, io.github.vibebms$suffix)"
