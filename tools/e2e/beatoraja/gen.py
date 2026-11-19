#!/usr/bin/env python3
"""The behaviour cases of the beatoraja comparison: test charts (written to
charts/, kept in the repository) and, for each case, the key log both
players are fed (written to the folder given, as JSON).

    python3 gen.py [--cases <dir>] [--check]

--check: fail when charts/ differs from what this writes (the charts are
committed). A case: {"name", "chart" (path from the case file), "rules"
(vibeBMS's RuleOptions), "beatoraja" (its player options), "inputs":
[[time µs, lane, key, pressed], ...], "notes"}. Lanes are vibeBMS's (7
keys: 0..6 the keys, 7 the scratch with key 0 up and 1 down); the
beatoraja runner turns them into its replay's key codes.

Every chart here is 7 keys (it has notes on key 6 or 7) and, unless it
says otherwise, runs at BPM 150 in 4/4: a measure is 1600 ms, cut into
320 slots of 5 ms. Standard library only.
"""

import argparse
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CHARTS = os.path.join(HERE, "charts")
LR2_CHARTS = os.path.join(HERE, "..", "..", "lr2-measure", "charts")

MEASURE_MS = 1600
SLOTS = 320
SLOT_MS = MEASURE_MS // SLOTS
LEAD_MS = 2 * MEASURE_MS
MS = 1000

# 1P channels by vibeBMS lane (7 keys); 7 is the scratch
KEY_CH = ["11", "12", "13", "14", "15", "18", "19", "16"]
SCRATCH = 7

# beatoraja's judge windows (docs/RULES.md, "Judge presets"), µs, 7 keys
# at #RANK EASY (100 %): (PG, GR, GD, BAD early, BAD late)
WINDOWS = {"key": (20, 60, 150, 220, 280), "scratch": (30, 70, 160, 230, 290)}
RANK_SCALE = {0: 0.25, 1: 0.50, 2: 0.75, 3: 1.00, 4: 1.25}
EMPTY_POOR_EARLY = 500

# beatoraja's option values (its config_player.json): gauge 0..5 as
# vibeBMS's GaugeType ids, lnmode 0 LN, 1 CN, 2 HCN
GAUGES = ["ASSIST EASY", "EASY", "NORMAL", "HARD", "EX-HARD", "HAZARD"]
GAUGE_IDS = ["AssistEasy", "Easy", "Normal", "Hard", "ExHard", "Hazard"]
LN_MODES = ["Ln", "Cn", "Hcn"]

files = {}
cases = []


