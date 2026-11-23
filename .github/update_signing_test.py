#!/usr/bin/env python3
"""Tests of how .github/workflows/ci.yml writes, signs, checks and publishes latest.json and the other
files of the nightly (the steps "write latest.json" of update-manifest, "sign latest.json" of
sign-update, "check the signature" of verify-update, "the web build's SHA-256" of web, and "files",
"check the files", "builds.json" and "release" of the nightly job), run as the workflow runs them,
with the runner's ssh-keygen, a key made for the test and stand-ins for gh and git:
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


def indent(line):
    return len(line) - len(line.lstrip())


def step_at(name):
    """The line where the step `name` starts, and the line after its last."""
    at = LINES.index(next(l for l in LINES if l.strip() == f"- name: {name}"))
    end = next(i for i in range(at + 1, len(LINES)) if LINES[i].strip() and (
        indent(LINES[i]) < indent(LINES[at]) or (indent(LINES[i]) == indent(LINES[at]) and LINES[i].lstrip()[0] in "-#")))
    return at, end


def step(name):
    """The `run: |` (or `run: &<anchor> |`) script of the step `name`."""
    at, end = step_at(name)
    run = next(i for i in range(at + 1, end) if re.fullmatch(r"run: (&[\w-]+ )?\|", LINES[i].strip()))
    body = []
    for line in LINES[run + 1:end]:
        if line.strip() and indent(line) <= indent(LINES[run]):
            break
        body.append(line)
    return textwrap.dedent("\n".join(body)).rstrip("\n") + "\n"


def key_of(name, key):
    """The value of `key:` in the step `name`, as written (with its anchor or as an alias)."""
    at, end = step_at(name)
    return next(l.strip()[len(key) + 1:].strip() for l in LINES[at:end] if l.strip().startswith(f"{key}:"))


def job_lines(job):
    """The lines of the job `job`."""
    at = LINES.index(f"  {job}:")
    end = next((i for i in range(at + 1, len(LINES)) if LINES[i].strip() and indent(LINES[i]) <= 2
                and not LINES[i].lstrip().startswith("#")), len(LINES))
    return LINES[at:end]


MANIFEST = step("write latest.json")
SIGN = step("sign latest.json")
CHECK = step("check the signature")
WEB = step("the web build's SHA-256")
FILES_STEP = step("files")
FILES = step("check the files")
BUILDS = step("builds.json")
RELEASE = step("release")

# the executables update-manifest takes: (artifact, file in it, name in the release, the job that
# builds it, which gives its SHA-256 as its output sha256-<artifact>)
EXECUTABLES = [(t, "vibebms", f"vibebms-{t}", job) for t, job in (
    ("linux-x64", "build-linux"), ("linux-arm64", "build-linux"),
    ("linux-x64-portable", "build-linux-portable"), ("linux-arm64-portable", "build-linux-portable"),
    ("macos-arm64", "macos"), ("macos-x64", "macos"))]
EXECUTABLES += [(t, "vibebms.exe", f"vibebms-{t}.exe", "build-windows") for t in ("windows-x64", "windows-arm64")]
EXECUTABLES += [("macos-universal", "vibebms-macos-universal.app.zip", "vibebms-macos-universal.app.zip", "macos")]
# the other files the nightly publishes as a job uploaded them: (artifact, file, job, its output)
PACKAGES = [
    ("linux-x64-flatpak", "vibeBMS-linux-x64.flatpak", "build-linux", "sha256-linux-x64-flatpak"),
    ("linux-arm64-flatpak", "vibeBMS-linux-arm64.flatpak", "build-linux", "sha256-linux-arm64-flatpak"),
    ("macos-dmg", "vibeBMS-macos-universal.dmg", "macos-dmg", "sha256"),
    ("ios-unsigned", "vibeBMS-unsigned.ipa", "ios", "sha256"),
    ("android", "vibeBMS.apk", "android-sign", "sha256"),
]
# the matrix jobs' legs: each sets the outputs of its own name
MATRIX = {"build-linux": ["linux-x64", "linux-arm64"], "build-windows": ["windows-x64", "windows-arm64"],
          "build-linux-portable": ["linux-x64-portable", "linux-arm64-portable"]}

# gh as the steps call it: `release download nightly -p <name> -D <dir>` copies <name> from
# $GH_RELEASE (failing when it is not there); every call is logged to $GH_LOG
GH = """#!/bin/bash
printf '%s\\n' "$*" >> "$GH_LOG"
if [ "$1 $2" = "release download" ]; then
  [ -f "$GH_RELEASE/$5" ] && cp "$GH_RELEASE/$5" "$7/"
