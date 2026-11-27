#!/usr/bin/env python3
"""Tests of tools/check_layers.py: python3 tools/check_layers_test.py. Each
puts a few lines into a copy of the app crate's sources and runs the script
on it."""
import importlib.util
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
sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("check_layers", SCRIPT)
check_layers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(check_layers)


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
            "#[cfg(test)]\nfn t() -> Result<u8, u8> {\n    %s();\n    Ok(1)\n}\n" % UP,
            "#[cfg(test)]\nfn t<A, B>(a: A, b: B) {\n    %s(a, b);\n}\n" % UP,
            "pub struct S {\n    #[cfg(test)]\n    pub a: Vec<Result<u8, u8>>,\n    #[cfg(test)]\n"
            "    pub b: [u8; { %s(); 1 }],\n}\n" % UP,
        ]:
            self.assertEqual(self.run_on({"formats/mod.rs": text})[0], 0, text)

    def test_code_after_a_test_item_with_generics_is_read(self):
        self.fails("#[cfg(test)]\nfn t() -> Result<u8, u8> {\n    Ok(1)\n}\n"
                   "pub fn h() {\n    %s();\n}\n" % UP, "formats (base) uses scene::skinvals")
        self.fails("pub enum E {\n    #[cfg(test)]\n    A = 1 << 2,\n    B,\n}\n"
                   "pub fn h() {\n    %s();\n}\n" % UP, "formats (base) uses scene::skinvals")
        self.fails("pub fn g(a: u8, b: u8) -> u8 {\n    match a {\n        #[cfg(test)]\n"
                   "        x if x < b => 0,\n        _ => { %s(); 2 }\n    }\n}\n" % UP,
                   "formats (base) uses scene::skinvals")

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

    def test_the_crate_under_another_name_fails(self):
        self.fails("use crate as root;\npub fn g() {\n    root::scene::skinvals::judge_option();\n}\n",
                   "formats/mod.rs:")
        self.fails("pub use  crate\n    as root;\n", "(`use crate as`)")
        code, out = self.run_on({"lib.rs": "extern crate self as me;\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("lib.rs:", out)
        self.assertIn("(`extern crate self as`)", out)

    def test_methods_of_another_modules_type_fail(self):
        for text in ["use crate::ctx::Overrides;\nimpl Overrides {\n    pub fn f() {}\n}\n",
                     "use crate::ctx::Overrides as O;\nimpl O {}\n",
                     "impl crate::ctx::Overrides {}\n",
                     "use crate::ctx;\nimpl<T> ctx::Mute<T> where T: Copy {}\n"]:
            code, out = self.run_on({"app/cli.rs": text})
            self.assertEqual(code, 1, text + out)
            self.assertIn("app/cli.rs:", out)
            self.assertIn("methods of ctx's ", out)
        # a trait's, and a type of its own module
        for text in ["use crate::ctx::Overrides;\nimpl Default for Overrides {}\n",
                     "use crate::ctx::Overrides;\nimpl<T> From<T> for Overrides {}\n",
                     "pub struct Here;\nimpl Here {}\nimpl super::Cli {}\n"]:
            self.assertEqual(self.run_on({"app/cli.rs": text})[0], 0, text)

    def test_a_cycle_between_top_level_modules_fails(self):
        # (two modules of one layer: neither use is upward)
        one = {"replay.rs": "pub fn s() {\n    crate::screenshot::dir();\n}\n"}
        self.assertEqual(self.run_on(one)[0], 0)
        code, out = self.run_on(dict(one, **{"screenshot.rs": "pub fn r() {\n    crate::replay::dir();\n}\n"}))
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between top-level modules: replay, screenshot "
                      "(replay -> screenshot::dir at replay.rs:", out)
        self.assertIn("screenshot -> replay::dir at screenshot.rs:", out)

    def test_a_listed_cycle_that_grows_fails(self):
        code, out = self.run_on({"hosts.rs": "pub fn b() {\n    crate::bulk::tick();\n}\n",
                                 "bulk.rs": "pub fn h() {\n    crate::hosts::fetch();\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between top-level modules: bulk, hosts, songfetch (", out)
        self.assertIn("the cycle bulk, songfetch has grown: hosts joined it", out)
        self.assertNotIn("no longer", out)

    def test_a_listed_cycle_closes_only_through_its_listed_uses(self):
        code, out = self.run_on({"songfetch.rs": "pub fn t() {\n    crate::bulk::tick();\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("songfetch uses bulk::tick (songfetch.rs:", out)
        self.assertIn("closes the cycle bulk, songfetch", out)
        # the other way, more is fine
        self.assertEqual(self.run_on({"bulk.rs": "pub fn t() {\n    crate::songfetch::poll();\n}\n"})[0], 0)
        listed = [(["a", "b", "c"], [("c", "a", ["x"])], "")]
        found = {("a", "b", "u"): ["a.rs:1"], ("b", "c", "v"): ["b.rs:2"], ("c::d", "a", "x"): ["c/d.rs:3"]}
        self.assertEqual(check_layers.cycle_problems(found, listed), ([], 1))
        # a cycle without the listed use
        bad, _ = check_layers.cycle_problems({**found, ("b", "a", "w"): ["b.rs:4"]}, listed)
        self.assertEqual(bad, ["a, b use each other without the uses CYCLES lists for them "
                               "(a -> b::u at a.rs:1; b -> a::w at b.rs:4)"])
        # a listed use that is gone
        del found[("c::d", "a", "x")]
        bad, _ = check_layers.cycle_problems({**found, ("c", "a", "y"): ["c.rs:5"]}, listed)
        self.assertEqual(bad, ["c uses a::y (c.rs:5), a use that closes the cycle a, b, c but not one CYCLES "
                               "lists for it", "c no longer uses a::x: take it out of CYCLES in tools/check_layers.py"])

    def test_cycles_are_found_through_submodules_and_a_gone_one_fails(self):
        found = {("a", "b", "x"): ["a.rs:1"], ("b::c", "c", "y"): ["b/c.rs:2"], ("c", "a::d", "z"): ["c.rs:3"],
                 ("c", "e", "w"): ["c.rs:4"]}
        self.assertEqual(check_layers.cycles(found),
                         {("a", "b", "c"): ["a -> b::x at a.rs:1", "b -> c::y at b/c.rs:2", "c -> a::d::z at c.rs:3"]})
        closing = [("c", "a::d", ["z"])]
        self.assertEqual(check_layers.cycle_problems(found, [(["c", "b", "a"], closing, "")]), ([], 1))
        bad, _ = check_layers.cycle_problems(found, [(["a", "b"], [], ""), (["a", "b", "c"], closing, "")])
        self.assertEqual(bad, ["the cycle a, b has grown: c joined it: change CYCLES in tools/check_layers.py"])
        del found[("c", "a::d", "z")]
        bad, n = check_layers.cycle_problems(found, [(["a", "b", "c"], closing, "")])
        self.assertEqual((bad[0], n), ("a, b, c no longer use each other in a cycle: take it out of CYCLES in "
                                       "tools/check_layers.py", 0))


if __name__ == "__main__":
    unittest.main()
