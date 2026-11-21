#!/usr/bin/env python3
"""Tests of how .github/workflows/ci.yml writes, signs, checks and publishes latest.json (the steps
"write latest.json" of update-manifest, "sign latest.json" of sign-update, "check the signature" of
verify-update, and "check the files" and "release" of the nightly job), run as the workflow runs
them, with the runner's ssh-keygen, a key made for the test and a stand-in for gh:
python3 .github/update_signing_test.py (CI's test job runs it)."""
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import textwrap
import unittest

WORKFLOW = pathlib.Path(__file__).resolve().parent / "workflows" / "ci.yml"
LINES = WORKFLOW.read_text().splitlines()


def step(name):
    """The `run: |` script of the step `name`."""
    at = LINES.index(next(l for l in LINES if l.strip() == f"- name: {name}"))
    run = next(i for i in range(at + 1, len(LINES)) if LINES[i].strip() == "run: |")
    indent = len(LINES[run]) - len(LINES[run].lstrip())
    body = []
    for line in LINES[run + 1:]:
        if line.strip() and len(line) - len(line.lstrip()) <= indent:
            break
        body.append(line)
    return textwrap.dedent("\n".join(body)) + "\n"


MANIFEST = step("write latest.json")
SIGN = step("sign latest.json")
CHECK = step("check the signature")
FILES = step("check the files")
RELEASE = step("release")

# the executables update-manifest takes: (artifact, file in it, name in the release)
EXECUTABLES = [(t, "vibebms", f"vibebms-{t}") for t in (
    "linux-x64", "linux-arm64", "linux-x64-portable", "linux-arm64-portable", "macos-arm64", "macos-x64")]
EXECUTABLES += [(t, "vibebms.exe", f"vibebms-{t}.exe") for t in ("windows-x64", "windows-arm64")]
EXECUTABLES += [("macos-universal", "vibebms-macos-universal.app.zip", "vibebms-macos-universal.app.zip")]

# gh as the steps call it: `release download nightly -p <name> -D <dir>` copies <name> from
# $GH_RELEASE (failing when it is not there); every call is logged to $GH_LOG
GH = """#!/bin/bash
printf '%s\\n' "$*" >> "$GH_LOG"
if [ "$1 $2" = "release download" ]; then
  [ -f "$GH_RELEASE/$5" ] && cp "$GH_RELEASE/$5" "$7/"
fi
"""


def run(script, cwd, **env):
    """`script` as a step runs it (bash -e -o pipefail), in `cwd`; its outputs in `.outputs`."""
    cwd = pathlib.Path(cwd)
    tmp = cwd / "runner-temp"
    tmp.mkdir(exist_ok=True)
    output = cwd / "github-output"
    output.write_text("")
    bin_dir = cwd.parent / "bin"
    env = {"PATH": f"{bin_dir}:{os.environ['PATH']}", "RUNNER_TEMP": str(tmp), "GITHUB_OUTPUT": str(output),
           "GH_LOG": str(cwd.parent / "gh.log"), **env}
    r = subprocess.run(["bash", "--noprofile", "--norc", "-eo", "pipefail", "-c", script],
                       cwd=cwd, env=env, capture_output=True, text=True)
    r.outputs = dict(l.split("=", 1) for l in output.read_text().splitlines() if "=" in l)
    return r


def sha256(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def keypair(where, name):
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "vibebms-update", "-f", str(where / name)],
                   check=True, stdin=subprocess.DEVNULL)
    return (where / name).read_text(), (where / f"{name}.pub").read_text()


def entry(content=b"x"):
    return {"version": "0.1.0", "sha256": hashlib.sha256(content).hexdigest(), "size": len(content)}


