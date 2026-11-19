# The pinned Apple and Microsoft toolchains of tools/repro.env, for the release
# builds made on a runner image (sourced by tools/build_release.sh and
# tools/package_macos.sh, after tools/repro.env):
#
#   use_xcode            DEVELOPER_DIR: the Xcode XCODE_VERSION names
#   use_msvc <x64|arm64> the environment of vcvarsall.bat for that target, with
#                        the toolset of MSVC_COMPONENT and WINDOWS_SDK
#
# CI (GITHUB_ACTIONS=true) or REPRO_STRICT=1 stops when the pinned one is not
# there, saying what is; elsewhere it warns and builds with what there is (other
# bytes than the release's). TOOLCHAIN then names, exactly, what the build uses
# (build.txt).
# shellcheck shell=bash disable=SC2034,SC2012 # TOOLCHAIN is the caller's; ls lists versions

# a pin this machine does not have: <message>
pin_missing() {
  if [ "${GITHUB_ACTIONS:-}" = true ] || [ "${REPRO_STRICT:-}" = 1 ]; then
    echo "error: $* (tools/repro.env; the Toolchain pins workflow moves the pins)" >&2
    exit 1
  fi
  echo "warning: $*: the build will not have the release's bytes (tools/repro.env)" >&2
}

# the newest of the versions on stdin
newest_version() { sort -t. -k1,1n -k2,2n -k3,3n -k4,4n | tail -1; }

# XCODE_VERSION is a major.minor release: /Applications/Xcode_<it>.app or
# Xcode_<it>.<patch>.app, the newest there (a runner image replaces a patch
# release by the next, and while it rolls out a label's runners have either)
use_xcode() {
  local v="$XCODE_VERSION" d n best="" dev have
  for d in /Applications/Xcode_*.app; do
    n="${d#/Applications/Xcode_}" n="${n%.app}"
    case "$n" in "$v" | "$v".[0-9]*) ;; *) continue ;; esac
    case "$n" in *[!0-9.]*) continue ;; esac
    [ -d "$d" ] || continue
    best="$(printf '%s\n' "$best" "$n" | sed '/^$/d' | newest_version)"
  done
  dev="/Applications/Xcode_$best.app/Contents/Developer"
  if [ -n "${DEVELOPER_DIR:-}" ] && [ "$DEVELOPER_DIR" != "$dev" ]; then
    echo "note: DEVELOPER_DIR is set ($DEVELOPER_DIR), not $dev" >&2
  elif [ -n "$best" ]; then
    export DEVELOPER_DIR="$dev"
  fi
  have="$(xcodebuild -version 2>/dev/null | paste -sd ' ' -)" || have=""
  case "$have" in
    "Xcode $v Build version "* | "Xcode $v".[0-9]*" Build version "*) ;;
    *)
      pin_missing "Xcode $v is not installed (in use: ${have:-none}; installed:" \
        "$(cd /Applications 2>/dev/null && ls -d Xcode*.app 2>/dev/null | paste -sd ' ' -))"
      ;;
  esac
  TOOLCHAIN="${have:-unknown Xcode}"
}

