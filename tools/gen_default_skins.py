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
    """Equal cells: 0-9 [, padding glyph]. All glyphs share one baseline; the
    font size is the largest (up to `size`) whose digits fit the cell height,
    and the cell width follows the widest glyph so nothing is clipped (`cw` is
    only a lower bound)."""
    cells = list(glyphs)
    if pad_glyph is not None:
        cells.append(pad_glyph)
    while True:
        f = fonts.get(which, size)
        boxes = [f.getbbox(c, stroke_width=stroke) for c in "0123456789+-"]
        top_ = min(b[1] for b in boxes[:10])
        bot = max(b[3] for b in boxes[:10])
        if bot - top_ <= ch - 2 or size <= 6:
            break
        size -= 1
    widest = max(b[2] - b[0] for b in boxes)
    cw = max(cw, widest + 2)
    oy = (ch - (bot - top_)) // 2 - top_
    img = Image.new("RGBA", (cw * len(cells), ch))
    for i, g in enumerate(cells):
        if g == " ":
            continue
        dim = g == "o"  # dim zero used as padding
        ch_ = "0" if dim else g
        l, _, r, _ = f.getbbox(ch_, stroke_width=stroke)
        ox = (cw - (r - l)) // 2 - l
        mask = Image.new("L", (cw, ch))
        smask = Image.new("L", (cw, ch))
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


def hstack(*imgs):
    out = Image.new("RGBA", (sum(i.size[0] for i in imgs), max(i.size[1] for i in imgs)))
    x = 0
    for i in imgs:
        out.paste(i, (x, 0))
        x += i.size[0]
    return out


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