class Base(unittest.TestCase):
    def setUp(self):
        self.dir = pathlib.Path(tempfile.mkdtemp())
        self.keys = self.dir / "keys"
        self.keys.mkdir()
        self.private, self.public = keypair(self.keys, "key")
        self.other_private, _ = keypair(self.keys, "other")
        (self.dir / "bin").mkdir()
        (self.dir / "bin" / "gh").write_text(GH)
        (self.dir / "bin" / "gh").chmod(0o755)
        # the jobs' folders: sign-update's download, verify-update's checkout and download
        self.sign = self.dir / "sign"
        (self.sign / "manifest").mkdir(parents=True)
        (self.sign / "manifest" / "latest.json").write_text('{"channels": {}}\n')
        self.check = self.dir / "check"
        (self.check / "tools").mkdir(parents=True)

    def tearDown(self):
        shutil.rmtree(self.dir)

    def signed(self, private, signed="true", manifest_sha=None):
        if manifest_sha is None:
            manifest_sha = sha256(self.sign / "manifest" / "latest.json")
        return run(SIGN, self.sign, SIGNED=signed, UPDATE_SIGNING_KEY=private, MANIFEST_SHA256=manifest_sha)

    def verify(self, public=None, manifest_sha=None, sig_sha=None):
        """verify-update on what sign-update made, with `public` committed."""
        if public is not None:
            (self.check / "tools" / "update_key.pub").write_text(public)
            shutil.copytree(self.sign / "manifest", self.check / "manifest", dirs_exist_ok=True)
        m = self.check / "manifest"
        if manifest_sha is None:
            manifest_sha = sha256(m / "latest.json") if (m / "latest.json").exists() else ""
        if sig_sha is None:
            sig_sha = sha256(m / "latest.json.sig") if (m / "latest.json.sig").exists() else ""
        return run(CHECK, self.check, MANIFEST_SHA256=manifest_sha, SIGNATURE_SHA256=sig_sha)

    def sign_file(self, private, path):
        """`path`.sig, as sign-update makes it."""
        key = self.keys / "signing"
        key.write_text(private)
        key.chmod(0o600)
        subprocess.run(["ssh-keygen", "-q", "-Y", "sign", "-n", "vibebms-update", "-f", str(key), str(path)],
                       check=True, stdin=subprocess.DEVNULL)


class Signing(Base):
    def assert_no_key_left(self):
        self.assertEqual(list((self.sign / "runner-temp").iterdir()), [])

    def test_no_public_key(self):
        # (today): nothing signed, nothing checked, the release goes out
        r = self.signed("", signed="false")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse((self.sign / "manifest" / "latest.json.sig").exists())
        r = self.verify()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("no tools/update_key.pub", r.stdout)

    def test_signed(self):
        r = self.signed(self.private)
        self.assertEqual(r.returncode, 0, r.stderr)
        sig = (self.sign / "manifest" / "latest.json.sig").read_text()
        self.assertTrue(sig.startswith("-----BEGIN SSH SIGNATURE-----\n"), sig)
        self.assertEqual(r.outputs["sha256"], sha256(self.sign / "manifest" / "latest.json.sig"))
        self.assert_no_key_left()
        r = self.verify(self.public)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn('Good "vibebms-update" signature', r.stdout + r.stderr)
        # a secret pasted without its last line end, or with Windows ones
        for private in (self.private.rstrip("\n"), self.private.replace("\n", "\r\n")):
            r = self.signed(private)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(self.verify(self.public).returncode, 0)

    def test_no_secret(self):
        r = self.signed("")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("::error::", r.stdout)
        self.assertIn("UPDATE_SIGNING_KEY", r.stdout)
        self.assertFalse((self.sign / "manifest" / "latest.json.sig").exists())
        # (and were it published without one, verify-update stops it)
        r = self.verify(self.public)
        self.assertNotEqual(r.returncode, 0)

    def test_wrong_key_or_file(self):
        # the secret is not the committed key's pair: nothing is published
        self.assertEqual(self.signed(self.other_private).returncode, 0)
        r = self.verify(self.public)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("::error::", r.stdout)
        # latest.json changed after signing
        self.assertEqual(self.signed(self.private).returncode, 0)
        (self.sign / "manifest" / "latest.json").write_text('{"channels": {"nightly": {}}}\n')
        self.assertNotEqual(self.verify(self.public).returncode, 0)

    def test_not_the_manifest_update_manifest_made(self):
        # another job's latest.json in the artifact: the key is never written, nothing is signed
        made = sha256(self.sign / "manifest" / "latest.json")
        (self.sign / "manifest" / "latest.json").write_text('{"channels": {"stable": {}}}\n')
        for sha in (made, ""):
            r = self.signed(self.private, manifest_sha=sha)
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("::error::the artifact update-manifest", r.stdout)
            self.assertFalse((self.sign / "manifest" / "latest.json.sig").exists())
            self.assert_no_key_left()
        # nor with no file at all
        (self.sign / "manifest" / "latest.json").unlink()
        self.assertNotEqual(self.signed(self.private, manifest_sha=made).returncode, 0)

    def test_verify_checks_the_files_it_was_given(self):
        self.assertEqual(self.signed(self.private).returncode, 0)
        good = self.verify(self.public)
        self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
        # a latest.json or signature that is not the one its job made fails, even when it verifies
        m = self.check / "manifest"
        for kind in ("manifest", "sig"):
            r = self.verify(manifest_sha="0" * 64 if kind == "manifest" else None, sig_sha="0" * 64 if kind == "sig" else None)
            self.assertNotEqual(r.returncode, 0, kind)
            self.assertIn("::error::", r.stdout)
        (m / "latest.json.sig").unlink()
        self.assertNotEqual(self.verify(sig_sha="").returncode, 0)

    def test_namespace(self):
        # the signature is the update list's alone: the same key's signature
        # for another namespace does not pass
        self.assertEqual(self.signed(self.private).returncode, 0)
        key = self.sign / "runner-temp" / "k"
        key.write_text(self.private)
        key.chmod(0o600)
        sig = self.sign / "manifest" / "latest.json.sig"
        sig.unlink()
        subprocess.run(["ssh-keygen", "-q", "-Y", "sign", "-n", "file", "-f", str(key),
                        str(self.sign / "manifest" / "latest.json")], check=True, stdin=subprocess.DEVNULL)
        self.assertNotEqual(self.verify(self.public).returncode, 0)
        # the namespace signed is the one the game checks (update/sig.rs)
        sig_rs = (WORKFLOW.parents[2] / "crates/vibebms/src/update/sig.rs").read_text()
        self.assertIn('pub const NAMESPACE: &str = "vibebms-update";', sig_rs)
        self.assertTrue(re.search(r"-Y sign -n vibebms-update ", SIGN), SIGN)


