#!/usr/bin/env python3
"""Generate vibeBMS's built-in default play skins.

All artwork is drawn procedurally here (shapes, gradients and text set in
Inter, SIL OFL 1.1); only generic genre layout conventions are followed
(note field at the side with turntable/keys under the judge line, gauge
below, BGA and score data beside it). The output is a set of beatoraja-format
JSON skins plus PNG atlases in crates/vibebms/assets/builtin/skins/default,
which are compiled into the executable.

    python3 tools/gen_default_skins.py [--fonts DIR]

Requires Pillow. Bold/italic Inter faces are taken from --fonts (or the
system font directory) and fall back to the bundled Inter Regular.
"""

import argparse
import json
import math
import os
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/vibebms/assets/builtin/skins/default"
BUNDLED_FONT = ROOT / "crates/vibebms/assets/fonts/Inter-Regular.otf"
FONT_DIRS = ["/usr/share/fonts/opentype/inter", "/usr/share/fonts/truetype/inter",
             "/Library/Fonts", os.path.expanduser("~/.local/share/fonts")]

# ---------------------------------------------------------------- palette
BASE = (11, 13, 20)
PANEL = (20, 24, 36)
PANEL_HI = (30, 36, 54)
EDGE = (52, 62, 92)
TEXT = (226, 232, 245)
DIM = (128, 140, 170)
TEAL = (45, 226, 199)
PINK = (255, 79, 163)
AMBER = (255, 201, 74)
RED = (255, 84, 96)
BLUE = (64, 156, 255)

NOTE_COLORS = {
    "w": ((236, 240, 250), (170, 180, 205)),
    "b": ((90, 170, 255), (36, 92, 200)),
    "s": ((255, 96, 120), (190, 36, 70)),
    # 9-button palette
    "pw": ((240, 240, 240), (170, 170, 180)),
    "py": ((255, 220, 70), (200, 150, 20)),
    "pg": ((90, 230, 120), (30, 150, 70)),
    "pb": ((80, 160, 255), (30, 80, 200)),
    "pr": ((255, 90, 90), (190, 30, 40)),
}
LANE_BG = {"w": (19, 22, 32), "b": (14, 17, 28), "s": (24, 16, 26),
           "pw": (20, 21, 28), "py": (24, 22, 16), "pg": (15, 23, 19),
           "pb": (14, 18, 30), "pr": (26, 15, 18)}

# gauge colours per type (beatoraja order) as (below border, above border)
GAUGES = [
    ((178, 128, 255), (255, 92, 150)),  # assist easy
    ((70, 220, 130), (255, 92, 150)),   # easy
    ((60, 170, 255), (255, 92, 150)),   # normal
    ((255, 80, 92), (255, 80, 92)),     # hard
    ((255, 214, 74), (255, 214, 74)),   # ex-hard
    ((255, 150, 60), (255, 150, 60)),   # hazard
    ((255, 110, 120), (255, 110, 120)), # class
    ((255, 214, 74), (255, 214, 74)),   # ex class
    ((255, 176, 32), (255, 176, 32)),   # ex-hard class
]


def find_font(name, fonts_dir=None):
    dirs = ([fonts_dir] if fonts_dir else []) + FONT_DIRS
    for d in dirs:
        p = Path(d) / name
        if p.exists():
            return str(p)
    return str(BUNDLED_FONT)


class Fonts:
    def __init__(self, d):
        self.black_it = find_font("InterDisplay-BlackItalic.otf", d)
        self.bold = find_font("InterDisplay-Bold.otf", d)
        self.semi = find_font("Inter-SemiBold.otf", d)
        self.regular = str(BUNDLED_FONT)

    def get(self, which, size):
        return ImageFont.truetype(getattr(self, which), size)


# ---------------------------------------------------------------- drawing helpers
def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def vgrad(w, h, top, bottom, alpha=255):
    img = Image.new("RGBA", (w, h))
    px = img.load()
    for y in range(h):
        c = mix(top, bottom, y / max(1, h - 1)) + (alpha,)
        for x in range(w):
            px[x, y] = c
    return img


def text_img(fonts, s, which, size, top, bottom, outline=(0, 0, 0), stroke=3, glow=None, pad=10):
    f = fonts.get(which, size)
    l, t, r, b = f.getbbox(s, stroke_width=stroke)
    w, h = r - l + pad * 2, b - t + pad * 2
    mask = Image.new("L", (w, h))
    ImageDraw.Draw(mask).text((pad - l, pad - t), s, font=f, fill=255)
    smask = Image.new("L", (w, h))
    ImageDraw.Draw(smask).text((pad - l, pad - t), s, font=f, fill=255,
                               stroke_width=stroke, stroke_fill=255)
    out = Image.new("RGBA", (w, h))
    if glow:
        g = Image.new("RGBA", (w, h), glow + (0,))
        g.putalpha(smask.filter(ImageFilter.GaussianBlur(stroke + 1)).point(lambda v: int(v * 0.5)))
        out = Image.alpha_composite(out, g)
    o = Image.new("RGBA", (w, h), outline + (255,))
    o.putalpha(smask)
    out = Image.alpha_composite(out, o)
    fill = vgrad(w, h, top, bottom)
    fill.putalpha(mask)
    return Image.alpha_composite(out, fill)


def digit_strip(fonts, cw, ch, which, size, top, bottom, glyphs="0123456789", stroke=2,
                outline=(0, 0, 0), pad_glyph=None):
    """Fixed cells: 0-9 [, padding glyph][, minus]."""
    cells = list(glyphs)
    if pad_glyph is not None:
        cells.append(pad_glyph)
    img = Image.new("RGBA", (cw * len(cells), ch))
    f = fonts.get(which, size)
    for i, g in enumerate(cells):
        if g == " ":
            continue
        dim = g == "o"  # dim zero used as padding
        ch_ = "0" if dim else g
        l, t, r, b = f.getbbox(ch_, stroke_width=stroke)
        gw, gh = r - l, b - t
        mask = Image.new("L", (cw, ch))
        smask = Image.new("L", (cw, ch))
        ox = (cw - gw) // 2 - l
        oy = (ch - gh) // 2 - t
        ImageDraw.Draw(mask).text((ox, oy), ch_, font=f, fill=255)
        ImageDraw.Draw(smask).text((ox, oy), ch_, font=f, fill=255, stroke_width=stroke,
                                   stroke_fill=255)
        cell = Image.new("RGBA", (cw, ch))
        o = Image.new("RGBA", (cw, ch), outline + (255,))
        o.putalpha(smask)
        cell = Image.alpha_composite(cell, o)
        t0, b0 = (mix(top, BASE, 0.75), mix(bottom, BASE, 0.75)) if dim else (top, bottom)
        fill = vgrad(cw, ch, t0, b0)
        fill.putalpha(mask)
        cell = Image.alpha_composite(cell, fill)
        img.paste(cell, (i * cw, 0))
    return img


