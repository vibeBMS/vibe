#!/usr/bin/env python3
"""Tests of the hold in .github/workflows/dependabot-merge.yml (its held.py, which
prints the updates that wait for a person): python3 .github/dependabot_merge_test.py
(CI's test job runs it)."""
import pathlib
import subprocess
import sys
import textwrap
import unittest

WORKFLOW = pathlib.Path(__file__).resolve().parent / "workflows" / "dependabot-merge.yml"
text = WORKFLOW.read_text()
HELD = textwrap.dedent(text[text.index("held.py\" <<'EOF'\n") + len("held.py\" <<'EOF'\n"):text.index("\n          EOF\n")])
OWN = "bmseeker" if 'OWN = {"bmseeker"}' in HELD else None


def held(message):
    """The names held, in the order printed."""
    out = subprocess.run([sys.executable, "-I", "-c", HELD], input=message, capture_output=True, text=True, check=True)
    return [line.split()[0] for line in out.stdout.splitlines()]


def bumps(name, old, new):
    return f"Bump {name} from {old} to {new}\n\nBumps [{name}](https://github.com/o/{name}) from {old} to {new}.\n"


def updates(*rows):
    body = "".join(f"\nUpdates `{name}` from {old} to {new}\n- [Commits](https://github.com/o/r)\n" for name, old, new in rows)
    return f"Bump the cargo group with {len(rows)} updates\n{body}"


class Held(unittest.TestCase):
    def test_versions(self):
        for old, new, waits in (
            ("1.2.3", "1.2.4", False), ("1.2.3", "1.3.0", False), ("1.2.3", "2.0.0", True),
            ("0.6.1", "0.6.2", False), ("0.6.1", "0.8.0", True), ("0.0.3", "0.0.4", True),
            ("v4", "v5", True), ("4.1.0", "4.2.0", False), ("1.99.0", "1.100.0", False), ("1.3.0", "1.2.9", True),
        ):
            self.assertEqual(held(bumps("serde", old, new)), ["serde"] if waits else [], f"{old} -> {new}")
            self.assertEqual(held(updates(("serde", old, new))), ["serde"] if waits else [], f"{old} -> {new}")

    def test_a_commit_is_no_version(self):
        # (the hashes whose digits read as a minor update, a major update, and no version at all)
        for old, new in (("09fbb4f", "9a5b111"), ("09fbb4f", "1c7cff8"), ("abcdefa", "fedcbaf"), ("1234567", "1234568"),
                         ("e18579b391950589b9ed16ee630c54ce4319bb29", "0123456789abcdef0123456789abcdef01234567")):
            for q in ("`", ""):
                o, n = f"{q}{old}{q}", f"{q}{new}{q}"
                self.assertEqual(held(bumps("some-crate", o, n)), ["some-crate"], f"{old} -> {new}")
                self.assertEqual(held(bumps("dtolnay/rust-toolchain", o, n)), ["dtolnay/rust-toolchain"])
                self.assertEqual(held(updates(("serde", "1.0.1", "1.0.2"), ("some-crate", o, n))), ["some-crate"])
                if OWN:
                    self.assertEqual(held(bumps(OWN, o, n)), [], f"{old} -> {new}")
                    self.assertEqual(held(updates((OWN, o, n), ("serde", "1.0.1", "1.0.2"))), [])
                    self.assertEqual(held(updates((OWN, o, n), ("some-crate", o, n))), ["some-crate"])
        # a version that becomes a commit, or the other way round, of anything
        for name in filter(None, ("some-crate", OWN)):
            self.assertEqual(held(bumps(name, "1.2.3", "`9a5b111`")), [name])
            self.assertEqual(held(bumps(name, "`9a5b111`", "1.2.3")), [name])

    def test_no_update_found(self):
        self.assertEqual(held("Something else\n"), ["no"])


if __name__ == "__main__":
    unittest.main()