class ManifestJob(Base):
    """update-manifest's folder, with every build's artifact."""

    STABLE = {"version": "0.1.0", "files": {"vibebms-linux-x64": entry(b"stable")}}

    def setUp(self):
        super().setUp()
        self.job = self.dir / "manifest"
        (self.job / "tools").mkdir(parents=True)
        (self.job / "Cargo.toml").write_text('[package]\nname = "vibebms"\nversion = "0.1.0-dev"\n')
        for artifact, name, _ in EXECUTABLES:
            d = self.job / "art" / f"vibeBMS-{artifact}"
            d.mkdir(parents=True, exist_ok=True)
            (d / name).write_bytes(f"the {artifact} build".encode())
            (d / "version.txt").write_text("0.1.0-nightly.42+abcdef0\n")
        self.release = self.dir / "release"
        self.release.mkdir()

    def previous(self, doc, sign_with=None):
        """The release's latest.json (and its signature, signed with `sign_with`)."""
        (self.release / "latest.json").write_text(json.dumps(doc))
        (self.release / "latest.json.sig").unlink(missing_ok=True)
        if sign_with:
            self.sign_file(sign_with, self.release / "latest.json")

    def manifest(self, key=True):
        # (each run a job of its own)
        for d in ("out", "prev"):
            shutil.rmtree(self.job / d, ignore_errors=True)
        pub = self.job / "tools" / "update_key.pub"
        if key:
            pub.write_text(self.public)
        elif pub.exists():
            pub.unlink()
        r = run(MANIFEST, self.job, GH_TOKEN="x", GH_REPO="o/r", GITHUB_RUN_NUMBER="42",
                GITHUB_SHA="abcdef0123456789abcdef0123456789abcdef01", GH_RELEASE=str(self.release))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = self.job / "out" / "latest.json"
        self.assertEqual(r.outputs["sha256"], sha256(out))
        self.assertEqual(r.outputs["signed"], "true" if key else "false")
        return json.loads(out.read_text())["channels"]

    def assert_nightly_is_this_run(self, channels):
        self.assertEqual(channels["nightly"]["version"], "0.1.0-nightly.42+abcdef0")
        files = channels["nightly"]["files"]
        self.assertEqual(sorted(files), sorted(n for _, _, n in EXECUTABLES))
        for artifact, _, name in EXECUTABLES:
            content = f"the {artifact} build".encode()
            self.assertEqual(files[name], {"version": "0.1.0-nightly.42+abcdef0",
                                           "sha256": hashlib.sha256(content).hexdigest(), "size": len(content)})


