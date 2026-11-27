#!/usr/bin/env python3
"""tools/check_layers.py [src]: no module of the vibebms crate uses an item
of a higher layer (LAYERS) but the uses listed in UPWARD, and no units
use each other in a cycle but those listed in CYCLES (docs/BUILDING.md,
"Development"; CI's test job runs it).

Every .rs file under src is a module (a/b.rs and a/b/mod.rs are a::b), in
the layer of the longest prefix of its path that LAYERS names. That
prefix is the unit the module is checked as: a top-level module, or a
part of a group that LAYERS names on its own (`net::hosts` in `net/`), so
that putting files in a folder hides none of these checks. What it uses
is read from its text, with comments, strings and test code left out (an
item under `#[cfg(test)]`, a file declared under it): its paths from
`crate::`, `super::` and `self::`, and from a module of its own by that
module's name (`scoredb::read` in `library/mod.rs`), the paths through a
module it imports (`use crate::platform::vfs;` then `vfs::read`), and the
crate's own macros (`tr!`). A path's target is its longest prefix that is a
module, the rest the item; an item a module re-exports from another unit
(`pub use courses::CourseStore;`) is the one of the module it is from,
wherever it is used through. A glob import of a module with parts that are units of
their own (`use crate::library::*;`) fails: what it brings cannot be told
apart. A use of a higher layer fails unless UPWARD lists it; a use
UPWARD lists that no longer exists fails too, so the list is shortened
when one goes. Likewise a cycle: units that use each other, each reached
from the others through uses between units, fail unless CYCLES lists
exactly that set, with the uses that close it: any other use in the
direction of one of those (from its unit to the other's) fails, and so
do uses between the members that make a cycle without them. What a
value's method reaches without naming its type is not seen, so an `impl`
of a type of another unit (not of a trait) fails. A `#[path]` attribute,
a path a macro makes from its argument after `crate::` (`crate::$m::…`),
`use crate as x;` and `extern crate self as x;` fail: what they reach
cannot be read from the text.

It also counts, per unit but platform, app and browser, the lines of
code (not tests) with a platform condition: a `cfg` that names a target
(`target_os`, `target_arch`, any `target_…`, `windows`, `unix`) or a group
build.rs names (`desktop`, ...), as an attribute, `cfg!` or `std::cfg!`.
A count above CONDITIONS fails, and so does one below it, so the number is
lowered when a condition goes. The lines that name a constant standing
for one (STAND_INS: `platform::IS_WEB`, ...) are counted likewise, against
STOOD_IN, so that a condition does not come back as one of them unseen.
"""
import functools
import os
import re
import sys
from collections import defaultdict

sys.dont_write_bytecode = True

# lowest first; a module's layer is that of the longest prefix named here
LAYERS = [
    ("base", ["formats", "i18n"]),
    ("platform", ["platform"]),
    ("library", ["library", "library::sqlite", "library::lr2db", "net::links"]),
    ("placement", ["net::placement", "library::charts"]),
    ("courses and scores", ["library::courses", "library::scoredb"]),
    ("settings", ["config"]),
    ("kept files", ["library::replay", "app::screenshot"]),
    ("paths", ["paths"]),
    ("drawing and input", ["gfx", "input", "perspective"]),
    ("looks and touch", ["gfx::theme", "gfx::video", "touch"]),
    ("skins", ["skin"]),
    ("sound", ["audio", "audio::loudness", "audio::sysound"]),
    ("services", ["net", "net::hosts", "profiles", "update", "ir", "integrations", "net::import", "net::events",
                  "net::songfetch", "net::bulk", "net::skinstore", "app::web"]),
    ("shared state", ["ctx"]),
    ("widgets", ["ui"]),
    ("browser", ["browser"]),
    ("scenes", ["scene"]),
    ("shell", ["app", ""]),  # "": lib.rs, main.rs
]

# (from module or the modules under it, to module, items, why it stays)
UPWARD = [
    ("i18n", "platform::android", ["locale"],
     "AUTO's language on Android is the Java locale; passing the system's locale in from the start instead "
     "would read it at every start, not only when AUTO is chosen"),
]

# (units that use each other, the uses that close the cycle as
# (from module or the modules under it, to module, items), why they stay)
CYCLES = [
    (["i18n", "platform"], [("i18n", "platform::android", ["locale"])], "the use listed in UPWARD"),
]


