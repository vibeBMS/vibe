#!/usr/bin/env bash
# Downloads the beatoraja release and the Java runtime of versions.env into
# <dir> (beatoraja/ and jre/), each checked against its SHA-256. Nothing is
# done when <dir> already holds them (the Actions cache). beatoraja's own
# skins other than its default one (ModernChic, PeacefulPlay: most of the
# download) are not extracted.
#
#   tools/e2e/beatoraja/fetch.sh <dir>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=versions.env
. "$here/versions.env"
dir="$1"
mkdir -p "$dir"
stamp="$dir/versions.env"
if cmp -s "$here/versions.env" "$stamp" && [ -f "$dir/beatoraja/beatoraja.jar" ] && [ -x "$dir/jre/bin/java" ]; then
  echo "beatoraja $BEATORAJA_VERSION and Java $JRE_VERSION already in $dir"
  exit 0
fi
rm -rf "$dir/beatoraja" "$dir/jre" "$stamp"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fsSL --retry 3 -o "$tmp/jre.tar.gz" "$JRE_URL"
echo "$JRE_SHA256  $tmp/jre.tar.gz" | sha256sum -c -
mkdir -p "$dir/jre"
tar -xzf "$tmp/jre.tar.gz" -C "$dir/jre" --strip-components=1
rm "$tmp/jre.tar.gz"

curl -fsSL --retry 3 -o "$tmp/beatoraja.zip" "$BEATORAJA_URL"
echo "$BEATORAJA_SHA256  $tmp/beatoraja.zip" | sha256sum -c -
mkdir -p "$tmp/x"
unzip -q "$tmp/beatoraja.zip" -d "$tmp/x" -x '*/skin/ModernChic/*' '*/skin/PeacefulPlay/*'
rm "$tmp/beatoraja.zip"
mv "$tmp/x/"*/ "$dir/beatoraja"
cp "$here/versions.env" "$stamp"
du -sh "$dir/beatoraja" "$dir/jre"
