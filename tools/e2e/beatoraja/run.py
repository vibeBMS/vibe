#!/usr/bin/env python3
"""Plays the cases of gen.py in beatoraja, a black box: each case's key log
becomes a replay in beatoraja's own format, which beatoraja plays with
`-r1 <chart>` (its README); the result is read from what it writes.

    python3 run.py --dist <beatoraja folder> --work <dir> --cases <dir>
                   --out <results.json> [--speed 1|3] [--only <name>...]

What was learnt by looking at the files beatoraja 0.8.9 writes (no source
read):
- A replay (player/<id>/replay/<sha256>.brd for slot 1, `_1` .. `_3` the
  others; a chart with long notes played as CN or HCN: `C` or `H` before
  the hash) is gzipped JSON; its "keyinput" is URL-safe base64 of gzipped
  records of 9 bytes: an int8 key code (+n press, -n release; 7 keys: 1..7
  the keys, 8 and 9 the scratch's two keys) and an int64 little-endian
  time in µs of chart time. "gauge" is the gauge (0 ASSIST EASY .. 5
  HAZARD, as the launcher's list), "randomoptionseed" the lane option seed.
- A replay's play is not stored in the score database ("score not saved:
  REPLAY mode", its log says), but its result is logged: beatoraja_log.xml
  has a record whose message starts `"score": {...}` with the counts by
  judgement, early (E) and late (L), the max combo, the lamp ("Clear", the
  ids vibeBMS imports) and the EX score.
- The gauge at the end is not in that record: a result skin of our own
  (skin/e2e/result.luaskin, written here) prints it from its Lua, where
  beatoraja's skins read it (`main_state.float_number(1107)`).
- Holding the 4 key while a replay plays runs it 3 times as fast (the
  hotkeys in manual/hotkeys.txt); the judgements are the same (checked with
  --speed 1).

The display: an X server with a mode of an exact name and refresh rate
(xorg.conf here, the dummy driver) and xrandr installed; LWJGL 2 reads the
modes from xrandr's output and stops when it finds none it can read
(Xvfb's 0 Hz mode is one).
"""

import argparse
import base64
import fnmatch
import gzip
import hashlib
import json
import os
import re
import struct
import subprocess
import sys
import time
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))

# result-screen numbers the probe prints (beatoraja's skin number ids):
# the EX score, notes, max combo, the lamp, and each judgement early / late
NUMBERS = {"Exscore": 71, "Notes": 74, "Combo": 75, "Clear": 370,
           "Epg": 410, "Lpg": 411, "Egr": 412, "Lgr": 413, "Egd": 414, "Lgd": 415,
           "Ebd": 416, "Lbd": 417, "Epr": 418, "Lpr": 419, "Ems": 421, "Lms": 422}

PROBE = r"""local main_state = require("main_state")
local done = false
local ids = { %s }
local function report()
  if done then return true end
  done = true
  local nums = {}
  for _, id in ipairs(ids) do nums[#nums + 1] = id .. ":" .. tostring(main_state.number(id)) end
  print(string.format("E2E-RESULT gauge=%%s gauge_int=%%s gauge_dec=%%s nums=%%s",
    tostring(main_state.float_number(1107)), tostring(main_state.number(107)),
    tostring(main_state.number(407)), table.concat(nums, ",")))
  pcall(function() io.stdout:flush() end)
  return true
end
return {
  type = 7, name = "vibeBMS e2e result", w = 1280, h = 720,
  input = 500, scene = 3600000, fadeout = 500,
  property = {}, filepath = {}, offset = {},
  source = { { id = 0, path = "dot.png" } },
  image = { { id = "dot", src = 0, x = 0, y = 0, w = 4, h = 4 } },
  destination = {
    { id = "dot", draw = report, dst = { { x = 0, y = 0, w = 64, h = 64 } } },
  },
}
""" % ", ".join(str(v) for v in NUMBERS.values())

# the default skin's scenes; the result is ours (see PROBE)
SKINS = {
    0: "skin/default/play/play7.luaskin", 1: "skin/default/play5.json",
    2: "skin/default/play14.json", 3: "skin/default/play10.json",
    4: "skin/default/play9.json", 5: "skin/default/select.json",
    6: "skin/default/decide/decide.luaskin", 7: "skin/e2e/result.luaskin",
    15: "skin/default/graderesult.json",
}
DIST_PARTS = ["beatoraja.jar", "natives", "font", "defaultsound", "sound", "bgm",
              "folder", "random", "table"]


