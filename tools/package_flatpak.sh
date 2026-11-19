#!/usr/bin/env bash
# Package the Linux executable as a Flatpak bundle (platform/flatpak):
#   <out>/vibeBMS-linux-<x64|arm64>.flatpak   `flatpak install` takes it and fetches the
#                                      runtime (org.gnome.Platform) from Flathub
#
#   tools/package_flatpak.sh <vibebms> <out dir>
#
# <vibebms> is the `browser` build of this machine's architecture (vibebms-linux-x64
# or -arm64): the runtime has its
# WebKitGTK 4.1. The identity comes from tools/ci_version.sh (VIBEBMS_APP_ID,
# VIBEBMS_APP_NAME, VIBEBMS_VERSION, VIBEBMS_CHANNEL, which is the branch);
# a local run is io.github.vibebms.dev. Nothing is compiled, so there is no
# flatpak-builder and no SDK: the script lays out the app's files and its
# metadata (runtime, command and permissions from platform/flatpak/vibebms.yml),
# composes the AppStream catalog and icons (appstreamcli compose, as
# flatpak-builder does), then flatpak build-finish, build-export and
# build-bundle. Needs flatpak and appstreamcli with its compose plugin (Debian
# and Ubuntu: appstream-compose); neither the runtime nor the SDK has to be
# installed. faketime, when it is there, makes the bundle reproducible (the
# same bytes from the same executable and commit). desktop-file-validate (when it is there) and appstreamcli check
# the desktop entry and the AppStream data. DRY_RUN=1 stops before the build,
# with the sources filled in (<out>/flatpak-src). It runs with flatpak 1.12 and
# AppStream 0.15.2 (Ubuntu 22.04, where CI packages each executable after
# building it) as well as newer ones: only options and metainfo those know.
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: $0 <vibebms> <out dir>" >&2; exit 2; }
exe="$1" out="$2"
dry="${DRY_RUN:-}"
root="$(cd "$(dirname "$0")/.." && pwd)"
flathub=https://flathub.org/repo/flathub.flatpakrepo
step() { echo "== $*"; }

cargo_version="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "([^"]+)"/) { print $1; exit }' "$root/Cargo.toml")"
id="${VIBEBMS_APP_ID:-io.github.vibebms.dev}"
name="${VIBEBMS_APP_NAME:-vibeBMS Dev}"
version="${VIBEBMS_VERSION:-$cargo_version}"
branch="${VIBEBMS_CHANNEL:-dev}"
# the commit's time, for the release date and the OSTree commit's timestamp: the
# same bundle from the same executable on any day (reproducible builds)
sde="${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct 2>/dev/null || date +%s)}"
date_of() { date -u -d "@$sde" "$1"; }
# they go into YAML, XML and the desktop entry as they are
[[ "$id" =~ ^[A-Za-z0-9._]+$ && "$name" =~ ^[A-Za-z0-9\ ]+$ && "$version" =~ ^[A-Za-z0-9.+-]+$ \
  && "$branch" =~ ^[a-z]+$ ]] || { echo "bad identity: $id / $name / $version / $branch" >&2; exit 1; }
case "$(uname -m)" in
  x86_64) arch=x64 ;;
  aarch64) arch=arm64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
bundle="$out/vibeBMS-linux-$arch.flatpak"
echo "packaging $name ($id, branch $branch) $version into $bundle"

step "sources"
src="$out/flatpak-src"
rm -rf "$src"
mkdir -p "$src"
install -m755 "$exe" "$src/vibebms"
cp "$root"/assets/logo/icon.svg "$root"/assets/logo/icon-{128,256,512}.png "$src/"
for f in vibebms.yml vibebms.desktop vibebms.metainfo.xml; do
  sed -e "s/@APP_ID@/$id/g" -e "s/@APP_NAME@/$name/g" -e "s/@VERSION@/$version/g" \
    -e "s/@DATE@/$(date_of +%Y-%m-%d)/g" "$root/platform/flatpak/$f" > "$src/$f"