fi
"""
# git as the nightly calls it (`git -C src log -1 --format=%ct`: the commit's time)
GIT = """#!/bin/sh
echo 1700000000
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
        for name, script in (("gh", GH), ("git", GIT)):
            (self.dir / "bin" / name).write_text(script)
            (self.dir / "bin" / name).chmod(0o755)
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
    """update-manifest's folder, with every build's artifact, and the builds' SHA-256 outputs."""

    STABLE = {"version": "0.1.0", "files": {"vibebms-linux-x64": entry(b"stable")}}

    def setUp(self):
        super().setUp()
        self.job = self.dir / "manifest"
        (self.job / "tools").mkdir(parents=True)
        (self.job / "Cargo.toml").write_text('[package]\nname = "vibebms"\nversion = "0.1.0-dev"\n')
        # what each build job uploaded, and the SHA-256 it gave of it
        self.outputs = {job: {} for job in ("build-linux", "build-windows", "build-linux-portable", "macos")}
        for artifact, name, _, job in EXECUTABLES:
            d = self.job / "art" / f"vibeBMS-{artifact}"
            d.mkdir(parents=True, exist_ok=True)
            (d / name).write_bytes(f"the {artifact} build".encode())
            (d / "version.txt").write_text("0.1.0-nightly.42+abcdef0\n")
            self.outputs[job][f"sha256-{artifact}"] = sha256(d / name)
        self.release = self.dir / "release"
        self.release.mkdir()

    def needs(self, outputs=None):
        """The `needs` context as toJSON gives it."""
        return json.dumps({job: {"result": "success", "outputs": o} for job, o in (outputs or self.outputs).items()})

    def previous(self, doc, sign_with=None):
        """The release's latest.json (and its signature, signed with `sign_with`)."""
        (self.release / "latest.json").write_text(json.dumps(doc))
        (self.release / "latest.json.sig").unlink(missing_ok=True)
        if sign_with:
            self.sign_file(sign_with, self.release / "latest.json")

    def run_manifest(self, key=True, outputs=None):
        # (each run a job of its own)
        for d in ("out", "prev"):
            shutil.rmtree(self.job / d, ignore_errors=True)
        pub = self.job / "tools" / "update_key.pub"
        if key:
            pub.write_text(self.public)
        elif pub.exists():
            pub.unlink()
        return run(MANIFEST, self.job, GH_TOKEN="x", GH_REPO="o/r", GITHUB_RUN_NUMBER="42",
                   GITHUB_SHA="abcdef0123456789abcdef0123456789abcdef01", GH_RELEASE=str(self.release),
                   NEEDS=self.needs(outputs))

    def manifest(self, key=True):
        r = self.run_manifest(key)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        out = self.job / "out" / "latest.json"
        self.assertEqual(r.outputs["sha256"], sha256(out))
        self.assertEqual(r.outputs["signed"], "true" if key else "false")
        return json.loads(out.read_text())["channels"]

    def assert_nightly_is_this_run(self, channels):
        self.assertEqual(channels["nightly"]["version"], "0.1.0-nightly.42+abcdef0")
        files = channels["nightly"]["files"]
        self.assertEqual(sorted(files), sorted(n for _, _, n, _ in EXECUTABLES))
        for artifact, _, name, _ in EXECUTABLES:
            content = f"the {artifact} build".encode()
            self.assertEqual(files[name], {"version": "0.1.0-nightly.42+abcdef0",
                                           "sha256": hashlib.sha256(content).hexdigest(), "size": len(content)})


