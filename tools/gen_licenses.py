#!/usr/bin/env python3
"""THIRD_PARTY_LICENSES.md from cargo-about's JSON (tools/gen_licenses.sh runs it).

  gen_licenses.py <cargo-about json>    the Markdown on stdout

Each licence text once, then the crates under it by name: no versions, so a
version bump leaves the file as it is and only a crate coming or going, or a
licence text changing, changes it. The workspace's own crates (no source) are
left out. The same input gives the same bytes: everything is sorted, and
nothing of the machine (paths, dates) is written.
"""
import json
import sys

HEADER = """\
# Third-party licences

vibeBMS's programs (the desktop app, the Android and iOS apps and the web
build) are built from the Rust crates below, each under its own licence. For
each licence text: the text, then the crates under it. A crate that offers a
choice (`MIT OR Apache-2.0`) is listed under the licence used; its own
expression is beside its name. Code vibeBMS took or derived directly (Lua,
the BeMusicSeeker fork, the fonts, UnRAR) is in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Made by `tools/gen_licenses.sh` with cargo-about, from Cargo.lock and
about.toml: do not edit it (CI checks that it is current).
"""

# what a licence's standard text has where a crate's own file names its holders
# (not the "[yyyy] [name of copyright owner]" of the Apache licence's appendix,
# which shows how to apply it, in every copy of it)
PLACEHOLDERS = ("<copyright holders>", "<year>", "<owner>", "<OWNER>", "<YEAR>")


def norm(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines)


def plural(n, word):
    return f"{n} {word}{'' if n == 1 else 's'}"


def joined(values):
    return ", ".join(sorted(v for v in values if v))


def main(path):
    with open(path, encoding="utf-8") as f:
        about = json.load(f)
    # (id, text, the standard text?) -> name -> its versions' fields
    sections = {}
    for lic in about["licenses"]:
        key = (lic["id"], norm(lic["text"]), lic["source_path"] is None)
        for used in lic["used_by"]:
            c = used["crate"]
            if c["source"] is None:
                continue
            crate = sections.setdefault(key, {}).setdefault(c["name"], {"expr": set(), "repo": set(), "authors": set()})
            crate["expr"].add(c["license"] or "")
            crate["repo"].add(c["repository"] or "")
            crate["authors"].update(c["authors"])
    names_of = {lic["id"]: lic["name"] for lic in about["licenses"]}

    order = sorted(sections, key=lambda k: (k[0], min(sections[k]), k[1]))
    out = [HEADER]
    kinds = {}
    for lid, _, _ in order:
        kinds[lid] = kinds.get(lid, 0) + 1
    crates = {n for k in order for n in sections[k]}
    out.append(f"{plural(len(crates), 'crate')}, {plural(len(order), 'licence text')}:\n")
    for lid in sorted(kinds):
        n = len({c for k in order if k[0] == lid for c in sections[k]})
        out.append(f"- {names_of[lid]} (`{lid}`): {plural(n, 'crate')}, {plural(kinds[lid], 'text')}")
    out.append("")
    for lid, text, generic in order:
        out.append(f"## {names_of[lid]} (`{lid}`)\n")
        placeholder = generic and any(p in text for p in PLACEHOLDERS)
        if generic:
            out.append("These crates ship no file with this licence's text: it is the licence's standard text"
                       + (", and their copyright holders are the authors each names." if placeholder else ".")
                       + "\n")
        if lid == "MPL-2.0":
            out.append("These crates are used unmodified; their source code is on crates.io and in the"
                       " repository listed.\n")
        for name in sorted(sections[(lid, text, generic)]):
            c = sections[(lid, text, generic)][name]
            line = f"- `{name}`" + (f" ({joined(c['expr'])})" if joined(c["expr"]) else "")
            if lid == "MPL-2.0":
                line += f": https://crates.io/crates/{name}"
                if joined(c["repo"]):
                    line += f", {joined(c['repo'])}"
            elif joined(c["repo"]):
                line += f": {joined(c['repo'])}"
            if placeholder:
                line += f"; authors: {joined(c['authors']) or 'none named (see its repository)'}"
            out.append(line)
        out.append("\n````text\n" + text + "\n````\n")
    sys.stdout.write("\n".join(out).rstrip("\n") + "\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: gen_licenses.py <cargo-about json>")
    main(sys.argv[1])
