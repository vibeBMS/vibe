#!/usr/bin/env python3
"""tools/check_layers.py [src]: no module of the vibebms crate uses an item
of a higher layer (LAYERS) but the uses listed in UPWARD (docs/BUILDING.md,
"Development"; CI's test job runs it).

Every .rs file under src is a module (a/b.rs and a/b/mod.rs are a::b), in
the layer of the longest prefix of its path that LAYERS names. What it
uses is read from its text, with comments, strings and test code left out
(an item under `#[cfg(test)]`, a file declared under it): its paths from
`crate::`, `super::` and `self::`, the paths through a module it imports
(`use crate::platform::vfs;` then `vfs::read`), and the crate's own macros
(`tr!`). A path's target is its longest prefix that is a module, the rest
the item. A use of a higher layer fails unless UPWARD lists it; a use
UPWARD lists that no longer exists fails too, so the list is shortened
when one goes. What a value's method reaches without naming its type is
not seen.
"""
import os
import re
import sys
from collections import defaultdict

sys.dont_write_bytecode = True

# lowest first; a module's layer is that of the longest prefix named here
LAYERS = [
    ("base", ["formats", "i18n"]),
    ("platform", ["platform"]),
    ("library", ["library", "lr2db", "links"]),
    ("placement", ["placement", "charts"]),
    ("courses and scores", ["courses", "scoredb"]),
    ("settings", ["config"]),
    ("kept files", ["replay", "screenshot"]),
    ("paths", ["paths"]),
    ("drawing and input", ["gfx", "input", "perspective"]),
    ("looks and touch", ["theme", "video", "touch"]),
    ("skins", ["skin"]),
    ("sound", ["loudness", "sysound"]),
    ("services", ["hosts", "profiles", "update", "ir", "integrations", "import", "archive", "songfetch", "bulk",
                  "skinstore", "web"]),
    ("widgets", ["ui"]),
    ("browser", ["browser"]),
    ("scenes", ["scene"]),
    ("shell", ["app", ""]),  # "": lib.rs, main.rs
]

# (from module or the modules under it, to module, items, why it stays)
SHELL = "defined in app/, the shell, until the scenes' contract and the shared state have modules of their own"
UPWARD = [
    ("scene", "app", ["Ctx", "Draw", "Next", "Scene", "IN_APP_BROWSER"], SHELL),
    ("ui", "app", ["Ctx", "Draw"], SHELL),
    ("browser", "app", ["Ctx", "Draw"], SHELL),
    ("skin::render", "app", ["Draw"], SHELL),
    ("loudness", "app", ["Ctx"], SHELL + "; it takes the whole Ctx to pass parts of it on"),
    ("bulk", "app", ["Ctx"], SHELL + "; it takes the whole Ctx to pass parts of it on"),
    ("import", "app", ["Ctx"], SHELL + "; it takes the whole Ctx to pass parts of it on"),
    ("archive", "app", ["AppEvent"], SHELL + "; what a service has done reaches the shell as an AppEvent"),
    ("ir", "app", ["AppEvent"], SHELL + "; what a service has done reaches the shell as an AppEvent"),
    ("skinstore", "app", ["AppEvent"], SHELL + "; what a service has done reaches the shell as an AppEvent"),
    ("songfetch", "app", ["AppEvent"], SHELL + "; what a service has done reaches the shell as an AppEvent"),
    ("i18n", "platform::android", ["locale"],
     "AUTO's language on Android is the Java locale; passing the system's locale in from the start instead "
     "would read it at every start, not only when AUTO is chosen"),
    ("formats", "gfx", ["fits"], "the size every image decoder keeps to: it belongs in formats"),
    ("platform", "library", ["is_chart"], "the test of a chart's extension: it belongs in vibebms-core beside "
     "CHART_EXTENSIONS"),
    ("platform::vfs", "placement", ["Vfs"], "the player's files as bmseeker sees them: platform code"),
    ("platform::opfs", "replay", ["dir"], "which folders the browser keeps: the start can say"),
    ("platform::ios", "input::hid", ["key_code"], "only iOS' keyboard reads HID usages: it belongs in platform"),
    ("library", "scene::skinvals", ["judge_rank_number"], "a chart's judge rank: it belongs to ChartMeta"),
    ("config", "archive", ["DEFAULT_INDEXES", "CATALOG", "OLD_DEFAULT"], "default settings: they belong in config"),
    ("config", "replay", ["Slot"], "which of a chart's replays: it belongs in vibebms-core's replay"),
    ("paths", "update", ["app_bundle"], "the macOS bundle's layout: it belongs in paths"),
    ("input", "touch", ["START", "SELECT", "START2", "SELECT2"], "lanes of the input: they belong in input"),
    ("gfx::text", "skin", ["read_font"], "the font reader is gfx's; the capped read belongs in platform::vfs"),
    ("skin", "sysound", ["lr2ss_header"], "an LR2 file's header: it belongs in skin::lr2"),
]


