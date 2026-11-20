#!/usr/bin/env bash
# The nightly's F-Droid repository (docs/BUILDING.md, "F-Droid repository"):
# an index that F-Droid, Droid-ify or Neo Store read, so that an app added
# once from it is offered each new build as an update. CI runs it in the
# nightly job; it runs the same anywhere with fdroidserver on PATH
# (tools/fdroidserver.txt), a JDK and the Android SDK's build-tools.
#
#   tools/fdroid_repo.sh build <apk> <work dir>
#     Makes <work dir>/site/repo/ (the repository: the index, signed, and the
#     APK as it is, the only version), <work dir>/fdroid.txt (the address,
#     the key's fingerprint and the links that add the repository) and
#     <work dir>/fdroid-qr.png (the link as a QR code). The metadata is
#     platform/android/fdroid/metadata/.
#     Environment: FDROID_KEYSTORE_FILE (a keystore with one key, as made by
#     the keytool command in docs/BUILDING.md), FDROID_KEYSTORE_PASS (the
#     keystore's and the key's password; keytool and fdroidserver read it
#     from the environment, so it is in no command line, which other
#     processes can read, nor in a file), FDROID_REPO_URL (where site/repo/
#     is served, https://.../repo: fdroidserver wants the /repo, and clients
#     keep this address, not the one typed in, as the repository's),
#     ANDROID_HOME (the SDK, for apksigner).
#
#   tools/fdroid_repo.sh push <work dir> <git remote> <branch>
#     Publishes site/ as the one commit of <branch> (no parent: the branch's
#     earlier commits, and their APKs, are left behind, not kept in history).
#     The caller sets up the remote's credentials.
set -euo pipefail

die() { echo "fdroid_repo.sh: $*" >&2; exit 1; }
src="$(cd "$(dirname "$0")/.." && pwd)"

