#!/usr/bin/env python3
"""The rendering report: beatoraja and vibeBMS show the same chart
(charts/render.bme) with the same skin (beatoraja's own default skin) at
1280x720 on one X display; each scene is captured from both, put side by
side and given a similarity score. A report, not a test: nothing fails on
a low score.

    python3 render.py --dist <beatoraja folder> --java <java> --vibebms <binary>
                      --work <dir> --out <dir> [--display :78]

Both are driven with their keys (xdotool), as a player would: song select
with the chart under the cursor, the decide screen, the play (one key
pressed, every note missed: a play needs a key for beatoraja to show its
result), the result. The play is recorded at 10 frames a second; its
frames are matched by chart time: beatoraja's from the time its log says
the play began, vibeBMS's by the shift (within the recording) that makes
them look most alike, as vibeBMS logs no such time. Needs Pillow and NumPy,
ffmpeg and xdotool.
"""

import argparse
import datetime
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402

W, H = 1280, 720
FPS = 10
PLAY_TIMES = [3.0, 6.0, 9.0, 12.0, 15.0]  # s of chart time
RECORD_S = 36


class Display:
    def __init__(self, display):
        self.env = dict(os.environ, DISPLAY=display)
        self.display = display

    def x(self, *args):
        subprocess.run(["xdotool", *args], env=self.env, check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def key(self, k, hold=0.15):
        # (held a moment: both read the key state once a frame)
        self.x("keydown", k)
        time.sleep(hold)
        self.x("keyup", k)

    def focus(self, name):
        self.x("search", "--name", name, "windowfocus")

    def shot(self, path):
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab", "-video_size",
                        f"{W}x{H}", "-i", self.display, "-frames:v", "1", path],
                       env=self.env, check=False)

    def record(self, folder):
        os.makedirs(folder, exist_ok=True)
        p = subprocess.Popen(["ffmpeg", "-loglevel", "error", "-y", "-f", "x11grab",
                              "-framerate", str(FPS), "-video_size", f"{W}x{H}",
                              "-i", self.display, "-q:v", "3",
                              os.path.join(folder, "%05d.jpg")],
                             env=self.env, stdin=subprocess.PIPE)
        return p, time.time()


def stop(p):
    if p.poll() is None:
        p.send_signal(signal.SIGINT)
        try:
            p.wait(10)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()


def log_times(work, what):
    """Wall times (epoch s) of beatoraja log records whose message has `what`."""
    try:
        x = open(os.path.join(work, "beatoraja_log.xml"), encoding="utf-8").read()
    except OSError:
        return []
    out = []
    for d, m in re.findall(r"<date>(.*?)</date>.*?<message>(.*?)</message>", x, re.S):
        if what in m:
            d = re.sub(r"(\.\d{6})\d*Z$", r"\1+00:00", d)
            out.append(datetime.datetime.fromisoformat(d).timestamp())
    return out


def wait_for(cond, limit):
    t0 = time.time()
    while time.time() - t0 < limit:
        if cond():
            return True
        time.sleep(0.2)
    return False


def capture(d, out, start, focus_name, ready, navigate):
    """One app: started by `start`, ready when `ready()`, `navigate` puts
    the chart under song select's cursor. Shots and the recording in `out`."""
    os.makedirs(out, exist_ok=True)
    proc = start()
    try:
        if not wait_for(ready, 90):
            raise RuntimeError("did not start")
        time.sleep(3)
        d.focus(focus_name)
        navigate()
        time.sleep(1.5)
        d.shot(os.path.join(out, "select.png"))
        rec, rec_t0 = d.record(os.path.join(out, "frames"))
        time.sleep(0.5)
        decide_t = time.time()
        d.key("z")
        time.sleep(8)
        d.key("z")  # one key in play
        time.sleep(RECORD_S - 8.5)
        stop(rec)
        time.sleep(2)
        d.shot(os.path.join(out, "result.png"))
        return {"recording_start": rec_t0, "decide": decide_t}
    finally:
        proc.terminate()
        try:
            proc.wait(10)
        except subprocess.TimeoutExpired:
            proc.kill()