def blank(text):
    """`text` with its comments and strings as spaces, newlines kept."""
    out = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                depth += {"/*": 1, "*/": -1}.get(text[j : j + 2], 0)
                j += 2 if text[j : j + 2] in ("/*", "*/") else 1
            out.append(re.sub(r"[^\n]", " ", text[i:j]))
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            j += 1
            out.append('"' + re.sub(r"[^\n]", " ", text[i + 1 : j - 1]) + '"')
        elif c == "r" and (m := re.compile(r'r(#*)"').match(text, i)) and not re.match(r"\w", text[i - 1 : i]):
            end = '"' + m.group(1)
            j = text.find(end, m.end())
            j = n if j < 0 else j + len(end)
            out.append(re.sub(r"[^\n]", " ", text[i:j]))
        elif c == "'" and (m := re.compile(r"'(\\.[^']*|[^\\'])'").match(text, i)):
            j = m.end()
            out.append(" " * (j - i))
        else:
            j = i + 1
            out.append(c)
        i = j
    return "".join(out)


def item_end(text, i):
    """Where the item starting at `i` ends: its `;`, or its `{ … }`."""
    while i < len(text) and text[i] not in "{;":
        i += 1
    if i < len(text) and text[i] == "{":
        depth = 0
        while i < len(text):
            depth += {"{": 1, "}": -1}.get(text[i], 0)
            if depth == 0:
                break
            i += 1
    return i + 1


def cut_tests(text):
    """`text` with each item under `#[cfg(test)]` (or `all(test, …)`) as
    spaces, newlines kept."""
    for m in reversed(list(re.finditer(r"#\[cfg\((all\()?test\b", text))):
        j = item_end(text, m.end())
        text = text[: m.start()] + re.sub(r"[^\n]", " ", text[m.start() : j]) + text[j:]
    return text


def expand(path):
    """`a::{b, c::{d, e}}` as [`a::b`, `a::c::d`, `a::c::e`]."""
    i = path.find("{")
    if i < 0:
        return [path]
    depth, j = 0, i
    for j in range(i, len(path)):
        depth += {"{": 1, "}": -1}.get(path[j], 0)
        if depth == 0:
            break
    head, body, tail = path[:i], path[i + 1 : j], path[j + 1 :]
    parts, depth, cur = [], 0, ""
    for ch in body:
        depth += {"{": 1, "}": -1}.get(ch, 0)
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    parts.append(cur)
    return [head + e + tail for p in parts if p for e in expand(p)]


def paths(text, roots):
    """(start, end, line, path, alias) for every path in `text` that starts
    with one of `roots`, groups expanded; `alias` is the name an `as`
    gives it."""
    pat = re.compile(r"(?<![\w:])(%s)((?:\s*::\s*(?:\w+|\{|\*))+)" % "|".join(roots))
    for m in pat.finditer(text):
        start, i, s = m.start(), m.end(), m.group(0)
        if s.rstrip().endswith("{"):
            depth = 1
            while i < len(text) and depth:
                depth += {"{": 1, "}": -1}.get(text[i], 0)
                i += 1
            s = text[start:i]
        s = re.sub(r"\s+as\s+", "@", s)
        s = re.sub(r"\s+", "", s)
        line = text.count("\n", 0, start) + 1
        for e in expand(s):
            p, _, alias = e.partition("@")
            yield start, i, line, p, alias


def module_of(src, path):
    rel = os.path.relpath(path, src)[: -len(".rs")].split(os.sep)
    if rel[-1] in ("mod", "lib", "main"):
        rel = rel[:-1]
    return "::".join(rel)


def layer_of(module):
    best = None
    for i, (_, mods) in enumerate(LAYERS):
        for m in mods:
            if module == m or (m and module.startswith(m + "::")) or (not m and not module):
                if best is None or len(m) > len(best[1]):
                    best = (i, m)
    return best[0] if best else None


def under(module, prefix):
    return module == prefix or module.startswith(prefix + "::")


