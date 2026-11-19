"""The LR2 side of the lr2-measure comparison: charts, key schedules,
latency calibration, and LR2's counts against vibeBMS's rows.
Standard library only.

    python e2e.py gen OUTDIR
        the lr2-measure charts, each with a BGM click (the start signal) at
        measure 1 (chart time 1600 ms, before any note), plus the
        calibration charts, and click.wav
    python e2e.py plan CHART OUT.csv
        the key presses for CHART (a file gen wrote), on chart time:
        "t_ms,scan,down" rows for `lr2probe play`
    python e2e.py scores SCORE.db CHARTDIR OUT.json
        LR2's score rows for the charts in CHARTDIR (by their MD5)
    python e2e.py calibrate OUT.json COARSE_OR_FINE A.json? ...
        see calibrate()
    python e2e.py compare SCORES.json OUT.md
        LR2's counts against vibeBMS's rows (lr2_measure.rs, the README's
        tolerances)

The charts' timing (tools/lr2-measure/gen.py): BPM 150, 4/4, a measure
1600 ms; notes from 3200 ms.
"""

import hashlib
import json
import math
import os
import sqlite3
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MEASURE_CHARTS = os.path.join(HERE, "..", "..", "lr2-measure", "charts")
MEASURE_MS = 1600
CLICK_MS = 1600
LEAD_MS = 3200
TRIAL_MS = 3200
TAP_MS = 30

# LR2's default keyboard keys for 1P: Z S X D C F V, scratch on the left
# Shift and Ctrl (scan codes)
SCAN = {1: 0x2C, 2: 0x1F, 3: 0x2D, 4: 0x20, 5: 0x2E, 6: 0x21, 7: 0x2F,
        "S": 0x2A, "S2": 0x1D}
LANE = {"11": 1, "12": 2, "13": 3, "14": 4, "15": 5, "18": 6, "19": 7, "16": "S"}

# calibration sweeps: (name, #RANK, first offset, step) in ms; each press
# is the note's time plus its offset (on top of the latency in use)
CAL = {
    "c1": (3, -200.0, 2.5), "c2": (3, -10.0, 2.5),
    "f1": (0, -80.0, 1.0), "f2": (0, -10.0, 1.0),
    # f1 again, once calibrated: each play's own offset (its hits less
    # 10 + G + 1), how much the start signal moves from play to play
    "f3": (0, -80.0, 1.0), "f4": (0, -80.0, 1.0), "f5": (0, -80.0, 1.0),
}
CAL_NOTES = {"c1": 85, "c2": 85, "f1": 91, "f2": 91, "f3": 91, "f4": 91, "f5": 91}
CAL_GAP_MS = 400


def click_wav(path):
    rate, n = 44100, 441
    frames = b"".join(struct.pack("<h", int(28000 * math.sin(2 * math.pi * 1000 * k / rate)))
                      for k in range(n))
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 36 + len(frames)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16))
        f.write(b"data" + struct.pack("<I", len(frames)) + frames)


def with_click(text, title_suffix=""):
    out = []
    for line in text.splitlines():
        if line.startswith("#TITLE") and title_suffix:
            line += title_suffix
        out.append(line)
        if line.startswith("#PLAYLEVEL"):
            out.append("#WAV02 click.wav")
    out.append("#00101:02")
    return "\r\n".join(out) + "\r\n"


def write_chart(path, title, rank, notes):
    """notes: (time ms, channel)"""
    rows = {}
    for t, ch in notes:
        m, rest = divmod(t, MEASURE_MS)
        slot = int(round(rest / 5))
        rows.setdefault((m, ch), ["00"] * 320)[slot] = "01"
    head = ["#PLAYER 1", "#GENRE LR2 MEASURE E2E", f"#TITLE {title}",
            "#ARTIST vibeBMS tools/e2e/lr2", "#BPM 150", "#PLAYLEVEL 1",
            f"#RANK {rank}", "#TOTAL 300"]
    body = [f"#{m:03d}{ch}:{''.join(v)}" for (m, ch), v in sorted(rows.items())]
    with open(path, "w", newline="") as f:
        f.write(with_click("\n".join(head + [""] + body)))