def png(w, h, rgba):
    raw = b"".join(b"\x00" + bytes(rgba) * w for _ in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def key_code(lane, key, seven):
    """vibeBMS's lane and key as a beatoraja replay's key code (1-based)."""
    scratch = 7 if seven else 5
    if lane == scratch:
        return scratch + 1 + key
    return lane + 1


def write_replay(path, sha, gauge, inputs, seven):
    raw = b"".join(
        struct.pack("<bq", key_code(lane, key, seven) * (1 if down else -1), t)
        for t, lane, key, down in inputs)
    d = {
        "player": "NO NAME", "sha256": sha,
        "keyinput": base64.urlsafe_b64encode(gzip.compress(raw, mtime=0)).decode(),
        "gauge": gauge, "laneShufflePattern": [None], "date": 0,
        "randomoptionseed": 1, "config": {"hispeed": 1.0},
    }
    with open(path, "wb") as f:
        f.write(gzip.compress(json.dumps(d, indent=1).encode(), mtime=0))


def prepare(dist, work):
    os.makedirs(work, exist_ok=True)
    for p in DIST_PARTS:
        dst = os.path.join(work, p)
        if not os.path.lexists(dst):
            os.symlink(os.path.abspath(os.path.join(dist, p)), dst)
    skin = os.path.join(work, "skin")
    os.makedirs(os.path.join(skin, "e2e"), exist_ok=True)
    if not os.path.lexists(os.path.join(skin, "default")):
        os.symlink(os.path.abspath(os.path.join(dist, "skin", "default")),
                   os.path.join(skin, "default"))
    with open(os.path.join(skin, "e2e", "result.luaskin"), "w") as f:
        f.write(PROBE)
    with open(os.path.join(skin, "e2e", "dot.png"), "wb") as f:
        f.write(png(4, 4, (255, 255, 255, 255)))
    os.makedirs(os.path.join(work, "player", "player1", "replay"), exist_ok=True)
    os.makedirs(os.path.join(work, "bms"), exist_ok=True)


def configure(work, opts, skins=SKINS):
    with open(os.path.join(HERE, "seed", "config_sys.json"), encoding="utf-8") as f:
        s = json.load(f)
    s["bmsroot"] = [os.path.abspath(os.path.join(work, "bms"))]
    s.update({"tableURL": [], "enableIpfs": False, "updatesong": False})
    with open(os.path.join(work, "config_sys.json"), "w", encoding="utf-8") as f:
        json.dump(s, f, indent=1, ensure_ascii=False)
    with open(os.path.join(HERE, "seed", "config_player.json"), encoding="utf-8") as f:
        p = json.load(f)
    for i, path in skins.items():
        p["skin"][i] = {"path": path, "properties": {"option": [], "file": [], "offset": []}}
    p["irconfig"] = []
    p["autosavereplay"] = [0, 0, 0, 0]
    p["gauge"] = opts.get("gauge", 2)
    p["lnmode"] = opts.get("lnmode", 0)
    p["random"] = 0
    for m in ("mode5", "mode7", "mode10", "mode14", "mode9"):
        p[m]["playconfig"]["judgetype"] = opts.get("judgetype", "Combo")
    with open(os.path.join(work, "player", "player1", "config_player.json"), "w",
              encoding="utf-8") as f:
        json.dump(p, f, indent=1, ensure_ascii=False)


def java_cmd(java, args):
    return [java, "-Xms512m", "-Xmx2g", "-Dfile.encoding=UTF-8", "-Dstdout.encoding=UTF-8",
            "-Djava.library.path=natives", "-jar", "beatoraja.jar"] + args


def log_messages(work):
    path = os.path.join(work, "beatoraja_log.xml")
    try:
        x = open(path, encoding="utf-8", errors="replace").read()
    except OSError:
        return []
    return [m for m in re.findall(r"<message>(.*?)</message>", x, re.S)]


def decode_ghost(g):
    """The score record's "Ghost": URL-safe base64 of gzipped bytes, one a
    note in chart order (as they read: 0 PGREAT .. 4 POOR, 5 not reached)."""
    if not g:
        return None
    try:
        return list(gzip.decompress(base64.urlsafe_b64decode(g)))
    except (ValueError, OSError):
        return None


def run_case(java, work, case_path, speed, timeout_extra=40):
    case = json.load(open(case_path))
    chart = os.path.normpath(os.path.join(os.path.dirname(case_path), case["chart"]))
    data = open(chart, "rb").read()
    sha = hashlib.sha256(data).hexdigest()
    seven = any(f"{c}:" in data.decode("ascii", "replace") for c in ("18", "19"))
    opts = case["beatoraja"]
    configure(work, opts)
    rdir = os.path.join(work, "player", "player1", "replay")
    for f in os.listdir(rdir):
        os.remove(os.path.join(rdir, f))
    has_ln = re.search(rb"^#\d{3}5\d:|^#LNOBJ", data, re.M) is not None
    prefix = ("", "C", "H")[opts.get("lnmode", 0)] if has_ln else ""
    write_replay(os.path.join(rdir, prefix + sha + ".brd"), sha, opts.get("gauge", 2),
                 case["inputs"], seven)
    for f in ("beatoraja_log.xml", "beatoraja_log.xml.lck"):
        if os.path.exists(os.path.join(work, f)):
            os.remove(os.path.join(work, f))
    length = max((i[0] for i in case["inputs"]), default=0) / 1e6
    limit = 30 + length / speed + timeout_extra
    out_path = os.path.join(work, "stdout.log")
    t0 = time.time()
    with open(out_path, "wb") as out:
        proc = subprocess.Popen(java_cmd(java, ["-r1", os.path.abspath(chart)]), cwd=work,
                                stdout=out, stderr=subprocess.STDOUT)
    held = False
    result = None
    try:
        while time.time() - t0 < limit:
            time.sleep(0.2)
            if speed != 1 and not held and any("STATE_READY" in m for m in log_messages(work)):
                # (no window manager: the game's window takes the keys
                # only once focused)
                subprocess.run(["xdotool", "search", "--name", "^beatoraja", "windowfocus"],
                               check=False)
                subprocess.run(["xdotool", "keydown", "4"], check=False)
                held = True
            text = open(out_path, encoding="utf-8", errors="replace").read()
            m = re.search(r"E2E-RESULT gauge=(\S+) gauge_int=(\S+) gauge_dec=(\S+) nums=(\S+)", text)
            if m:
                result = m
                # (the score record may reach the log a moment later)
                for _ in range(50):
                    if any(x.startswith('"score"') for x in log_messages(work)):
                        break
                    time.sleep(0.1)
                break
            if proc.poll() is not None:
                break
    finally:
        if held:
            subprocess.run(["xdotool", "keyup", "4"], check=False)
        proc.terminate()
        try:
            proc.wait(5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    wall = time.time() - t0
    score = None
    for m in log_messages(work):
        if m.startswith('"score"'):
            score = json.loads(m.split(":", 1)[1])
    r = {"name": case["name"], "sha256": sha, "wall": round(wall, 1), "speed": speed}
    if result is not None:
        nums = dict(x.split(":") for x in result.group(4).split(","))
        shown = {k: int(nums[str(v)]) for k, v in NUMBERS.items()}
        r["result_screen"] = shown
        if score is None:
            # a failed play is not logged: the result screen's numbers
            score = dict(shown)
            r["source"] = "result screen"
        else:
            r["source"] = "log"
            r["screen_differs"] = sorted(k for k, v in shown.items() if score.get(k) != v)
    if score is None or result is None:
        tail = open(out_path, encoding="utf-8", errors="replace").read()[-3000:]
        r["error"] = "no result (score record: %s, gauge line: %s)" % (
            score is not None, result is not None)
        r["log_tail"] = tail
        if score is not None:
            r["score"] = score
        return r
    r["score"] = {k: v for k, v in score.items() if k not in ("Ghost", "Scorehash")}
    r["ghost"] = decode_ghost(score.get("Ghost"))
    r["gauge"] = float(result.group(1))
    r["gauge_shown"] = int(result.group(2)) + int(result.group(3)) / 10
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dist", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--cases", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--java", default=os.environ.get("JAVA", "java"))
    ap.add_argument("--speed", type=int, default=3, choices=(1, 3))
    ap.add_argument("--only", nargs="*", help="case names (shell patterns)")
    ap.add_argument("--recheck", metavar="VIBE_JSONL",
                    help="play again at speed 1 each case whose result differs from "
                         "vibeBMS's (compare.py), keeping that result")
    a = ap.parse_args()
    prepare(a.dist, a.work)
    names = sorted(f[:-5] for f in os.listdir(a.cases) if f.endswith(".json"))
    if a.only:
        names = [n for n in names if any(fnmatch.fnmatch(n, p) for p in a.only)]
    results = []
    for n in names:
        r = run_case(a.java, a.work, os.path.join(a.cases, n + ".json"), a.speed)
        results.append(r)
        s = r.get("score", {})
        print(f"{n:28} {r['wall']:6.1f}s " + (
            r["error"] if "error" in r else
            f"clear={s.get('Clear')} ex={s.get('Exscore')} combo={s.get('Combo')} "
            f"gauge={r['gauge']}"), flush=True)
        with open(a.out, "w") as f:
            json.dump(results, f, indent=1)
    if a.recheck and a.speed != 1:
        import compare
        vibe = compare.read_vibe(a.recheck)
        for k, r in enumerate(results):
            v = vibe.get(r["name"])
            if v is None or "error" in v or ("error" not in r and not compare.field_diffs(v, r)):
                continue
            again = run_case(a.java, a.work, os.path.join(a.cases, r["name"] + ".json"), 1)
            again["first"] = {x: r.get(x) for x in ("score", "gauge", "error", "speed")}
            results[k] = again
            s = again.get("score", {})
            print(f"{r['name']:28} {again['wall']:6.1f}s again at speed 1: " + (
                again["error"] if "error" in again else
                f"clear={s.get('Clear')} ex={s.get('Exscore')} combo={s.get('Combo')} "
                f"gauge={again['gauge']}"), flush=True)
            with open(a.out, "w") as f:
                json.dump(results, f, indent=1)
    errors = [r for r in results if "error" in r]
    if errors:
        print(f"{len(errors)} case(s) gave no result", file=sys.stderr)


if __name__ == "__main__":
    main()
