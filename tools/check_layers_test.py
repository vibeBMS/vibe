#!/usr/bin/env python3
"""Tests of tools/check_layers.py: python3 tools/check_layers_test.py. Each
puts a few lines into a copy of the app crate's sources and runs the script
on it."""
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE / "check_layers.py"
SRC = HERE.parent / "crates" / "vibebms" / "src"
# an item of the scenes, the highest layer but the shell
UP = "crate::scene::skinvals::judge_option"


class CheckLayers(unittest.TestCase):
    def run_on(self, files):
        """The script's exit code and output on the sources with `files`
        ({path: text}) added (appended to a file that is there)."""
        with tempfile.TemporaryDirectory() as tmp:
            src = pathlib.Path(tmp) / "src"
            shutil.copytree(SRC, src)
            for rel, text in files.items():
                p = src / rel
                p.parent.mkdir(parents=True, exist_ok=True)
                with open(p, "a", encoding="utf-8") as f:
                    f.write("\n" + text)
            r = subprocess.run([sys.executable, str(SCRIPT), str(src)], capture_output=True, text=True)
            return r.returncode, r.stdout

    def fails(self, text, says):
        code, out = self.run_on({"formats/mod.rs": text})
        self.assertEqual(code, 1, out)
        self.assertIn(says, out)

    def test_the_tree_as_it_is(self):
        self.assertEqual(self.run_on({})[0], 0)

    def test_code_after_a_test_field_is_read(self):
        self.fails("pub struct S {\n    #[cfg(test)]\n    pub a: u8,\n}\n"
                   "pub static UP: fn() = %s;\n" % UP, "formats (base) uses scene::skinvals")

    def test_code_after_a_test_variant_or_arm_is_read(self):
        self.fails("pub enum E {\n    A,\n    #[cfg(test)]\n    B,\n}\n"
                   "pub fn g(e: E) -> u8 {\n    match e {\n        E::A => 0,\n"
                   "        #[cfg(test)]\n        E::B => 1,\n    }\n}\n"
                   "pub fn h() {\n    %s();\n}\n" % UP, "formats (base) uses scene::skinvals")
        self.fails("pub fn g(e: u8) -> u8 {\n    match e {\n        #[cfg(test)]\n        1 => 0,\n"
                   "        _ => 2,\n    }\n}\npub fn h() {\n    %s();\n}\n" % UP, "formats (base) uses")

    def test_test_items_are_still_left_out(self):
        for text in [
            "#[cfg(test)]\npub fn t() {\n    %s();\n}\n" % UP,
            "#[cfg(test)]\nuse %s;\n" % UP,
            "#[cfg(all(test, not(target_arch = \"wasm32\")))]\nfn t(a: u8, b: u8) {\n    %s(a, b);\n}\n" % UP,
            "pub struct S {\n    #[cfg(test)]\n    pub a: [u8; { %s(); 1 }],\n}\n" % UP,
        ]:
            self.assertEqual(self.run_on({"formats/mod.rs": text})[0], 0, text)

    def test_a_byte_raw_string_is_a_string(self):
        for lit in ['br#"{"a": "}"}"#', 'br"}"']:
            text = ("#[cfg(test)]\nmod more_tests {\n    const J: &[u8] = %s;\n"
                    "    #[test]\n    fn t() {\n        %s();\n    }\n}\n" % (lit, UP))
            self.assertEqual(self.run_on({"formats/mod.rs": text}), (0, self.run_on({})[1]), lit)

    def test_a_path_attribute_fails(self):
        code, out = self.run_on({
            "formats/mod.rs": '#[path = "../scene/low/helper.rs"]\npub mod helper;\n',
            "scene/low/helper.rs": "pub fn h() {\n    %s();\n}\n" % UP,
        })
        self.assertEqual(code, 1, out)
        self.assertIn("formats/mod.rs:", out)
        self.assertIn("#[path]", out)

    def test_a_macro_made_path_fails(self):
        self.fails("macro_rules! up {\n    ($m:ident) => {\n        crate::$m::skinvals::judge_option()\n"
                   "    };\n}\npub fn g() {\n    up!(scene);\n}\n", "macro's argument")
        self.fails("macro_rules! up2 {\n    ($m:ident) => {\n        $crate::$m::f()\n    };\n}\n",
                   "macro's argument")


if __name__ == "__main__":
    unittest.main()