build() {
  local apk work ks alias fp_hex app_id addr host_path link
  [ $# -eq 2 ] || die "usage: build <apk> <work dir>"
  apk="$(realpath "$1")"
  work="$2"
  [ -f "$apk" ] || die "no APK at $1"
  ks="$(realpath "${FDROID_KEYSTORE_FILE:?the keystore of the repository}")"
  : "${FDROID_KEYSTORE_PASS:?the password of the keystore}"
  export FDROID_KEYSTORE_PASS
  addr="${FDROID_REPO_URL:?the address of the repository}"
  addr="${addr%/}"
  case "$addr" in https://*/repo | http://*/repo) ;; *) die "FDROID_REPO_URL must be https://.../repo: $addr" ;; esac
  command -v fdroid >/dev/null || die "fdroid not found (pip install fdroidserver)"

  # the key: a keystore made for this has one (as the APK's does)
  alias="$(keytool -list -keystore "$ks" -storepass:env FDROID_KEYSTORE_PASS | grep -m1 PrivateKeyEntry | cut -d, -f1)"
  [ -n "$alias" ] || die "no private key in the keystore"
  # fdroidserver signs index-v1.jar (for older clients) with SHA1withRSA and does not notice when
  # that fails, so a key of another type leaves it unsigned: the repository key must be RSA
  keytool -list -v -keystore "$ks" -storepass:env FDROID_KEYSTORE_PASS -alias "$alias" |
    grep -q '^Subject Public Key Algorithm: .*RSA' ||
    die "the key $alias is not an RSA key; F-Droid's index needs one (docs/BUILDING.md)"
  # what clients pin: the SHA-256 of the key's certificate
  fp_hex="$(keytool -exportcert -keystore "$ks" -storepass:env FDROID_KEYSTORE_PASS -alias "$alias" |
    sha256sum | cut -d' ' -f1 | tr a-f A-F)"
  [ ${#fp_hex} -eq 64 ] || die "could not read the key's certificate"

  rm -rf "$work"
  mkdir -p "$work/fdroid/repo"
  work="$(realpath "$work")"
  cp -r "$src/platform/android/fdroid/metadata" "$work/fdroid/metadata"
  # the repository's icon (fdroidserver copies it to repo/icons/)
  cp "$src/assets/logo/icon-256.png" "$work/fdroid/icon.png"
  cp "$apk" "$work/fdroid/repo/"

  # config.yml as JSON (YAML reads it), so nothing needs quoting. It names
  # the variable that has the password ({env: ...}), not the password, and
  # stays in the work dir, out of site/
  (umask 077 && jq -n --arg url "$addr" --arg ks "$ks" --arg alias "$alias" --arg sdk "${ANDROID_HOME:-}" '{
      repo_url: $url,
      repo_name: "vibeBMS Nightly",
      repo_description: "Nightly builds of vibeBMS, a BMS player: the latest build of the default branch of https://github.com/vibeBMS/vibe.",
      repo_icon: "icon.png",
      archive_older: 0,
      keystore: $ks, keystorepass: {env: "FDROID_KEYSTORE_PASS"}, keypass: {env: "FDROID_KEYSTORE_PASS"},
      repo_keyalias: $alias
    } + (if $sdk == "" then {} else {sdk_path: $sdk} end)' > "$work/fdroid/config.yml")

  # --rename-apks names it <app id>_<versionCode>.apk: each build has its own
  # URL, so no cache between a client and the host serves an older one
  (cd "$work/fdroid" && fdroid update --rename-apks)

  # one APK, the given one byte for byte, and the index lists it as it is
  local apks
  apks=("$work"/fdroid/repo/*.apk)
  [ ${#apks[@]} -eq 1 ] || die "expected one APK in the repository, found ${#apks[@]}"
  cmp -s "$apk" "${apks[0]}" || die "the repository's APK differs from $apk"
  jq -e --arg name "/${apks[0]##*/}" --arg sha "$(sha256sum "$apk" | cut -d' ' -f1)" \
    '[.packages[].versions[].file | select(.name == $name and .sha256 == $sha)] | length == 1' \
    "$work/fdroid/repo/index-v2.json" >/dev/null || die "index-v2.json does not list ${apks[0]##*/}"
  app_id="$(jq -r '.packages | keys[0]' "$work/fdroid/repo/index-v2.json")"
  # the index is signed with this key (entry.jar for current clients, index-v1.jar for older ones).
  # The certificate is read from the signature block with openssl: index-v1.jar is signed with
  # SHA-1 digests (for old clients), and a recent JDK's keytool treats such a jar as unsigned.
  for jar in entry.jar index-v1.jar; do
    signer="$(unzip -Z1 "$work/fdroid/repo/$jar" | grep -m1 -E '^META-INF/[^/]+\.(RSA|EC|DSA)$' || true)"
    [ -n "$signer" ] || die "$jar has no signature"
    got="$(unzip -p "$work/fdroid/repo/$jar" "$signer" | openssl pkcs7 -inform DER -print_certs |
      openssl x509 -noout -fingerprint -sha256 | cut -d= -f2 | tr -d : | tr a-f A-F)"
    [ "$got" = "$(tr a-f A-F <<< "$fp_hex")" ] || die "$jar is signed with the key ${got:-?}, not $fp_hex"
  done

  # what is published: the repository, without fdroidserver's run logs
  mkdir -p "$work/site"
  cp -r "$work/fdroid/repo" "$work/site/repo"
  rm -rf "$work/site/repo/status"
  cat > "$work/site/README.md" <<EOF
# vibeBMS F-Droid repository

Made by CI (\`tools/fdroid_repo.sh\` in the default branch) on each nightly; every run replaces this branch with
one new commit, so do not commit here. Address: $addr, fingerprint $fp_hex.
EOF

  host_path="${addr#https://}"
  link="$addr?fingerprint=$fp_hex"
  cat > "$work/fdroid.txt" <<EOF
vibeBMS F-Droid repository: $app_id, the nightly build of the default branch.
Add it once in F-Droid, Droid-ify or Neo Store, and each new nightly is offered as an update.

Address:     $addr
Fingerprint: $fp_hex

Open on the phone to add it: fdroidrepos://$host_path?fingerprint=$fp_hex
or: $link
(fdroid-qr.png is that link as a QR code.)
EOF
  cp "$work/fdroid/repo/index.png" "$work/fdroid-qr.png"
  echo "$fp_hex" > "$work/fingerprint"
  echo "$app_id" > "$work/app_id"
  echo "repository $addr: ${apks[0]##*/}, key $fp_hex"
}

push() {
  local work remote branch site apks
  [ $# -eq 3 ] || die "usage: push <work dir> <git remote> <branch>"
  work="$1" remote="$2" branch="$3"
  site="$work/site"
  [ -f "$site/repo/entry.jar" ] || die "no repository in $site (run build first)"
  rm -rf "$site/.git"
  apks=("$site"/repo/*.apk)
  git -C "$site" init -q -b "$branch"
  git -C "$site" add -A
  git -C "$site" -c user.name="github-actions[bot]" -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
    commit -q -m "F-Droid repository: ${apks[*]##*/}"
  git -C "$site" push -q --force "$remote" "HEAD:refs/heads/$branch"
}

cmd="${1:-}"
[ $# -gt 0 ] && shift
case "$cmd" in
  build) build "$@" ;;
  push) push "$@" ;;
  *) die "usage: $0 build <apk> <work dir> | push <work dir> <git remote> <branch>" ;;
esac
