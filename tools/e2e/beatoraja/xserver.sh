#!/usr/bin/env bash
# Starts the comparison's X server (xorg.conf) on display <n> and waits for
# it; prints the DISPLAY to use. Needs Xorg's dummy driver and xrandr
# (Ubuntu: xserver-xorg-video-dummy, x11-xserver-utils).
#
#   tools/e2e/beatoraja/xserver.sh [<n>]   (default 77)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
n="${1:-77}"
sudo=""
[ "$(id -u)" = 0 ] || sudo="sudo"
log="${TMPDIR:-/tmp}/e2e-xorg-$n.log"
$sudo Xorg ":$n" -noreset -nolisten tcp -config "$here/xorg.conf" -logfile "$log" >/dev/null 2>&1 &
for _ in $(seq 100); do
  if DISPLAY=":$n" xrandr >/dev/null 2>&1; then
    # (a primary output reads "connected primary", which LWJGL 2 does not)
    DISPLAY=":$n" xrandr --output DUMMY0 --noprimary 2>/dev/null || true
    echo ":$n"
    exit 0
  fi
  sleep 0.1
done
echo "X did not start (log: $log)" >&2
exit 1
