#!/usr/bin/env python3
"""Tests of how .github/workflows/ci.yml signs latest.json (the steps "sign latest.json" of
sign-update and "check the signature" of verify-update), run as the workflow runs them, with
the runner's ssh-keygen and a key made for the test: python3 .github/update_signing_test.py
(CI's test job runs it)."""
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


SIGN = step("sign latest.json")
CHECK = step("check the signature")


def run(script, cwd, **env):
    """`script` as a step runs it (bash -e -o pipefail), in `cwd`."""
    tmp = pathlib.Path(cwd) / "runner-temp"
    tmp.mkdir(exist_ok=True)
    env = {"PATH": os.environ["PATH"], "RUNNER_TEMP": str(tmp), **env}
    return subprocess.run(["bash", "--noprofile", "--norc", "-eo", "pipefail", "-c", script],
                          cwd=cwd, env=env, capture_output=True, text=True)


def keypair(where, name):
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "vibebms-update", "-f", str(where / name)],
                   check=True, stdin=subprocess.DEVNULL)
    return (where / name).read_text(), (where / f"{name}.pub").read_text()


class Signing(unittest.TestCase):
    def setUp(self):
        self.dir = pathlib.Path(tempfile.mkdtemp())
        keys = self.dir / "keys"
        keys.mkdir()
        self.private, self.public = keypair(keys, "key")
        self.other_private, _ = keypair(keys, "other")
        # the jobs' folders: sign-update's download, verify-update's checkout and download
        self.sign = self.dir / "sign"
        (self.sign / "manifest").mkdir(parents=True)
        (self.sign / "manifest" / "latest.json").write_text('{"channels": {}}\n')
        self.check = self.dir / "check"
        (self.check / "tools").mkdir(parents=True)

    def tearDown(self):
        shutil.rmtree(self.dir)

    def signed(self, private, signed="true"):
        return run(SIGN, self.sign, SIGNED=signed, UPDATE_SIGNING_KEY=private)

    def verify(self, public=None):
        """verify-update on what sign-update made, with `public` committed."""
        if public is not None:
            (self.check / "tools" / "update_key.pub").write_text(public)
            shutil.copytree(self.sign / "manifest", self.check / "manifest", dirs_exist_ok=True)
        return run(CHECK, self.check)

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


if __name__ == "__main__":
    unittest.main()