def b36(v):
    digits = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    return digits[v // 36] + digits[v % 36]


def header(title, rank=2, total=300, extra=()):
    h = [
        "*---------------------- HEADER FIELD",
        "#PLAYER 1",
        "#GENRE VIBEBMS E2E",
        f"#TITLE {title}",
        "#ARTIST vibeBMS tools/e2e",
        "#BPM 150",
        "#PLAYLEVEL 1",
    ]
    if rank is not None:
        h.append(f"#RANK {rank}")
    if total is not None:
        h.append(f"#TOTAL {total}")
    return h + list(extra)


def put(name, lines):
    files[name] = "\r\n".join(lines) + "\r\n"


def timed(name, head, events):
    """A chart at BPM 150: events (ms from the end of the lead-in, channel,
    object). Returns the note times (ms from the chart's start)."""
    rows = {}
    for t, ch, obj in events:
        t += LEAD_MS
        assert t % SLOT_MS == 0, (name, t)
        m, slot = divmod(t // SLOT_MS, SLOTS)
        cell = rows.setdefault((m, ch), ["00"] * SLOTS)
        assert cell[slot] == "00", (name, t, ch)
        cell[slot] = obj
    body = ["", "*---------------------- MAIN DATA FIELD"]
    for (m, ch) in sorted(rows):
        body.append(f"#{m:03d}{ch}:{''.join(rows[(m, ch)])}")
    put(name, head + body)


def press(inputs, t_ms, lane, key=0, hold_ms=60):
    """A press at t_ms (float, chart time) held hold_ms (long enough to be
    seen at 3 times the speed: beatoraja reads a replay's keys as states)."""
    t = round(t_ms * MS)
    inputs.append([t, lane, key, True])
    inputs.append([t + round(hold_ms * MS), lane, key, False])


def case(name, chart, inputs, gauge=2, ln=0, notes="", rules=None):
    r = {"gauge": GAUGE_IDS[gauge], "ln_mode": LN_MODES[ln]}
    r.update(rules or {})
    cases.append({
        "name": name,
        "chart": chart,
        "rules": r,
        "beatoraja": {"gauge": gauge, "lnmode": ln},
        "inputs": sorted(inputs, key=lambda i: (i[0], i[3])),
        "notes": notes,
    })


# ----- judge windows by #RANK -------------------------------------------

def judge_rank(rank, label=None, extra=(), scale=None):
    """One note per trial, 500 ms apart, the keys in turn then the scratch;
    each pressed once at an offset around a window's edge: 1 ms inside, on
    it, 1 ms outside, early and late; then far early and far late."""
    scale = scale or RANK_SCALE.get(rank, 0.75)
    label = label or f"rank{rank}"
    name = f"judge-{label}.bme"
    ev, inputs = [], []
    t = 0
    k = 0
    for kind in ("key", "scratch"):
        pg, gr, gd, be, bl = WINDOWS[kind]
        offsets = []
        for w in (pg, gr, gd):
            for d in (-1, 0, 1):
                offsets += [w * scale + d, -(w * scale + d)]
        for d in (-1, 0, 1):
            offsets += [be * scale + d, -(bl * scale + d)]
        offsets += [EMPTY_POOR_EARLY - 20, EMPTY_POOR_EARLY + 20]
        for off in offsets:
            lane = SCRATCH if kind == "scratch" else k % 7
            k += 1
            ev.append((t, KEY_CH[lane], "01"))
            # offset > 0: early
            press(inputs, LEAD_MS + t - off, lane, key=k % 2 if lane == SCRATCH else 0)
            t += 500 if kind == "key" else 1000
    timed(name, header(f"judge {label}", rank=rank, extra=extra), ev)
    case(f"judge-{label}", name, inputs,
         notes="presses 1 ms inside, on and 1 ms outside each window's edge")


# ----- long notes -------------------------------------------------------

def long_notes():
    """Long notes of 1200 ms (#LNTYPE 1, no #LNMODE: they play as the LN
    mode chosen), one at a time on the keys in turn, held in different
    ways; played as LN, CN and HCN."""
    name = "ln.bme"
    ev, inputs = [], []
    ln_len = 1200
    # (press offset ms (+ early), release relative to the end ms (+ late),
    # extra (press, release) pairs relative to the start)
    trials = [
        (0, 0, ()), (0, -100, ()), (0, -150, ()), (0, -190, ()), (0, -250, ()),
        (0, -400, ()), (0, -900, ()), (0, 100, ()), (0, 250, ()), (0, 400, ()),
        (-30, 0, ()), (-100, 0, ()), (40, 0, ()), (None, None, ()),
        (0, None, ((600, 300),)),  # let go at 300 ms, again at 600 to the end
        (0, 1200, ()),  # held 1.2 s past the end
    ]
    t = 0
    for i, (p, r, again) in enumerate(trials):
        lane = i % 7
        ch = "5" + KEY_CH[lane][1]
        ev += [(t, ch, "01"), (t + ln_len, ch, "01")]
        start, end = LEAD_MS + t, LEAD_MS + t + ln_len
        if p is not None:
            down = start - p
            up = end + r if r is not None else start + again[0][1]
            press(inputs, down, lane, hold_ms=up - down)
            for (again_at, _) in again:
                press(inputs, start + again_at, lane, hold_ms=end - start - again_at)
        t += 3200
    timed(name, header("long notes", extra=["#LNTYPE 1"]), ev + [(t, KEY_CH[6], "01")])
    press(inputs, LEAD_MS + t, 6)
    for ln in range(3):
        case(f"ln-{LN_MODES[ln].lower()}", name, inputs, ln=ln,
             notes="LN, CN, HCN: on time, released early or late by steps, pressed late, missed")


def bss():
    """Scratch long notes (back-spin scratch as CN / HCN): turned one way,
    then let go at the end, or turned the other way at the end."""
    name = "bss.bme"
    ev, inputs = [], []
    t = 0
    trials = ["release", "reverse", "early", "late-reverse", "miss"]
    for i, how in enumerate(trials):
        ev += [(t, "56", "01"), (t + 1200, "56", "01")]
        start, end = LEAD_MS + t, LEAD_MS + t + 1200
        if how == "release":
            press(inputs, start, SCRATCH, 0, hold_ms=1200)
        elif how == "reverse":
            press(inputs, start, SCRATCH, 0, hold_ms=1200)
            press(inputs, end, SCRATCH, 1)
        elif how == "early":
            press(inputs, start, SCRATCH, 0, hold_ms=700)
        elif how == "late-reverse":
            press(inputs, start, SCRATCH, 1, hold_ms=1300)
            press(inputs, end + 100, SCRATCH, 0)
        t += 3200
    ev.append((t, KEY_CH[6], "01"))
    press(inputs, LEAD_MS + t, 6)
    timed(name, header("back-spin scratch", extra=["#LNTYPE 1"]), ev)
    for ln in range(3):
        case(f"bss-{LN_MODES[ln].lower()}", name, inputs, ln=ln,
             notes="scratch long notes: let go, turned back, let go early, missed")


# ----- mines ------------------------------------------------------------

def mines():
    """Eight key-7 notes, then mines on key 1 (values 10, 20, 50 and ZZ):
    held through, tapped on time, tapped 50 ms or 200 ms early, tapped late,
    not touched."""
    name = "mines.bme"
    ev = [(i * 100, KEY_CH[6], "01") for i in range(8)]
    inputs = []
    for i in range(8):
        press(inputs, LEAD_MS + i * 100, 6)
    t = 2 * MEASURE_MS
    plan = [("hold", 10), ("tap", 20), ("early50", 10), ("early200", 10),
            ("late", 10), ("none", 50), ("tap", 50), ("hold", 36 * 36 - 1)]
    for i, (how, v) in enumerate(plan):
        ev.append((t, "D1", b36(v)))
        at = LEAD_MS + t
        if how == "hold":
            press(inputs, at - 300, 0, hold_ms=600)
        elif how == "tap":
            press(inputs, at, 0)
        elif how.startswith("early"):
            press(inputs, at - int(how[5:]), 0)
        elif how == "late":
            press(inputs, at + 30, 0)
        t += MEASURE_MS
    ev.append((t, KEY_CH[6], "01"))
    press(inputs, LEAD_MS + t, 6)
    timed(name, header("mines", total=1000), ev)
    for g in (2, 3):
        case(f"mines-{GAUGES[g].lower()}", name, inputs, gauge=g,
             notes="mines held through, tapped on time, early and late")


# ----- gauges and #TOTAL ------------------------------------------------

def mixed(name, title, total, n=240, fail_from=None, bad_every=(7, 13, 29, 31)):
    """n notes 200 ms apart on the keys in turn (a key-7 note first), hit
    with a pattern of timing errors: GREATs, GOODs, BADs, POORs (missed),
    empty POORs; from `fail_from` on, every note missed."""
    ev, inputs = [], []
    gr_every, gd_every, bd_every, pr_every = bad_every
    for i in range(n):
        lane = 6 if i == 0 else i % 7
        t = i * 200
        ev.append((t, KEY_CH[lane], "01"))
        at = LEAD_MS + t
        if fail_from is not None and i >= fail_from:
            continue
        if i % pr_every == pr_every - 1:
            continue  # missed: POOR
        if i % bd_every == bd_every - 1:
            off = -180  # a late BAD (NORMAL: GOOD to 112.5 ms, BAD to 210 late)
        elif i % gd_every == gd_every - 1:
            off = 80
        elif i % gr_every == gr_every - 1:
            off = -30
        else:
            off = (-5, 0, 5, 10)[i % 4]
        press(inputs, at - off, lane)
        if i % 17 == 16:
            press(inputs, at + 120, (lane + 3) % 7)  # an empty POOR, far from notes
    timed(name, header(title, total=total), ev)
    return inputs


def gauges():
    name = "gauge-mix.bme"
    inputs = mixed(name, "gauges, mixed play", 160)
    for g in range(6):
        case(f"gauge-{GAUGES[g].lower().replace(' ', '-')}", name, inputs, gauge=g,
             notes="GREAT, GOOD, BAD, POOR and empty POOR mixed in")
    name = "gauge-fail.bme"
    inputs = mixed(name, "gauges, failing", 300, fail_from=150)
    for g in range(6):
        case(f"gauge-fail-{GAUGES[g].lower().replace(' ', '-')}", name, inputs, gauge=g,
             notes="every note missed from the 150th: survival gauges fail")
    # HARD low: damage softened below 50 % (beatoraja's HARD)
    name = "gauge-hard-low.bme"
    inputs = mixed(name, "HARD low", 300, n=300, bad_every=(5, 9, 11, 13))
    for g in (3, 4):
        case(f"gauge-low-{GAUGES[g].lower()}", name, inputs, gauge=g,
             notes="many POORs: HARD falls low (its damage softened below 50 %)")


def totals():
    for total in (100, 160, 260, 500, None):
        label = "none" if total is None else str(total)
        name = f"total-{label}.bme"
        inputs = mixed(name, f"TOTAL {label}", total, n=120)
        for g in (1, 2):
            case(f"total-{label}-{GAUGES[g].lower()}", name, inputs, gauge=g,
                 notes="the same play at another #TOTAL")


# ----- BPM changes and stops --------------------------------------------

def bpm_stop():
    """Measures of 4 quarter notes on the keys in turn through BPM changes
    (channel 03 and 08), STOPs (09) and a measure of 3/4 (02); the note
    times worked out here, each pressed at a small error."""
    name = "bpm-stop.bme"
    head = header("BPM changes and stops", extra=[
        "#BPM01 75", "#BPM02 300", "#BPM03 187.5", "#BPM04 120",
        "#STOP01 48", "#STOP02 96", "#STOP03 192",
    ])
    # per measure: (length, [(pos, bpm change)], [(pos, stop in 1/192 of a 4/4 measure)])
    plan = {
        2: (1.0, [], []),
        3: (1.0, [(0.5, 240)], []),          # 03 channel (hex F0)
        4: (1.0, [(0.0, ("01", 75))], []),
        5: (1.0, [(0.25, ("02", 300))], [(0.75, "01")]),
        6: (0.75, [(0.0, ("03", 187.5))], []),
        7: (1.0, [], [(0.0, "02"), (0.5, "03")]),
        8: (1.0, [(0.0, ("04", 120))], [(0.25, "01")]),
        9: (1.0, [(0.0, 150)], []),
    }
    stops = {"01": 48, "02": 96, "03": 192}
    lines = []
    events = []  # (measure, pos, kind, value)
    for m, (length, bpms, sts) in plan.items():
        if length != 1.0:
            lines.append(f"#{m:03d}02:{length}")
        for pos, v in bpms:
            events.append((m, pos, "bpm", v))
        for pos, s in sts:
            events.append((m, pos, "stop", s))
    # notes: 4 a measure (3 in 3/4), on the keys in turn
    notes = []
    k = 0
    for m, (length, _, _) in plan.items():
        for b in range(round(length * 4)):
            notes.append((m, b / 4, k % 7))
            k += 1
    for m, (length, _, _) in plan.items():
        q = round(length * 4)
        by_lane = {}
        for (mm, pos, lane) in notes:
            if mm == m:
                slot = round(pos / length * q)
                by_lane.setdefault(lane, ["00"] * q)[slot] = "01"
        for lane in sorted(by_lane):
            lines.append(f"#{m:03d}{KEY_CH[lane]}:{''.join(by_lane[lane])}")
    # BPM / stop channels at 1/8 of the measure
    for m, (length, bpms, sts) in plan.items():
        res = 8
        c03, c08, c09 = ["00"] * res, ["00"] * res, ["00"] * res
        for pos, v in bpms:
            s = round(pos / length * res) if length != 1.0 else round(pos * res)
            if isinstance(v, tuple):
                c08[s] = v[0]
            else:
                c03[s] = f"{v:02X}"
        for pos, sid in sts:
            s = round(pos * res)
            c09[s] = sid
        for ch, c in (("03", c03), ("08", c08), ("09", c09)):
            if any(x != "00" for x in c):
                lines.append(f"#{m:03d}{ch}:{''.join(c)}")
    put(name, head + ["", "*---------------------- MAIN DATA FIELD"] + sorted(lines))

    # times: walk the measures (positions are fractions of the measure's
    # length in 4/4 measures, i.e. beats / 4)
    def bpm_value(v):
        return v[1] if isinstance(v, tuple) else v

    marks = []  # (measure, pos in 4/4 measures from the measure's start, kind, value)
    for (m, pos, kind, v) in events:
        marks.append((m, pos, kind, bpm_value(v) if kind == "bpm" else stops[v]))
    t = 0.0
    bpm = 150.0
    times = {}
    for m in range(0, 10):
        length = plan.get(m, (1.0, [], []))[0]
        pts = sorted([x for x in marks if x[0] == m], key=lambda x: (x[1], x[2] == "stop"))
        ns = sorted([n for n in notes if n[0] == m], key=lambda n: n[1])
        pos = 0.0
        # merge events and notes in position order; at a position: BPM
        # changes, then notes, then stops
        items = [(p, 0, kind, v) for (_, p, kind, v) in pts if kind == "bpm"]
        items += [(p, 1, "note", lane) for (_, p, lane) in ns]
        items += [(p, 2, kind, v) for (_, p, kind, v) in pts if kind == "stop"]
        items.sort(key=lambda x: (x[0], x[1]))
        for (p, _, kind, v) in items:
            t += (p - pos) * 4 * 60000.0 / bpm
            pos = p
            if kind == "bpm":
                bpm = v
            elif kind == "note":
                times.setdefault(m, []).append((t, v))
            else:
                t += v / 192 * 4 * 60000.0 / bpm
        t += (length - pos) * 4 * 60000.0 / bpm
    inputs = []
    errs = [-12, -4, 0, 6, 14]
    i = 0
    for m in sorted(times):
        for (tt, lane) in times[m]:
            press(inputs, tt - errs[i % len(errs)], lane)
            i += 1
    case("bpm-stop", name, inputs,
         notes="BPM changes (03, 08), STOPs and a 3/4 measure; presses a few ms off")


# ----- the lr2-measure charts -------------------------------------------

def lr2_measure():
    """tools/lr2-measure's charts under beatoraja's rules: every note
    pressed with a small timing error in turn, mines left alone."""
    for f in sorted(os.listdir(LR2_CHARTS)):
        if not f.endswith(".bme"):
            continue
        text = open(os.path.join(LR2_CHARTS, f), encoding="ascii").read()
        bpm = 150
        notes = []
        seven = any(f"{c}:" in text for c in ("18", "19"))
        for line in text.splitlines():
            if not line.startswith("#") or ":" not in line or not line[1:4].isdigit():
                continue
            m, ch, data = int(line[1:4]), line[4:6], line.split(":", 1)[1].strip()
            if ch not in KEY_CH:
                continue
            lane = KEY_CH.index(ch)
            if not seven and lane == SCRATCH:
                lane = 5
            n = len(data) // 2
            for s in range(n):
                if data[2 * s:2 * s + 2] != "00":
                    notes.append((m * MEASURE_MS + s * MEASURE_MS / n, lane))
        assert bpm == 150
        inputs = []
        errs = [-25, -10, 0, 10, 25]
        notes.sort()
        free = {}  # (lane, key): when its last press is let go
        for i, (t, lane) in enumerate(notes):
            scratch = lane == (SCRATCH if seven else 5)
            key = i % 2 if scratch else 0
            at = t - errs[i % len(errs)]
            # a press while the key is still down from the last is left out
            if at < free.get((lane, key), -1e9) + 10:
                continue
            free[(lane, key)] = at + 40
            press(inputs, at, lane, key=key, hold_ms=40)
        case(f"lr2m-{f[:-4]}", os.path.join(os.path.relpath(LR2_CHARTS, CHARTS), f), inputs,
             notes="tools/lr2-measure chart, every note pressed a few ms off")


def render_chart():
    """The rendering report's chart (render.py): 12 measures of notes on
    every lane, chords, a few long notes; no sounds or images."""
    ev = []
    for m in range(12):
        base = m * MEASURE_MS
        for i in range(8):
            lane = (i * 3 + m) % 8
            ev.append((base + i * 200, KEY_CH[lane], "01"))
        if m % 3 == 1:
            lane = (m + 2) % 7
            ev = [e for e in ev if not (e[1] == KEY_CH[lane] and base + 100 <= e[0] <= base + 700)]
            ev += [(base + 100, "5" + KEY_CH[lane][1], "01"),
                   (base + 700, "5" + KEY_CH[lane][1], "01")]
        if m % 4 == 2:
            ev += [(base + 1500, KEY_CH[c], "01") for c in (0, 2, 4, 6)]
    seen = set()
    ev = [e for e in ev if (e[0], e[1][1]) not in seen and not seen.add((e[0], e[1][1]))]
    timed("render.bme", header("vibeBMS e2e render", extra=["#LNTYPE 1", "#DIFFICULTY 2"]), ev)


def build():
    render_chart()
    for r in (0, 1, 2, 3, 4):
        judge_rank(r)
    judge_rank(None, "norank")
    judge_rank(2, "defexrank120", extra=["#DEFEXRANK 120"], scale=0.75 * 1.2)
    long_notes()
    bss()
    mines()
    gauges()
    totals()
    bpm_stop()
    lr2_measure()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", help="write the cases here")
    ap.add_argument("--check", action="store_true", help="fail when charts/ is not current")
    a = ap.parse_args()
    build()
    stale = []
    os.makedirs(CHARTS, exist_ok=True)
    for name, text in files.items():
        path = os.path.join(CHARTS, name)
        old = open(path, "rb").read() if os.path.exists(path) else None
        if old != text.encode("ascii"):
            stale.append(name)
            if not a.check:
                with open(path, "wb") as f:
                    f.write(text.encode("ascii"))
    if a.check and stale:
        sys.exit(f"charts/ is not current (python3 gen.py): {', '.join(stale)}")
    if a.cases:
        os.makedirs(a.cases, exist_ok=True)
        for c in cases:
            c = dict(c)
            c["chart"] = os.path.relpath(os.path.join(CHARTS, c["chart"]), a.cases)
            with open(os.path.join(a.cases, c["name"] + ".json"), "w") as f:
                json.dump(c, f)
        print(f"{len(cases)} cases in {a.cases}")


if __name__ == "__main__":
    main()
