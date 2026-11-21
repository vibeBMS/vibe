#!/usr/bin/env python3
"""Tests of tools/repro_pins.py and of which pull request .github/workflows/toolchain-pins.yml takes as its own:
python3 tools/repro_pins_test.py (CI's test job runs it)."""
import contextlib
import importlib.util
import io
import json
import pathlib
import re
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("repro_pins", HERE / "repro_pins.py")
repro_pins = importlib.util.module_from_spec(spec)
spec.loader.exec_module(repro_pins)

ENV = (HERE / "repro.env").read_text()
PINS = repro_pins.parse(ENV)
DIGEST = "sha256:" + "ab" * 32
COMMIT = "0123456789abcdef0123456789abcdef01234567"
# what a registry's answer must never get into a file bash sources
HOSTILE = [
    "4.1.3$(touch pwned)", "4.1.3`touch pwned`", "4.1.3;touch pwned", "4.1.3 touch", "4.1.3\ttouch",
    '4.1.3"', "4.1.3'", "4.1.3\\", "4.1.3&touch pwned", "4.1.3|touch", "4.1.3>pwned", "$IFS", "${HOME}",
    "4.1.3\nNDK_VERSION=1", "4.1.3\r", "null", "", "v4.1.3", "4.1.3-rc1", "4..3", ".4", "4.",
]