class Manifest(ManifestJob):
    """update-manifest's latest.json: this run's nightly, of the files the build jobs made, and the
    other channels of the release's previous latest.json only when that verifies with the committed key."""

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




    def assert_refused(self, r, artifact, job):
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(f"::error::the artifact vibeBMS-{artifact} is not the build the job {job} made", r.stdout)
        self.assertFalse((self.job / "out" / "latest.json").exists())

    def test_a_build_missing_fails(self):
        (self.job / "art" / "vibeBMS-windows-arm64" / "vibebms.exe").unlink()
        r = self.run_manifest()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("::error::the artifact vibeBMS-windows-arm64 has no vibebms.exe", r.stdout)

    def test_a_replaced_executable_stops_it(self):
        # another job's file in any leg's artifact: no latest.json names it
        for artifact, name, _, job in EXECUTABLES:
            with self.subTest(artifact):
                f = self.job / "art" / f"vibeBMS-{artifact}" / name
                made = f.read_bytes()
                f.write_bytes(b"another build")
                self.assert_refused(self.run_manifest(), artifact, job)
                f.write_bytes(made)
        self.manifest()

    def test_each_leg_is_checked_against_its_own_output(self):
        for job, legs in MATRIX.items():
            for leg in legs:
                with self.subTest(leg):
                    # a leg that gave no SHA-256 (its output empty, so not set)
                    outputs = json.loads(json.dumps(self.outputs))
                    del outputs[job][f"sha256-{leg}"]
                    self.assert_refused(self.run_manifest(outputs=outputs), leg, job)
                    # the other leg's SHA-256 under its name
                    other = next(l for l in legs if l != leg)
                    outputs[job][f"sha256-{leg}"] = self.outputs[job][f"sha256-{other}"]
                    self.assert_refused(self.run_manifest(outputs=outputs), leg, job)
                    # an output of another job does not count
                    outputs = json.loads(json.dumps(self.outputs))
                    outputs["test"] = {f"sha256-{leg}": outputs[job].pop(f"sha256-{leg}")}
                    self.assert_refused(self.run_manifest(outputs=outputs), leg, job)


class Outputs(unittest.TestCase):
    """Each SHA-256 the steps read is an output its job declares, and the reader needs that job."""

    def outputs(self, job):
        lines = job_lines(job)
        at = lines.index("    outputs:")
        out = {}
        for line in lines[at + 1:]:
            if line.strip().startswith("#"):
                continue
            if indent(line) <= 4:
                break
            key, value = line.strip().split(": ", 1)
            out[key] = value
        return out

    def needs(self, job):
        line = next(l for l in job_lines(job) if l.startswith("    needs: "))
        return [n.strip() for n in line.split(": ", 1)[1].strip("[]").split(",")]

    def step_ids(self, job):
        return {m.group(1) for l in job_lines(job) if (m := re.fullmatch(r"(?:- )?id: ([\w-]+)", l.strip()))}

    def test_declared(self):
        read = {(job, f"sha256-{artifact}") for artifact, _, _, job in EXECUTABLES}
        for job in ("update-manifest", "nightly"):
            for maker, _ in read:
                self.assertIn(maker, self.needs(job), job)
        read |= {(job, out) for _, _, job, out in PACKAGES}
        read |= {("web", "sha256"), ("update-manifest", "sha256"), ("update-manifest", "signed"),
                 ("sign-update", "sha256")}
        for maker, out in read:
            self.assertIn(maker, self.needs("nightly"))
            self.assertIn(out, self.outputs(maker), maker)
            ids = self.step_ids(maker)
            for sid in re.findall(r"steps\.([\w-]+)\.outputs", self.outputs(maker)[out]):
                self.assertIn(sid, ids, (maker, out))

    def test_each_leg_sets_only_its_own(self):
        # the legs' outputs are one set: a name that two legs set is the last one's
        for job, legs in MATRIX.items():
            outputs = self.outputs(job)
            names = {f"sha256-{leg}" for leg in legs}
            if job == "build-linux":
                names |= {f"sha256-{leg}-flatpak" for leg in legs}
            self.assertEqual(set(outputs), names, job)
            for name, value in outputs.items():
                leg = name[len("sha256-"):].removesuffix("-flatpak")
                self.assertIn(f"name: {leg}", "\n".join(job_lines(job)))
                self.assertRegex(value, rf"^\$\{{\{{ matrix\.name == '{re.escape(leg)}' && steps\.[\w-]+\.outputs\.sha256 \|\| '' \}}\}}$")