def gen(outdir):
    os.makedirs(outdir, exist_ok=True)
    click_wav(os.path.join(outdir, "click.wav"))
    for name in sorted(os.listdir(MEASURE_CHARTS)):
        if name.endswith(".bme"):
            with open(os.path.join(MEASURE_CHARTS, name), newline="") as f:
                text = f.read()
            with open(os.path.join(outdir, name), "w", newline="") as f:
                f.write(with_click(text, " (e2e)"))
    lanes = ["11", "12", "13", "14", "15", "18", "19"]
    for name, (rank, _, _) in CAL.items():
        notes = [(LEAD_MS + k * CAL_GAP_MS, lanes[k % 7]) for k in range(CAL_NOTES[name])]
        write_chart(os.path.join(outdir, f"cal-{name}.bme"), f"E2E CAL {name}", rank, notes)


def notes_of(path):
    """(time ms, lane) of the chart's notes, and its mines"""
    notes, mines = [], []
    for line in open(path, encoding="ascii", errors="replace"):
        line = line.strip()
        if len(line) < 7 or not line[1:6].isdigit() and not line[1:4].isdigit():
            continue
        if not line.startswith("#") or ":" not in line:
            continue
        head, data = line[1:].split(":", 1)
        if len(head) != 5 or not head[:3].isdigit():
            continue
        m, ch = int(head[:3]), head[3:]
        n = len(data) // 2
        for i in range(n):
            if data[2 * i:2 * i + 2] != "00":
                t = m * MEASURE_MS + i * MEASURE_MS / n
                if ch in LANE:
                    notes.append((t, LANE[ch]))
                elif ch == "D1":
                    mines.append(t)
    return sorted(notes, key=lambda x: (x[0], str(x[1]))), mines


def taps(t, keys):
    out = []
    for k in keys:
        out += [(t, SCAN[k], 1), (t + TAP_MS, SCAN[k], 0)]
    return out


def plan(chart):
    name = os.path.basename(chart)[:-4]
    notes, mines = notes_of(chart)
    ev = []
    if name.startswith("cal-"):
        _, lo, step = CAL[name[4:]]
        for k, (t, lane) in enumerate(notes):
            ev += taps(t + lo + k * step, [lane])
    elif name in ("a11-1", "a11-2") or name.startswith("a3-"):
        # key 1 + key 7 together on each key-7 note
        for t, lane in notes:
            if lane == 7:
                ev += taps(t, [1, 7])
    elif name == "a11-3":
        # scratch + key 6 on the key-6 note, the other scratch key + key 7
        # on the key-7 note
        for t, lane in notes:
            if lane == 6:
                ev += taps(t, ["S", 6])
            elif lane == 7:
                ev += taps(t, ["S2", 7])
    elif name == "a8-mines":
        # run 1: the key-7 notes hit, then key 1 held from the rest after
        # them to the end
        for t, lane in notes:
            ev += taps(t, [lane])
        hold = LEAD_MS + TRIAL_MS
        ev += [(hold, SCAN[1], 1), (max(mines) + 2000, SCAN[1], 0)]
    else:
        raise SystemExit(f"no plan for {name}")
    return sorted(ev, key=lambda e: (e[0], -e[2]))


def md5(path):
    with open(path, "rb") as f:
        return hashlib.md5(f.read()).hexdigest()


def scores(db, chartdir, out):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    cols = [r[1] for r in con.execute("PRAGMA table_info(score)")]
    res = {"columns": cols, "charts": {}}
    for name in sorted(os.listdir(chartdir)):
        if name.endswith(".bme"):
            row = con.execute("SELECT * FROM score WHERE hash = ?", (md5(os.path.join(chartdir, name)),)).fetchone()
            res["charts"][name[:-4]] = dict(zip(cols, row)) if row else None
    json.dump(res, open(out, "w"), indent=1)
    print("score columns:", ", ".join(cols))
    for k, v in res["charts"].items():
        if v:
            print(k, {c: v[c] for c in cols if c in ("perfect", "great", "good", "bad", "poor", "clear", "maxcombo", "minbp", "playcount")})


def hits(row):
    return row["perfect"] + row["great"] + row["good"]