def run(argv):
    """main's exit status, stdout and stderr."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = repro_pins.main(["repro_pins.py", *map(str, argv)])
    return rc, out.getvalue(), err.getvalue()


class Shapes(unittest.TestCase):
    def test_the_pins_of_today_are_well_formed(self):
        self.assertEqual(repro_pins.MOVED, repro_pins.SHAPE.keys())
        for key in repro_pins.MOVED:
            self.assertTrue(repro_pins.well_formed(key, PINS[key]), f"{key}={PINS[key]}")

    def test_values_of_each_kind(self):
        good = {
            "CARGO_NDK_VERSION": ["4.1.3", "5", "10.0.26100.0"],
            "UBUNTU_SNAPSHOT": ["20261201T000000Z"],
            "MACOS_RUNNER": ["macos-27", "macos-27-xlarge"],
            "WINDOWS_RUNNER": ["windows-2028", "windows-2025-vs2026"],
            "WEB_IMAGE": [f"docker.io/library/ubuntu:26.04@{DIGEST}", f"quay.io/pypa/manylinux_2_28_x86_64@{DIGEST}"],
            "LIBDMG_HFSPLUS_COMMIT": [COMMIT],
        }
        bad = {
            "CARGO_NDK_VERSION": HOSTILE,
            "UBUNTU_SNAPSHOT": ["20261201", "2026-12-01T00:00:00Z", "20261201T000000Z;id", "20261201T000000Z\n"],
            "MACOS_RUNNER": ["macos-latest", "ubuntu-24.04", "macos-27 ", "macos-27$(id)", "macos-27\nX=1", "self-hosted"],
            "WINDOWS_RUNNER": ["windows-latest", "windows-2028`id`", "Windows-2028"],
            "WEB_IMAGE": ["docker.io/library/ubuntu:26.04", f"ubuntu@{DIGEST[:-1]}", f"ubuntu@{DIGEST}0",
                          f"ubuntu:26.04@{DIGEST};id", f"ubuntu $(id)@{DIGEST}", f"$(id)@{DIGEST}", f"-v/:/host@{DIGEST}",
                          f"ubuntu@{DIGEST}\n", f"ubuntu@sha256:{'AB' * 32}"],
            "LIBDMG_HFSPLUS_COMMIT": ["main", COMMIT[:39], COMMIT + "0", COMMIT.upper(), COMMIT[:-1] + "g", COMMIT + "\n",
                                      "$(id)" + COMMIT[5:]],
        }
        for key, values in good.items():
            for v in values:
                self.assertTrue(repro_pins.well_formed(key, v), f"{key}={v!r}")
        for key, values in bad.items():
            for v in values:
                self.assertFalse(repro_pins.well_formed(key, v), f"{key}={v!r}")


class Choose(unittest.TestCase):
    def choose(self, facts):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            up, ma, upd, maj = repro_pins.choose(ENV, facts)
        return up, ma, upd, maj, err.getvalue()

    def test_a_good_fact_is_written(self):
        new = PINS["CARGO_NDK_VERSION"] + ".1"
        up, ma, upd, maj, err = self.choose({"CARGO_NDK_VERSION": new, "LIBDMG_HFSPLUS_COMMIT": COMMIT})
        self.assertEqual(upd, {"CARGO_NDK_VERSION": new, "LIBDMG_HFSPLUS_COMMIT": COMMIT})
        self.assertIn(f"\nCARGO_NDK_VERSION={new}\n", up)
        self.assertEqual((ma, maj, err), (ENV, {}, ""))

    def test_a_bad_fact_is_left_out_with_a_warning(self):
        # (Xcode, MSVC and the SDK come as lists, the labels as *_newest: below)
        for key in sorted(repro_pins.MOVED - repro_pins.LABELS - {"XCODE_VERSION", "MSVC_COMPONENT", "WINDOWS_SDK"}):
            for value in HOSTILE:
                if not value:
                    continue
                up, ma, upd, maj, err = self.choose({key: value})
                self.assertEqual((up, ma, upd, maj), (ENV, ENV, {}, {}), f"{key}={value!r}")
                self.assertIn(f"::warning::{key}: not a value for it", err)

    def test_lists_and_labels(self):
        mac, win = PINS["MACOS_RUNNER"], PINS["WINDOWS_RUNNER"]
        xcode = PINS["XCODE_VERSION"]
        newer = f"{xcode.split('.')[0]}.{int(xcode.split('.')[1]) + 1}"
        # a bad entry of a list does not hide the good ones
        up, ma, upd, maj, err = self.choose({f"xcode@{mac}": f"{xcode} 99.0$(id) {newer}"})
        self.assertEqual((upd, maj), ({"XCODE_VERSION": newer}, {}))
        self.assertIn("99.0$(id)", err)
        # a label that is not one takes nothing of what was found for it
        for label in ("macos-27;id", "macos-27 x", "$(id)"):
            up, ma, upd, maj, err = self.choose({"macos_newest": label, f"xcode@{label}": "27.0",
                                                 "windows_newest": label, f"msvc@{label}": "14.50.18.0",
                                                 f"sdk@{label}": "10.0.30000.0"})
            self.assertEqual((up, ma, upd, maj), (ENV, ENV, {}, {}), label)
            self.assertIn("::warning::MACOS_RUNNER", err)
            self.assertIn("::warning::WINDOWS_RUNNER", err)
        up, ma, upd, maj, err = self.choose({"macos_newest": "macos-99", "xcode@macos-99": "99.1 99.0",
                                             "windows_newest": win})
        self.assertEqual((upd, maj, err), ({}, {"MACOS_RUNNER": "macos-99", "XCODE_VERSION": "99.1"}, ""))

    def test_a_facts_file_with_shell_code_changes_nothing(self):
        with tempfile.TemporaryDirectory() as d:
            env, facts, up, ma = (pathlib.Path(d, n) for n in ("repro.env", "facts.txt", "update.env", "major.env"))
            env.write_text(ENV)
            facts.write_text(
                f"CARGO_NDK_VERSION=4.1.3$(touch {d}/pwned)\n"
                f"JAVA_VERSION=17.0.22 $(touch {d}/pwned)\n"
                f"GRADLE_VERSION=`touch {d}/pwned`\n"
                f"NDK_VERSION=30.0.1\"; touch {d}/pwned; \"\n"
                f"LIBDMG_HFSPLUS_COMMIT={COMMIT}; touch {d}/pwned\n"
                f"WEB_IMAGE=docker.io/library/ubuntu:26.04@{DIGEST} && touch {d}/pwned\n"
                f"UBUNTU_SNAPSHOT=$(touch {d}/pwned)\n"
                f"macos_newest=macos-27$(touch {d}/pwned)\n")
            rc, out, err = run(["choose", env, facts, up, ma])
            self.assertEqual((rc, out), (0, ""))
            self.assertEqual(err.count("::warning::"), 8, err)
            for f in (up, ma):
                self.assertEqual(f.read_text(), ENV)
                subprocess.run(["bash", "-euc", '. "$1"', "sh", f], check=True, cwd=d)
            self.assertFalse(pathlib.Path(d, "pwned").exists())


class Check(unittest.TestCase):
    def check(self, key, value, quoted=False):
        old = f"\n{key}={PINS[key]}\n"
        self.assertIn(old, ENV)
        head = ENV.replace(old, f'\n{key}="{value}"\n' if quoted else f"\n{key}={value}\n")
        with tempfile.TemporaryDirectory() as d:
            base, new = pathlib.Path(d, "base.env"), pathlib.Path(d, "head.env")
            base.write_text(ENV)
            new.write_text(head)
            return run(["check", base, new])

    def test_a_good_change_is_merged_or_held(self):
        self.assertEqual(self.check("CARGO_NDK_VERSION", PINS["CARGO_NDK_VERSION"] + ".1")[0], 0)
        self.assertEqual(self.check("LIBDMG_HFSPLUS_COMMIT", COMMIT)[0], 0)
        self.assertEqual(self.check("WEB_IMAGE", f"{PINS['WEB_IMAGE'].split('@')[0]}@{DIGEST}")[0], 0)
        self.assertEqual(self.check("CARGO_NDK_VERSION", "99.0.0")[0], 3)
        self.assertEqual(self.check("MACOS_RUNNER", "macos-99")[0], 3)
        self.assertEqual(self.check("WEB_IMAGE", f"docker.io/library/debian:13@{DIGEST}")[0], 3)

    def test_a_bad_value_is_an_error(self):
        for key in sorted(repro_pins.MOVED):
            for value in HOSTILE:
                if "\n" in value or "\r" in value:
                    continue
                for quoted in (False, True):
                    rc, out, _ = self.check(key, value, quoted)
                    self.assertEqual(rc, 1, f"{key}={value!r} quoted={quoted}: {out}")
        # the versions the review named: the numbers alone compared as newer
        for value in ("4.1.3$(echo${IFS}INJECTED>&2)", "17.0.22 $(echo INJECTED >&2)"):
            rc, out, _ = self.check("CARGO_NDK_VERSION", value)
            self.assertEqual(rc, 1)
            self.assertIn("error: CARGO_NDK_VERSION: not a value for it", out)

    def test_a_value_with_a_newline_is_an_error(self):
        # (it is another line of the file: a pin set twice, or a line that is not a pin's)
        for extra in ("NDK_VERSION=1", "touch pwned", "X=1"):
            rc, _, _ = self.check("CARGO_NDK_VERSION", f"4.1.3\n{extra}")
            self.assertEqual(rc, 1, extra)


class OwnPullRequest(unittest.TestCase):
    """Which open pull request the workflow (.github/workflows/toolchain-pins.yml) takes as its own."""

    WORKFLOW = (HERE.parent / ".github" / "workflows" / "toolchain-pins.yml").read_text()
    FILTER = re.search(r"gh pr list --head \"\$branch\".*?--jq '([^']*)'", WORKFLOW, re.S).group(1)

    def own(self, prs):
        out = subprocess.run(["jq", "-r", self.FILTER], input=json.dumps(prs), capture_output=True, text=True, check=True)
        return out.stdout.strip()

    def test_only_its_own_from_this_repository(self):
        ours = {"number": 7, "isCrossRepository": False, "author": {"login": "app/github-actions", "is_bot": True}}
        fork = {"number": 5, "isCrossRepository": True, "author": {"login": "app/github-actions"}}
        person = {"number": 6, "isCrossRepository": False, "author": {"login": "someone"}}
        lookalike = {"number": 8, "isCrossRepository": False, "author": {"login": "github-actions-fan"}}
        self.assertEqual(self.own([ours]), "7")
        self.assertEqual(self.own([{**ours, "author": {"login": "github-actions[bot]"}}]), "7")
        self.assertEqual(self.own([fork, person, lookalike, ours]), "7")
        self.assertEqual(self.own([{**fork, "author": {"login": "someone"}}]), "")
        self.assertEqual(self.own([fork, person, lookalike]), "")
        self.assertEqual(self.own([]), "")


if __name__ == "__main__":
    unittest.main()