def rounded(w, h, r, fill, outline=None, width=1):
    img = Image.new("RGBA", (w, h))
    ImageDraw.Draw(img).rounded_rectangle((0, 0, w - 1, h - 1), r, fill=fill, outline=outline,
                                          width=width)
    return img


def note_img(kind, w=64, h=16, style="note"):
    hi, lo = NOTE_COLORS[kind]
    img = Image.new("RGBA", (w, h))
    d = ImageDraw.Draw(img)
    if style == "body":
        body = Image.new("RGBA", (w, h))
        bd = ImageDraw.Draw(body)
        bd.rectangle((4, 0, w - 5, h - 1), fill=mix(lo, BASE, 0.35) + (210,))
        bd.rectangle((4, 0, 6, h - 1), fill=hi + (230,))
        bd.rectangle((w - 7, 0, w - 5, h - 1), fill=hi + (230,))
        return body
    if style == "active":
        body = Image.new("RGBA", (w, h))
        bd = ImageDraw.Draw(body)
        bd.rectangle((4, 0, w - 5, h - 1), fill=mix(hi, (255, 255, 255), 0.2) + (235,))
        bd.rectangle((10, 0, w - 11, h - 1), fill=mix(hi, (255, 255, 255), 0.55) + (240,))
        return body
    if style == "mine":
        d.rounded_rectangle((2, 1, w - 3, h - 2), 4, fill=(60, 10, 24, 255),
                            outline=(255, 60, 90, 255), width=2)
        for x in range(6, w - 6, 10):
            d.line((x, h - 4, x + 6, 3), fill=(255, 60, 90, 255), width=2)
        return img
    g = vgrad(w, h, hi, lo)
    m = Image.new("L", (w, h))
    ImageDraw.Draw(m).rounded_rectangle((1, 1, w - 2, h - 2), 4, fill=255)
    img.paste(g, (0, 0), m)
    d.line((5, 3, w - 6, 3), fill=(255, 255, 255, 200), width=2)
    d.rounded_rectangle((1, 1, w - 2, h - 2), 4, outline=mix(lo, (0, 0, 0), 0.4) + (255,))
    if style in ("start", "end"):
        # small notches mark LN ends
        y = h - 5 if style == "start" else 4
        d.rectangle((w // 2 - 6, y - 1, w // 2 + 5, y + 1), fill=mix(lo, (0, 0, 0), 0.5) + (255,))
    return img


def beam_img(kind, w=64, h=256):
    hi, _ = NOTE_COLORS[kind]
    img = Image.new("RGBA", (w, h))
    px = img.load()
    for y in range(h):
        t = (y / (h - 1)) ** 2.2
        for x in range(w):
            e = 1 - abs((x + 0.5) / w * 2 - 1) ** 3
            a = int(200 * t * e)
            px[x, y] = hi + (a,)
    return img


def bomb_sheet(kind, size=128, frames=8):
    hi, _ = NOTE_COLORS[kind]
    sheet = Image.new("RGBA", (size * frames, size))
    for f in range(frames):
        t = (f + 1) / frames
        cell = Image.new("RGBA", (size, size))
        d = ImageDraw.Draw(cell)
        c = size / 2
        r = 10 + t * (size / 2 - 14)
        a = int(255 * (1 - t) ** 0.7)
        d.ellipse((c - r, c - r, c + r, c + r), outline=hi + (a,), width=max(2, int(8 * (1 - t)) + 2))
        r2 = r * 0.55
        d.ellipse((c - r2, c - r2, c + r2, c + r2), fill=mix(hi, (255, 255, 255), 0.6) + (int(a * 0.5),))
        for k in range(8):
            ang = k * math.pi / 4 + t * 0.6
            r0, r1 = r * 0.6, r * 1.05
            d.line((c + math.cos(ang) * r0, c + math.sin(ang) * r0,
                    c + math.cos(ang) * r1, c + math.sin(ang) * r1), fill=(255, 255, 255, a), width=3)
        cell = cell.filter(ImageFilter.GaussianBlur(1.2))
        sheet.paste(cell, (f * size, 0))
    return sheet


def key_img(kind, w, h, pressed):
    hi, lo = NOTE_COLORS[kind]
    base = (210, 214, 226) if kind in ("w", "pw") else mix(lo, BASE, 0.25)
    if kind.startswith("p") and kind != "pw":
        base = mix(hi, BASE, 0.15)
    if pressed:
        base = mix(base, (255, 255, 255), 0.45)
    img = Image.new("RGBA", (w, h))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, w - 1, h - 1), 6, fill=mix(base, BASE, 0.55) + (255,))
    g = vgrad(w - 6, h - 8, mix(base, (255, 255, 255), 0.25), base)
    m = Image.new("L", (w - 6, h - 8))
    ImageDraw.Draw(m).rounded_rectangle((0, 0, w - 7, h - 9), 5, fill=255)
    img.paste(g, (3, 3 if pressed else 2), m)
    if pressed:
        d.rounded_rectangle((0, 0, w - 1, h - 1), 6, outline=hi + (255,), width=2)
    return img


def turntable_img(size=128):
    img = Image.new("RGBA", (size, size))
    d = ImageDraw.Draw(img)
    c = size / 2
    d.ellipse((2, 2, size - 3, size - 3), fill=(26, 28, 38, 255), outline=EDGE + (255,), width=3)
    for i in range(1, 6):
        r = c - 6 - i * 7
        d.ellipse((c - r, c - r, c + r, c + r), outline=(40, 44, 60, 255), width=1)
    d.ellipse((c - 14, c - 14, c + 14, c + 14), fill=(50, 56, 78, 255), outline=TEAL + (255,), width=2)
    # marker so the rotation is visible
    d.rounded_rectangle((c - 3, 8, c + 3, 30), 3, fill=PINK + (255,))
    return img


def ring_img(size=128):
    img = Image.new("RGBA", (size, size))
    d = ImageDraw.Draw(img)
    d.ellipse((2, 2, size - 3, size - 3), outline=NOTE_COLORS["s"][0] + (255,), width=5)
    return img.filter(ImageFilter.GaussianBlur(1.5))


# ---------------------------------------------------------------- atlas
class Atlas:
    def __init__(self, name, width=2048):
        self.name = name
        self.width = width
        self.items = {}
        self.pending = []

    def add(self, key, img):
        self.pending.append((key, img))
        return key

    def build(self):
        x = y = row_h = 0
        placed = []
        for key, img in sorted(self.pending, key=lambda p: -p[1].size[1]):
            w, h = img.size
            if x + w > self.width:
                x, y, row_h = 0, y + row_h + 2, 0
            placed.append((key, img, x, y))
            self.items[key] = (x, y, w, h)
            x += w + 2
            row_h = max(row_h, h)
        self.height = y + row_h
        sheet = Image.new("RGBA", (self.width, self.height))
        for key, img, px, py in placed:
            sheet.paste(img, (px, py))
        return sheet