def button_img(kind, size, pressed):
    hi, lo = NOTE_COLORS[kind]
    base = mix(hi, (255, 255, 255), 0.45) if pressed else hi
    img = Image.new("RGBA", (size, size))
    d = ImageDraw.Draw(img)
    d.ellipse((0, 0, size - 1, size - 1), fill=mix(lo, BASE, 0.5) + (255,))
    g = vgrad(size - 8, size - 8, mix(base, (255, 255, 255), 0.35), mix(base, lo, 0.4))
    m = Image.new("L", (size - 8, size - 8))
    ImageDraw.Draw(m).ellipse((0, 0, size - 9, size - 9), fill=255)
    img.paste(g, (4, 5 if pressed else 3), m)
    if pressed:
        d.ellipse((0, 0, size - 1, size - 1), outline=(255, 255, 255, 255), width=2)
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
        if k.startswith("p"):
            a.add(f"key_{k}", button_img(k, 64, False))
            a.add(f"keyp_{k}", button_img(k, 64, True))
        else:
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
    # judgements: PGREAT is a flashing GREAT; its combo digits flash in sync
    PG_A = ((235, 255, 255), (70, 210, 255), (0, 40, 80), TEAL)
    PG_B = ((255, 255, 255), (255, 150, 225), (70, 0, 55), PINK)
    GR = ((255, 245, 170), (255, 190, 40), (70, 40, 0), AMBER)
    GD = ((190, 255, 200), (60, 210, 110), (0, 50, 20), None)
    judges = [("GREAT",) + PG_A, ("GREAT",) + PG_B, ("GREAT",) + GR, ("GOOD",) + GD,
              ("BAD", (230, 200, 255), (160, 100, 255), (30, 0, 60), None),
              ("POOR", (255, 190, 190), (255, 70, 90), (60, 0, 10), None)]
    imgs = [text_img(fonts, s_, "black_it", 50, t, b, o, 3, g) for s_, t, b, o, g in judges]
    jw = max(i.size[0] for i in imgs)
    jh = max(i.size[1] for i in imgs)

    def fit(i):
        c = Image.new("RGBA", (jw, jh))
        c.paste(i, ((jw - i.size[0]) // 2, (jh - i.size[1]) // 2))
        return c

    a.add("j_pg", hstack(fit(imgs[0]), fit(imgs[1])))
    for key, im in zip(["j_gr", "j_gd", "j_bd", "j_pr"], imgs[2:]):
        a.add(key, fit(im))

    def combo(t, b, o, g):
        return digit_strip(fonts, 36, 60, "black_it", 50, t, b, outline=o, stroke=3)

    a.add("n_combo_pg", hstack(combo(*PG_A), combo(*PG_B)))
    a.add("n_combo_gr", combo(*GR))
    a.add("n_combo_gd", combo(*GD))
    a.add("fast", text_img(fonts, "FAST", "black_it", 20, (200, 240, 255), BLUE, (0, 20, 50), 2))
    a.add("slow", text_img(fonts, "SLOW", "black_it", 20, (255, 220, 200), RED, (50, 0, 10), 2))
    a.add("ready", text_img(fonts, "READY", "black_it", 64, (255, 255, 255), TEAL, (0, 30, 40), 4, TEAL))
    # number fonts
    a.add("n_big", digit_strip(fonts, 26, 38, "bold", 34, (255, 255, 255), (160, 200, 255), pad_glyph="o"))
    a.add("n_mid", digit_strip(fonts, 18, 28, "bold", 24, (255, 255, 255), (180, 200, 230), pad_glyph="o"))
    a.add("n_small", digit_strip(fonts, 12, 20, "semi", 17, TEXT, (170, 180, 205), stroke=1))
    # beatoraja signed numbers: 0-9, padding, sign; positive set then negative set
    a.add("n_signed", hstack(
        digit_strip(fonts, 12, 20, "semi", 17, (200, 230, 255), (120, 180, 255), stroke=1,
                    glyphs="0123456789 +"),
        digit_strip(fonts, 12, 20, "semi", 17, (255, 210, 210), (255, 110, 120), stroke=1,
                    glyphs="0123456789 -")))
    a.add("n_bpm", digit_strip(fonts, 34, 50, "black_it", 46, (255, 255, 255), AMBER, stroke=2))
    a.add("n_bpm_s", digit_strip(fonts, 16, 24, "bold", 21, (255, 255, 255), (215, 220, 235), stroke=1))
    gauge_c = ((255, 255, 255), (255, 170, 190))
    a.add("n_gauge", digit_strip(fonts, 18, 28, "bold", 26, *gauge_c))
    a.add("n_gauge_s", digit_strip(fonts, 12, 19, "bold", 17, *gauge_c, stroke=1))
    dot = Image.new("RGBA", (5, 5))
    ImageDraw.Draw(dot).ellipse((0, 0, 4, 4), fill=gauge_c[1] + (255,))
    a.add("gauge_dot", dot)
    a.add("pct", text_img(fonts, "%", "bold", 16, *gauge_c, stroke=1, pad=1))
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

    # images per number font (11 = with padding glyph, 24 = signed, 20 = two
    # animated sets of 10)
    FRAMES = {"n_big": 11, "n_mid": 11, "n_signed": 24, "n_combo_pg": 20}

    def num(self, key, ref, digit, align=0, zeropad=False):
        x, y, w, h = self.parts.items[key]
        n = self.FRAMES.get(key, 10)
        iid = f"{key}#{ref}#{digit}#{int(zeropad)}"
        if iid not in self._ids:
            self._ids.add(iid)
            v = {"id": iid, "src": "parts", "x": x, "y": y, "w": w, "h": h, "divx": n,
                 "digit": digit, "ref": ref, "align": align}
            if zeropad:
                v["zeropadding"] = 1
            if key == "n_combo_pg":
                v["cycle"] = 80  # same cycle as the j_pg image: flashes in sync
            self.j["value"].append(v)
        cell = w // n
        return iid, cell, h

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
          judge_at, translucent=False, part="all"):
    """Note field, keys, beams, bombs and judgement for one player.

    `part` "under" draws what goes below the notes and "over" the rest, so
    that double play can draw both "under" parts before the shared notes."""
    ls, fw = lanes(mode, fx, side, scratch_w, key_w)
    judge_y = top + lane_h
    lanes_out = [(x, w, k, i) for i, (x, w, k) in enumerate(ls)]
    if part in ("all", "under"):
        field_under(sk, mode, player, fx, fw, top, lane_h, keys_h, lanes_out, translucent)
    if part in ("all", "over"):
        field_over(sk, mode, player, side, fx, fw, top, lane_h, keys_y, keys_h, judge_at,
                   lanes_out)


def field_under(sk, mode, player, fx, fw, top, lane_h, keys_h, lanes_out, translucent):
    judge_y = top + lane_h
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
    sk.j["note"]["group"].append({"id": sk.img("barline"),
                                  "dst": [sk.key(fx, top, fw, lane_h, fixed=True, time=0)]})


def field_over(sk, mode, player, side, fx, fw, top, lane_h, keys_y, keys_h, judge_at,
               lanes_out):
    judge_y = top + lane_h
    if player == 0:
        sk.put("notes", fx, top, fw, lane_h, fixed=True)
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
        if mode == "9":
            # round buttons in two staggered rows (even buttons up)
            bs = min(w + 8, keys_h * 6 // 10)
            by = keys_y + (0 if li % 2 else keys_h - bs)
            bx = x + (w - bs) // 2
            sk.put(sk.img(f"key_{k}"), bx, by, bs, bs, fixed=True)
            sk.put(sk.img(f"keyp_{k}"), bx, by, bs, bs, timer=on, fixed=True)
            continue
        raised = k == "b"
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


# --- panels shared by all layouts --------------------------------------------
# Each helper draws its static part into the background image and adds the
# dynamic objects to the skin, both in the same (1P, y-down) coordinates.

def gauge_number(sk, x, y):
    """Gauge value: big integer, small decimal and percent sign (bottom aligned)."""
    w = sk.number("n_gauge", 107, 3, x, y)
    sk.put(sk.img("gauge_dot"), x + w, y + 22, 5, 5)
    sk.number("n_gauge_s", 407, 1, x + w + 6, y + 9)
    pw, ph = sk.parts.items["pct"][2:]
    sk.put(sk.img("pct"), x + w + 19, y + 28 - ph + 1, pw, ph)
    return w + 19 + pw


def gauge_number_width(sk):
    return sk.num("n_gauge", 107, 3)[1] * 3 + 19 + sk.parts.items["pct"][2]


def gauge_block(sk, img, f, x, y, w):
    """Gauge across the full width `w`, its value above the right end (54 px)."""
    draw_label(img, f, x + 2, y + 12, "GROOVE GAUGE", 11)
    gauge_number(sk, x + w - gauge_number_width(sk), y)
    ImageDraw.Draw(img).rounded_rectangle((x - 2, y + 28, x + w + 1, y + 53), 4,
                                          fill=(6, 7, 11, 255), outline=EDGE + (255,))
    gauge(sk, x, y + 30, w, 22)


def info_row(sk, img, f, x, y, w, h=30):
    panel(img, (x, y, w, h))
    ty = y + (h - 22) // 2
    draw_label(img, f, x + 10, ty + 4, "DURATION", 12)
    sk.number("n_green", 312, 4, x + 100, ty)
    draw_label(img, f, x + w // 2 + 20, ty + 4, "LEVEL", 12)
    sk.number("n_green", 96, 2, x + w // 2 + 80, ty, color=TEXT)


def header(sk, img, f, x, y, w, h, align=0):
    panel(img, (x, y, w, h), accent=PINK)
    tx = x + 16
    if h >= 80:
        sk.label_text(13, tx, y + 6, w - 32, 14, align=align, color=DIM)
        sk.label_text(12, tx, y + 26, w - 32, 30, align=align)
        sk.label_text(16, tx, y + 62, w - 32, 16, align=align, color=DIM)
    else:
        sk.label_text(12, tx, y + 8, w - 32, 28, align=align)
        sk.label_text(16, tx, y + 42, w - 32, 16, align=align, color=DIM)


def bga_frame(sk, img, x, y, w, h, a=255):
    ImageDraw.Draw(img).rounded_rectangle((x, y, x + w, y + h), 6, fill=(0, 0, 0, 255),
                                          outline=EDGE + (255,))
    sk.j["bga"] = {"id": "bga"}
    sk.put("bga", x + 4, y + 4, w - 8, h - 8, a=a)


def rate_number(sk, img, f, r, y, key="n_mid"):
    """`100.00%` right-aligned at r."""
    cw, ch = sk.num(key, 102, 3)[1:]
    pct = 14
    x = r - pct - cw * 5 - 6
    sk.number(key, 102, 3, x, y)
    sk.put(sk.img("white"), x + cw * 3 + 1, y + ch - 7, 4, 4)
    sk.number(key, 103, 2, x + cw * 3 + 6, y, zeropad=True)
    draw_label(img, f, r - pct + 2, y + ch - 18, "%", 14, TEXT)


def score_panel(sk, img, f, x, y, w, h):
    panel(img, (x, y, w, h), "SCORE", f)
    r = x + w - 14
    big = "n_big" if w >= 300 else "n_mid"
    step = (h - 30) // 3
    rows = [y + 26 + step * i for i in range(3)]
    for ry, s_ in zip(rows, ["EX SCORE", "MAX COMBO", "RATE"]):
        draw_label(img, f, x + 12, ry + 8, s_ if w >= 260 else s_.split()[-1], 13, TEXT)
    cw, ch = sk.num(big, 71, 5)[1:]
    sk.number(big, 71, 5, r - cw * 5, rows[0] + (24 - ch) // 2 + 4)
    cw, ch = sk.num("n_mid", 75, 5)[1:]
    sk.number("n_mid", 75, 5, r - cw * 5, rows[1])
    rate_number(sk, img, f, r, rows[2])


def best_panel(sk, img, f, x, y, w, h=64):
    panel(img, (x, y, w, h), "PERSONAL BEST", f, accent=AMBER)
    draw_label(img, f, x + 12, y + 30, "BEST", 13, TEXT)
    sk.number("n_small", 150, 5, x + 70, y + 29)
    draw_label(img, f, x + w // 2 + 8, y + 30, "DIFF", 13, TEXT)
    cw = sk.num("n_signed", 152, 6)[1]
    sk.number("n_signed", 152, 6, x + w - 14 - cw * 6, y + 29)


def graph_panel(sk, img, f, x, y, w, h):
    panel(img, (x, y, w, h), "SCORE GRAPH", f)
    d = ImageDraw.Draw(img)
    gx, gy, gw, gh = x + 16, y + 28, w - 76, h - 52
    for name, frac in (("AAA", 8 / 9), ("AA", 7 / 9), ("A", 6 / 9)):
        ly = gy + gh - int(gh * frac)
        d.line((gx, ly, gx + gw, ly), fill=(70, 80, 115, 255), width=1)
        draw_label(img, f, gx + gw + 6, ly - 8, name, 12)
    d.line((gx, gy + gh, gx + gw, gy + gh), fill=EDGE + (255,), width=1)
    bw = gw // 2 - 24
    for i, (ref, col, label) in enumerate(((110, TEAL, "NOW"), (111, PINK, "TOTAL"))):
        bx = gx + 12 + i * (gw // 2)
        gid = f"graph{ref}"
        if gid not in sk._ids:
            sk._ids.add(gid)
            px, py, pw, ph = sk.parts.items["white"]
            sk.j["graph"].append({"id": gid, "src": "parts", "x": px + 4, "y": py + 4,
                                  "w": pw - 8, "h": ph - 8, "angle": 1, "type": ref})
        sk.put(gid, bx, gy, bw, gh, r=col[0], g=col[1], b=col[2], a=230)
        draw_label(img, f, bx + bw // 2, gy + gh + 4, label, 11, anchor="ma")


def judge_panel(sk, img, f, x, y, w, h):
    """Judge table: count, FAST and SLOW per judgement; the last row holds the
    combo breaks and the FAST / SLOW totals."""
    panel(img, (x, y, w, h), "JUDGE", f)
    d = ImageDraw.Draw(img)
    wide = w >= 300
    cw = sk.num("n_small", 110, 4)[1]
    keta = 4 if wide else 3
    tot_r = x + (w * 50) // 100
    fast_r = x + (w * 76) // 100
    slow_r = x + w - 12
    fast_c, slow_c = (140, 195, 255), (255, 140, 150)
    draw_label(img, f, fast_r - cw * keta // 2, y + 7, "FAST", 11, BLUE, anchor="ma")
    draw_label(img, f, slow_r - cw * keta // 2, y + 7, "SLOW", 11, RED, anchor="ma")
    rh = (h - 40) // 6
    names = ["PERFECT", "GREAT", "GOOD", "BAD", "POOR"] if wide else ["PG", "GR", "GD", "BD", "PR"]
    for i, (name, ref) in enumerate(zip(names, (110, 111, 112, 113, 114))):
        ry = y + 24 + i * rh
        draw_label(img, f, x + 12, ry + 2, name, 13, TEXT)
        sk.number("n_small", ref, 4, tot_r - cw * 4, ry)
        sk.number("n_small", 410 + i * 2, keta, fast_r - cw * keta, ry, color=fast_c)
        sk.number("n_small", 411 + i * 2, keta, slow_r - cw * keta, ry, color=slow_c)
    ry = y + 24 + 5 * rh + 4
    d.line((x + 10, ry - 3, x + w - 10, ry - 3), fill=EDGE + (255,), width=1)
    draw_label(img, f, x + 12, ry + 2, "COMBO BREAK" if wide else "CB", 13, AMBER)
    sk.number("n_small", 425, 4, tot_r - cw * 4, ry, color=(255, 220, 140))
    sk.number("n_small", 423, keta, fast_r - cw * keta, ry, color=fast_c)
    sk.number("n_small", 424, keta, slow_r - cw * keta, ry, color=slow_c)


def bpm_panel(sk, img, f, x, y, w, h):
    """Current BPM large; min / max small and coloured with MIN, BPM, MAX labels.
    Wide panels put min and max beside the current value, narrow ones below it."""
    panel(img, (x, y, w, h), None, accent=AMBER)
    cw, ch = sk.num("n_bpm", 160, 3)[1:]
    sw, sh = sk.num("n_bpm_s", 91, 3)[1:]
    cx = x + w // 2
    lx, rx = x + 14 + sw * 3 // 2, x + w - 14 - sw * 3 // 2
    if cw * 3 + sw * 6 + 60 <= w:
        row = y + (h - ch - 18) // 2
        base = row + ch - sh - 3
        ly = row + ch + 2
    else:
        row = y + (h - ch - sh - 18) // 2
        base = row + ch + 2
        ly = base + sh
    sk.number("n_bpm", 160, 3, cx - cw * 3 // 2, row)
    sk.number("n_bpm_s", 91, 3, lx - sw * 3 // 2, base, color=(130, 190, 255))
    sk.number("n_bpm_s", 90, 3, rx - sw * 3 // 2, base, color=(255, 130, 180))
    draw_label(img, f, lx, ly, "MIN", 11, anchor="ma")
    draw_label(img, f, cx, row + ch + 2, "BPM", 12, AMBER, "bold", anchor="ma")
    draw_label(img, f, rx, ly, "MAX", 11, anchor="ma")


def time_panel(sk, img, f, x, y, w, h):
    panel(img, (x, y, w, h), "TIME LEFT", f)
    cw, ch = sk.num("n_big", 163, 2)[1:]
    tw = cw * 4 + 14
    tx = x + (w - tw) // 2
    sk.number("n_big", 163, 2, tx, y + 26)
    draw_label(img, f, tx + cw * 2 + 7, y + 26 + ch // 2, ":", 30, TEXT, "bold", anchor="mm")
    sk.number("n_big", 164, 2, tx + cw * 2 + 14, y + 26, zeropad=True)
    draw_label(img, f, x + 12, y + h - 24, "FPS", 11)
    sk.number("n_small", 20, 4, x + 44, y + h - 26, color=DIM)


# --- landscape single play: 7/5 keys at the side (2P swaps the blocks) --------
LAND_SP = {"7": (78, 46), "5": (80, 64)}


def land_sp(fonts, parts, mode, side):
    W, H = 1280, 720
    kind = {"7": 0, "5": 1}[mode]
    sk = Skin(kind, f"vibeBMS Default {mode}K{' 2P' if side else ''}", W, H, parts)
    bgname = f"bg_land_{'2p' if side else '1p'}.png"
    sk.source("bg", bgname)
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H, fixed=True)
    if side:
        sk.xmap = lambda x: x - 460 if x >= 460 else x + 820
    img = background(W, H)
    d = ImageDraw.Draw(img)
    d.rectangle((32, 0, 448, H), fill=(8, 9, 14, 255))
    d.rounded_rectangle((18, 4, 26, 536), 3, fill=(24, 28, 42, 255))
    f = fonts
    header(sk, img, f, 468, 8, 796, 86)
    bga_frame(sk, img, 476, 100, 456, 456)
    sw, kw = LAND_SP[mode]
    kinds, _ = lane_kinds(mode)
    fw = sw + kw * len(kinds)
    fx = 40 + (400 - fw) // 2 + (820 if side else 0)
    ls, _ = lanes(mode, fx, side, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    field(sk, mode, 0, side, fx, 0, 540, sw, kw, 548, 80, 0.62)
    progress(sk, 16, 0, 540)
    gauge_block(sk, img, f, fx - (820 if side else 0), 630, fw)
    info_row(sk, img, f, 40, 688, 400)
    score_panel(sk, img, f, 944, 100, 320, 140)
    best_panel(sk, img, f, 944, 248, 320)
    graph_panel(sk, img, f, 944, 320, 320, 236)
    judge_panel(sk, img, f, 476, 564, 236, 150)
    bpm_panel(sk, img, f, 720, 564, 212, 150)
    time_panel(sk, img, f, 944, 564, 320, 150)
    ready(sk, fx + fw // 2, 270)
    if side:
        out = img.copy()
        out.paste(img.crop((460, 0, W, H)), (0, 0))
        out.paste(img.crop((0, 0, 460, H)), (820, 0))
        img = out
    return sk, img, bgname


# --- landscape 9 buttons: field in the centre ------------------------------------
def land_9(fonts, parts):
    W, H = 1280, 720
    sk = Skin(4, "vibeBMS Default 9K", W, H, parts)
    sk.source("bg", "bg_land_9.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H)
    img = background(W, H)
    f = fonts
    kw = 50
    fw = kw * 9
    fx = (W - fw) // 2
    ImageDraw.Draw(img).rectangle((fx - 8, 0, fx + fw + 8, H), fill=(8, 9, 14, 255))
    ImageDraw.Draw(img).rounded_rectangle((fx - 22, 4, fx - 14, 516), 3, fill=(24, 28, 42, 255))
    ls, _ = lanes("9", fx, 0, 0, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    field(sk, "9", 0, 0, fx, 0, 520, 0, kw, 526, 104, 0.6)
    progress(sk, fx - 24, 0, 520)
    gauge_block(sk, img, f, fx, 632, fw)
    info_row(sk, img, f, fx, 690, fw, 28)
    header(sk, img, f, 16, 8, 376, 86)
    bga_frame(sk, img, 16, 100, 376, 376)
    score_panel(sk, img, f, 16, 484, 376, 140)
    best_panel(sk, img, f, 16, 632, 376, 80)
    judge_panel(sk, img, f, 888, 8, 376, 150)
    graph_panel(sk, img, f, 888, 166, 376, 310)
    bpm_panel(sk, img, f, 888, 484, 376, 120)
    time_panel(sk, img, f, 888, 612, 376, 100)
    ready(sk, fx + fw // 2, 260)
    return sk, img


# --- landscape double play --------------------------------------------------
LAND_DP = {"7": (62, 44), "5": (66, 60)}


def land_dp(fonts, parts, mode):
    W, H = 1280, 720
    kind = {"7": 2, "5": 3}[mode]
    sk = Skin(kind, f"vibeBMS Default {'14' if mode == '7' else '10'}K", W, H, parts)
    sk.source("bg", "bg_land_dp.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H)
    img = background(W, H)
    f = fonts
    d = ImageDraw.Draw(img)
    for x in (12, 868):
        d.rectangle((x, 0, x + 400, H), fill=(8, 9, 14, 255))
    sw, kw = LAND_DP[mode]
    kinds, _ = lane_kinds(mode)
    fw = sw + kw * len(kinds)
    f1 = 16 + (392 - fw) // 2
    f2 = W - f1 - fw
    l1, _ = lanes(mode, f1, 0, sw, kw)
    l2, _ = lanes(mode, f2, 1, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(l1 + l2)])
    for part in ("under", "over"):
        field(sk, mode, 0, 0, f1, 0, 548, sw, kw, 556, 76, 0.62, part=part)
        field(sk, mode, 1, 1, f2, 0, 548, sw, kw, 556, 76, 0.62, part=part)
    header(sk, img, f, 420, 6, 440, 74, align=1)
    d.rounded_rectangle((440, 92, 452, 390), 3, fill=(24, 28, 42, 255))
    progress(sk, 440, 88, 306)
    bga_frame(sk, img, 466, 86, 348, 310)
    gauge_block(sk, img, f, 428, 400, 424)
    score_panel(sk, img, f, 420, 460, 216, 118)
    bpm_panel(sk, img, f, 644, 460, 216, 118)
    judge_panel(sk, img, f, 420, 584, 216, 130)
    time_panel(sk, img, f, 644, 584, 216, 130)
    ready(sk, f1 + fw // 2, 270)
    ready(sk, f2 + fw // 2, 270)
    return sk, img


# --- portrait single play ---------------------------------------------------
PORT_SP = {"7": (102, 78), "5": (108, 104), "9": (0, 70)}


def port_sp(fonts, parts, mode, side):
    W, H = 720, 1280
    kind = {"7": 0, "5": 1, "9": 4}[mode]
    sk = Skin(kind, f"vibeBMS Default {mode}K portrait{' 2P' if side else ''}", W, H, parts)
    sk.source("bg", "bg_port.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H)
    img = background(W, H)
    f = fonts
    d = ImageDraw.Draw(img)
    sw, kw = PORT_SP[mode]
    kinds, scratch = lane_kinds(mode)
    fw = (sw if scratch else 0) + kw * len(kinds)
    fx = 32 + (656 - fw) // 2
    header(sk, img, f, 8, 6, 704, 66)
    # level and time at the right end of the header
    draw_label(img, f, 600, 14, "LV", 11)
    sk.number("n_green", 96, 2, 640, 12, color=TEXT)
    draw_label(img, f, 600, 44, "TIME", 11)
    sk.number("n_small", 163, 1, 642, 42)
    draw_label(img, f, 657, 41, ":", 15, TEXT)
    sk.number("n_small", 164, 2, 662, 42, zeropad=True)
    d.rectangle((30, 76, 690, 1056), fill=(0, 0, 0, 255))
    d.rounded_rectangle((10, 80, 22, 966), 3, fill=(24, 28, 42, 255))
    sk.j["bga"] = {"id": "bga"}
    sk.put("bga", fx, 76, fw, 890, a=110)
    ls, _ = lanes(mode, fx, side, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    field(sk, mode, 0, side, fx, 76, 890, sw, kw, 974, 84, 0.6, translucent=True)
    progress(sk, 10, 80, 886)
    gauge_block(sk, img, f, fx, 1058, fw)
    score_panel(sk, img, f, 24, 1118, 216, 156)
    bpm_panel(sk, img, f, 248, 1118, 216, 156)
    judge_panel(sk, img, f, 472, 1118, 224, 156)
    ready(sk, fx + fw // 2, 500)
    return sk, img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fonts", help="directory with Inter Display/Inter OTF faces")
    args = ap.parse_args()
    fonts = Fonts(args.fonts)
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.iterdir():
        old.unlink()
    parts = build_parts(fonts)
    sheet = parts.build()
    sheet.save(OUT / "parts.png", optimize=True)
    bgs = {}

    def emit(sk, img, json_name, bg_name):
        sk.save(OUT / json_name)
        bgs[bg_name] = img

    for mode in ("7", "5"):
        for side in (0, 1):
            sk, img, bg = land_sp(fonts, parts, mode, side)
            emit(sk, img, f"play{mode}{'_2p' if side else ''}.json", bg)
    emit(*land_9(fonts, parts), "play9.json", "bg_land_9.png")
    for mode in ("7", "5", "9"):
        for side in ((0, 1) if mode != "9" else (0,)):
            emit(*port_sp(fonts, parts, mode, side),
                 f"play{mode}_portrait{'_2p' if side else ''}.json", "bg_port.png")
    emit(*land_dp(fonts, parts, "7"), "play14.json", "bg_land_dp.png")
    emit(*land_dp(fonts, parts, "5"), "play10.json", "bg_land_dp.png")
    for name, img in bgs.items():
        img.convert("RGB").quantize(128).convert("RGB").save(OUT / name, optimize=True)
    total = sum(p.stat().st_size for p in OUT.iterdir())
    print(f"wrote {OUT} ({total // 1024} KiB)")


if __name__ == "__main__":
    sys.exit(main())