def calibrate(scores_json, kind):
    """x, the latency still to add (ms), and G, the GOOD window (ms), from
    the two sweeps of KIND (c: coarse, f: fine)"""
    rows = json.load(open(scores_json))["charts"]
    a, b = rows.get(f"cal-{kind}1"), rows.get(f"cal-{kind}2")
    if not a or not b:
        raise SystemExit(f"no scores for cal-{kind}1/2")
    step = CAL[f"{kind}1"][2]
    na, nb = hits(a), hits(b)
    x = step * (na - nb) / 2
    g = step * (na + nb - 2) / 2 - 10
    n = CAL_NOTES[f"{kind}1"]
    ok = 0 < na < n and 0 < nb < n
    print(json.dumps({"kind": kind, "hits_a": na, "hits_b": nb, "x_ms": x, "good_window_ms": g, "valid": ok}))
    return x, g, ok


def jitter(scores_json):
    rows = json.load(open(scores_json))["charts"]
    _, g, _ = calibrate(scores_json, "f")
    out = {}
    for k in ("f3", "f4", "f5"):
        r = rows.get(f"cal-{k}")
        if r:
            # hits = (10 + G + x) + 1 with 1 ms steps
            out[k] = hits(r) - 11 - g
    print(json.dumps({"good_window_ms": g, "offset_ms_per_play": out}))


# vibeBMS's rows: (judgement, count) for the counts lr2_measure.rs and the
# README read; 12 of 16 decides ("about", README)
EXPECT = {
    "a11-1": {"bad": 16, "poor": 0},
    "a11-2": {"good": 16, "poor": 16},
    "a11-3": {"bad": 32, "poor": 0},
    "a3-rank0-70": {"bad": 16}, "a3-rank2-70": {"good": 16}, "a3-rank3-70": {"good": 16},
    "a3-rank0-110": {"bad": 16}, "a3-rank2-110": {"bad": 16}, "a3-rank3-110": {"good": 16},
    # 4, 5 and -1 as NORMAL (2)
    "a3-rank4-70": {"good": 16}, "a3-rank5-70": {"good": 16}, "a3-rankm1-70": {"good": 16},
    "a3-rank4-110": {"bad": 16}, "a3-rank5-110": {"bad": 16}, "a3-rankm1-110": {"bad": 16},
    # run 1 (GROOVE, key 1 held): FULL COMBO, max combo the 8 notes, POOR
    # 4 (S-OLR2: the screen may show 8)
    "a8-mines": {"poor": (4, 8), "maxcombo": 8},
}


def within(want, got, scale):
    """README: "about": 12 of 16 or more decides; a count of 0 allows 4"""
    if want == 0:
        return got <= 4 * scale
    return abs(got - want) <= 4 * scale


def compare(scores_json, out):
    rows = json.load(open(scores_json))["charts"]
    lines = ["| chart | count | vibeBMS | LR2 | |", "|---|---|---|---|---|"]
    bad = 0
    for chart, want in EXPECT.items():
        row = rows.get(chart)
        for col, n in want.items():
            if row is None:
                shown = " or ".join(map(str, n)) if isinstance(n, tuple) else n
                lines.append(f"| {chart} | {col} | {shown} | not played | DIFFERS |")
                bad += 1
                continue
            got = row[col]
            if isinstance(n, tuple):
                ok = got in n
                n = " or ".join(map(str, n))
            elif chart.startswith("a8"):
                ok = got == n
            else:
                ok = within(n, got, n // 16 if n >= 16 else 1)
            bad += not ok
            lines.append(f"| {chart} | {col} | {n} | {got} | {'ok' if ok else 'DIFFERS'} |")
    lamp = rows.get("a8-mines")
    if lamp:
        lines.append(f"| a8-mines | clear (LR2's lamp code) | FULL COMBO | {lamp['clear']} | |")
    text = "\n".join(lines) + "\n"
    open(out, "w").write(text)
    print(text)
    return bad


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "gen":
        gen(sys.argv[2])
    elif cmd == "plan":
        with open(sys.argv[3], "w") as f:
            for t, sc, d in plan(sys.argv[2]):
                f.write(f"{t:.3f},{sc:x},{d}\n")
    elif cmd == "scores":
        scores(sys.argv[2], sys.argv[3], sys.argv[4])
    elif cmd == "calibrate":
        x, g, ok = calibrate(sys.argv[2], sys.argv[3])
        sys.exit(0 if ok else 1)
    elif cmd == "jitter":
        jitter(sys.argv[2])
    elif cmd == "compare":
        sys.exit(1 if compare(sys.argv[2], sys.argv[3]) else 0)
    else:
        raise SystemExit(__doc__)
