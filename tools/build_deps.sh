#!/usr/bin/env bash
# Install the pinned system packages of a release build made in a container
# (tools/repro.env), as root in that container: CI's jobs and
# tools/reproduce.sh run it there.
#
#   tools/build_deps.sh <artifact>
#
# linux-x64-portable, linux-arm64-portable: in the manylinux_2_28 image (dnf).
# web: in the Ubuntu image, every package from the Ubuntu snapshot service as it
# was at UBUNTU_SNAPSHOT, so the same versions every time (clang above all).
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 <artifact>" >&2; exit 2; }
# shellcheck source=tools/repro.env
. "$(dirname "$0")/repro.env"
case "$1" in
  linux-x64-portable | linux-arm64-portable)
    # shellcheck disable=SC2086
    dnf install -y $PORTABLE_PACKAGES
    ;;
  web)
    . /etc/os-release
    case "$(dpkg --print-architecture)" in
      amd64 | i386) archive=ubuntu ;;
      *) archive=ubuntu-ports ;;
    esac
    # the snapshot service answers over HTTPS only: its certificate is
    # checked with the image archive's ca-certificates, which touch no
    # build output (the packages are checked by the archive's keys in the
    # image, which sign the snapshot too)
    if [ ! -s /etc/ssl/certs/ca-certificates.crt ]; then
      apt-get -o Acquire::Retries=3 update
      DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=3 install -y --no-install-recommends ca-certificates
    fi
    rm -f /etc/apt/sources.list.d/ubuntu.sources
    for suite in "$VERSION_CODENAME" "$VERSION_CODENAME-updates" "$VERSION_CODENAME-security"; do
      echo "deb https://snapshot.ubuntu.com/$archive/$UBUNTU_SNAPSHOT $suite main universe"
    done > /etc/apt/sources.list
    apt-get -o Acquire::Retries=3 update
    # shellcheck disable=SC2086
    DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=3 install -y --no-install-recommends $WEB_PACKAGES
    ;;
  *) echo "$1 is not built in a container" >&2; exit 2 ;;
esac