# MSVC_COMPONENT is a side-by-side toolset, the Visual Studio installer's
# component Microsoft.VisualStudio.Component.VC.<it>.<x86.x64|ARM64>, e.g.
# 14.44.17.14 (MSVC 14.44, as Visual Studio 17.14 left it): no image update
# replaces it, where Visual Studio's own toolset is replaced by each of its
# updates. Its folder is VC/Tools/MSVC/<major.minor>.<build>. In CI an
# architecture the image lacks is added with the installer (the hosted images
# have the x64 tools; the ARM64 ones are added for windows-arm64).
msvc_vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
msvc_installs() { "$msvc_vswhere" -all -products '*' -property installationPath | tr -d '\r' | while IFS= read -r i; do cygpath -u "$i"; done; }
# what there is, for the error messages
msvc_present() {
  local i d
  while IFS= read -r i; do
    for d in "$i"/VC/Tools/MSVC/*/; do
      d="${d%/}"
      printf '%s (%s) ' "${d##*/}" "$(cd "$d/lib" 2>/dev/null && ls -d x64 arm64 2>/dev/null | paste -sd, -)"
    done
  done < <(msvc_installs)
  printf '; SDKs: %s' "$(cd "/c/Program Files (x86)/Windows Kits/10/Lib" 2>/dev/null && ls -d 10.* 2>/dev/null | paste -sd ' ' -)"
}
# the toolset folder in Visual Studio <root>, or nothing
msvc_folder() {
  local mm="${MSVC_COMPONENT%.*.*}"
  (cd "$1/VC/Tools/MSVC" 2>/dev/null && ls -d "$mm".* 2>/dev/null) | newest_version
}
# the component for <target>, added with the installer: <root> <target>
msvc_add() {
  local root="$1" comp bat
  case "$2" in x64) comp="Microsoft.VisualStudio.Component.VC.$MSVC_COMPONENT.x86.x64" ;; *) comp="Microsoft.VisualStudio.Component.VC.$MSVC_COMPONENT.ARM64" ;; esac
  echo "adding $comp to $root (the Visual Studio installer)" >&2
  bat="$(mktemp -d)/add.bat"
  printf '@echo off\r\nstart "" /wait "%s" modify --installPath "%s" --add %s --quiet --norestart --nocache\r\nexit /b %%errorlevel%%\r\n' \
    "$(cygpath -w "$(dirname "$msvc_vswhere")/vs_installer.exe")" "$(cygpath -w "$root")" "$comp" > "$bat"
  cmd.exe //d //c "$(cygpath -w "$bat")" || echo "the installer answered $?" >&2
  rm -rf "$(dirname "$bat")"
  # (the installer may hand the work to its engine and return first)
  local n=0
  while [ $n -lt 180 ] && tasklist 2>/dev/null | grep -qi '^setup\.exe\|^vs_installer'; do sleep 10; n=$((n + 1)); done
}