def frames(folder):
    return sorted(os.path.join(folder, f) for f in os.listdir(folder) if f.endswith(".jpg"))


def gray(path):
    img = Image.open(path).convert("L").resize((320, 180), Image.BILINEAR)
    return np.asarray(img.filter(ImageFilter.GaussianBlur(1.5)), dtype=np.float64)


def box(a, k=7):
    """Mean over k x k windows (valid part)."""
    c = np.cumsum(np.cumsum(np.pad(a, ((1, 0), (1, 0))), 0), 1)
    return (c[k:, k:] - c[:-k, k:] - c[k:, :-k] + c[:-k, :-k]) / (k * k)


def ssim(a, b):
    """Structural similarity of two grey images (1: the same), after
    shrinking them to 320x180 and blurring a little: small offsets and
    differences in text rendering cost little."""
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    ma, mb = box(a), box(b)
    va, vb = box(a * a) - ma * ma, box(b * b) - mb * mb
    cov = box(a * b) - ma * mb
    s = ((2 * ma * mb + c1) * (2 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2))
    return float(s.mean())


def side_by_side(a, b, path, label):
    ia, ib = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    out = Image.new("RGB", (W * 2 + 8, H + 28), (24, 24, 24))
    out.paste(ia.resize((W, H)), (0, 28))
    out.paste(ib.resize((W, H)), (W + 8, 28))
    dr = ImageDraw.Draw(out)
    dr.text((8, 8), f"beatoraja  |  {label}", fill=(255, 255, 255))
    dr.text((W + 16, 8), "vibeBMS", fill=(255, 255, 255))
    out.save(path)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dist", required=True)
    ap.add_argument("--java", default="java")
    ap.add_argument("--vibebms", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--display", default=":78")
    a = ap.parse_args()
    d = Display(a.display)
    work, out = os.path.abspath(a.work), os.path.abspath(a.out)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(out, exist_ok=True)
    songs = os.path.join(work, "songs")
    os.makedirs(os.path.join(songs, "render"))
    shutil.copy(os.path.join(HERE, "charts", "render.bme"), os.path.join(songs, "render"))
    skin = os.path.abspath(os.path.join(a.dist, "skin", "default"))

    # beatoraja: its default skin everywhere, the song folder scanned at start
    bw = os.path.join(work, "beatoraja")
    run.prepare(a.dist, bw)
    skins = dict(run.SKINS)
    skins[7] = "skin/default/result/result.luaskin"
    run.configure(bw, {"gauge": 2}, skins=skins)
    p = os.path.join(bw, "config_sys.json")
    s = json.load(open(p, encoding="utf-8"))
    s.update(bmsroot=[songs], updatesong=True)
    json.dump(s, open(p, "w", encoding="utf-8"), indent=1, ensure_ascii=False)

    def bja_start():
        return subprocess.Popen(run.java_cmd(a.java, ["-s"]), cwd=bw, env=d.env,
                                stdout=open(os.path.join(bw, "stdout.log"), "wb"),
                                stderr=subprocess.STDOUT)

    def bja_nav():
        # the cursor starts by NEW (the folder of charts added today): into
        # it, into TODAY, on the chart
        for k in ("Up", "z", "z"):
            d.key(k)
            time.sleep(1.5)

    t = capture(d, os.path.join(work, "shots-beatoraja"), bja_start, "^beatoraja",
                lambda: log_times(bw, "初期化時間"), bja_nav)
    plays = [x for x in log_times(bw, "STATE_PLAY") if x > t["decide"]]
    t["play"] = plays[0] if plays else None

    # vibeBMS: the same skin files, the same song folder
    vw = os.path.join(work, "vibebms")
    os.makedirs(os.path.join(vw, "config"))
    with open(os.path.join(vw, "config", "config.toml"), "w") as f:
        f.write(f"""bms_dirs = [{json.dumps(songs)}]
scan_root = false
archives = []
check_updates = false
tables = []

[skins]
"7KEYS" = {json.dumps(skin + "/play/play7.luaskin")}
SELECT = {json.dumps(skin + "/select.json")}
DECIDE = {json.dumps(skin + "/decide/decide.luaskin")}
RESULT = {json.dumps(skin + "/result/result.luaskin")}
""")
    venv = dict(d.env, VIBEBMS_ROOT=vw)
    subprocess.run([a.vibebms, "songs", "scan"], env=venv, check=True,
                   stdout=subprocess.DEVNULL)
    vlog = os.path.join(vw, "stdout.log")

    def vibe_start():
        # beatoraja's hi-speed (its default 2.56 at this chart's BPM)
        return subprocess.Popen([a.vibebms, "--hs", "2.56"], env=venv,
                                stdout=open(vlog, "wb"), stderr=subprocess.STDOUT)

    def vibe_nav():
        # the cursor starts on ALL SONGS, the song's folder 4 above; in a
        # folder the cursor is on its way back, the chart 2 below
        for k in ("Up", "Up", "Up", "Up", "z", "Down", "Down"):
            d.key(k)
            time.sleep(0.8)

    v = capture(d, os.path.join(work, "shots-vibebms"), vibe_start, "vibeBMS",
                lambda: os.path.exists(vlog) and "startup: library" in open(vlog, errors="replace").read(),
                vibe_nav)

    # pairs
    bdir, vdir = os.path.join(work, "shots-beatoraja"), os.path.join(work, "shots-vibebms")
    bf, vf = frames(os.path.join(bdir, "frames")), frames(os.path.join(vdir, "frames"))
    rows = []

    def pair(name, label, pa, pb, note=""):
        dst = os.path.join(out, f"{name}.png")
        side_by_side(pa, pb, dst, label)
        rows.append({"scene": name, "label": label, "similarity": round(ssim(gray(pa), gray(pb)), 3),
                     "image": f"{name}.png", "note": note})

    pair("1-select", "song select, the chart under the cursor",
         os.path.join(bdir, "select.png"), os.path.join(vdir, "select.png"))

    def at(fs, t0, rec0, s):
        k = int(round((t0 + s - rec0) * FPS))
        return fs[k] if 0 <= k < len(fs) else None

    db = at(bf, t["decide"], t["recording_start"], 0.7)
    dv = at(vf, v["decide"], v["recording_start"], 0.7)
    if db and dv:
        pair("2-decide", "decide, 0.7 s after the key", db, dv)
    if t["play"] and bf and vf:
        bk = [int(round((t["play"] + s - t["recording_start"]) * FPS)) for s in PLAY_TIMES]
        bk = [k for k in bk if 0 <= k < len(bf)]
        gb = {k: gray(bf[k]) for k in bk}
        gv = [gray(f) for f in vf]
        best, shift = -2.0, 0
        # vibeBMS's frame for beatoraja's frame k: k + shift (its play
        # began at another moment of its recording)
        for sh in range(-len(bf), len(vf)):
            ks = [k for k in bk if 0 <= k + sh < len(vf)]
            if len(ks) < len(bk):
                continue
            m = np.mean([ssim(gb[k], gv[k + sh]) for k in ks])
            if m > best:
                best, shift = m, sh
        for s, k in zip(PLAY_TIMES, bk):
            pair(f"3-play-{s:04.1f}s", f"play, chart time {s:.1f} s", bf[k], vf[k + shift],
                 note=f"vibeBMS frame matched at {shift / FPS:+.1f} s")
    pair("4-result", "result (every note missed but one key)",
         os.path.join(bdir, "result.png"), os.path.join(vdir, "result.png"))

    json.dump(rows, open(os.path.join(out, "report.json"), "w"), indent=1)
    md = ["## Rendering: beatoraja and vibeBMS, beatoraja's default skin", "",
          "Similarity: SSIM of the two screens shrunk to 320x180 and blurred (1 = the same). "
          "A report, not a test; the images are in the `rendering` artifact.", "",
          "| scene | similarity | image | note |", "|---|---|---|---|"]
    md += [f"| {r['label']} | {r['similarity']:.3f} | `{r['image']}` | {r['note']} |" for r in rows]
    open(os.path.join(out, "report.md"), "w").write("\n".join(md) + "\n")
    print("\n".join(md))


if __name__ == "__main__":
    main()
