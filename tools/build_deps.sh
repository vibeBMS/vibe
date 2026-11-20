#!/usr/bin/env bash
# Install the pinned system packages of a release build made in a container
# (tools/repro.env), as root in that container: CI's jobs and
# tools/reproduce.sh run it there.
#
#   tools/build_deps.sh <artifact>
#   tools/build_deps.sh lock linux-arm64 > tools/linux-arm64.lock
#
# linux-x64-portable, linux-arm64-portable: in the manylinux_2_28 image (dnf).
# linux-x64 (Ubuntu 22.04) and web (Ubuntu 26.04): in the Ubuntu image, every
# package from the Ubuntu snapshot service as it was at UBUNTU_SNAPSHOT, so the
# same versions every time (gcc, clang, the libraries' headers).
# linux-arm64 (Ubuntu 22.04): its packages come from the ports archive, which
# the snapshot service has no copy of and which keeps only the newest version
# of a package; tools/linux-arm64.lock lists each by version and SHA-256, and
# each is fetched from ports.ubuntu.com or, once it is gone from there, from
# Launchpad's librarian, which keeps every package Ubuntu published (plain HTTP
# both: the SHA-256 checks them).
#
# lock: the lock of linux-arm64 as the ports archive resolves its packages now,
# run as root in the image (docker run --platform linux/arm64 -v "$PWD:/vibe"
# -w /vibe "$LINUX_IMAGE_ARM64" tools/build_deps.sh lock linux-arm64), or
# anywhere with LOCK_ROOT=<dir> holding the image's /etc/apt and /var/lib/dpkg
# (this machine's apt resolves for that root; needs curl).
set -euo pipefail
{ [ $# -eq 1 ] || [ "${1:-}" = lock ]; } || { echo "usage: $0 <artifact> | lock linux-arm64" >&2; exit 2; }
die() { echo "$*" >&2; exit 1; }
# shellcheck source=tools/repro.env
. "$(dirname "$0")/repro.env"

# apt from the snapshot at UBUNTU_SNAPSHOT: <packages>
snapshot_install() {
  local suite apt=(apt-get -o Acquire::Retries=3)
  . /etc/os-release
  [ "$(dpkg --print-architecture)" = amd64 ] || die "the snapshot service has the amd64 archive only"
  # the snapshot service answers over HTTPS only: its certificate is
  # checked with the image archive's ca-certificates, which touch no
  # build output (the packages are checked by the archive's keys in the
  # image, which sign the snapshot too)
  if [ ! -s /etc/ssl/certs/ca-certificates.crt ]; then
    "${apt[@]}" update
    DEBIAN_FRONTEND=noninteractive "${apt[@]}" install -y --no-install-recommends ca-certificates
  fi
  rm -f /etc/apt/sources.list.d/ubuntu.sources
  for suite in "$VERSION_CODENAME" "$VERSION_CODENAME-updates" "$VERSION_CODENAME-security"; do
    echo "deb https://snapshot.ubuntu.com/ubuntu/$UBUNTU_SNAPSHOT $suite main universe"
  done > /etc/apt/sources.list
  # three tries: a download cut off can come back as a hash mismatch, which
  # apt's own retries resume rather than fetch again (the packages already
  # fetched stay in apt's cache; the cut-off one is fetched anew)
  local try
  for try in 1 2 3; do
    # every phased update: apt would otherwise take one or not by the
    # machine's id, so two machines could get different versions
    "${apt[@]}" update &&
      DEBIAN_FRONTEND=noninteractive "${apt[@]}" -o APT::Get::Always-Include-Phased-Updates=true \
        install -y --no-install-recommends "$@" && return
    [ "$try" = 3 ] && die "apt failed three times"
    rm -rf /var/cache/apt/archives/partial/* /var/lib/apt/lists/partial/*
    echo "apt failed, trying again in $((try * 15)) s" >&2
    sleep $((try * 15))
  done
}

ports=http://ports.ubuntu.com/ubuntu-ports
# a lock's line: <package>:<arch> <version> <sha256> <path in the archive> <librarian URL or ->

# the packages of <lock>, as listed
lock_install() {
  local lock="$1" dir
  [ -f "$lock" ] || die "no $lock"
  if [ "$(sed -n 's/^# image: //p' "$lock")" != "$LINUX_IMAGE_ARM64" ] ||
    [ "$(sed -n 's/^# packages: //p' "$lock")" != "$LINUX_PACKAGES" ]; then
    die "$lock is not for LINUX_IMAGE_ARM64 and LINUX_PACKAGES of tools/repro.env: make it again (tools/build_deps.sh lock linux-arm64, at the top of $0)"
  fi
  dir="$(mktemp -d)"
  chmod 755 "$dir"
  chown _apt "$dir" 2>/dev/null || true
  # shellcheck disable=SC2016 # expanded by sh
  grep -v '^#' "$lock" | xargs -P 8 -L 1 sh -c '
    f="$0/${4##*/}"
    /usr/lib/apt/apt-helper -o Acquire::Retries=3 download-file "'"$ports"'/$4" "$f" "SHA256:$3" > /dev/null 2>&1 ||
      { [ "$5" != - ] && /usr/lib/apt/apt-helper -o Acquire::Retries=3 download-file "$5" "$f" "SHA256:$3" > /dev/null; } ||
      { echo "$1 $2: not in the ports archive and not at $5 (or another SHA-256)" >&2; exit 255; }' "$dir"
  [ "$(find "$dir" -name '*.deb' | wc -l)" -eq "$(grep -vc '^#' "$lock")" ] || die "not every package of $lock was fetched"
  # only these files: no archive's index is read
  rm -rf /var/lib/apt/lists/*
  DEBIAN_FRONTEND=noninteractive apt-get -o Dir::Etc::SourceList=/dev/null -o Dir::Etc::SourceParts=/nonexistent \
    install -y --no-install-recommends --allow-downgrades "$dir"/*.deb
  rm -rf "$dir"
  # each at the lock's version
  grep -v '^#' "$lock" | while read -r pkg ver _; do
    [ "$(dpkg-query -W -f '${Version}' "${pkg%:all}")" = "$ver" ] || die "$pkg is not $ver"
  done
}

# the lock of <artifact> (linux-arm64), on stdout
lock_make() {
  local root="${LOCK_ROOT:-}" apt=(-o Acquire::Retries=3) suite name ver arch q show file sha url
  if [ -n "$root" ]; then
    root="$(cd "$root" && pwd)"
    apt+=(-o "Dir=$root" -o Dir::Bin::Methods=/usr/lib/apt/methods -o Dir::Bin::dpkg=/usr/bin/dpkg
      -o "Dir::State::status=$root/var/lib/dpkg/status" -o APT::Architecture=arm64 -o "APT::Architectures::=arm64"
      -o Debug::NoLocking=1 -o APT::Sandbox::User=root)
    mkdir -p "$root/var/lib/apt/lists/partial" "$root/var/cache/apt/archives/partial"
  fi
  # shellcheck disable=SC1091
  . "$root/etc/os-release"
  rm -f "$root/etc/apt/sources.list.d/ubuntu.sources"
  for suite in "$VERSION_CODENAME" "$VERSION_CODENAME-updates" "$VERSION_CODENAME-security"; do
    echo "deb $ports $suite main universe"
  done > "$root/etc/apt/sources.list"
  apt-get "${apt[@]}" -qq update >&2
  # what installing them would install, each with its version (every phased update)
  # shellcheck disable=SC2086 # the list is words
  apt-get "${apt[@]}" -s -o APT::Get::Always-Include-Phased-Updates=true install --no-install-recommends $LINUX_PACKAGES |
    sed -n 's/^Inst \([^ ]*\) \(\[[^]]*\] \)\{0,1\}(\([^ ]*\) [^[]*\[\([^]]*\)\]).*$/\1 \3 \4/p' | LC_ALL=C sort > "$root/lock.txt"
  [ -s "$root/lock.txt" ] || die "apt would install nothing"
  # Launchpad's address of each (HTTPS to look it up; its librarian also answers plain HTTP)
  if ! command -v curl > /dev/null; then
    DEBIAN_FRONTEND=noninteractive apt-get "${apt[@]}" -qq install -y --no-install-recommends ca-certificates curl >&2
  fi
  echo "# tools/build_deps.sh lock linux-arm64 (tools/build_deps.sh installs these, tools/repro.env)"
  echo "# image: $LINUX_IMAGE_ARM64"
  echo "# packages: $LINUX_PACKAGES"
  echo "# resolved: $(date -u +%Y-%m-%dT%H:%M:%SZ), $ports $VERSION_CODENAME, -updates, -security (main, universe)"
  while read -r name ver arch; do
    q="$name:$arch=$ver"
    [ "$arch" != all ] || q="$name=$ver"
    show="$(apt-cache "${apt[@]}" show "$q")"
    file="$(sed -n 's/^Filename: //p' <<< "$show" | head -1)"
    sha="$(sed -n 's/^SHA256: //p' <<< "$show" | head -1)"
    url="$(curl -fsS --retry 3 -o /dev/null -w '%{redirect_url}' "https://launchpad.net/ubuntu/+archive/primary/+files/${file##*/}" || true)"
    case "$url" in https://launchpadlibrarian.net/*) url="http://${url#https://}" ;; *) url=- ;; esac
    echo "$name:$arch $ver $sha $file $url"
  done < "$root/lock.txt"
  rm -f "$root/lock.txt"
}

if [ "$1" = lock ]; then
  [ "${2:-}" = linux-arm64 ] || die "usage: $0 lock linux-arm64"
  lock_make
  exit
fi
# shellcheck disable=SC2086 # the lists are words
case "$1" in
  linux-x64-portable | linux-arm64-portable)
    # (a mirror now and then serves an empty repomd.xml: a fresh one is tried)
    for try in 1 2 3; do
      dnf install -y $PORTABLE_PACKAGES && break
      [ "$try" = 3 ] && exit 1
      dnf clean metadata
      sleep $((try * 10))
    done
    ;;
  linux-x64) snapshot_install $LINUX_PACKAGES ;;
  linux-arm64) lock_install "$(dirname "$0")/linux-arm64.lock" ;;
  web) snapshot_install $WEB_PACKAGES ;;
  *) echo "$1 is not built in a container" >&2; exit 2 ;;
esac