class Manifest(ManifestJob):
    """update-manifest's latest.json: this run's nightly, and the other channels of the
    release's previous latest.json only when that verifies with the committed key."""

    def test_no_previous(self):
        for key in (False, True):
            channels = self.manifest(key)
            self.assertEqual(list(channels), ["nightly"])
            self.assert_nightly_is_this_run(channels)

    def test_a_signed_previous_keeps_its_other_channels(self):
        self.previous({"channels": {"stable": self.STABLE, "nightly": {"version": "0", "files": {}}}}, self.private)
        channels = self.manifest()
        self.assertEqual(channels["stable"], self.STABLE)
        self.assert_nightly_is_this_run(channels)

    def test_an_unsigned_previous_keeps_nothing(self):
        planted = {"channels": {"stable": self.STABLE}}
        # no signature, a signature by another key, a signature of another file, or no key committed
        for sign_with, key in ((None, True), (self.other_private, True), (self.private, False)):
            self.previous(planted, sign_with)
            channels = self.manifest(key)
            self.assertEqual(list(channels), ["nightly"], (sign_with is not None, key))
        self.previous({"channels": {}}, self.private)
        (self.release / "latest.json").write_text(json.dumps(planted))
        self.assertEqual(list(self.manifest()), ["nightly"])

    def test_a_channel_of_another_shape_is_dropped(self):
        good = self.STABLE
        bad = {
            "number": 1, "list": [], "no-version": {"files": {}}, "version-number": {"version": 1, "files": {}},
            "files-list": {"version": "1", "files": []},
            "short-sha": {"version": "1", "files": {"f": {**entry(), "sha256": "ab"}}},
            "sha-not-hex": {"version": "1", "files": {"f": {**entry(), "sha256": "g" * 64}}},
            "sha-number": {"version": "1", "files": {"f": {**entry(), "sha256": 1}}},
            "size-zero": {"version": "1", "files": {"f": {**entry(), "size": 0}}},
            "size-fraction": {"version": "1", "files": {"f": {**entry(), "size": 1.5}}},
            "size-string": {"version": "1", "files": {"f": {**entry(), "size": "1"}}},
            "size-huge": {"version": "1", "files": {"f": {**entry(), "size": 1e30}}},
            "file-version-missing": {"version": "1", "files": {"f": {"sha256": "a" * 64, "size": 1}}},
            "one-bad-file": {"version": "1", "files": {"g": entry(), "f": {**entry(), "size": -1}}},
        }
        self.previous({"channels": {"stable": good, **bad}}, self.private)
        channels = self.manifest()
        self.assertEqual(sorted(channels), ["nightly", "stable"])
        # (and channels that are not an object at all)
        for doc in ({"channels": []}, {"channels": "x"}, [], {}):
            self.previous(doc, self.private)
            self.assertEqual(list(self.manifest()), ["nightly"], doc)

    def test_old_nightly_entries_are_not_carried(self):
        old = {"version": "0.1.0-nightly.41+1111111", "files": {
            "vibebms-linux-x64": entry(b"old"), "vibebms-old-target": entry(b"old")}}
        # nor from the old layout's top-level "files"
        self.previous({"channels": {"nightly": old}, "files": old["files"]}, self.private)
        channels = self.manifest()
        self.assertEqual(list(channels), ["nightly"])
        self.assert_nightly_is_this_run(channels)

    def test_a_build_missing_fails(self):
        (self.job / "art" / "vibeBMS-windows-arm64" / "vibebms.exe").unlink()
        r = run(MANIFEST, self.job, GH_TOKEN="x", GH_REPO="o/r", GITHUB_RUN_NUMBER="42",
                GITHUB_SHA="abcdef0123456789abcdef0123456789abcdef01", GH_RELEASE=str(self.release))
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("::error::the artifact vibeBMS-windows-arm64 has no vibebms.exe", r.stdout)