# The units where platform conditions belong: the platform
# layer, the window and its loop, the in-app browser.
CONDITIONS_AT_HOME = ["platform", "app", "browser"]

# Lines of the other units ("lib": lib.rs, main.rs) with a
# platform condition (conditions()), outside test code: a count may only go
# down.
CONDITIONS = {
    "app::screenshot": 2, "audio::loudness": 1, "config": 1, "ctx": 11, "gfx": 12, "gfx::video": 5, "i18n": 3,
    "input": 13, "integrations": 28, "ir": 5, "lib": 1, "library": 12, "library::replay": 2, "net": 1,
    "net::links": 5, "paths": 2, "scene": 40, "skin": 2, "update": 12,
}

# What names a platform in a cfg (as patterns): the target's predicates, and
# the groups build.rs names (read from it).
TARGET_CFGS = [r"target_\w+", "windows", "unix"]

# The constants that stand for a platform condition (their value is one).
STAND_INS = ["IS_WEB", "IN_APP_BROWSER"]

# Lines of the other units, outside test code, that name one of STAND_INS
# (stand_ins()): a count may only go down.
STOOD_IN = {
    "ctx": 1, "gfx": 1, "ir": 1, "library::replay": 1, "net::hosts": 1, "net::skinstore": 1, "paths": 1,
    "profiles": 1, "scene": 7,
}


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
        elif c in "br" and (m := re.compile(r'b?r(#*)"').match(text, i)) and not re.match(r"\w", text[i - 1 : i]):
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
    """Where the item starting at `i` (its attributes first) ends: after its
    `;` or `,` (a field, a variant, a match arm) but one in `<…>`, before a
    `}` that closes what holds it, or after its `{ … }`."""
    depth = angle = 0
    while i < len(text):
        c = text[i]
        # (`<<` shifts, `->` returns, `=>` ends a match arm's pattern)
        if depth == 0 and text.startswith("<<", i):
            i += 2
            continue
        if depth == 0 and c == "<":
            angle += 1
        elif depth == 0 and c == ">" and text[i - 1 : i] not in ("-", "="):
            angle = max(angle - 1, 0)
        elif depth == 0 and text.startswith("=>", i):
            angle = 0
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return i
            depth -= 1
            if depth == 0 and c == "}":
                return i + 1
        elif (c == ";" or c == "," and angle == 0) and depth == 0:
            return i + 1
        i += 1
    return i


def cut_tests(text):
    """`text` with each item under `#[cfg(test)]` (or `all(test, …)`) as
    spaces, newlines kept."""
    for m in reversed(list(re.finditer(r"#\[cfg\((all\()?test\b", text))):
        j = item_end(text, m.start())
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
        elif m2 := re.compile(r"\s+as\s+\w+").match(text, i):
            i = m2.end()
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


def unit_of(module):
    """The unit `module` is checked as: the name LAYERS gives its layer by
    (layer_of()), else its top-level module."""
    best = None
    for _, mods in LAYERS:
        for m in mods:
            if module == m or (m and module.startswith(m + "::")) or (not m and not module):
                if best is None or len(m) > len(best):
                    best = m
    return module.split("::")[0] if best is None else best


def under(module, prefix):
    return module == prefix or module.startswith(prefix + "::")


@functools.lru_cache(maxsize=None)
def read(src):
    """The files under `src`, each as its text with comments and strings
    blanked ({file: text}), and the modules declared under #[cfg(test)]."""
    files = sorted(os.path.join(r, n) for r, _, ns in os.walk(src) for n in ns if n.endswith(".rs"))
    texts = {f: blank(open(f, encoding="utf-8").read()) for f in files}
    tests = set()
    for f, text in texts.items():
        mod = module_of(src, f)
        for m in re.finditer(r"#\[cfg\(test\)\]\s*(pub(\([\w:]+\))?\s+)?mod\s+(\w+)\s*;", text):
            tests.add("::".join(filter(None, [mod, m.group(3)])))
    return texts, tests


