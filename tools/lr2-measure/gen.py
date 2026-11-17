#!/usr/bin/env python3
"""Writes the lr2-measure test charts (charts/*.bme) and prints the tables
of README.md that are worked out from them. Standard library only.

Every chart runs at BPM 150 in 4/4: a measure is 1600 ms, cut into 320
slots of 5 ms, so every time below is a multiple of 5 ms.
"""

import math
import os

MEASURE_MS = 1600
SLOTS = 320
SLOT_MS = MEASURE_MS // SLOTS
LEAD_MS = 2 * MEASURE_MS  # two empty measures before the first note
TRIAL_MS = 2 * MEASURE_MS
TRIALS = 16

# 1P channels
KEY = {1: "11", 2: "12", 3: "13", 4: "14", 5: "15", 6: "18", 7: "19"}
SCRATCH = "16"
MINE_KEY1 = "D1"

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "charts")


def b36(v):
    digits = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    return digits[v // 36] + digits[v % 36]


def write(name, title, events, rank=None, total=None):
    """events: (time in ms from the first measure, channel, object id)."""
    head = [
        "*---------------------- HEADER FIELD",
        "#PLAYER 1",
        "#GENRE LR2 MEASURE",
        f"#TITLE {title}",
        "#ARTIST vibeBMS tools/lr2-measure",
        "#BPM 150",
        "#PLAYLEVEL 1",
    ]
    if rank is not None:
        head.append(f"#RANK {rank}")
    if total is not None:
        head.append(f"#TOTAL {total}")
    rows = {}
    for t, ch, obj in events:
        t += LEAD_MS
        assert t % SLOT_MS == 0, (name, t)
        m, slot = divmod(t // SLOT_MS, SLOTS)
        rows.setdefault((m, ch), ["00"] * SLOTS)
        cell = rows[(m, ch)]
        assert cell[slot] == "00", (name, t, ch)
        cell[slot] = obj
    body = ["", "*---------------------- MAIN DATA FIELD"]
    for (m, ch) in sorted(rows):
        body.append(f"#{m:03d}{ch}:{''.join(rows[(m, ch)])}")
    with open(os.path.join(OUT, name), "w", newline="\r\n") as f:
        f.write("\n".join(head + body) + "\n")


def trials(per_trial):
    """`per_trial(t)` gives a trial's events from its start t."""
    return [e for k in range(TRIALS) for e in per_trial(k * TRIAL_MS)]


def a11():
    # 1: the earlier note late beyond GOOD (VERY HARD: 40), the later one
    # on the guide
    write("a11-1.bme", "A11-1 late note or the next",
          trials(lambda t: [(t, KEY[1], "01"), (t + 120, KEY[1], "01"),
                            (t + 120, KEY[7], "01")]), rank=0, total=300)
    # 2: both in GOOD (EASY: 120), the later one nearer
    write("a11-2.bme", "A11-2 lower or nearer",
          trials(lambda t: [(t, KEY[1], "01"), (t + 90, KEY[1], "01"),
                            (t + 90, KEY[7], "01")]), rank=3, total=300)
    # 3: both late beyond GOOD (VERY HARD) at the first press, a second
    # press 80 ms later, on the scratch
    write("a11-3.bme", "A11-3 both late",
          trials(lambda t: [(t, SCRATCH, "01"), (t + 80, SCRATCH, "01"),
                            (t + 160, KEY[6], "01"), (t + 240, KEY[7], "01")]),
          rank=0, total=300)


A7_NOTES = (200, 500, 1000)
A7_GROUPS = {200: 20, 500: 25, 1000: 25}


def a7():
    """Groups of 10 notes 100 ms apart, each followed by a rest to read the
    gauge; past the read groups the rest of the notes as a stream."""
    for n in A7_NOTES:
        ev = []
        read = A7_GROUPS[n]
        for g in range(read):
            ev += [(g * TRIAL_MS + i * 100, KEY[4], "01") for i in range(10)]
        start = read * TRIAL_MS
        ev += [(start + i * 100, KEY[4], "01") for i in range(n - 10 * read)]
        write(f"a7-n{n}.bme", f"A7 default TOTAL {n} notes", ev, rank=3)


def lr2_default_total(n):
    return 160 + (n + min(max(n - 400, 0), 200)) * 0.16


def shown(v):
    """LR2 shows the gauge in 2 % steps, rounded down."""
    v = min(v, 100.0)
    return int(math.floor(v / 2 + 1e-9)) * 2


def a7_table():
    print("A7: GROOVE gauge after each group of 10 notes (autoplay)")
    for n in A7_NOTES:
        t = lr2_default_total(n)
        row = [shown(20 + 10 * g * t / n) for g in range(1, A7_GROUPS[n] + 1)]
        alt = [shown(20 + 10 * g * 160 / n) for g in range(1, A7_GROUPS[n] + 1)]
        print(f"  {n} notes, vibeBMS TOTAL {t:g}: {' '.join(map(str, row))}")
        print(f"  {n} notes, TOTAL 160:        {' '.join(map(str, alt))}")


MINES = [4, 20, 50]


def a8():
    ev = [(i * 100, KEY[7], "01") for i in range(8)]
    for k, v in enumerate(MINES + [36 * 36 - 1]):
        ev.append(((k + 2) * TRIAL_MS, MINE_KEY1, b36(v)))
    write("a8-mines.bme", "A8 mines", ev, total=1000)


def a8_table():
    print("A8: GROOVE gauge after each mine, from 100 (values 4, 20, 50, ZZ)")
    for name, f in [("value %", lambda v: v), ("value / 2 %", lambda v: v / 2),
                    ("value % + 6 (S10's fit)", lambda v: v + 6)]:
        g, row = 100.0, []
        for v in MINES:
            g = max(g - f(v), 2.0)
            row.append(shown(g))
        print(f"  {name}: {' '.join(map(str, row))}")


A3_RANKS = (0, 2, 3, 4, 5, -1)
A3_LATE = (70, 110)


def a3():
    for r in A3_RANKS:
        for late in A3_LATE:
            tag = f"m{-r}" if r < 0 else str(r)
            write(f"a3-rank{tag}-{late}.bme", f"A3 RANK {r} late {late}",
                  trials(lambda t: [(t, KEY[1], "01"),
                                    (t + late, KEY[7], "01")]),
                  rank=r, total=300)


A4_NOTES = (20, 29, 30, 31, 45, 60, 90, 110, 124, 125)


def a4():
    for n in A4_NOTES:
        ev = [(i * 100, KEY[7], "01") for i in range(n - 1)]
        start = (n - 1) * 100 + 3 * MEASURE_MS
        start -= start % SLOT_MS
        ev.append((start, KEY[1], "01"))
        write(f"a4-n{n}.bme", f"A4 HARD {n} notes", ev, total=300)


def vibe_note_factor(n):
    if n >= 1000:
        return 1.0
    if n >= 500:
        return 1 + 0.002 * (1000 - n)
    if n >= 250:
        return 2 + 0.004 * (500 - n)
    if n >= 125:
        return 3 + 0.008 * (250 - n)
    if n >= 60:
        return 4 + (125 - n) / 65
    if n >= 30:
        return 5 + (60 - n) / 15
    if n > 20:
        return 8 + 0.2 * (30 - n)
    return 10.0


def doubling_factor(n):
    """S1's slopes doubled at every halving, as its "..." reads, to 10."""
    hi, f, slope = 1000.0, 1.0, 0.002
    while n < hi / 2 and f < 10:
        f += slope * hi / 2
        slope *= 2
        hi /= 2
    return min(10.0, f + slope * (hi - n)) if n < 1000 else 1.0


def a4_table():
    print("A4: HARD gauge after 3 empty POORs (each 2 x factor %)")
    for n in A4_NOTES:
        v, d = vibe_note_factor(n), doubling_factor(n)
        print(f"  {n} notes: vibeBMS factor {v:.3f} -> {shown(100 - 6 * v)}; "
              f"doubling slopes {d:.3f} -> {shown(100 - 6 * d)}")


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    a11()
    a7()
    a8()
    a3()
    a4()
    a7_table()
    a8_table()
    a4_table()
