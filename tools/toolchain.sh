# The pinned Apple and Microsoft toolchains of tools/repro.env, for the release
# builds made on a runner image (sourced by tools/build_release.sh and
# tools/package_macos.sh, after tools/repro.env):
#
#   use_xcode            DEVELOPER_DIR: /Applications/Xcode_$XCODE_VERSION.app
#   use_msvc <x64|arm64> the environment of vcvarsall.bat for that target, with
#                        MSVC_TOOLSET and WINDOWS_SDK
#
# CI (GITHUB_ACTIONS=true) or REPRO_STRICT=1 stops when the pinned one is not
# installed; elsewhere it warns and builds with what there is (other bytes
# than the release's). TOOLCHAIN then names what the build uses (build.txt).
# shellcheck shell=bash disable=SC2034 # TOOLCHAIN is the caller's

# a pin this machine does not have: <message>
pin_missing() {
  if [ "${GITHUB_ACTIONS:-}" = true ] || [ "${REPRO_STRICT:-}" = 1 ]; then
    echo "error: $* (tools/repro.env; the Toolchain pins workflow moves the pins)" >&2
    exit 1
  fi
  echo "warning: $*: the build will not have the release's bytes (tools/repro.env)" >&2
}

use_xcode() {
  local dev="/Applications/Xcode_$XCODE_VERSION.app/Contents/Developer" have
  if [ -n "${DEVELOPER_DIR:-}" ] && [ "$DEVELOPER_DIR" != "$dev" ]; then
    echo "note: DEVELOPER_DIR is set ($DEVELOPER_DIR), not $dev" >&2
  elif [ -d "$dev" ]; then
    export DEVELOPER_DIR="$dev"
  fi
  have="$(xcodebuild -version 2>/dev/null | paste -sd ' ' -)" || have=""
  case "$have" in
    "Xcode $XCODE_VERSION Build version "*) ;;
    *) pin_missing "Xcode $XCODE_VERSION is not installed as $dev (in use: ${have:-none})" ;;
  esac
  TOOLCHAIN="${have:-unknown Xcode}"
}

use_msvc() {
  local target="$1" arch inst root="" bat out line name value
  case "$target" in
    x64) arch=x64 ;;
    arm64) arch=x64_arm64 ;; # (cross-compiled on an x64 runner)
    *) echo "use_msvc: x64 or arm64, not $target" >&2; exit 2 ;;
  esac
  local vswhere="/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe"
  # the Visual Studio that has the toolset (an image may have several)
  if [ -x "$vswhere" ]; then
    while IFS= read -r inst; do
      inst="$(cygpath -u "${inst%$'\r'}")"
      if [ -d "$inst/VC/Tools/MSVC/$MSVC_TOOLSET" ]; then root="$inst"; break; fi
    done < <("$vswhere" -all -products '*' -property installationPath)
  fi
  if [ -z "$root" ]; then
    pin_missing "the MSVC toolset $MSVC_TOOLSET is not installed (Visual Studio's VC/Tools/MSVC)"
    TOOLCHAIN="MSVC as found by rustc (not $MSVC_TOOLSET)"
    return
  fi
  if [ ! -d "/c/Program Files (x86)/Windows Kits/10/Lib/$WINDOWS_SDK/um/$target" ]; then
    pin_missing "the Windows SDK $WINDOWS_SDK ($target) is not installed (Windows Kits/10/Lib)"
    TOOLCHAIN="MSVC as found by rustc (not SDK $WINDOWS_SDK)"
    return
  fi
  bat="$(mktemp -d)/vcvars.bat"
  printf '@echo off\r\ncall "%s" %s %s -vcvars_ver=%s\r\nif errorlevel 1 exit /b 1\r\nset\r\n' \
    "$(cygpath -w "$root/VC/Auxiliary/Build/vcvarsall.bat")" "$arch" "$WINDOWS_SDK" "$MSVC_TOOLSET" > "$bat"
  out="$(cmd.exe //d //c "$(cygpath -w "$bat")")" || out="$out"$'\n''[ERROR:vcvars] it failed'
  rm -rf "$(dirname "$bat")"
  if grep -q '^\[ERROR:' <<< "$out"; then
    grep '^\[ERROR:' <<< "$out" >&2
    pin_missing "vcvarsall.bat $arch $WINDOWS_SDK -vcvars_ver=$MSVC_TOOLSET failed"
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
  [ "${VCToolsVersion:-}" = "$MSVC_TOOLSET" ] ||
    pin_missing "vcvarsall.bat gave the toolset ${VCToolsVersion:-none}, not $MSVC_TOOLSET"
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
  TOOLCHAIN="MSVC ${VCToolsVersion:-?}, Windows SDK $sdk (${VSCMD_ARG_HOST_ARCH:-?} -> ${VSCMD_ARG_TGT_ARCH:-?})"
}
