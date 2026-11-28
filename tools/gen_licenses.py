#!/usr/bin/env python3
"""THIRD_PARTY_LICENSES.md from cargo-about's JSON (tools/gen_licenses.sh runs it).

  gen_licenses.py <cargo-about json>    the Markdown on stdout

Each licence text once, then the crates under it by name, each with the
copyright notices its own licence file has: no versions, so a version bump
leaves the file as it is and only a crate coming or going, a licence text or
a notice changing, changes it. The workspace's own crates (no source) are
left out. The same input gives the same bytes: everything is sorted, and
nothing of the machine (paths, dates) is written.

Two files of one licence are one text when they are equal once their notices
are taken out (merge_key says what else is ignored): exact, no similarity
measure, so a file with other words (a clause, an exception, a note) keeps a
text of its own. The text printed for several is the licence's standard text
when one of the crates has no file (cargo-about uses it for those), else the
form most of their files have (the first in sorted order on a tie), without
its notices.

What cargo-about cannot read is in about.toml (clarify: the crate's own
licence file where cargo-about does not recognise it) and in
licence-notices.toml (files: a crate's notices that are no licence text,
printed under it as the crate has them; upstream: the licence file of a crate
that ships none, from its repository). Every file named there is checked
against its checksum, and a crate that uses a licence's standard text while
it has a licence file of its own stops it: a person reads the file and says
what it is in one of the two. (A crate with no licence file at all whose
standard text is another crate's file is listed as shipping none.)
"""
import hashlib
import json
import os
import re
import sys
import tomllib

HEADER = """\
# Third-party licences

vibeBMS's programs (the desktop app, the Android and iOS apps and the web
build) are built from the Rust crates below, each under its own licence. Each
licence text is here once, then the crates under it, each with the copyright
notices its own licence file has (files that differ only in white space,
letter case, a title line, `http`/`https` or the Apache licence's appendix are
one text). A crate that offers a choice (`MIT OR Apache-2.0`) is listed under
the licence used; its own expression is beside its name. Each crate ships the
text it is under, with the copyright notices listed under it (none: its file
has none) or, for a text printed as the files have it, in it, unless its line
says that it ships no file with that text. A crate's notice files that are
not a licence text (a `COPYRIGHT`, a third party's notice) are under it as the
crate has them. Code vibeBMS took or derived directly (Lua, the BeMusicSeeker
fork, the fonts, UnRAR) is in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Made by `tools/gen_licenses.sh` with cargo-about, from Cargo.lock and
about.toml: do not edit it (CI checks that it is current).
"""

# what a licence's standard text has where a crate's own file names its holders
# (not the "[yyyy] [name of copyright owner]" of the Apache licence's appendix,
# which shows how to apply it, in every copy of it)
PLACEHOLDERS = ("<copyright holders>", "<year>", "<owner>", "<OWNER>", "<YEAR>")
# a template's line, not a notice: dropped from the comparison, and kept where
# the printed text has it
TEMPLATE = re.compile(r"[\[{<](yyyy|year|name of copyright owner|copyright holders)[\]}>]", re.I)
# a notice: a line that begins (after a comment marker or bullet) with one of
# these, and then not with the words of a licence's own sentence that a line
# break put there ("copyright holders be liable", "(c) You must retain")
NOTICE = re.compile(r"\s*(?:(?://|#+|\*|;|--|-|•)\s*)?(portions\s+copyright\b|copyright\b|\(c\)|©)(.*)", re.I)
PROSE = re.compile(r"\W*(holders?|and|notice|licen[cs]e|owner|statement)\b", re.I)

# kept as the files have them, the notices in them: texts with more than the
# licence (ring's two, unrar-rs's UnRAR condition) and licences whose text is
# its own notice
WHOLE_CRATES = {"ring", "unrar-rs"}
WHOLE_KINDS = {"MPL-2.0", "Unicode-3.0", "CDLA-Permissive-2.0", "CC0-1.0", "bzip2-1.0.6"}
# a first line that only names the licence
TITLES = {"MIT": r"(the )?mit licen[cs]e( \(mit\))?", "Zlib": r"(the )?zlib licen[cs]e"}
APACHE_URL = re.compile(r"http://(www\.)?apache\.org/licenses/(license-2\.0(\.txt|\.html)?)?")
# the Apache licence after its terms, as merge_key leaves it
APACHE_APPENDIX = (
    'appendix: how to apply the apache license to your work. to apply the apache license to your work, attach the'
    ' following boilerplate notice, with the fields enclosed by brackets "[]" replaced with your own identifying'
    " information. (don't include the brackets!) the text should be enclosed in the appropriate comment syntax for"
    " the file format. we also recommend that a file or class name and description of purpose be included on the"
    ' same "printed page" as the copyright notice for easier identification within third-party archives.')