def uses(src):
    """(from module, to module, item) -> ["file:line", ...]"""
    texts, tests = read(src)
    modules = {module_of(src, f) for f in texts}
    macros = {}
    for f, text in texts.items():
        mod = module_of(src, f)
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

    def parts(m):
        """The modules under `m` that are units of their own."""
        return [x for x in modules if x != m and under(x, m) and unit_of(x) != unit_of(m)
                and not any(under(x, t) for t in tests)]

    found = defaultdict(list)
    odd = []
    reexports = {}  # (module, name) -> (module, item) it is
    impls = []  # (module, (module, item) its type is, place)
    for f, text in texts.items():
        mod = module_of(src, f)
        if any(under(mod, t) for t in tests):
            continue
        here = mod.split("::") if mod else []
        # (its own modules by their names; but the crate root's: the
        # highest layer)
        children = sorted({m.split("::")[-1] for m in modules if mod and m.startswith(mod + "::")
                           and "::" not in m[len(mod) + 2:] and not any(under(m, t) for t in tests)})
        at = "%s:%%d" % os.path.relpath(f, src)
        # (a module's layer is read from where its file is)
        for m in re.finditer(r"#\s*\[\s*path\b", text):
            line = text.count("\n", 0, m.start()) + 1
            odd.append(at % line + ": a #[path] attribute: keep the file where its `mod` line puts it")
        text = cut_tests(text)
        # (a path a macro makes from what it is given is not read)
        for m in re.finditer(r"(?<!\w)crate\s*::\s*\$", text):
            line = text.count("\n", 0, m.start()) + 1
            odd.append(at % line + ": a path made from a macro's argument after `crate::`: name its module")
        # (the crate under another name: its paths are not read)
        for m in re.finditer(r"(?<!\w)(use\s+crate|extern\s+crate\s+self)\s+as\b", text):
            line = text.count("\n", 0, m.start()) + 1
            odd.append(at % line + ": the crate under another name (`%s as`): name it `crate`"
                       % re.sub(r"\s+", " ", m.group(1)))
        inline = [(m.end(), item_end(text, m.end() - 1), m.group(1))
                  for m in re.finditer(r"\bmod\s+(\w+)\s*\{", text)]
        aliases = {}
        named = {}  # item imported (or named by its path) -> its module
        spans = []
        for start, end, line, p, alias in paths(text, ["crate", "super", "self"] + children):
            inner = [name for a, b, name in inline if a <= start < b]
            if p.split("::")[0] in children:
                if inner:
                    continue
                p = "self::" + p
            spans.append((start, end))
            t = target(here + inner, re.sub(r"::(self|\*)$", "", p))
            if p.endswith("::*") and parts(t[0]):
                odd.append(at % line + ": a glob import of %s, whose parts %s are units of their own: name what "
                           "is used" % (t[0] or "lib", ", ".join(sorted(parts(t[0])))))
            if t[1] and re.search(r"\bpub\s*(\([^)]*\))?\s*use\s+$", text[max(start - 80, 0) : start]):
                reexports[(mod, alias or p.split("::")[-1])] = t
            if t[0] == mod:
                continue
            if t[1]:
                found[(mod, t[0], t[1])].append(at % line)
                if p.split("::")[-1] == t[1]:
                    named[alias or t[1]] = t
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
        # (methods here of another unit's type reach this module
        # from wherever the type is used, unseen)
        inherent = (r"(?<!\w)impl\s*(?:<[^{;]*?>)?\s*([\w:]+?)\s*(?:<(?:(?!\bfor\b)[^{;])*>)?\s*"
                    r"(?:where\b[^{;]*)?\{")
        for m in re.finditer(inherent, text):
            name = re.sub(r"\s+", "", m.group(1))
            first, _, rest = name.partition("::")
            if first in ("crate", "super", "self"):
                to = target(here, name)
            elif rest and first in children:
                to = target(here, "self::" + name)
            elif rest and first in aliases:
                to = target(aliases[first][0].split("::"), "self::" + rest)
            else:
                to = named.get(name)
            if to is not None:
                impls.append((mod, to, name, at % (text.count("\n", 0, m.start()) + 1)))
        for m in re.finditer(r"(?<![\w:])(\w+)!\s*[(\[{]", text):
            if macros.get(m.group(1), mod) != mod:
                to = macros[m.group(1)]
                found[(mod, to, m.group(1) + "!")].append(at % (text.count("\n", 0, m.start()) + 1))

    def defined(t):
        """Where the item of `t` (module, item) is from: through what
        re-exports it from another unit."""
        for _ in range(len(reexports) + 1):
            if t not in reexports or unit_of(reexports[t][0]) == unit_of(t[0]):
                break
            t = reexports[t]
        return t

    resolved = defaultdict(list)
    for (a, b, i), at in found.items():
        b, i = defined((b, i))
        if b != a:
            resolved[(a, b, i)] += at
    for mod, to, name, at in impls:
        to = defined(to)[0]
        if unit_of(to) != unit_of(mod):
            odd.append(at + ": methods of %s's %s: write them in its module, or as functions here"
                       % (to or "lib", name.split("::")[-1]))
    return modules - {t for t in modules if any(under(t, x) for x in tests)}, resolved, odd


