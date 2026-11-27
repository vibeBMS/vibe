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
        one = {"library/replay.rs": "pub fn s() {\n    crate::app::screenshot::dir();\n}\n"}
        self.assertEqual(self.run_on(one)[0], 0)
        two = {"app/screenshot.rs": "pub fn r() {\n    crate::library::replay::dir();\n}\n"}
        code, out = self.run_on(dict(one, **two))
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between units: app::screenshot, library::replay "
                      "(app::screenshot -> library::replay::dir at app/screenshot.rs:", out)
        self.assertIn("library::replay -> app::screenshot::dir at library/replay.rs:", out)

    def test_a_listed_cycle_that_grows_fails(self):
        code, out = self.run_on({"platform/mod.rs": "pub fn b() {\n    crate::formats::f();\n}\n",
                                 "formats/mod.rs": "pub fn h() {\n    crate::i18n::get(\"x\");\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between units: formats, i18n, platform (", out)
        self.assertIn("the cycle i18n, platform has grown: formats joined it", out)
        self.assertNotIn("no longer", out)

    def test_a_listed_cycle_closes_only_through_its_listed_uses(self):
        code, out = self.run_on({"i18n.rs": "pub fn t() {\n    crate::platform::unix_now();\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("i18n uses platform::unix_now (i18n.rs:", out)
        self.assertIn("closes the cycle i18n, platform", out)
        # the other way, more is fine
        self.assertEqual(self.run_on({"platform/mod.rs": "pub fn t() {\n    crate::i18n::current();\n}\n"})[0], 0)
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

    def test_a_groups_parts_are_units(self):
        # (a part LAYERS names, else the top-level module)
        self.assertEqual([check_layers.unit_of(m) for m in ["net::hosts", "net", "scene::select::skin", "", "a::b"]],
                         ["net::hosts", "net", "scene", "", "a"])
        # parts of one folder in one layer: a cycle between them fails
        code, out = self.run_on({"net/hosts.rs": "pub fn e() {\n    crate::net::events::poll();\n}\n",
                                 "net/events.rs": "pub fn h() {\n    crate::net::hosts::fetch();\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between units: net::events, net::hosts (", out)
        # methods of another part's type
        code, out = self.run_on({"net/hosts.rs": "use crate::net::events::Archives;\nimpl Archives {}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("methods of net::events's Archives", out)
        # a part's platform condition is counted under it
        code, out = self.run_on({"net/hosts.rs": '#[cfg(windows)]\npub fn f() {}\n'})
        self.assertEqual(code, 1, out)
        self.assertIn("net::hosts: 1 lines with a platform condition, more than the 0", out)
        # also one in a folder where conditions belong
        code, out = self.run_on({"app/screenshot.rs": '#[cfg(windows)]\npub fn f() {}\n'})
        self.assertEqual(code, 1, out)
        self.assertIn("app::screenshot: 3 lines with a platform condition, more than the 2", out)

    def test_a_folders_parts_named_in_it_are_read(self):
        # (its own modules by their names, as `self::scoredb::…` is)
        for rel, text, says in [
            ("library/mod.rs", "pub use scoredb::Rows;\n", "library (library) uses library::scoredb::Rows"),
            ("library/mod.rs", "pub fn f(b: u8) {\n    scoredb::read(b, false);\n}\n",
             "library (library) uses library::scoredb::read"),
            ("library/mod.rs", "pub use courses::{CourseStore, Course as C};\n",
             "library (library) uses library::courses::Course"),
            ("gfx/mod.rs", "pub fn f() -> usize {\n    theme::NAMES.len()\n}\n",
             "gfx (drawing and input) uses gfx::theme::NAMES"),
            ("gfx/mod.rs", "pub fn f() {\n    video::set_offline(true);\n}\n",
             "gfx (drawing and input) uses gfx::video::set_offline"),
            ("library/mod.rs", "impl courses::CourseStore {}\n", "methods of library::courses's CourseStore"),
        ]:
            code, out = self.run_on({rel: text})
            self.assertEqual(code, 1, text + out)
            self.assertIn(says, out)
        code, out = self.run_on({"library/mod.rs": "pub fn f() {\n    sqlite::read_file();\n}\n",
                                 "library/sqlite.rs": "pub fn g(_: crate::library::Song) {}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("a cycle between units: library, library::sqlite (", out)

    def test_a_reexported_item_is_its_modules(self):
        code, out = self.run_on({"library/mod.rs": "pub use courses::CourseStore;\n",
                                 "net/placement.rs": "pub fn f(_: crate::library::CourseStore) {}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("net::placement (placement) uses library::courses::CourseStore", out)
        code, out = self.run_on({"library/mod.rs": "pub(crate) use self::courses::CourseStore as Store;\n",
                                 "net/placement.rs": "use crate::library::Store;\nimpl Store {}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("net/placement.rs:", out)
        self.assertIn("methods of library::courses's Store", out)

    def test_a_glob_of_a_folder_with_units_fails(self):
        for text in ["use crate::library::*;\npub fn f(b: u8) {\n    scoredb::read(b, false);\n}\n",
                     "pub use super::super::library::*;\n"]:
            code, out = self.run_on({"net/placement.rs": text})
            self.assertEqual(code, 1, text + out)
            self.assertIn("net/placement.rs:", out)
            self.assertIn("a glob import of library, whose parts library::charts, library::courses", out)
        # a folder that is one unit
        self.assertEqual(self.run_on({"net/placement.rs": "use crate::formats::*;\n"})[0], 0)

    def test_a_platform_condition_more_fails(self):
        for text in ['#[cfg(target_os = "linux")]\npub fn f() {}\n',
                     '#[cfg(not(any(windows, target_arch = "wasm32")))]\npub fn f() {}\n',
                     "#[cfg_attr(\n    desktop,\n    allow(dead_code)\n)]\npub fn f() {}\n",
                     "pub fn f() -> bool {\n    cfg!(unix)\n}\n",
                     "pub fn f() -> bool {\n    std::cfg!(windows)\n}\n",
                     "pub fn f() -> bool {\n    core :: cfg!(target_os = \"ios\")\n}\n",
                     '#[cfg(target_pointer_width = "32")]\npub fn f() {}\n',
                     '#[cfg(any(target_endian = "big", target_has_atomic = "64"))]\npub fn f() {}\n',
                     "pub fn f() -> bool {\n    cfg!(all(test, small_device))\n}\n"]:
            code, out = self.run_on({"formats/mod.rs": text})
            self.assertEqual(code, 1, text + out)
            self.assertIn("formats: 1 lines with a platform condition, more than the 0 CONDITIONS allows", out)
            self.assertIn("(formats/mod.rs:", out)

    def test_a_condition_over_lines_is_one_line(self):
        code, out = self.run_on({"formats/mod.rs": "#[cfg(any(\n    windows,\n    unix,\n))]\npub fn f() {}\n"
                                                   "pub fn g() -> bool {\n    cfg!(windows) || cfg!(unix)\n}\n"})
        self.assertEqual(code, 1, out)
        self.assertIn("formats: 2 lines with a platform condition", out)

    def test_what_is_no_platform_condition(self):
        for rel, text in [
            ("formats/mod.rs", "#[cfg(test)]\nmod more_tests {\n    #[cfg(unix)]\n    fn f() {}\n}\n"),
            ("formats/mod.rs", '// #[cfg(windows)]\n/// cfg!(unix)\npub const S: &str = "#[cfg(unix)]";\n'),
            ("formats/mod.rs", '#[cfg(feature = "x")]\npub fn f() {}\n#[cfg_attr(test, doc = "windows")]\npub fn g() {}\n'),
            ("formats/mod.rs",
             "pub fn f(windows: u8) -> u8 {\n    mycfg(windows) + self.cfg(windows) + util::cfg(windows)\n}\n"),
            ("platform/mod.rs", "#[cfg(windows)]\npub fn f() {}\n"),
            ("app/mod.rs", "#[cfg(desktop)]\npub fn f() {}\n"),
            ("browser/mod.rs", '#[cfg(target_os = "ios")]\npub fn f() {}\n'),
        ]:
            self.assertEqual(self.run_on({rel: text})[0], 0, rel + ": " + text)

    def test_a_constant_for_a_platform_more_fails(self):
        for text in ["pub fn f() -> bool {\n    crate::platform::IS_WEB\n}\n",
                     "use crate::platform::IS_WEB;\npub fn f() -> bool {\n    !IS_WEB\n}\n",
                     "pub fn f() -> bool {\n    crate::ctx::IN_APP_BROWSER\n}\n"]:
            code, out = self.run_on({"formats/mod.rs": text})
            self.assertEqual(code, 1, text + out)
            self.assertIn("formats: 1 lines with IS_WEB or IN_APP_BROWSER, more than the 0 STOOD_IN allows", out)
        # (net::hosts has one)
        for rel, text in [("net/hosts.rs", "#[allow(unused_imports)]\nuse crate::platform::IS_WEB;\n"),
                          ("net/hosts.rs", "// IS_WEB\npub const S: &str = \"IS_WEB\";\n"),
                          ("net/hosts.rs", "#[cfg(test)]\nfn t() -> bool {\n    crate::platform::IS_WEB\n}\n"),
                          ("app/mod.rs", "pub fn f() -> bool {\n    crate::platform::IS_WEB\n}\n")]:
            self.assertEqual(self.run_on({rel: text})[0], 0, rel + ": " + text)

    def test_a_count_gone_down_is_to_be_lowered(self):
        found = {"a": ["a.rs:1"], "b": ["b/c.rs:2", "b/c.rs:3"]}
        self.assertEqual(check_layers.condition_problems(found, {"a": 1, "b": 2}), [])
        self.assertEqual(check_layers.condition_problems(found, {"a": 1, "b": 3, "c": 1}), [
            "b: 2 lines with a platform condition, fewer than the 3 CONDITIONS allows: lower it in "
            "tools/check_layers.py",
            "c: 0 lines with a platform condition, fewer than the 1 CONDITIONS allows: lower it in "
            "tools/check_layers.py"])
        self.assertEqual(check_layers.condition_problems(found, {"b": 2}), [
            "a: 1 lines with a platform condition, more than the 0 CONDITIONS allows in tools/check_layers.py: "
            "put the difference behind platform/ (a.rs:1)"])

    def test_the_groups_are_read_from_build_rs(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = pathlib.Path(tmp) / "build.rs"
            p.write_text('fn main() {\n    let groups = [\n        // ("old", false),\n'
                         '        ("desktop", desktop),\n        (\n            "in_app_browser",\n'
                         '            os == "android",\n        ),\n    ];\n    let other = [("x", 1)];\n}\n')
            self.assertEqual(check_layers.groups(str(p)), ["desktop", "in_app_browser"])


if __name__ == "__main__":
    unittest.main()