APACHE_BOILERPLATE = (
    'licensed under the apache license, version 2.0 (the "license"); you may not use this file except in compliance'
    " with the license. you may obtain a copy of the license at http://www.apache.org/licenses/ unless required by"
    ' applicable law or agreed to in writing, software distributed under the license is distributed on an "as is"'
    " basis, without warranties or conditions of any kind, either express or implied. see the license for the"
    " specific language governing permissions and limitations under the license.")
APACHE_END = "end of terms and conditions"


def norm(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def is_notice(line):
    m = NOTICE.match(line)
    if not m or TEMPLATE.search(line):
        return False
    if m.group(1).lower() in ("(c)", "©"):
        return re.match(r"\s*(\d|copyright\b)", m.group(2), re.I) is not None
    return not PROSE.match(m.group(2))


def split(text):
    """the text's lines without its notices, and the notices (a notice goes on
    over the lines below it indented deeper, e.g. a list of holders)"""
    lines = norm(text).split("\n")
    body, notices, i = [], [], 0
    while i < len(lines):
        if not is_notice(lines[i]):
            body.append(lines[i])
            i += 1
            continue
        indent = len(lines[i]) - len(lines[i].lstrip())
        notice = [lines[i].strip()]
        i += 1
        while (i < len(lines) and lines[i].strip() and len(lines[i]) - len(lines[i].lstrip()) > indent
               and not is_notice(lines[i])):
            notice.append(lines[i].strip())
            i += 1
        notices.append("\n".join(notice))
    return body, notices


def tidy(lines):
    """the lines as a text, with no run of blank lines where a notice was"""
    out = []
    for line in lines:
        if line or (out and out[-1]):
            out.append(line)
    return norm("\n".join(out))


def merge_key(lid, body):
    """what two files of a licence must have equal to be one text: the words,
    without the template lines, white space, letter case, the first line if it
    only names the licence, http/https, and the Apache licence's URL form and
    standard appendix (which a crate's file may have or not)"""
    lines = [line for line in body if line.strip() and not TEMPLATE.search(line)]
    if lines and lid in TITLES and re.fullmatch(TITLES[lid], " ".join(lines[0].split()).casefold()):
        lines.pop(0)
    t = re.sub(r"https://", "http://", " ".join(" ".join(lines).split()).casefold())
    if lid.startswith("Apache-2.0"):
        t = APACHE_URL.sub("http://www.apache.org/licenses/", t)
        end = t.find(APACHE_END)
        if end >= 0:
            end += len(APACHE_END)
            tail = t[end:].replace('brackets "{}"', 'brackets "[]"').strip()
            tail = tail.removeprefix(APACHE_APPENDIX).strip().removeprefix(APACHE_BOILERPLATE).strip()
            if not tail:
                t = t[:end]
    return t


def plural(n, word):
    return f"{n} {word}{'' if n == 1 else 's'}"


def joined(values):
    return ", ".join(sorted(v for v in values if v))


ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# a file at a crate's root that is a licence text: one cargo-about used, or one
# about.toml says what it is
LICENCE_FILE = re.compile(r"(LICEN[CS]E|COPYING|UNLICEN[CS]E)([._-].*)?", re.I)


def licence_files(d):
    """the licence files in a crate's folder and the folders in it"""
    found = []
    for name in sorted(os.listdir(d)):
        p = os.path.join(d, name)
        if os.path.isdir(p):
            found += [f"{name}/{f}" for f in sorted(os.listdir(p))
                      if LICENCE_FILE.fullmatch(f) and os.path.isfile(os.path.join(p, f))]
        elif LICENCE_FILE.fullmatch(name):
            found.append(name)
    return found


def excerpt(d, entry, who):
    """an entry's file (path, from start to end if given, as cargo-about's
    clarify reads them), checked against its checksum"""
    p = os.path.join(d, entry["path"])
    if not os.path.isfile(p):
        sys.exit(f"licence-notices.toml: {who} has no {entry['path']}: read what it has now and say so there")
    with open(p, encoding="utf-8", newline="") as f:
        text = f.read()
    a = text.find(entry["start"]) if "start" in entry else 0
    b = text.find(entry["end"], max(a, 0)) + len(entry["end"]) if "end" in entry else len(text)
    if a < 0 or b < len(entry.get("end", "")):
        sys.exit(f"licence-notices.toml: {who}'s {entry['path']} has no longer the part named: read it")
    text = text[a:b]
    if hashlib.sha256(text.encode()).hexdigest() != entry["checksum"]:
        sys.exit(f"licence-notices.toml: {who}'s {entry['path']} changed: read it, then update its checksum")
    return norm(text)


def main(path):
    with open(path, encoding="utf-8") as f:
        about = json.load(f)
    with open(os.path.join(ROOT, "about.toml"), "rb") as f:
        clarified = {k: v["clarify"] for k, v in tomllib.load(f).items() if isinstance(v, dict) and "clarify" in v}
    with open(os.path.join(ROOT, "licence-notices.toml"), "rb") as f:
        extra = tomllib.load(f)
    names_of = {lic["id"]: lic["name"] for lic in about["licenses"]}
    # (name, version) -> {"dir", "expr", "standard": licences of standard text, "file": licences from a file}
    shipped = {}
    # key -> {"forms": printed text -> (crates, the standard text?), "crates": name -> its fields}
    groups = {}
    # sorted, so that a crate's notices from several versions' files come in the same order every time
    for lic in sorted(about["licenses"], key=lambda lic: (lic["id"], norm(lic["text"]))):
        users = [u["crate"] for u in lic["used_by"] if u["crate"]["source"] is not None]
        if not users:
            continue
        generic = lic["source_path"] is None
        if lic["id"] in WHOLE_KINDS or any(c["name"] in WHOLE_CRATES for c in users):
            key = (lic["id"], "whole", norm(lic["text"]), generic)
            text, notices = norm(lic["text"]), []
        else:
            body, notices = split(lic["text"])
            key = (lic["id"], "merged", merge_key(lic["id"], body))
            text = tidy(body)
        g = groups.setdefault(key, {"forms": {}, "crates": {}})
        form = g["forms"].setdefault(text, [set(), False])
        form[1] |= generic
        for c in users:
            v = shipped.setdefault((c["name"], c["version"]), {"dir": os.path.dirname(c["manifest_path"]),
                                                               "expr": c["license"], "standard": set(),
                                                               "file": set()})
            v["standard" if generic else "file"].add(lic["id"])
            form[0].add(c["name"])
            crate = g["crates"].setdefault(c["name"], {"expr": set(), "repo": set(), "authors": set(),
                                                       "notices": [], "no file": False, "extra": [],
                                                       "upstream": []})
            crate["expr"].add(c["license"] or "")
            crate["repo"].add(c["repository"] or "")
            crate["authors"].update(c["authors"])
            crate["notices"] += [n for n in notices if n not in crate["notices"]]
            crate["no file"] |= generic

    names = {n for n, _ in shipped}
    for name, c in sorted(clarified.items()):
        if name not in names:
            sys.exit(f"about.toml clarifies {name}, which no release has now: take it out")
        exprs = {v["expr"] for (n, _), v in shipped.items() if n == name} - {None}
        if exprs - {c["license"]}:
            sys.exit(f"about.toml says {name} is {c['license']}, its Cargo.toml {', '.join(sorted(exprs))}: read why")
    for (name, version), v in sorted(shipped.items()):
        if name in clarified:
            continue
        files = licence_files(v["dir"])
        if v["standard"] and [f for f in files if "/" not in f]:
            sys.exit(f"{name} {version} has {', '.join(f for f in files if '/' not in f)}, which cargo-about did not"
                     f" take for its {', '.join(sorted(v['standard']))} text: read them and clarify {name} in"
                     " about.toml")
        if v["file"] and not files:
            # the standard text, equal to another crate's file, whose entry it is in
            for k, g in groups.items():
                if k[0] in v["file"] and name in g["crates"]:
                    g["crates"][name]["no file"] = True

    def under(name, lid, what):
        """the crate's entries in the groups of that licence"""
        found = [g["crates"][name] for k, g in groups.items() if k[0] == lid and name in g["crates"]]
        if not found:
            sys.exit(f"licence-notices.toml: {what} is for {name} under {lid}, which it is not now")
        return found

    for name, entries in sorted(extra.get("files", {}).items()):
        dirs = sorted({v["dir"] for (n, _), v in shipped.items() if n == name})
        if not dirs:
            sys.exit(f"licence-notices.toml names {name}, which no release has now: take it out")
        for e in entries:
            texts = {excerpt(d, e, name) for d in dirs}
            label = ("from its" if "start" in e or "end" in e else "its") + f" `{e['path']}`"
            for crate in under(name, e["license"], e["path"]):
                crate["extra"] += [(label, t) for t in sorted(texts) if (label, t) not in crate["extra"]]
    for e in extra.get("upstream", []):
        if hashlib.sha256(e["text"].encode()).hexdigest() != e["checksum"]:
            sys.exit("licence-notices.toml: an upstream text is not the file its checksum is of")
        for at in sorted(e["crates"]):
            name, version = at.split("@")
            if (name, version) not in shipped:
                sys.exit(f"licence-notices.toml: {name} is not at {version} now: read the licence file in its"
                         " repository at the new version's commit and update the entry")
            if e["license"] not in shipped[(name, version)]["standard"]:
                sys.exit(f"licence-notices.toml: {name} {version} has its own {e['license']} file now: take its"
                         " upstream entry out")
            for crate in under(name, e["license"], "an upstream text"):
                crate["upstream"] += [n for n in split(e["text"])[1] if n not in crate["upstream"]]

    def printed(g):
        forms = g["forms"]
        return min(forms, key=lambda t: (not forms[t][1], -len(forms[t][0]), t))

    order = sorted(groups, key=lambda k: (k[0], min(groups[k]["crates"]), printed(groups[k])))
    out = [HEADER]
    kinds = {}
    for k in order:
        kinds[k[0]] = kinds.get(k[0], 0) + 1
    crates = {n for k in order for n in groups[k]["crates"]}
    out.append(f"{plural(len(crates), 'crate')}, {plural(len(order), 'licence text')} (each once, then the crates"
               " under it):\n")
    for lid in sorted(kinds):
        n = len({c for k in order if k[0] == lid for c in groups[k]["crates"]})
        out.append(f"- {names_of[lid]} (`{lid}`): {plural(n, 'crate')}, {plural(kinds[lid], 'text')}")
    out.append("")
    for k in order:
        lid, g = k[0], groups[k]
        text = printed(g)
        whole_generic = k[1] == "whole" and k[3]
        placeholder = any(p in text for p in PLACEHOLDERS)
        out.append(f"## {names_of[lid]} (`{lid}`)\n")
        if whole_generic:
            out.append("These crates ship no file with this licence's text: it is the licence's standard text"
                       + (", and their copyright holders are the authors each names." if placeholder else ".")
                       + "\n")
        if lid == "MPL-2.0":
            out.append("These crates are used unmodified; their source code is on crates.io and in the"
                       " repository listed.\n")
        out.append("````text\n" + text + "\n````\n")
        for name in sorted(g["crates"]):
            c = g["crates"][name]
            line = f"- `{name}`" + (f" ({joined(c['expr'])})" if joined(c["expr"]) else "")
            if lid == "MPL-2.0":
                line += f": https://crates.io/crates/{name}"
                if joined(c["repo"]):
                    line += f", {joined(c['repo'])}"
            elif joined(c["repo"]):
                line += f": {joined(c['repo'])}"
            if c["no file"] and not whole_generic:
                line += "; ships no file with this licence's text"
            if c["no file"] and placeholder:
                unnamed = "none named" + ("" if c["upstream"] else " (see its repository)")
                line += f"; authors: {joined(c['authors']) or unnamed}"
            if c["upstream"]:
                line += "; the licence file in its repository (not in the crate) has:"
            out.append(line)
            out.extend("  - " + n.replace("\n", "\n    ") for n in c["notices"] + c["upstream"])
            for label, t in c["extra"]:
                if "\n" not in t:
                    out.append(f"  - {t}")
                else:
                    body = "\n".join(("    " + x).rstrip() for x in t.split("\n"))
                    out.append(f"  - {label}:\n\n    ````text\n{body}\n    ````\n")
        out.append("")
    sys.stdout.write("\n".join(out).rstrip("\n") + "\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: gen_licenses.py <cargo-about json>")
    main(sys.argv[1])