use_msvc() {
  local target="$1" arch inst root="" folder bat out line name value
  case "$target" in
    x64) arch=x64 ;;
    arm64) arch=x64_arm64 ;; # (cross-compiled on an x64 runner)
    *) echo "use_msvc: x64 or arm64, not $target" >&2; exit 2 ;;
  esac
  # the Visual Studio that has the toolset (an image may have several), else
  # the newest one
  if [ -x "$msvc_vswhere" ]; then
    while IFS= read -r inst; do
      if [ -n "$(msvc_folder "$inst")" ]; then root="$inst"; break; fi
      [ -n "$root" ] || root="$inst"
    done < <(msvc_installs)
  fi
  if [ -z "$root" ]; then
    pin_missing "no Visual Studio for the MSVC toolset $MSVC_COMPONENT"
    TOOLCHAIN="MSVC as found by rustc (no Visual Studio)"
    return
  fi
  folder="$(msvc_folder "$root")"
  if { [ -z "$folder" ] || [ ! -f "$root/VC/Tools/MSVC/$folder/bin/Hostx64/$target/link.exe" ] ||
    [ ! -d "$root/VC/Tools/MSVC/$folder/lib/$target" ]; } &&
    { [ "${GITHUB_ACTIONS:-}" = true ] || [ "${MSVC_INSTALL:-}" = 1 ]; }; then
    msvc_add "$root" "$target"
    [ -n "$folder" ] || folder="$(msvc_folder "$root")"
  fi
  if [ -z "$folder" ] || [ ! -f "$root/VC/Tools/MSVC/$folder/bin/Hostx64/$target/link.exe" ]; then
    pin_missing "the MSVC toolset $MSVC_COMPONENT for $target is not installed (installed: $(msvc_present))"
    TOOLCHAIN="MSVC as found by rustc (not $MSVC_COMPONENT)"
    return
  fi
  if [ ! -d "/c/Program Files (x86)/Windows Kits/10/Lib/$WINDOWS_SDK/um/$target" ]; then
    pin_missing "the Windows SDK $WINDOWS_SDK ($target) is not installed (installed: $(msvc_present))"
    TOOLCHAIN="MSVC as found by rustc (not SDK $WINDOWS_SDK)"
    return
  fi
  bat="$(mktemp -d)/vcvars.bat"
  printf '@echo off\r\ncall "%s" %s %s -vcvars_ver=%s\r\nif errorlevel 1 exit /b 1\r\nset\r\n' \
    "$(cygpath -w "$root/VC/Auxiliary/Build/vcvarsall.bat")" "$arch" "$WINDOWS_SDK" "$folder" > "$bat"
  out="$(cmd.exe //d //c "$(cygpath -w "$bat")")" || out="$out"$'\n''[ERROR:vcvars] it failed'
  rm -rf "$(dirname "$bat")"
  if grep -q '^\[ERROR:' <<< "$out"; then
    grep '^\[ERROR:' <<< "$out" >&2
    pin_missing "vcvarsall.bat $arch $WINDOWS_SDK -vcvars_ver=$folder failed (installed: $(msvc_present))"
    TOOLCHAIN="MSVC as found by rustc (vcvarsall.bat failed)"
    return
  fi
  # what vcvarsall sets (PATH, INCLUDE, LIB, LIBPATH, VCINSTALLDIR,
  # VCToolsVersion, VSCMD_ARG_TGT_ARCH, WindowsSdkDir, ...); rustc's linker
  # lookup and the cc crate take cl.exe and link.exe from PATH when
  # VCINSTALLDIR is set and VSCMD_ARG_TGT_ARCH is the target's, and
  # VCToolsVersion's toolset for another target (the build scripts)
  while IFS= read -r line; do
    line="${line%$'\r'}"
    name="${line%%=*}" value="${line#*=}"
    case "$name" in
      [Pp][Aa][Tt][Hh]) PATH="$(cygpath -u -p "$value")"; export PATH ;;
      INCLUDE | LIB | LIBPATH | EXTERNAL_INCLUDE | Platform | CommandPromptType | DevEnvDir | ExtensionSdkDir | \
        UCRTVersion | UniversalCRTSdkDir | NETFXSDKDir | VisualStudioVersion | WindowsLibPath | \
        VC[A-Za-z]* | VS[A-Za-z0-9_]* | WindowsSdk[A-Za-z]* | WindowsSDK[A-Za-z]* | Framework[A-Za-z0-9]*)
        export "$name=$value" ;;
    esac
  done <<< "$out"
  [ "${VCToolsVersion:-}" = "$folder" ] ||
    pin_missing "vcvarsall.bat gave the toolset ${VCToolsVersion:-none}, not $folder"
  local sdk="${WindowsSDKVersion:-}"
  sdk="${sdk%\\}"
  [ "$sdk" = "$WINDOWS_SDK" ] || pin_missing "vcvarsall.bat gave the SDK ${sdk:-none}, not $WINDOWS_SDK"
  [ "${VSCMD_ARG_TGT_ARCH:-}" = "$target" ] ||
    pin_missing "vcvarsall.bat targets ${VSCMD_ARG_TGT_ARCH:-nothing}, not $target"
  # the first link.exe on PATH is the toolset's (not Git's /usr/bin/link)
  case "$(cygpath -w "$(command -v link.exe)")" in
    "${VCToolsInstallDir:-?}"*) ;;
    *) pin_missing "link.exe on PATH is $(command -v link.exe), not the toolset's (${VCToolsInstallDir:-none})" ;;
  esac
  TOOLCHAIN="MSVC ${VCToolsVersion:-?} ($MSVC_COMPONENT), Windows SDK $sdk (${VSCMD_ARG_HOST_ARCH:-?} -> ${VSCMD_ARG_TGT_ARCH:-?})"
}