def groups(build_rs):
    """The platform groups `build_rs` names (`("desktop", desktop)`, ...)."""
    with open(build_rs, encoding="utf-8") as f:
        text = re.sub(r"//[^\n]*", "", f.read())
    m = re.search(r"let\s+groups\s*=\s*\[(.*?)\];", text, re.S)
    return re.findall(r'\(\s*"(\w+)"\s*,', m.group(1)) if m else []


def conditions(src, names):
    """The lines with a platform condition (a `cfg(…)`, `cfg_attr(…, …)` or
    `cfg!(…)`, `std::cfg!(…)` too, whose condition names one of `names`,
    patterns) outside test code, by unit: {unit: ["file:line", ...]} (an
    attribute or a `cfg!` over several lines is the line it starts on)."""
    texts, tests = read(src)
    named = re.compile(r"(?<![\w:])(%s)(?!\w)" % "|".join(names))
    out = defaultdict(list)
    for f, text in texts.items():
        mod = module_of(src, f)
        if any(under(mod, t) for t in tests):
            continue
        text = cut_tests(text)
        lines = set()
        for m in re.finditer(r"(?<![\w:])(?:\w+\s*::\s*)*cfg(_attr)?\s*(!\s*)?\(", text):
            # (a plain `cfg(…)`: an attribute's, or in another's condition)
            before = text[max(m.start() - 64, 0) : m.start()]
            if not m.group(1) and not m.group(2) and not re.search(r"#!?\s*\[\s*$|[(,]\s*$", before):
                continue
            i, depth, parts, cur = m.end(), 1, [], m.end()
            while i < len(text) and depth:
                c = text[i]
                depth += {"(": 1, ")": -1, "[": 1, "]": -1}.get(c, 0)
                if c == "," and depth == 1:
                    parts.append(text[cur:i])
                    cur = i + 1
                i += 1
            parts.append(text[cur : i - 1])
            # (cfg_attr: its condition, not the attributes it adds)
            if named.search(parts[0] if m.group(1) else ",".join(parts)):
                lines.add(text.count("\n", 0, m.start()) + 1)
        top = unit_of(mod) or "lib"
        if lines and top not in CONDITIONS_AT_HOME:
            out[top] += ["%s:%d" % (os.path.relpath(f, src), n) for n in sorted(lines)]
    return out


def stand_ins(src):
    """The lines outside test code that name one of STAND_INS (but in a
    `use`, and where it is defined), by unit as conditions() counts."""
    texts, tests = read(src)
    named = re.compile(r"(?<!\w)(?<!const )(%s)(?!\w)" % "|".join(STAND_INS))
    out = defaultdict(list)
    for f, text in texts.items():
        mod = module_of(src, f)
        top = unit_of(mod) or "lib"
        if any(under(mod, t) for t in tests) or top in CONDITIONS_AT_HOME:
            continue
        text = re.sub(r"\buse\s[^;]*;", lambda m: re.sub(r"[^\n]", " ", m.group(0)), cut_tests(text))
        lines = {text.count("\n", 0, m.start()) + 1 for m in named.finditer(text)}
        out[top] += ["%s:%d" % (os.path.relpath(f, src), n) for n in sorted(lines)]
    return {k: v for k, v in out.items() if v}


def condition_problems(found, listed, what="lines with a platform condition", table="CONDITIONS"):
    """What is wrong with the `what` of `found` (conditions(), stand_ins())
    against `listed` (`table`)."""
    bad = []
    for mod in sorted(set(found) | set(listed)):
        n, most = len(found.get(mod, [])), listed.get(mod, 0)
        if n > most:
            bad.append("%s: %d %s, more than the %d %s allows in tools/check_layers.py: put the difference "
                       "behind platform/ (%s)" % (mod, n, what, most, table, " ".join(found[mod])))
        elif n < most:
            bad.append("%s: %d %s, fewer than the %d %s allows: lower it in tools/check_layers.py"
                       % (mod, n, what, most, table))
    return bad