class Nightly(ManifestJob):
    """The nightly job's check of what it publishes, and the order it uploads it in."""

    def setUp(self):
        super().setUp()
        self.channels = self.manifest()
        self.manifest_sha = sha256(self.job / "out" / "latest.json")
        shutil.copy(self.job / "out" / "latest.json", self.sign / "manifest" / "latest.json")
        r = self.signed(self.private, manifest_sha=self.manifest_sha)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.sig_sha = r.outputs["sha256"]
        # the nightly's folders: its "files" step's out/, and the update-* downloads
        self.nightly = self.dir / "nightly"
        (self.nightly / "update").mkdir(parents=True)
        shutil.copy(self.sign / "manifest" / "latest.json", self.nightly / "update")
        shutil.copy(self.sign / "manifest" / "latest.json.sig", self.nightly / "update")
        out = self.nightly / "out"
        shutil.copytree(self.job / "out", out)
        (out / "latest.json").unlink()
        (out / "vibeBMS.apk").write_bytes(b"the signed APK")
        (out / "vibeBMS-web.zip").write_bytes(b"web")
        self.apk_sha = sha256(out / "vibeBMS.apk")

    def files(self, signed="true", **env):
        env = {"SIGNED": signed, "MANIFEST_SHA256": self.manifest_sha, "SIGNATURE_SHA256": self.sig_sha,
               "APK_SHA256": self.apk_sha, **env}
        return run(FILES, self.nightly, **env)

    def assert_refused(self, r, what):
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(f"::error::{what} is not", r.stdout)
        self.assertFalse((self.nightly / "out" / "latest.json").exists())

    def test_the_files_made_go_out(self):
        r = self.files()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = self.nightly / "out"
        self.assertEqual(sha256(out / "latest.json"), self.manifest_sha)
        self.assertEqual(sha256(out / "latest.json.sig"), self.sig_sha)
        # without a key: no signature to check or publish
        (out / "latest.json").unlink()
        (out / "latest.json.sig").unlink()
        r = self.files(signed="false", SIGNATURE_SHA256="")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertFalse((out / "latest.json.sig").exists())

    def test_a_replaced_file_stops_the_release(self):
        (self.nightly / "out" / "vibeBMS.apk").write_bytes(b"another APK")
        self.assert_refused(self.files(), "vibeBMS.apk")

    def test_a_replaced_executable_stops_the_release(self):
        (self.nightly / "out" / "vibebms-windows-x64.exe").write_bytes(b"another build")
        self.assert_refused(self.files(), "vibebms-windows-x64.exe")
        (self.nightly / "out" / "vibebms-windows-x64.exe").unlink()
        self.assert_refused(self.files(), "vibebms-windows-x64.exe")

    def test_a_replaced_list_or_signature_stops_the_release(self):
        self.assert_refused(self.files(MANIFEST_SHA256="0" * 64), "latest.json")
        self.assert_refused(self.files(SIGNATURE_SHA256="0" * 64), "latest.json.sig")
        (self.nightly / "update" / "latest.json.sig").unlink()
        self.assert_refused(self.files(), "latest.json.sig")

    def test_the_list_is_uploaded_last(self):
        self.assertEqual(self.files().returncode, 0)
        tools = self.nightly / "src" / "tools"
        tools.mkdir(parents=True)
        (tools / "ci_moved_on.sh").write_text("#!/bin/sh\necho no\n")
        (tools / "ci_moved_on.sh").chmod(0o755)
        log = self.dir / "gh.log"
        log.write_text("")
        r = run(RELEASE, self.nightly, GH_TOKEN="x", GH_REPO="o/r", SHA="abc", VERSION="v", GITHUB_REF_NAME="main")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        uploads = [l.split()[3:-1] for l in log.read_text().splitlines() if l.startswith("release upload")]
        self.assertEqual(uploads[-2:], [["out/latest.json.sig"], ["out/latest.json"]])
        self.assertEqual(sorted(f for u in uploads[:-2] for f in u),
                         sorted(f"out/{f.name}" for f in (self.nightly / "out").iterdir()
                                if f.name not in ("latest.json", "latest.json.sig")))


if __name__ == "__main__":
    unittest.main()