class Nightly(ManifestJob):
    """The nightly job's files, its check of what it publishes, and the order it uploads it in."""

    def setUp(self):
        super().setUp()
        self.channels = self.manifest()
        self.manifest_sha = sha256(self.job / "out" / "latest.json")
        shutil.copy(self.job / "out" / "latest.json", self.sign / "manifest" / "latest.json")
        r = self.signed(self.private, manifest_sha=self.manifest_sha)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.sig_sha = r.outputs["sha256"]
        # the web job's folder: its build has hidden files, which upload-artifact leaves out, and a link,
        # which it follows
        web = self.dir / "web-job"
        (web / "web" / "pkg").mkdir(parents=True)
        (web / "web" / ".cache").mkdir()
        (web / "web" / "index.html").write_text("<html></html>\n")
        (web / "web" / "pkg" / "vibebms_web_bg.wasm").write_bytes(b"\0asm")
        (web / "web" / ".gitignore").write_text("*\n")
        (web / "web" / ".cache" / "x").write_text("x\n")
        (web / "web" / "pkg" / "link.html").symlink_to("../index.html")
        r = run(WEB, web)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.outputs["web"] = {"sha256": r.outputs["sha256"]}
        # the nightly's folders: the artifacts as download-artifact puts them, the update-* downloads,
        # the sparse checkout
        self.nightly = self.dir / "nightly"
        shutil.copytree(self.job / "art", self.nightly / "art")
        for artifact, name, job, out in PACKAGES:
            d = self.nightly / "art" / f"vibeBMS-{artifact}"
            d.mkdir()
            (d / name).write_bytes(f"the {name} made".encode())
            self.outputs.setdefault(job, {})[out] = sha256(d / name)
        w = self.nightly / "art" / "vibeBMS-web"
        (w / "pkg").mkdir(parents=True)
        shutil.copy(web / "web" / "index.html", w / "index.html")
        shutil.copy(web / "web" / "pkg" / "vibebms_web_bg.wasm", w / "pkg")
        shutil.copy(web / "web" / "index.html", w / "pkg" / "link.html")
        (self.nightly / "update").mkdir()
        shutil.copy(self.sign / "manifest" / "latest.json", self.nightly / "update")
        shutil.copy(self.sign / "manifest" / "latest.json.sig", self.nightly / "update")
        self.outputs["update-manifest"] = {"sha256": self.manifest_sha, "signed": "true"}
        self.outputs["sign-update"] = {"sha256": self.sig_sha}
        tools = self.nightly / "src" / "tools"
        tools.mkdir(parents=True)
        shutil.copy(WORKFLOW.parents[2] / "tools" / "build_release.sh", tools)
        (tools / "ci_moved_on.sh").write_text("#!/bin/sh\necho no\n")
        (tools / "ci_moved_on.sh").chmod(0o755)
        self.steps = {}

    def files(self, **env):
        """The step "files", in a new out/."""
        shutil.rmtree(self.nightly / "out", ignore_errors=True)
        env = {"GH_TOKEN": "x", "GH_REPO": "o/r", "WEB_SHA256": self.outputs["web"]["sha256"], "BASE_VERSION": "0.1.0",
               "VERSION": "0.1.0-nightly.42+abcdef0", "GITHUB_REPOSITORY": "o/r", "GITHUB_SHA": "abcdef0" + "0" * 33,
               "GITHUB_RUN_NUMBER": "42", "GH_RELEASE": str(self.release), **env}
        r = run(FILES_STEP, self.nightly, **env)
        if r.returncode == 0:
            self.steps["files"] = {"outputs": r.outputs}
        return r

    def check_files(self, outputs=None):
        """The step "check the files"."""
        return run(FILES, self.nightly, NEEDS=self.needs(outputs), STEPS=json.dumps(self.steps))

    def builds(self):
        r = run(BUILDS, self.nightly, GH_TOKEN="x", GH_REPO="o/r", VERSION="0.1.0-nightly.42+abcdef0",
                GITHUB_SHA="abcdef0" + "0" * 33, GITHUB_RUN_NUMBER="42", GH_RELEASE=str(self.release))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def fdroid(self):
        """What the step "F-Droid repository" puts in out/."""
        out = self.nightly / "out"
        (out / "fdroid.txt").write_text("address\n")
        (out / "fdroid-qr.png").write_bytes(b"png")

    def release_uploads(self):
        log = self.dir / "gh.log"
        log.write_text("")
        r = run(RELEASE, self.nightly, GH_TOKEN="x", GH_REPO="o/r", SHA="abc", VERSION="v", GITHUB_REF_NAME="main")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        return [l.split()[3:-1] for l in log.read_text().splitlines() if l.startswith("release upload")]

    def assert_ok(self, r):
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    def assert_refused(self, r, what):
        self.assertNotEqual(r.returncode, 0)
        self.assertIn(f"::error::{what} is not", r.stdout)

    def test_the_files_made_go_out(self):
        self.assert_ok(self.files())
        self.assert_ok(self.check_files())
        out = self.nightly / "out"
        published = {f.name for f in out.iterdir()}
        self.assertEqual(published, {n for _, _, n, _ in EXECUTABLES} | {n for _, n, _, _ in PACKAGES} | {
            "vibeBMS-web.zip", "altstore.json", "latest.json", "latest.json.sig"})
        self.builds()
        self.fdroid()
        published |= {"builds.json", "fdroid.txt", "fdroid-qr.png"}
        self.assertEqual(sha256(out / "latest.json"), self.manifest_sha)
        self.assertEqual(sha256(out / "latest.json.sig"), self.sig_sha)
        # the zip has the web job's build as the artifact holds it (no hidden files, the link's file)
        names = subprocess.run(["unzip", "-Z1", str(out / "vibeBMS-web.zip")], capture_output=True, text=True,
                               check=True).stdout.split()
        self.assertEqual(sorted(names), ["index.html", "pkg/link.html", "pkg/vibebms_web_bg.wasm"])
        uploads = self.release_uploads()
        self.assertEqual(sorted(f for u in uploads for f in u), sorted(f"out/{n}" for n in published))

    def test_without_a_key(self):
        # no signature to check or publish, and none may be there
        (self.nightly / "update" / "latest.json.sig").unlink()
        self.outputs["update-manifest"]["signed"] = "false"
        self.outputs["sign-update"] = {}
        self.assert_ok(self.files())
        self.assert_ok(self.check_files())
        self.assertFalse((self.nightly / "out" / "latest.json.sig").exists())
        shutil.copy(self.sign / "manifest" / "latest.json.sig", self.nightly / "out")
        self.assert_refused(self.check_files(), "latest.json.sig")

    def test_a_replaced_file_stops_the_release(self):
        # another job's file in any artifact (an executable, the Flatpaks, the dmg, the ipa, the APK)
        for artifact, name, published in [(a, f, n) for a, f, n, _ in EXECUTABLES] + [(a, f, f) for a, f, _, _ in PACKAGES]:
            with self.subTest(published):
                f = self.nightly / "art" / f"vibeBMS-{artifact}" / name
                made = f.read_bytes()
                f.write_bytes(b"another build")
                self.assert_ok(self.files())
                self.assert_refused(self.check_files(), published)
                f.write_bytes(made)
        self.assert_ok(self.files())
        self.assert_ok(self.check_files())

    def test_a_maker_that_gave_no_sha256_stops_the_release(self):
        for job, out in [(j, f"sha256-{a}") for a, _, _, j in EXECUTABLES] + [(j, o) for _, _, j, o in PACKAGES]:
            with self.subTest(f"{job} {out}"):
                outputs = json.loads(json.dumps(self.outputs))
                del outputs[job][out]
                self.assert_ok(self.files())
                r = self.check_files(outputs)
                self.assertNotEqual(r.returncode, 0)
                self.assertIn("(it gave no SHA-256): nothing is published", r.stdout)

    def test_a_replaced_web_build_stops_the_release(self):
        w = self.nightly / "art" / "vibeBMS-web"
        for change in ("replace", "add", "remove"):
            with self.subTest(change):
                {"replace": lambda: (w / "index.html").write_text("<html>another</html>\n"),
                 "add": lambda: (w / "extra.js").write_text("x\n"),
                 "remove": lambda: (w / "pkg" / "link.html").unlink()}[change]()
                r = self.files()
                self.assertNotEqual(r.returncode, 0)
                self.assertIn("::error::the artifact vibeBMS-web is not the build the job web made", r.stdout)
                self.assertFalse((self.nightly / "out" / "vibeBMS-web.zip").exists())
                shutil.copy(self.dir / "web-job" / "web" / "index.html", w / "index.html")
                shutil.copy(self.dir / "web-job" / "web" / "index.html", w / "pkg" / "link.html")
                (w / "extra.js").unlink(missing_ok=True)
        r = self.files(WEB_SHA256="")
        self.assertIn("::error::the artifact vibeBMS-web is not the build the job web made", r.stdout)
        self.assert_ok(self.files())

    def test_a_replaced_zip_or_list_stops_the_release(self):
        # the files this job makes (the web zip, altstore.json) and the ones it downloads by name
        for name in ("vibeBMS-web.zip", "altstore.json", "latest.json", "latest.json.sig"):
            with self.subTest(name):
                self.assert_ok(self.files())
                (self.nightly / "out" / name).write_bytes(b"another")
                self.assert_refused(self.check_files(), name)
        self.assert_ok(self.files())
        self.assert_refused(self.check_files({**self.outputs, "update-manifest": {"sha256": "0" * 64, "signed": "true"}}),
                            "latest.json")
        self.assert_refused(self.check_files({**self.outputs, "sign-update": {"sha256": "0" * 64}}), "latest.json.sig")
        (self.nightly / "out" / "latest.json.sig").unlink()
        self.assert_refused(self.check_files(), "latest.json.sig")

    def test_a_file_no_job_made_stops_the_release(self):
        for name in ("vibebms-linux-riscv64", ".hidden", "folder"):
            with self.subTest(name):
                self.assert_ok(self.files())
                f = self.nightly / "out" / name
                f.mkdir() if name == "folder" else f.write_bytes(b"x")
                r = self.check_files()
                self.assertNotEqual(r.returncode, 0)
                self.assertIn(f"::error::{name} is not the file of any job", r.stdout)

    def test_the_list_is_uploaded_last(self):
        self.assert_ok(self.files())
        self.assert_ok(self.check_files())
        uploads = self.release_uploads()
        self.assertEqual(uploads[-2:], [["out/latest.json.sig"], ["out/latest.json"]])
        self.assertEqual(sorted(f for u in uploads[:-2] for f in u),
                         sorted(f"out/{f.name}" for f in (self.nightly / "out").iterdir()
                                if f.name not in ("latest.json", "latest.json.sig")))


if __name__ == "__main__":
    unittest.main()
