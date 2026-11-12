#!/usr/bin/env python3
"""Generate a small self-made demo BMS (tones + drums) for testing vibeBMS.

usage: python3 tools/gen_demo.py <output-folder>
"""
import math, os, random, struct, sys, wave

out = sys.argv[1] if len(sys.argv) > 1 else "demo"
os.makedirs(out, exist_ok=True)
SR = 44100

def write(name, samples):
    with wave.open(os.path.join(out, name), "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", max(-32767, min(32767, int(s * 32767)))) for s in samples))

def tone(freq, dur=0.25, vol=0.35):
    n = int(SR * dur)
    return [vol * math.sin(2 * math.pi * freq * i / SR) * math.exp(-6 * i / n) for i in range(n)]

def kick():
    n = int(SR * 0.25)
    return [0.8 * math.sin(2 * math.pi * (50 + 120 * math.exp(-30 * i / SR)) * i / SR) * math.exp(-8 * i / n) for i in range(n)]

def hat():
    n = int(SR * 0.06)
    r = random.Random(1)
    return [0.25 * (r.random() * 2 - 1) * math.exp(-10 * i / n) for i in range(n)]

notes = [261.6, 293.7, 329.6, 349.2, 392.0, 440.0, 493.9, 130.8]
for i, f in enumerate(notes):
    write(f"k{i + 1}.wav", tone(f))
write("kick.wav", kick())
write("hat.wav", hat())

def b36(n):
    d = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    return d[n // 36] + d[n % 36]

lines = ["#PLAYER 1", "#GENRE DEMO", "#TITLE vibeBMS demo", "#SUBTITLE [7KEYS]", "#ARTIST vibeBMS",
         "#BPM 140", "#PLAYLEVEL 3", "#RANK 2", "#TOTAL 300", "#DIFFICULTY 2", "#LNOBJ ZZ"]
for i in range(8):
    lines.append(f"#WAV{b36(i + 1)} k{i + 1}.wav")
lines += ["#WAV0A kick.wav", "#WAV0B hat.wav", "#WAVZZ k1.wav"]
chan = {0: "11", 1: "12", 2: "13", 3: "14", 4: "15", 5: "18", 6: "19", 7: "16"}
rnd = random.Random(7)
for m in range(1, 17):
    lines.append(f"#{m:03d}01:0A000A000A000A00")
    lines.append(f"#{m:03d}01:000B000B000B000B")
    grid = [[0] * 16 for _ in range(8)]
    for s in range(0, 16, 2 if m < 9 else 1):
        lane = rnd.randrange(8)
        grid[lane][s] = lane + 1
    if m % 4 == 0:
        grid[2] = [0] * 16; grid[2][0] = 3; grid[2][8] = 36 * 36 - 1  # LN via LNOBJ
    for lane, cells in enumerate(grid):
        if any(cells):
            lines.append(f"#{m:03d}{chan[lane]}:" + "".join("00" if c == 0 else b36(c) for c in cells))
lines.append("#00903:C8")  # BPM change to 200 at measure 9
open(os.path.join(out, "demo.bme"), "w").write("\n".join(lines) + "\n")
print("wrote", out)