# ---------------------------------------------------------------- assets
def build_parts(fonts):
    a = Atlas("parts.png")
    for k in NOTE_COLORS:
        a.add(f"note_{k}", note_img(k))
        a.add(f"lns_{k}", note_img(k, style="start"))
        a.add(f"lne_{k}", note_img(k, style="end"))
        a.add(f"lnb_{k}", note_img(k, h=32, style="body"))
        a.add(f"lna_{k}", note_img(k, h=32, style="active"))
        a.add(f"key_{k}", key_img(k, 48, 60, False))
        a.add(f"keyp_{k}", key_img(k, 48, 60, True))
        a.add(f"lane_{k}", Image.new("RGBA", (16, 16), LANE_BG[k] + (255,)))
        a.add(f"lanea_{k}", Image.new("RGBA", (16, 16), LANE_BG[k] + (200,)))
    a.add("mine", note_img("s", style="mine"))
    # white sprites, tinted per lane by the destination colour
    a.add("beam", beam_img("pw", 32, 256))
    a.add("bomb", bomb_sheet("pw", 112))
    a.add("hold", bomb_sheet("pw", 80, 4))
    a.add("tt", turntable_img())
    a.add("tt_ring", ring_img())
    # solid fills are padded and sampled inset (see Skin.img) so that
    # stretching them does not bleed in neighbouring atlas pixels
    a.add("white", Image.new("RGBA", (16, 16), (255, 255, 255, 255)))
    a.add("sep", Image.new("RGBA", (16, 16), (60, 70, 100, 255)))
    a.add("barline", Image.new("RGBA", (16, 16), (150, 160, 190, 150)))
    jl = vgrad(64, 8, (255, 140, 160), (230, 40, 80))
    a.add("judgeline", jl)
    # lane cover
    cov = vgrad(64, 256, (30, 34, 50), (18, 20, 30))
    d = ImageDraw.Draw(cov)
    for y in range(0, 256, 16):
        d.line((0, y, 64, y + 8), fill=(36, 42, 62, 255), width=2)
    d.rectangle((0, 250, 63, 255), fill=TEAL + (255,))
    a.add("cover", cov)
    # progress
    a.add("prog_knob", rounded(12, 24, 4, TEAL + (255,)))
    # gauge cells: 9 types x (lit above, lit below, unlit above, unlit below)
    for ti, (below, above) in enumerate(GAUGES):
        for ci, (lit, col) in enumerate([(1, above), (1, below), (0, above), (0, below)]):
            c = col if lit else mix(col, BASE, 0.8)
            cell = Image.new("RGBA", (8, 24))
            cd = ImageDraw.Draw(cell)
            cd.rectangle((0, 0, 5, 23), fill=c + (255,))
            if lit:
                cd.rectangle((0, 0, 5, 3), fill=mix(c, (255, 255, 255), 0.5) + (255,))
            a.add(f"g{ti}_{ci}", cell)
    # judgements (PERFECT flashes between two frames)
    judges = [
        ("PERFECT", (230, 255, 255), (60, 220, 255), (0, 40, 80), TEAL),
        ("PERFECT", (255, 255, 255), (255, 140, 220), (60, 0, 50), PINK),
        ("GREAT", (255, 245, 170), (255, 190, 40), (70, 40, 0), AMBER),
        ("GOOD", (190, 255, 200), (60, 210, 110), (0, 50, 20), None),
        ("BAD", (230, 200, 255), (160, 100, 255), (30, 0, 60), None),
        ("POOR", (255, 190, 190), (255, 70, 90), (60, 0, 10), None),
    ]
    imgs = [text_img(fonts, s, "black_it", 50, t, b, o, 3, g) for s, t, b, o, g in judges]
    jw = max(i.size[0] for i in imgs)
    jh = max(i.size[1] for i in imgs)

    def fit(i):
        c = Image.new("RGBA", (jw, jh))
        c.paste(i, ((jw - i.size[0]) // 2, (jh - i.size[1]) // 2))
        return c

    pg = Image.new("RGBA", (jw * 2, jh))
    pg.paste(fit(imgs[0]), (0, 0))
    pg.paste(fit(imgs[1]), (jw, 0))
    a.add("j_pg", pg)
    for key, im in zip(["j_gr", "j_gd", "j_bd", "j_pr"], imgs[2:]):
        a.add(key, fit(im))
    a.add("fast", text_img(fonts, "FAST", "black_it", 20, (200, 240, 255), BLUE, (0, 20, 50), 2))
    a.add("slow", text_img(fonts, "SLOW", "black_it", 20, (255, 220, 200), RED, (50, 0, 10), 2))
    a.add("ready", text_img(fonts, "READY", "black_it", 64, (255, 255, 255), TEAL, (0, 30, 40), 4, TEAL))
    # number fonts
    a.add("n_combo_pg", digit_strip(fonts, 36, 60, "black_it", 50, (230, 255, 255), (60, 220, 255), stroke=3))
    a.add("n_combo_gr", digit_strip(fonts, 36, 60, "black_it", 50, (255, 245, 170), (255, 190, 40), stroke=3))
    a.add("n_combo_gd", digit_strip(fonts, 36, 60, "black_it", 50, (190, 255, 200), (60, 210, 110), stroke=3))
    a.add("n_big", digit_strip(fonts, 26, 38, "bold", 34, (255, 255, 255), (160, 200, 255), pad_glyph="o"))
    a.add("n_mid", digit_strip(fonts, 18, 28, "bold", 24, (255, 255, 255), (180, 200, 230), pad_glyph="o"))
    a.add("n_small", digit_strip(fonts, 12, 20, "semi", 17, TEXT, (170, 180, 205), stroke=1))
    a.add("n_small_pad", digit_strip(fonts, 12, 20, "semi", 17, TEXT, (170, 180, 205), stroke=1, pad_glyph="0"))
    # beatoraja signed numbers: 0-9, padding, sign; positive set then negative set
    pos = digit_strip(fonts, 12, 20, "semi", 17, (200, 230, 255), (120, 180, 255), stroke=1,
                      glyphs="0123456789 +")
    neg = digit_strip(fonts, 12, 20, "semi", 17, (255, 210, 210), (255, 110, 120), stroke=1,
                      glyphs="0123456789 -")
    sgn = Image.new("RGBA", (pos.size[0] * 2, pos.size[1]))
    sgn.paste(pos, (0, 0))
    sgn.paste(neg, (pos.size[0], 0))
    a.add("n_signed", sgn)
    a.add("n_teal", digit_strip(fonts, 18, 28, "bold", 24, (220, 255, 250), TEAL))
    a.add("n_bpm", digit_strip(fonts, 34, 50, "black_it", 44, (255, 255, 255), AMBER, stroke=2))
    a.add("n_gauge", digit_strip(fonts, 18, 28, "bold", 24, (255, 255, 255), (255, 170, 190)))
    a.add("n_green", digit_strip(fonts, 14, 22, "bold", 19, (200, 255, 200), (70, 220, 120), stroke=1))
    return a


def draw_label(img, fonts, x, y, s, size=13, fill=DIM, which="semi", anchor="la"):
    ImageDraw.Draw(img).text((x, y), s, font=fonts.get(which, size), fill=fill, anchor=anchor)


def panel(img, r, title=None, fonts=None, accent=TEAL):
    x, y, w, h = r
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((x, y, x + w - 1, y + h - 1), 8, fill=PANEL + (235,), outline=EDGE + (255,))
    d.line((x + 10, y, x + 60, y), fill=accent + (255,), width=2)
    if title:
        draw_label(img, fonts, x + 12, y + 7, title, 12)


def background(w, h):
    img = vgrad(w, h, (16, 19, 30), BASE)
    d = ImageDraw.Draw(img)
    for x in range(-h, w, 28):
        d.line((x, h, x + h, 0), fill=(18, 21, 32, 255), width=1)
    return img


# ---------------------------------------------------------------- skin json
class Skin:
    def __init__(self, kind, name, w, h, parts):
        self.w, self.h = w, h
        self.parts = parts
        self.j = {
            "type": kind, "name": name, "author": "vibeBMS", "w": w, "h": h,
            "input": 500, "scene": 3600000, "close": 1500, "fadeout": 1000,
            "source": [{"id": "parts", "path": "parts.png"}],
            "image": [], "value": [], "text": [], "slider": [], "graph": [],
            "judge": [], "destination": [],
        }
        # x remapping for layouts that move whole blocks (2P side)
        self.xmap = None
        self._ids = set()

    def source(self, sid, path):
        self.j["source"].append({"id": sid, "path": path})

    def img(self, key, divx=1, cycle=0, timer=0, sid=None):
        iid = key if not timer else f"{key}@{timer}"
        if iid in self._ids:
            return iid
        self._ids.add(iid)
        if sid:
            d = {"id": iid, "src": sid, "x": 0, "y": 0, "w": -1, "h": -1}
        else:
            x, y, w, h = self.parts.items[key]
            if key in ("white", "sep", "barline") or key.startswith("lane"):
                x, y, w, h = x + 4, y + 4, w - 8, h - 8
            d = {"id": iid, "src": "parts", "x": x, "y": y, "w": w, "h": h}
        if divx > 1:
            d["divx"] = divx
        if cycle:
            d["cycle"] = cycle
        if timer:
            d["timer"] = timer
        self.j["image"].append(d)
        return iid

    def num(self, key, ref, digit, align=0, zeropad=False):
        x, y, w, h = self.parts.items[key]
        n = {"n_small_pad": 11, "n_big": 11, "n_mid": 11, "n_signed": 24}.get(key, 10)
        iid = f"{key}#{ref}#{digit}#{int(zeropad)}"
        if iid not in self._ids:
            self._ids.add(iid)
            v = {"id": iid, "src": "parts", "x": x, "y": y, "w": w, "h": h, "divx": n,
                 "digit": digit, "ref": ref, "align": align}
            if zeropad:
                v["zeropadding"] = 1
            self.j["value"].append(v)
        return iid, w // n, h

    def text(self, ref, align=0):
        iid = f"text{ref}_{align}"
        if iid not in self._ids:
            self._ids.add(iid)
            self.j["text"].append({"id": iid, "ref": ref, "align": align})
        return iid

    def key(self, x, y, w, h, **kw):
        """y-down rect -> beatoraja (y-up) keyframe."""
        if self.xmap and not kw.pop("fixed", False):
            x = self.xmap(x)
        kw.pop("fixed", None)
        x, y, w, h = (int(round(v)) for v in (x, y, w, h))
        k = {"x": x, "y": self.h - (y + h), "w": w, "h": h}
        k.update(kw)
        return k

    def put(self, iid, x, y, w, h, timer=0, loop=0, op=None, blend=0, center=0, keys=None,
            fixed=False, **kw):
        d = {"id": iid}
        if timer:
            d["timer"] = timer
        if loop:
            d["loop"] = loop
        if op:
            d["op"] = op
        if blend:
            d["blend"] = blend
        if center:
            d["center"] = center
        if keys:
            d["dst"] = [self.key(*(k[:4]), fixed=fixed, **k[4]) for k in keys]
        else:
            d["dst"] = [self.key(x, y, w, h, fixed=fixed, time=0, **kw)]
        self.j["destination"].append(d)
        return d

    def number(self, key, ref, digit, x, y, align=0, zeropad=False, color=None, scale=1.0, **kw):
        iid, cw, ch = self.num(key, ref, digit, align, zeropad)
        cw, ch = int(cw * scale), int(ch * scale)
        extra = {}
        if color:
            extra = {"r": color[0], "g": color[1], "b": color[2]}
        self.put(iid, x, y, cw, ch, **extra, **kw)
        return cw * digit

    def label_text(self, ref, x, y, w, size, align=0, color=TEXT):
        self.put(self.text(ref, align), x, y, w, size, r=color[0], g=color[1], b=color[2])

    def save(self, path):
        with open(path, "w") as f:
            json.dump(self.j, f, separators=(",", ":"))


# ---------------------------------------------------------------- layouts
def lane_kinds(mode):
    if mode == "7":
        return ["w", "b", "w", "b", "w", "b", "w"], True
    if mode == "5":
        return ["w", "b", "w", "b", "w"], True
    return ["pw", "py", "pg", "pb", "pr", "pb", "pg", "py", "pw"], False


def lanes(mode, fx, side, scratch_w, key_w):
    """Our lane order (keys then scratch) -> list of (x, w, kind)."""
    kinds, scratch = lane_kinds(mode)
    out = []
    x = fx
    if scratch and side == 0:
        sx = x
        x += scratch_w
    for k in kinds:
        out.append((x, key_w, k))
        x += key_w
    if scratch:
        if side == 1:
            sx = x
            x += scratch_w
        out.append((sx, scratch_w, "s"))
    return out, x - fx


LR2_KEY = {0: 1}


def timer_ids(mode, li, player):
    """(key on, key off, bomb, hold) LR2 timers for our lane index within a player."""
    kinds, scratch = lane_kinds(mode)
    n = len(kinds)
    k = 0 if (scratch and li == n) else li + 1
    base = 10 * player
    return 100 + base + k, 120 + base + k, 50 + base + k, 70 + base + k


def tint(kind):
    c = mix(NOTE_COLORS[kind][0], (255, 255, 255), 0.2)
    return {"r": c[0], "g": c[1], "b": c[2]}


def note_def(sk, all_lanes):
    kinds = [k for (_, _, k, _) in all_lanes]
    sk.j["note"] = {
        "id": "notes",
        "note": [sk.img(f"note_{k}") for k in kinds],
        "lnstart": [sk.img(f"lns_{k}") for k in kinds],
        "lnend": [sk.img(f"lne_{k}") for k in kinds],
        "lnbody": [sk.img(f"lnb_{k}") for k in kinds],
        "lnactive": [sk.img(f"lna_{k}") for k in kinds],
        "mine": [sk.img("mine") for _ in kinds],
        "dst": [],
        "group": [],
    }


def field(sk, mode, player, side, fx, top, lane_h, scratch_w, key_w, keys_y, keys_h,
          judge_at, translucent=False):
    """Note field, keys, beams, bombs and judgement for one player.

    Returns the lanes as (x, w, kind, lane index within player)."""
    ls, fw = lanes(mode, fx, side, scratch_w, key_w)
    judge_y = top + lane_h
    lanes_out = [(x, w, k, i) for i, (x, w, k) in enumerate(ls)]
    for x, w, k, _ in lanes_out:
        sk.put(sk.img(f"lanea_{k}" if translucent else f"lane_{k}"), x, top, w, lane_h, fixed=True)
    for x, w, k, _ in sorted(lanes_out)[1:]:
        sk.put(sk.img("sep"), x - 1, top, 2, lane_h, fixed=True, a=160)
    sk.put(sk.img("sep"), fx - 2, top, 2, lane_h + keys_h + 8, fixed=True)
    sk.put(sk.img("sep"), fx + fw, top, 2, lane_h + keys_h + 8, fixed=True)
    # key beams (behind notes)
    for x, w, k, li in lanes_out:
        on, off, _, _ = timer_ids(mode, li, player)
        b = sk.img("beam")
        t = tint(k)
        bh = int(lane_h * 0.55)
        sk.put(b, x, judge_y - bh, w, bh, timer=on, blend=2, fixed=True, a=210, **t)
        sk.put(b, 0, 0, 0, 0, timer=off, loop=-1, blend=2, fixed=True, keys=[
            (x, judge_y - bh, w, bh, dict(time=0, a=210, **t)),
            (x + w // 2 - 1, judge_y - bh, 2, bh, dict(time=120, a=0, **t)),
        ])
    # notes
    for x, w, k, li in lanes_out:
        sk.j["note"]["dst"].append(sk.key(x, top, w, lane_h, fixed=True))
    if player == 0:
        sk.j["note"]["group"] = [{"id": sk.img("barline"),
                                  "dst": [sk.key(fx, top, fw, lane_h, fixed=True, time=0)]}]
    sk.put("notes", fx, top, fw, lane_h, fixed=True) if player == 0 else None
    # lane cover (SUDDEN+): slides down from above the field
    cov = f"cover{player}"
    if cov not in sk._ids:
        sk._ids.add(cov)
        x, y, w, h = sk.parts.items["cover"]
        sk.j["slider"].append({"id": cov, "src": "parts", "x": x, "y": y, "w": w, "h": h,
                               "angle": 2, "range": lane_h, "type": 4})
    sk.put(cov, fx, top - lane_h, fw, lane_h, op=[271], fixed=True)
    # judge line
    sk.put(sk.img("judgeline"), fx, judge_y - 4, fw, 8, fixed=True)
    # bombs + hold glow
    for x, w, k, li in lanes_out:
        _, _, bomb, hold = timer_ids(mode, li, player)
        cx = x + w // 2
        t = tint(k)
        sk.put(sk.img("bomb", divx=8, cycle=240, timer=bomb), 0, 0, 0, 0, timer=bomb,
               loop=-1, blend=2, fixed=True, keys=[
                   (cx - 70, judge_y - 70, 140, 140, dict(time=0, **t)),
                   (cx - 70, judge_y - 70, 140, 140, dict(time=240, **t)),
               ])
        sk.put(sk.img("hold", divx=4, cycle=200), cx - 40, judge_y - 40, 80, 80, timer=hold,
               blend=2, fixed=True, a=200, **t)
    # keys / turntable
    for x, w, k, li in lanes_out:
        on, _, _, _ = timer_ids(mode, li, player)
        if k == "s":
            s = min(w, keys_h) - 4
            tx, ty = x + (w - s) // 2, keys_y + (keys_h - s) // 2
            sk.put(sk.img("tt"), 0, 0, 0, 0, center=5, fixed=True, keys=[
                (tx, ty, s, s, {"time": 0, "angle": 0}),
                (tx, ty, s, s, {"time": 3000, "angle": -360 if side == 0 else 360}),
            ])
            sk.put(sk.img("tt_ring"), tx - 2, ty - 2, s + 4, s + 4, timer=on, blend=2, fixed=True)
            continue
        raised = k in ("b",) or (mode == "9" and False)
        kh = keys_h - 14
        ky = keys_y + (0 if raised else 14)
        sk.put(sk.img(f"key_{k}"), x + 2, ky, w - 4, kh, fixed=True)
        sk.put(sk.img(f"keyp_{k}"), x + 2, ky, w - 4, kh, timer=on, fixed=True)
    # judgement + combo
    jx, jy, jw, jh = sk.parts.items["j_gr"]
    jcx = fx + fw // 2
    jtop = top + int(lane_h * judge_at)
    timer = 46 if player == 0 else 47
    imgs = []
    for key in ["j_pg", "j_gr", "j_gd", "j_bd", "j_pr", "j_pr"]:
        iid = sk.img(key, divx=2, cycle=80) if key == "j_pg" else sk.img(key)
        imgs.append({"id": iid, "timer": timer, "loop": -1, "offset": 0, "dst": [
            sk.key(jcx - jw // 2, jtop + 10, jw, jh, fixed=True, time=0, a=120),
            sk.key(jcx - jw // 2, jtop, jw, jh, fixed=True, time=50, a=255),
            sk.key(jcx - jw // 2, jtop, jw, jh, fixed=True, time=700),
        ]})
    nums = []
    for key in ["n_combo_pg", "n_combo_gr", "n_combo_gd", "n_combo_gd", "n_combo_gd", "n_combo_gd"]:
        iid, cw, ch = sk.num(key, 75, 4)
        # relative to the judge image: right of it, bottom aligned (y-up offsets)
        nums.append({"id": iid, "timer": timer, "loop": -1, "dst": [
            {"time": 0, "x": jw, "y": (jh - ch) // 2, "w": cw, "h": ch},
            {"time": 700, "x": jw, "y": (jh - ch) // 2, "w": cw, "h": ch},
        ]})
    jid = f"judge{player}"
    sk.j["judge"].append({"id": jid, "index": player, "images": imgs, "numbers": nums,
                          "shift": True})
    sk.put(jid, jcx - jw // 2, jtop, jw, jh, fixed=True)
    # FAST / SLOW above the judgement (not on PERFECT)
    for key, op in (("fast", 1242), ("slow", 1243)):
        x, y, w, h = sk.parts.items[key]
        sk.put(sk.img(key), jcx - w // 2, jtop - h - 2, w, h, timer=timer, loop=-1, op=[op, -241],
               fixed=True, keys=[
                   (jcx - w // 2, jtop - h - 2, w, h, {"time": 0}),
                   (jcx - w // 2, jtop - h - 2, w, h, {"time": 500}),
               ])
    return lanes_out, fw


def ready(sk, cx, cy):
    x, y, w, h = sk.parts.items["ready"]
    sk.put(sk.img("ready"), 0, 0, 0, 0, timer=40, loop=-1, fixed=True, keys=[
        (cx - w // 2, cy - h // 2, w, h, {"time": 0, "a": 0}),
        (cx - w // 2, cy - h // 2, w, h, {"time": 200, "a": 255}),
        (cx - w // 2, cy - h // 2, w, h, {"time": 900, "a": 255}),
        (cx - w // 2 - 20, cy - h // 2, w + 40, h, {"time": 1100, "a": 0}),
    ])


def gauge(sk, x, y, w, h, fixed=False):
    nodes = [sk.img(f"g{t}_{c}") for t in range(9) for c in range(4)]
    sk.j["gauge"] = {"id": "gauge", "nodes": nodes, "parts": 50}
    sk.put("gauge", x, y, w, h, fixed=fixed)


def progress(sk, x, y, h, fixed=False):
    if "prog" not in sk._ids:
        sk._ids.add("prog")
        px, py, pw, ph = sk.parts.items["prog_knob"]
        sk.j["slider"].append({"id": "prog", "src": "parts", "x": px, "y": py, "w": pw, "h": ph,
                               "angle": 2, "range": h - ph, "type": 6})
    sk.put("prog", x, y, 12, 24, fixed=fixed)


# --- landscape single play (1P on the left; 2P mirrors) ----------------------
LAND_SP = {"7": (78, 46), "5": (80, 64), "9": (0, 44)}


def bg_land_sp(fonts, side):
    img = bg_land_1p(fonts)
    if not side:
        return img
    out = img.copy()
    out.paste(img.crop((460, 0, 1280, 720)), (0, 0))
    out.paste(img.crop((0, 0, 460, 720)), (820, 0))
    return out


def bg_land_1p(fonts):
    W, H = 1280, 720
    img = background(W, H)
    flip = lambda r: r
    d = ImageDraw.Draw(img)
    # field frame
    fr = flip((32, 0, 416, 720))
    d.rectangle((fr[0], 0, fr[0] + fr[2], H), fill=(8, 9, 14, 255))
    tr = flip((14, 0, 16, 540))
    d.rounded_rectangle((tr[0] + 4, 4, tr[0] + 12, 536), 3, fill=(24, 28, 42, 255))
    panel(img, flip((40, 636, 400, 36)), None)
    panel(img, flip((40, 676, 400, 38)), None)
    draw_label(img, fonts, flip((48, 0, 60, 0))[0], 683, "DURATION", 12)
    draw_label(img, fonts, flip((260, 0, 60, 0))[0], 683, "LEVEL", 12)
    # header
    panel(img, flip((468, 8, 796, 86)), None, accent=PINK)
    # BGA frame
    b = flip((476, 100, 456, 456))
    d.rounded_rectangle((b[0], b[1], b[0] + b[2], b[1] + b[3]), 6, fill=(0, 0, 0, 255), outline=EDGE + (255,))
    # score
    c = lambda x, y, w, h: flip((x, y, w, h))
    panel(img, c(944, 100, 320, 140), "SCORE", fonts)
    for i, s in enumerate(["EX SCORE", "MAX COMBO", "RATE"]):
        draw_label(img, fonts, c(956, 0, 1, 0)[0], 128 + i * 38, s, 13, TEXT)
    draw_label(img, fonts, c(1240, 0, 1, 0)[0], 206, "%", 16, TEXT)
    panel(img, c(944, 248, 320, 64), "PERSONAL BEST", fonts, accent=AMBER)
    draw_label(img, fonts, c(956, 0, 1, 0)[0], 276, "BEST", 13, TEXT)
    draw_label(img, fonts, c(1100, 0, 1, 0)[0], 276, "DIFF", 13, TEXT)
    panel(img, c(944, 320, 320, 236), "SCORE GRAPH", fonts)
    gx, gy, gh = c(960, 0, 200, 0)[0], 344, 200
    for name, frac in (("AAA", 8 / 9), ("AA", 7 / 9), ("A", 6 / 9)):
        y = gy + gh - int(gh * frac)
        d.line((gx, y, gx + 200, y), fill=(70, 80, 115, 255), width=1)
        draw_label(img, fonts, gx + 206, y - 8, name, 12)
    d.line((gx, gy + gh, gx + 200, gy + gh), fill=EDGE + (255,), width=1)
    draw_label(img, fonts, gx + 30, gy + gh + 4, "NOW", 11)
    draw_label(img, fonts, gx + 110, gy + gh + 4, "TOTAL", 11)
    panel(img, c(476, 564, 236, 150), "JUDGE", fonts)
    for i, s in enumerate(["PERFECT", "GREAT", "GOOD", "BAD", "POOR"]):
        draw_label(img, fonts, c(488, 0, 1, 0)[0], 584 + i * 20, s, 13, TEXT)
    draw_label(img, fonts, c(600, 0, 1, 0)[0], 584, "FAST", 12, BLUE)
    draw_label(img, fonts, c(600, 0, 1, 0)[0], 624, "SLOW", 12, RED)
    panel(img, c(720, 564, 212, 150), "BPM", fonts, accent=AMBER)
    draw_label(img, fonts, c(736, 0, 1, 0)[0], 682, "MIN", 11)
    draw_label(img, fonts, c(856, 0, 1, 0)[0], 682, "MAX", 11)
    panel(img, c(944, 564, 320, 150), "TIME LEFT", fonts)
    draw_label(img, fonts, c(1068, 0, 1, 0)[0], 588, ":", 30, TEXT, "bold")
    draw_label(img, fonts, c(956, 0, 1, 0)[0], 650, "FPS", 11)
    return img


def land_sp(fonts, parts, mode, side):
    kind = {"7": 0, "5": 1, "9": 4}[mode]
    sk = Skin(kind, f"vibeBMS Default {mode}K{' 2P' if side else ''}", 1280, 720, parts)
    bg = f"bg_land_{'2p' if side else '1p'}.png"
    sk.source("bg", bg)
    sk.put(sk.img("bg", sid="bg"), 0, 0, 1280, 720, fixed=True)
    if side:
        sk.xmap = lambda x: x - 460 if x >= 460 else x + 820
    # BGA + stage file when the chart has no BGA
    sk.j["bga"] = {"id": "bga"}
    sk.put("bga", 480, 104, 448, 448)
    lanes_all = []
    sw, kw = LAND_SP[mode]
    kinds, scratch = lane_kinds(mode)
    fw = (sw if scratch else 0) + kw * len(kinds)
    fx = 40 + (400 - fw) // 2 + (820 if side else 0)
    ls, _ = lanes(mode, fx, side, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    field(sk, mode, 0, side, fx, 0, 540, sw, kw, 548, 80, 0.62)
    progress(sk, 16, 0, 540)
    gauge(sk, 48, 642, 300, 24)
    sk.number("n_gauge", 107, 3, 352, 640)
    sk.put(sk.img("white"), 406, 662, 4, 4)
    sk.number("n_gauge", 407, 1, 412, 640)
    sk.number("n_green", 312, 4, 140, 684)
    sk.number("n_green", 96, 2, 320, 684, color=TEXT)
    # header texts
    sk.label_text(13, 484, 14, 760, 14, color=DIM)
    sk.label_text(12, 484, 34, 760, 30)
    sk.label_text(16, 484, 70, 760, 16, color=DIM)
    # score panel
    sk.number("n_big", 71, 5, 1120, 118)
    sk.number("n_mid", 75, 5, 1150, 156)
    sk.number("n_mid", 102, 3, 1150, 194)
    sk.put(sk.img("white"), 1205, 216, 4, 4)
    sk.number("n_mid", 103, 2, 1210, 194, zeropad=True)
    sk.number("n_small", 150, 5, 1000, 274)
    sk.number("n_signed", 152, 5, 1150, 274)
    # graph
    for i, (ref, col) in enumerate(((110, TEAL), (111, PINK))):
        gid = f"graph{ref}"
        sk._ids.add(gid)
        x, y, w, h = parts.items["white"]
        x, y, w, h = x + 4, y + 4, w - 8, h - 8
        sk.j["graph"].append({"id": gid, "src": "parts", "x": x, "y": y, "w": w, "h": h,
                              "angle": 1, "type": ref})
        sk.put(gid, 976 + i * 80, 344, 56, 200, r=col[0], g=col[1], b=col[2], a=230)
    # judge counts
    for i, ref in enumerate((110, 111, 112, 113, 114)):
        sk.number("n_small", ref, 4, 544, 582 + i * 20)
    sk.number("n_small", 423, 4, 650, 600, color=(160, 200, 255))
    sk.number("n_small", 424, 4, 650, 640, color=(255, 170, 170))
    # bpm
    sk.number("n_bpm", 160, 3, 774, 596)
    sk.number("n_small", 91, 3, 768, 680)
    sk.number("n_small", 90, 3, 888, 680)
    # time
    sk.number("n_big", 163, 2, 1012, 590)
    sk.number("n_big", 164, 2, 1084, 590, zeropad=True)
    sk.number("n_small", 20, 4, 990, 648, color=DIM)
    ready(sk, fx + fw // 2, 270)
    return sk, bg


# --- landscape double play --------------------------------------------------
LAND_DP = {"7": (62, 44), "5": (66, 60)}


def bg_land_dp(fonts):
    W, H = 1280, 720
    img = background(W, H)
    d = ImageDraw.Draw(img)
    for x in (12, 868):
        d.rectangle((x, 0, x + 400, H), fill=(8, 9, 14, 255))
    panel(img, (420, 6, 440, 74), None, accent=PINK)
    d.rounded_rectangle((470, 86, 810, 396), 6, fill=(0, 0, 0, 255), outline=EDGE + (255,))
    panel(img, (420, 404, 440, 96), "SCORE", fonts)
    draw_label(img, fonts, 434, 428, "EX SCORE", 13, TEXT)
    draw_label(img, fonts, 434, 462, "MAX COMBO", 13, TEXT)
    draw_label(img, fonts, 650, 428, "RATE", 13, TEXT)
    draw_label(img, fonts, 840, 430, "%", 14, TEXT)
    draw_label(img, fonts, 650, 462, "BEST", 13, TEXT)
    panel(img, (420, 508, 440, 62), "GAUGE", fonts)
    panel(img, (420, 578, 216, 136), "JUDGE", fonts)
    for i, s in enumerate(["PERFECT", "GREAT", "GOOD", "BAD", "POOR"]):
        draw_label(img, fonts, 432, 598 + i * 22, s, 13, TEXT)
    panel(img, (644, 578, 216, 136), "BPM", fonts, accent=AMBER)
    draw_label(img, fonts, 656, 668, "MIN", 11)
    draw_label(img, fonts, 760, 668, "MAX", 11)
    draw_label(img, fonts, 656, 690, "TIME", 11)
    draw_label(img, fonts, 760, 690, "LV", 11)
    draw_label(img, fonts, 715, 688, ":", 16, TEXT)
    return img


def land_dp(fonts, parts, mode):
    kind = {"7": 2, "5": 3}[mode]
    sk = Skin(kind, f"vibeBMS Default {'14' if mode == '7' else '10'}K", 1280, 720, parts)
    sk.source("bg", "bg_land_dp.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, 1280, 720)
    sk.j["bga"] = {"id": "bga"}
    sk.put("bga", 474, 90, 332, 302)
    sw, kw = LAND_DP[mode]
    kinds, _ = lane_kinds(mode)
    fw = sw + kw * len(kinds)
    f1 = 16 + (392 - fw) // 2
    f2 = 1280 - f1 - fw
    l1, _ = lanes(mode, f1, 0, sw, kw)
    l2, _ = lanes(mode, f2, 1, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(l1 + l2)])
    field(sk, mode, 0, 0, f1, 0, 548, sw, kw, 556, 76, 0.62)
    field(sk, mode, 1, 1, f2, 0, 548, sw, kw, 556, 76, 0.62)
    # the second field() call skipped the "notes" destination; one covers both
    gauge(sk, 440, 530, 300, 24)
    sk.number("n_gauge", 107, 3, 752, 528)
    sk.put(sk.img("white"), 806, 550, 4, 4)
    sk.number("n_gauge", 407, 1, 812, 528)
    sk.label_text(12, 430, 12, 420, 28, align=1)
    sk.label_text(16, 430, 48, 420, 16, align=1, color=DIM)
    sk.number("n_mid", 71, 5, 540, 424)
    sk.number("n_mid", 75, 5, 540, 458)
    sk.number("n_mid", 102, 3, 740, 424)
    sk.put(sk.img("white"), 795, 446, 4, 4)
    sk.number("n_mid", 103, 2, 800, 424, zeropad=True)
    sk.number("n_small", 150, 5, 780, 460)
    for i, ref in enumerate((110, 111, 112, 113, 114)):
        sk.number("n_small", ref, 5, 560, 596 + i * 22)
    sk.number("n_bpm", 160, 3, 700, 600)
    sk.number("n_small", 91, 3, 690, 666)
    sk.number("n_small", 90, 3, 794, 666)
    sk.number("n_small", 163, 1, 702, 688)
    sk.number("n_small_pad", 164, 2, 718, 688, zeropad=True)
    sk.number("n_small", 96, 2, 794, 688)
    progress(sk, 446, 90, 302)
    ready(sk, f1 + fw // 2, 270)
    ready(sk, f2 + fw // 2, 270)
    return sk


# --- portrait single play ---------------------------------------------------
PORT_SP = {"7": (102, 78), "5": (108, 104), "9": (0, 70)}


def bg_port(fonts):
    W, H = 720, 1280
    img = background(W, H)
    d = ImageDraw.Draw(img)
    panel(img, (8, 6, 704, 66), None, accent=PINK)
    d.rectangle((30, 76, 690, 1090), fill=(0, 0, 0, 255))
    d.rounded_rectangle((10, 80, 22, 996), 3, fill=(24, 28, 42, 255))
    panel(img, (24, 1094, 672, 40), None)
    panel(img, (24, 1140, 216, 134), "SCORE", fonts)
    draw_label(img, fonts, 36, 1166, "EX", 13, TEXT)
    draw_label(img, fonts, 36, 1206, "COMBO", 13, TEXT)
    draw_label(img, fonts, 36, 1244, "RATE", 13, TEXT)
    draw_label(img, fonts, 222, 1246, "%", 14, TEXT)
    panel(img, (248, 1140, 216, 134), "BPM", fonts, accent=AMBER)
    draw_label(img, fonts, 260, 1222, "MIN", 11)
    draw_label(img, fonts, 360, 1222, "MAX", 11)
    draw_label(img, fonts, 260, 1250, "TIME", 11)
    draw_label(img, fonts, 325, 1247, ":", 16, TEXT)
    draw_label(img, fonts, 648, 32, "LV", 11)
    panel(img, (472, 1140, 224, 134), "JUDGE", fonts)
    for i, s in enumerate(["PERFECT", "GREAT", "GOOD", "BAD", "POOR"]):
        draw_label(img, fonts, 484, 1160 + i * 22, s, 13, TEXT)
    return img


def port_sp(fonts, parts, mode, side):
    kind = {"7": 0, "5": 1, "9": 4}[mode]
    sk = Skin(kind, f"vibeBMS Default {mode}K portrait{' 2P' if side else ''}", 720, 1280, parts)
    sk.source("bg", "bg_port.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, 720, 1280)
    sw, kw = PORT_SP[mode]
    kinds, scratch = lane_kinds(mode)
    fw = (sw if scratch else 0) + kw * len(kinds)
    fx = 32 + (656 - fw) // 2
    sk.j["bga"] = {"id": "bga"}
    sk.put("bga", fx, 76, fw, 920, a=110)
    ls, _ = lanes(mode, fx, side, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    field(sk, mode, 0, side, fx, 76, 920, sw, kw, 1004, 84, 0.6, translucent=True)
    progress(sk, 10, 80, 916)
    sk.label_text(12, 20, 12, 600, 28)
    sk.label_text(16, 20, 46, 600, 16, color=DIM)
    sk.number("n_green", 96, 2, 672, 30, color=TEXT)
    gauge(sk, 36, 1102, 500, 24)
    sk.number("n_gauge", 107, 3, 580, 1100)
    sk.put(sk.img("white"), 634, 1122, 4, 4)
    sk.number("n_gauge", 407, 1, 640, 1100)
    sk.number("n_mid", 71, 5, 140, 1160)
    sk.number("n_mid", 75, 5, 140, 1198)
    sk.number("n_small", 102, 3, 160, 1242)
    sk.put(sk.img("white"), 196, 1258, 3, 3)
    sk.number("n_small_pad", 103, 2, 200, 1242, zeropad=True)
    sk.number("n_bpm", 160, 3, 304, 1160)
    sk.number("n_small", 91, 3, 296, 1220)
    sk.number("n_small", 90, 3, 400, 1220)
    sk.number("n_small", 163, 1, 312, 1248)
    sk.number("n_small_pad", 164, 2, 328, 1248, zeropad=True)
    for i, ref in enumerate((110, 111, 112, 113, 114)):
        sk.number("n_small", ref, 5, 620, 1158 + i * 22)
    ready(sk, fx + fw // 2, 500)
    return sk


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fonts", help="directory with Inter Display/Inter OTF faces")
    args = ap.parse_args()
    fonts = Fonts(args.fonts)
    OUT.mkdir(parents=True, exist_ok=True)
    parts = build_parts(fonts)
    sheet = parts.build()

    def save_png(img, name):
        img.save(OUT / name, optimize=True)

    save_png(sheet, "parts.png")
    save_png(bg_land_sp(fonts, 0).convert("RGB").quantize(128).convert("RGB"), "bg_land_1p.png")
    save_png(bg_land_sp(fonts, 1).convert("RGB").quantize(128).convert("RGB"), "bg_land_2p.png")
    save_png(bg_land_dp(fonts).convert("RGB").quantize(128).convert("RGB"), "bg_land_dp.png")
    save_png(bg_port(fonts).convert("RGB").quantize(128).convert("RGB"), "bg_port.png")
    for mode in ("7", "5", "9"):
        for side in (0, 1):
            sk, _ = land_sp(fonts, parts, mode, side)
            sk.save(OUT / f"play{mode}{'_2p' if side else ''}.json")
            port_sp(fonts, parts, mode, side).save(OUT / f"play{mode}_portrait{'_2p' if side else ''}.json")
    land_dp(fonts, parts, "7").save(OUT / "play14.json")
    land_dp(fonts, parts, "5").save(OUT / "play10.json")
    total = sum(p.stat().st_size for p in OUT.iterdir())
    print(f"wrote {OUT} ({total // 1024} KiB)")


if __name__ == "__main__":
    sys.exit(main())
