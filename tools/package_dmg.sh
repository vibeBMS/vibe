#!/usr/bin/env bash
# Package the macOS app's zip (tools/package_macos.sh) as the .dmg people
# download, reproducibly: the same zip and SOURCE_DATE_EPOCH give the same
# bytes on any machine (docs/BUILDING.md, "Reproducible builds").
#
#   tools/package_dmg.sh <vibebms-macos-universal.app.zip> <out dir>
#     -> <out dir>/vibeBMS-macos-universal.dmg
#
# The image is what hdiutil made before: an HFS+ volume named after the app
# ("vibeBMS Nightly") with the app and a link to /Applications, no .DS_Store
# (Finder's default window), in a zlib-compressed UDIF image (UDZO). It is
# made on Linux, with the tools of libdmg-hfsplus (a fork Mozilla keeps for
# Firefox's .dmg) and hfsprogs, in the pinned image of tools/repro.env, where
# tools/build_deps.sh dmg installs them (libdmg-hfsplus built at
# LIBDMG_HFSPLUS_COMMIT):
# 1. mkfs.hfsplus (hfsprogs) makes an empty HFS+ volume in a file;
# 2. libdmg-hfsplus's `hfsplus` adds each folder and file (`mkdir`, `add`,
#    `chmod`) in name order, then the link (`symlink`). Not `addall`: it
#    adds a folder's entries in the order readdir gives them, which on ext4
#    (and overlayfs over it) follows a hash seeded per file system, and that
#    order is the order of the catalog node IDs and of the files' blocks;
# 3. tools/hfsplus_pin.py sets what would differ from one run to the next:
#    the volume header's dates (created, modified, checked: SOURCE_DATE_EPOCH;
#    backed up: never) in both copies of the header, every folder's and file's
#    dates (SOURCE_DATE_EPOCH, mkfs.hfsplus and `hfsplus` write the time they
#    run), and the volume's 64-bit ID in the header's Finder info (mkfs.hfsplus
#    hashes the time, the load average and the CPU time used into it), made
#    from a hash of the image instead;
# 4. libdmg-hfsplus's `dmg build` puts it in an Apple partition map (an
#    Apple_HFSX partition; hdiutil's had a GUID partition table) and
#    compresses it to UDIF (zlib, level 9, runs in order whatever thread
#    compressed them). Its own fields are fixed: the
#    CRC-32 checksums are the data's, the resources' plist has no dates, and
#    the koly block's segment ID is glibc's rand() unseeded (the same numbers
#    every run).
# fsck.hfsplus checks the volume before it is compressed.
# Nothing in it is signed: the app inside is (ad hoc, by tools/package_macos.sh).
#
# Needs mkfs.hfsplus, fsck.hfsplus, hfsplus, dmg, unzip and python3 (tools/build_deps.sh dmg),
# and SOURCE_DATE_EPOCH (the commit's time; from git when unset).
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: $0 <vibebms-macos-universal.app.zip> <out dir>" >&2; exit 2; }
die() { echo "$*" >&2; exit 1; }
here="$(cd "$(dirname "$0")" && pwd)"
zip="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")" out="$2"
for t in mkfs.hfsplus fsck.hfsplus hfsplus dmg unzip python3; do
  command -v "$t" > /dev/null || die "no $t: tools/build_deps.sh dmg installs the tools"
done
sde="${SOURCE_DATE_EPOCH:-$(git -C "$here/.." log -1 --format=%ct)}"
umask 022
export LC_ALL=C TZ=UTC
mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# the app, as the zip has it
stage="$work/stage"
mkdir "$stage"
unzip -q "$zip" -d "$stage"
apps="$(cd "$stage" && find . -mindepth 1 -maxdepth 1 | sed 's|^\./||')"
case "$apps" in
  *.app) [ "$(printf '%s\n' "$apps" | wc -l)" -eq 1 ] || die "$1: more than one entry at the top" ;;
  *) die "$1: not an app's zip" ;;
esac
[ -z "$(find "$stage" -type l)" ] || die "$1: the app has symbolic links (not handled)"
[ -z "$(find "$stage" -name '*[^ -~]*')" ] || die "$1: a name that is not ASCII (hfsplus takes ASCII names)"
volume="${apps%.app}"

# room for the files (in 4 KiB blocks) and the volume's own structures
bytes="$(find "$stage" -type f -printf '%s\n' | awk '{ b += int(($1 + 4095) / 4096) * 4096 } END { print b + 0 }')"
mib=$(((bytes * 11 / 10 + 4 * 1048576 + 1048575) / 1048576))
img="$work/volume.hfs"
truncate -s "${mib}M" "$img"
echo "== $volume: HFS+ volume of $mib MiB"
mkfs.hfsplus -v "$volume" "$img" > /dev/null

# each folder and file in name order (a folder before what it holds): folders
# 755, files 644 or, if executable, 755 (as tools/package_macos.sh made them)
(cd "$stage" && find . -mindepth 1 | sed 's|^\./||' | sort) | while IFS= read -r p; do
  if [ -d "$stage/$p" ]; then
    hfsplus "$img" mkdir "/$p" > /dev/null
  else
    hfsplus "$img" add "$stage/$p" "/$p" > /dev/null
    if [ -x "$stage/$p" ]; then hfsplus "$img" chmod 755 "/$p" > /dev/null; fi
  fi
done
# (a symbolic link's mode stays 644: macOS does not read it)
hfsplus "$img" symlink /Applications /Applications > /dev/null
hfsplus "$img" ls / | sed 's/^/  /'

python3 "$here/hfsplus_pin.py" "$img" "$sde"
# the volume as macOS's fsck_hfs sees it (hfsprogs' port of it)
fsck.hfsplus -fn "$img"

dmg="$out/vibeBMS-macos-universal.dmg"
rm -f "$dmg"
dmg -c zlib -l 9 build "$img" "$dmg" > /dev/null
ls -l "$dmg"