done
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$src/vibebms.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net "$src/vibebms.metainfo.xml"
fi
if [ -n "$dry" ]; then
  echo "dry run: not built"
  exit 0
fi

step "build"
field() { sed -n "s/^$1: *'\{0,1\}\([^']*\)'\{0,1\}$/\1/p" "$src/vibebms.yml"; }
# the list under finish-args:, up to the next top-level key
finish_args=()
while read -r a; do finish_args+=("$a"); done \
  < <(sed -n "/^finish-args:/,/^[^ #]/s/^ *- *\(--[^ ]*\) *$/\1/p" "$src/vibebms.yml")
rt_version="$(field runtime-version)" command="$(field command)"
farch="$(flatpak --default-arch)"
if [ -z "$(field runtime)" ] || [ -z "$(field sdk)" ] || [ -z "$rt_version" ] || [ -z "$command" ] \
  || [ ${#finish_args[@]} -eq 0 ]; then
  echo "vibebms.yml: missing fields" >&2
  exit 1
fi
# what `flatpak build-init` writes, without its check that the SDK and the
# runtime are installed (the SDK only matters to `flatpak run --devel`);
# export/ is build-finish's (it refuses a directory that has one)
build="$out/flatpak-build"
rm -rf "$build"
mkdir -p "$build/files"
printf '[Application]\nname=%s\nruntime=%s/%s/%s\nsdk=%s/%s/%s\ncommand=%s\n' "$id" \
  "$(field runtime)" "$farch" "$rt_version" "$(field sdk)" "$farch" "$rt_version" "$command" \
  > "$build/metadata"
app="$build/files"
install -Dm755 "$src/vibebms" "$app/bin/$command"
install -Dm644 "$src/vibebms.desktop" "$app/share/applications/$id.desktop"
install -Dm644 "$src/vibebms.metainfo.xml" "$app/share/metainfo/$id.metainfo.xml"
install -Dm644 "$src/icon.svg" "$app/share/icons/hicolor/scalable/apps/$id.svg"
for n in 128 256 512; do
  install -Dm644 "$src/icon-$n.png" "$app/share/icons/hicolor/${n}x$n/apps/$id.png"
done
# the AppStream catalog (metainfo and desktop entry merged) and its cached
# 64 and 128 px icons, which build-bundle puts in the bundle's header for
# software centres to show before installing; the arguments flatpak-builder
# passes
appstreamcli compose --no-color --prefix=/ --origin="$id" --result-root="$app" \
  --data-dir="$app/share/app-info/xmls" --icons-dir="$app/share/app-info/icons/flatpak" \
  --components="$id" "$app"
[ -f "$app/share/app-info/xmls/$id.xml.gz" ] || { echo "appstreamcli compose: no catalog" >&2; exit 1; }
# the permissions into the metadata; the desktop entry, icons and metainfo
# into export/
flatpak build-finish "${finish_args[@]}" "$build"
rm -rf "$out/flatpak-repo"
flatpak build-export --timestamp="$(date_of +%Y-%m-%dT%H:%M:%SZ)" "$out/flatpak-repo" "$build" "$branch"

step "bundle"
# the bundle (an OSTree static delta) also carries the time it was made, which
# nothing overrides: faketime gives it the commit's time
bundle_cmd=(flatpak build-bundle --runtime-repo="$flathub" "$out/flatpak-repo" "$bundle" "$id" "$branch")
if command -v faketime >/dev/null; then
  TZ=UTC faketime -f "$(date_of '+%Y-%m-%d %H:%M:%S')" "${bundle_cmd[@]}"
else
  echo "note: no faketime, so the bundle carries the time it was made (not reproducible)" >&2
  "${bundle_cmd[@]}"
fi
ls -l "$bundle"
echo "done"