def uses(src):
    """(from module, to module, item) -> ["file:line", ...]"""
    files = sorted(os.path.join(r, n) for r, _, ns in os.walk(src) for n in ns if n.endswith(".rs"))
    texts = {f: blank(open(f, encoding="utf-8").read()) for f in files}
    modules = {module_of(src, f) for f in files}
    tests = set()  # files declared under #[cfg(test)]
    macros = {}
    for f, text in texts.items():
        mod = module_of(src, f)
        for m in re.finditer(r"#\[cfg\(test\)\]\s*(pub(\([\w:]+\))?\s+)?mod\s+(\w+)\s*;", text):
            tests.add("::".join(filter(None, [mod, m.group(3)])))
        for m in re.finditer(r"macro_rules!\s*(\w+)", text):
            macros[m.group(1)] = mod

    def target(here, path):
        segs = path.split("::")
        if segs[0] == "crate":
            full = segs[1:]
        else:
            k = 0
            while k < len(segs) and segs[k] in ("self", "super"):
                k += 1
            ups = segs[:k].count("super")
            full = here[: len(here) - ups] + segs[k:]
        for k in range(len(full), 0, -1):
            if "::".join(full[:k]) in modules:
                return "::".join(full[:k]), "::".join(full[k:]).split("::")[0]
        return "", (full or [""])[0]

    found = defaultdict(list)
    for f, text in texts.items():
        mod = module_of(src, f)
        if any(under(mod, t) for t in tests):
            continue
        here = mod.split("::") if mod else []
        text = cut_tests(text)
        at = "%s:%%d" % os.path.relpath(f, src)
        inline = [(m.end(), item_end(text, m.end() - 1), m.group(1))
                  for m in re.finditer(r"\bmod\s+(\w+)\s*\{", text)]
        aliases = {}
        spans = []
        for start, end, line, p, alias in paths(text, ["crate", "super", "self"]):
            spans.append((start, end))
            inner = [name for a, b, name in inline if a <= start < b]
            t = target(here + inner, re.sub(r"::(self|\*)$", "", p))
            if t[0] == mod:
                continue
            if t[1]:
                found[(mod, t[0], t[1])].append(at % line)
            elif p.endswith("::*"):
                found[(mod, t[0], "*")].append(at % line)
            else:
                aliases[alias or t[0].split("::")[-1]] = (t[0], line)
        for name, (to, line) in aliases.items():
            seen = False
            for m in re.finditer(r"(?<![\w:])%s\s*::\s*(\w+)" % re.escape(name), text):
                if any(a <= m.start() < b for a, b in spans):
                    continue
                t = target(to.split("::"), "self::" + m.group(1))
                found[(mod, t[0], t[1] or "(module)")].append(at % (text.count("\n", 0, m.start()) + 1))
                seen = True
            if not seen:
                found[(mod, to, "(module)")].append(at % line)
        for m in re.finditer(r"(?<![\w:])(\w+)!\s*[(\[{]", text):
            if macros.get(m.group(1), mod) != mod:
                to = macros[m.group(1)]
                found[(mod, to, m.group(1) + "!")].append(at % (text.count("\n", 0, m.start()) + 1))
    return modules - {t for t in modules if any(under(t, x) for x in tests)}, found


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "crates", "vibebms", "src")
    modules, found = uses(src)
    bad = ["%s: no layer (add it to LAYERS in tools/check_layers.py)" % (m or "lib")
           for m in sorted(modules) if layer_of(m) is None]
    if bad:
        print("\n".join(bad))
        return 1
    upward = {k: at for k, at in found.items() if layer_of(k[1]) > layer_of(k[0])}
    for (a, b, i), at in sorted(upward.items()):
        if not any(under(a, x) and b == y and i in items for x, y, items, _ in UPWARD):
            bad.append("%s (%s) uses %s::%s (%s), a higher layer: %s"
                       % (a or "lib", LAYERS[layer_of(a)][0], b, i, LAYERS[layer_of(b)][0], " ".join(at)))
    for a, b, items, _ in UPWARD:
        for i in items:
            if not any(under(x, a) and y == b and j == i for x, y, j in upward):
                bad.append("%s no longer uses %s::%s: take it out of UPWARD in tools/check_layers.py" % (a, b, i))
    for line in bad:
        print(line)
    if bad:
        return 1
    print("layers: %d modules, %d upward uses (at %d places), each listed"
          % (len(modules), len(upward), sum(len(at) for at in upward.values())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
