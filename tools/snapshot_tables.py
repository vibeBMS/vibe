#!/usr/bin/env python3
"""Copy the default difficulty tables next to the web build.

Table servers send no CORS headers, so a page on another site cannot read
them; the web build reads these copies instead (see platform::load_table).

    python3 tools/snapshot_tables.py web    # writes web/tables/

Writes tables/<n>/header.json (its data_url pointing at data.json),
tables/<n>/data.json and tables/index.json (table URL -> header copy).
A table that cannot be fetched is left out.
"""

import json
import re
import sys
import urllib.request
from pathlib import Path
from urllib.parse import urljoin

ROOT = Path(__file__).resolve().parent.parent
LIB = ROOT / "crates/vibebms-tables/src/lib.rs"


def consts(name):
    """The (a, b) string pairs of a Rust `&[(&str, &str)]` constant."""
    src = LIB.read_text(encoding="utf-8")
    body = src[src.index(f"pub const {name}"):]
    body = body[:body.index("];")]
    return re.findall(r'\(\s*"([^"]*)",\s*"([^"]*)",?\s*\)', body)


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "vibeBMS table snapshot"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def text(b):
    b = b.removeprefix(b"\xef\xbb\xbf")
    try:
        return b.decode("utf-8")
    except UnicodeDecodeError:
        return b.decode("shift_jis", errors="replace")


def fetch(url):
    """(header, data) of the table at `url` (its page or header)."""
    body = text(get(url))
    if body.lstrip().startswith("{"):
        header_url = url
    else:
        m = re.search(r'<meta[^>]*name=["\']bmstable["\'][^>]*content=["\']([^"\']+)', body, re.I) \
            or re.search(r'<meta[^>]*content=["\']([^"\']+)["\'][^>]*name=["\']bmstable', body, re.I)
        if not m:
            raise ValueError("no bmstable meta")
        header_url = urljoin(url, m.group(1))
        body = text(get(header_url))
    header = json.loads(body)
    data = json.loads(text(get(urljoin(header_url, header["data_url"]))))
    return header, data


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "web") / "tables"
    out.mkdir(parents=True, exist_ok=True)
    mirrors = dict(consts("MIRRORS"))
    index = {}
    for n, (name, url) in enumerate(consts("DEFAULT_TABLES")):
        for src in [url] + ([mirrors[url]] if url in mirrors else []):
            try:
                header, data = fetch(src)
                break
            except Exception as e:  # noqa: BLE001 - any failure: try the mirror, then skip
                print(f"{name}: {src}: {e}", file=sys.stderr)
        else:
            continue
        header["data_url"] = "data.json"
        d = out / str(n)
        d.mkdir(exist_ok=True)
        (d / "header.json").write_text(json.dumps(header, ensure_ascii=False), encoding="utf-8")
        (d / "data.json").write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
        index[url] = f"tables/{n}/header.json"
        print(f"{name}: {len(data)} charts")
    (out / "index.json").write_text(json.dumps(index, ensure_ascii=False, indent=1), encoding="utf-8")
    return 0 if index else 1


if __name__ == "__main__":
    sys.exit(main())
