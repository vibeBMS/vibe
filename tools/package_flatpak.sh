#!/usr/bin/env bash
# Package the Linux executable as a Flatpak bundle (platform/flatpak):
#   <out>/vibeBMS-linux-x64.flatpak    `flatpak install` takes it and fetches the
#                                      runtime (org.gnome.Platform) from Flathub
#
#   tools/package_flatpak.sh <vibebms> <out dir>
#
# <vibebms> is the `browser` build (vibebms-linux-x64): the runtime has its
# WebKitGTK 4.1. The identity comes from tools/ci_version.sh (VIBEBMS_APP_ID,
# VIBEBMS_APP_NAME, VIBEBMS_VERSION, VIBEBMS_CHANNEL, which is the branch);
# a local run is io.github.vibebms.dev. Needs flatpak and flatpak-builder; the
# runtime and SDK are installed for the user from Flathub when no installation
# has them. desktop-file-validate and appstreamcli check the desktop entry and
# the AppStream data when they are there. DRY_RUN=1 stops before the build,
# with the sources filled in (<out>/flatpak-src).
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
    -e "s/@DATE@/$(date -u +%Y-%m-%d)/g" "$root/platform/flatpak/$f" > "$src/$f"
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

step "runtime and SDK"
field() { sed -n "s/^$1: *'\{0,1\}\([^']*\)'\{0,1\}$/\1/p" "$src/vibebms.yml"; }
rt_version="$(field runtime-version)"
for ref in "$(field runtime)//$rt_version" "$(field sdk)//$rt_version"; do
  if ! flatpak info "$ref" >/dev/null 2>&1; then
    flatpak remote-add --user --if-not-exists flathub "$flathub"
    flatpak install --user --noninteractive flathub "$ref"
  fi
done

step "build"
# rofiles-fuse needs FUSE, which containers and CI runners may not allow
flatpak-builder --force-clean --disable-rofiles-fuse --state-dir="$out/flatpak-state" \
  --default-branch="$branch" --repo="$out/flatpak-repo" "$out/flatpak-build" "$src/vibebms.yml"

step "bundle"
flatpak build-bundle --runtime-repo="$flathub" "$out/flatpak-repo" "$bundle" "$id" "$branch"
ls -l "$bundle"
echo "done"