def cycles(found):
    """The sets of units that use each other (strongly connected, more
    than one), each as a sorted tuple, with the uses between their
    members: {(unit, ...): ["a -> b::item at file:line", ...]}."""
    top = unit_of
    edges = defaultdict(dict)
    for (a, b, i), at in sorted(found.items()):
        if top(a) != top(b):
            edges[top(a)].setdefault(top(b), "%s::%s at %s" % (b, i, at[0]))
    nodes = sorted(set(edges) | {b for e in edges.values() for b in e})
    # (Tarjan's: a component is complete when its first node's low is its own)
    index, low, stack, on, out = {}, {}, [], set(), {}

    def visit(v):
        index[v] = low[v] = len(index)
        stack.append(v)
        on.add(v)
        for w in sorted(edges[v]):
            if w not in index:
                visit(w)
                low[v] = min(low[v], low[w])
            elif w in on:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while not comp or comp[-1] != v:
                comp.append(stack.pop())
                on.discard(comp[-1])
            if len(comp) > 1:
                c = tuple(sorted(comp))
                out[c] = ["%s -> %s" % (a or "lib", u) for a in c for b, u in sorted(edges[a].items()) if b in c]

    for v in nodes:
        if v not in index:
            visit(v)
    return out


def cycle_problems(found, listed):
    """What is wrong with the cycles of `found` against `listed` (CYCLES)."""
    top = unit_of
    have = cycles(found)
    want = {tuple(sorted(c)) for c, _, _ in listed}
    bad = ["a cycle between units: %s (%s)" % (", ".join(m or "lib" for m in c), "; ".join(at))
           for c, at in sorted(have.items()) if c not in want]
    for c in sorted(want):
        if c in have:
            continue
        grown = [h for h in have if set(c) < set(h)]
        if grown:
            bad += ["the cycle %s has grown: %s joined it: change CYCLES in tools/check_layers.py"
                    % (", ".join(c), ", ".join(m or "lib" for m in h if m not in c)) for h in grown]
        else:
            bad.append("%s no longer use each other in a cycle: take it out of CYCLES in tools/check_layers.py"
                       % ", ".join(c))
    for members, closing, _ in listed:
        back = {(top(x), top(y)) for x, y, _ in closing}
        between = {k: at for k, at in found.items()
                   if top(k[0]) != top(k[1]) and top(k[0]) in members and top(k[1]) in members}
        for (a, b, i), at in sorted(between.items()):
            if (top(a), top(b)) in back and not any(under(a, x) and b == y and i in items
                                                    for x, y, items in closing):
                bad.append("%s uses %s::%s (%s), a use that closes the cycle %s but not one CYCLES lists for it"
                           % (a, b, i, " ".join(at), ", ".join(sorted(members))))
        for x, y, items in closing:
            for i in items:
                if not any(under(a, x) and b == y and j == i for a, b, j in found):
                    bad.append("%s no longer uses %s::%s: take it out of CYCLES in tools/check_layers.py" % (x, y, i))
        rest = cycles({k: at for k, at in between.items() if (top(k[0]), top(k[1])) not in back})
        bad += ["%s use each other without the uses CYCLES lists for them (%s)" % (", ".join(c), "; ".join(at))
                for c, at in sorted(rest.items())]
    return bad, len(have)


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "crates", "vibebms", "src")
    modules, found, odd = uses(src)
    bad = odd + ["%s: no layer (add it to LAYERS in tools/check_layers.py)" % (m or "lib")
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
    cycle_bad, n_cycles = cycle_problems(found, CYCLES)
    bad += cycle_bad
    build_rs = os.path.join(root, "crates", "vibebms", "build.rs")
    named = groups(build_rs)
    if not named:
        bad.append("%s: its platform groups (`let groups = [...]`) not found: change groups() in "
                   "tools/check_layers.py" % build_rs)
    conds = conditions(src, TARGET_CFGS + [re.escape(g) for g in named])
    bad += condition_problems(conds, CONDITIONS)
    stood = stand_ins(src)
    bad += condition_problems(stood, STOOD_IN, "lines with %s" % " or ".join(STAND_INS), "STOOD_IN")
    for line in bad:
        print(line)
    if bad:
        return 1
    print("layers: %d modules, %d upward uses (at %d places), %d cycles between units, each listed; "
          "%d lines with a platform condition outside %s, %d with a constant that stands for one"
          % (len(modules), len(upward), sum(len(at) for at in upward.values()), n_cycles,
             sum(len(at) for at in conds.values()), ", ".join(CONDITIONS_AT_HOME),
             sum(len(at) for at in stood.values())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
