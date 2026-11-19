#!/usr/bin/env bash
# Package the macOS app from the arm64 and x86_64 executables:
#   <out>/<name>.app                         one universal executable, ad-hoc signed
#   <out>/vibeBMS-macos-universal.dmg        the app and a link to /Applications (for people)
#   <out>/vibebms-macos-universal.app.zip    the app alone (what the self-update downloads)
#
#   tools/package_macos.sh <arm64 vibebms> <x86_64 vibebms> <out dir>
#
# The identity comes from tools/ci_version.sh (VIBEBMS_APP_ID, VIBEBMS_APP_NAME,
# VIBEBMS_BASE_VERSION, VIBEBMS_BUILD); a local run is io.github.vibebms.dev.
# Needs lipo, codesign, ditto and hdiutil (macOS): lipo and codesign of the
# Xcode tools/repro.env pins (tools/toolchain.sh). DRY_RUN=1 runs it anywhere:
# the first executable stands in for the universal one, nothing is signed,
# `zip` makes the zip and there is no .dmg.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: $0 <arm64 vibebms> <x86_64 vibebms> <out dir>" >&2; exit 2; }
arm="$1" x64="$2" out="$3"
dry="${DRY_RUN:-}"
step() { echo "== $*"; }
if [ -z "$dry" ]; then
  # shellcheck source=tools/repro.env
  . "$(dirname "$0")/repro.env"
  # shellcheck source=tools/toolchain.sh
  . "$(dirname "$0")/toolchain.sh"
  use_xcode
fi

base="$(perl -ne 'if (/^\[workspace\.package\]/) { $w = 1 } elsif (/^\[/) { $w = 0 }
  if ($w && /^version = "(\d+\.\d+\.\d+)/) { print $1; exit }' "$(dirname "$0")/../Cargo.toml")"
id="${VIBEBMS_APP_ID:-io.github.vibebms.dev}"
name="${VIBEBMS_APP_NAME:-vibeBMS Dev}"
version="${VIBEBMS_BASE_VERSION:-$base}"
build="${VIBEBMS_BUILD:-0}"
# the bundle's file name is what the Finder shows: each channel installs
# beside the others ("vibeBMS Nightly.app")
app="$out/$name.app"
echo "packaging $name ($id) $version ($build) into $out"

step "bundle"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
if [ -n "$dry" ]; then
  cp "$arm" "$app/Contents/MacOS/vibebms"
else
  lipo -create -output "$app/Contents/MacOS/vibebms" "$arm" "$x64"
  lipo -info "$app/Contents/MacOS/vibebms"
fi
chmod 755 "$app/Contents/MacOS/vibebms"
cp "$(dirname "$0")/../platform/macos/vibeBMS.icns" "$app/Contents/Resources/vibeBMS.icns"

step "Info.plist"
esc() { printf %s "$1" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g'; }
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>vibebms</string>
	<key>CFBundleIconFile</key>
	<string>vibeBMS</string>
	<key>CFBundleIdentifier</key>
	<string>$(esc "$id")</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>$(esc "$name")</string>
	<key>CFBundleDisplayName</key>
	<string>$(esc "$name")</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>$(esc "$version")</string>
	<key>CFBundleVersion</key>
	<string>$(esc "$build")</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.games</string>
	<key>LSMinimumSystemVersion</key>
	<string>11.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
</dict>
</plist>
EOF
if [ -z "$dry" ]; then plutil -lint "$app/Contents/Info.plist"; fi

# the modes as a umask of 022 makes them (the zip and the .dmg keep them)
chmod -R u+rwX,go=rX "$app"

step "sign (ad hoc)"
if [ -z "$dry" ]; then
  codesign --force --deep -s - "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
fi

step "zip (self-update)"
zip="$out/vibebms-macos-universal.app.zip"
rm -f "$zip"
# every file with the commit's time and no extended attributes, so the zip is
# the same for the same executables (reproducible builds; the .dmg is not)
sde="${SOURCE_DATE_EPOCH:-$(git -C "$(dirname "$0")/.." log -1 --format=%ct 2>/dev/null || date +%s)}"
stamp="$(date -u -r "$sde" +%Y%m%d%H%M.%S 2>/dev/null || date -u -d "@$sde" +%Y%m%d%H%M.%S)"
find "$app" -exec env TZ=UTC touch -h -t "$stamp" {} +
if [ -z "$dry" ]; then
  # ditto keeps the modes and the signature as they are
  ditto -c -k --norsrc --noextattr --noacl --keepParent "$app" "$zip"
else
  (cd "$out" && zip -qry "$(basename "$zip")" "$(basename "$app")")
fi
ls -l "$zip"

step "dmg"
if [ -z "$dry" ]; then
  stage="$(mktemp -d)"
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  dmg="$out/vibeBMS-macos-universal.dmg"
  rm -f "$dmg"
  hdiutil create -volname "$name" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
  rm -rf "$stage"
  ls -l "$dmg"
fi
echo "done"
