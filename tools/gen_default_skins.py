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
    # result screen: clear lamps (beatoraja clear type order), DJ ranks, badges
    lamps = [
        ("NO PLAY", (200, 205, 215), (120, 125, 140)),
        ("FAILED", (255, 170, 170), (200, 50, 70)),
        ("ASSIST CLEAR", (235, 210, 255), (170, 110, 255)),
        ("L-ASSIST CLEAR", (235, 210, 255), (170, 110, 255)),
        ("EASY CLEAR", (200, 255, 210), (60, 210, 110)),
        ("CLEAR", (200, 235, 255), (60, 160, 255)),
        ("HARD CLEAR", (255, 220, 220), (255, 70, 90)),
        ("EX-HARD CLEAR", (255, 250, 200), (255, 200, 40)),
        ("FULL COMBO", (220, 255, 250), (45, 226, 199)),
        ("PERFECT", (255, 255, 255), (255, 150, 225)),
        ("MAX", (255, 255, 255), (255, 230, 150)),
    ]
    imgs = [text_img(fonts, t, "black_it", 46, a_, b_, (0, 0, 0), 3) for t, a_, b_ in lamps]
    lw, lh = max(i.size[0] for i in imgs), max(i.size[1] for i in imgs)
    for k, im in enumerate(imgs):
        c = Image.new("RGBA", (lw, lh))
        c.paste(im, (0, (lh - im.size[1]) // 2))
        a.add(f"lamp{k}", c)
    for k, (t, a_, b_) in enumerate(lamps):
        a.add(f"lamp_s{k}", text_img(fonts, t, "bold", 15, a_, b_, (0, 0, 0), 0, pad=2))
    ranks = ["AAA", "AA", "A", "B", "C", "D", "E", "F"]
    for k, r_ in enumerate(ranks):
        cell = rounded(42, 18, 4, AMBER + (255,))
        ImageDraw.Draw(cell).text((21, 9), r_, font=fonts.get("bold", 11), fill=(30, 20, 0),
                                  anchor="mm")
        a.add(f"rk_cell{k}", cell)
    rimgs = [text_img(fonts, r_, "black_it", 84, (255, 250, 220), (255, 190, 60), (40, 20, 0), 4,
                      AMBER) for r_ in ranks]
    rw, rh = max(i.size[0] for i in rimgs), max(i.size[1] for i in rimgs)
    for k, im in enumerate(rimgs):
        c = Image.new("RGBA", (rw, rh))
        c.paste(im, (rw - im.size[0], (rh - im.size[1]) // 2))
        a.add(f"rank{k}", c)
    a.add("newrec", text_img(fonts, "NEW RECORD", "black_it", 18, (255, 255, 255), PINK, (60, 0, 40), 2))
    a.add("autotag", text_img(fonts, "AUTOPLAY - NOT SAVED", "bold", 14, AMBER, AMBER, (40, 30, 0), 1))
    # music select: bar bodies per bar type (beatoraja order), cursor, lamp, buttons
    bar_accents = [TEAL, AMBER, (90, 220, 120), PINK, (150, 110, 130), BLUE, (170, 120, 255),
                   (110, 115, 130)]
    for k, acc in enumerate(bar_accents):
        bw, bh = 560, 44
        b = Image.new("RGBA", (bw, bh))
        g = vgrad(bw - 2, bh - 2, (34, 40, 58), (22, 26, 38))
        m = Image.new("L", (bw - 2, bh - 2))
        ImageDraw.Draw(m).rounded_rectangle((0, 0, bw - 3, bh - 3), 6, fill=255)
        b.paste(g, (1, 1), m)
        bd = ImageDraw.Draw(b)
        bd.rounded_rectangle((0, 0, bw - 1, bh - 1), 7, outline=EDGE + (255,))
        bd.rounded_rectangle((6, 6, 11, bh - 7), 2, fill=acc + (255,))
        if k in (1, 2, 5, 6):  # folders: a small chevron
            bd.polygon([(bw - 26, 16), (bw - 18, 22), (bw - 26, 28)], fill=acc + (255,))
        a.add(f"bar{k}", b)
    cur = Image.new("RGBA", (580, 52))
    ImageDraw.Draw(cur).rounded_rectangle((2, 2, 577, 49), 9, outline=TEAL + (255,), width=2)
    a.add("barcursor", cur.filter(ImageFilter.GaussianBlur(0.6)))
    a.add("lampcell", rounded(10, 32, 3, (255, 255, 255, 255)))
    a.add("optbtn", rounded(124, 62, 8, PANEL_HI + (255,), EDGE + (255,)))

    def label_button(text, w, h, accent):
        b = rounded(w, h, 7, PANEL_HI + (255,), accent + (255,))
        im = text_img(fonts, text, "bold", 14, TEXT, TEXT, (0, 0, 0), 0, pad=2)
        b.alpha_composite(im, ((w - im.size[0]) // 2, (h - im.size[1]) // 2))
        return b

    a.add("btn_retry", label_button("RETRY", 120, 30, PINK))
    a.add("btn_continue", label_button("CONTINUE", 140, 30, TEAL))
    a.add("btn_back", label_button("\u25c0 BACK", 84, 28, EDGE))
    for name, label, col in (("lbl_ln", "LN", TEAL), ("lbl_rand", "RANDOM", PINK),
                             ("lbl_bpm", "BPM CHANGE", AMBER), ("lbl_bga", "BGA", BLUE)):
        im = text_img(fonts, label, "bold", 13, (255, 255, 255), (255, 255, 255), (0, 0, 0), 0, pad=6)
        bg = rounded(im.size[0] + 8, 22, 5, col + (255,))
        bg.alpha_composite(im, (4, (22 - im.size[1]) // 2))
        a.add(name, bg)
    for name, label in (("gt_normal", "NORMAL"), ("gt_hard", "HARD"), ("gt_hazard", "HAZARD"),
                        ("gt_easy", "EASY"), ("gt_aeasy", "ASSIST EASY"), ("gt_exhard", "EX-HARD")):
        a.add(name, text_img(fonts, label, "bold", 13, TEXT, DIM, (0, 0, 0), 1, pad=2))
    a.add("hs_panel", hs_panel_img(fonts))
    knob = Image.new("RGBA", (44, 12))
    kd = ImageDraw.Draw(knob)
    kd.rounded_rectangle((0, 0, 43, 11), 3, fill=AMBER + (255,))
    kd.rectangle((2, 2, 41, 4), fill=mix(AMBER, (255, 255, 255), 0.5) + (255,))
    a.add("hs_knob", knob)
    a.add("auto_tag", text_img(fonts, "AUTO PLAY", "bold", 13, AMBER, AMBER, (40, 30, 0), 1, pad=2))
    # difficulty badges (beatoraja difficulty 1..5)
    for k, name in enumerate(("BEGINNER", "NORMAL", "HYPER", "ANOTHER", "INSANE"), 1):
        col = DIFF_COLORS[k]
        im = text_img(fonts, name, "bold", 13, (255, 255, 255), (255, 255, 255), (0, 0, 0), 0, pad=2)
        bg = rounded(108, 22, 4, col + (255,))
        bg.alpha_composite(im, ((108 - im.size[0]) // 2, (22 - im.size[1]) // 2))
        a.add(f"diff{k}", bg)
    # small rank names for "NEXT <rank>", right aligned in a common width
    rs = [text_img(fonts, r_, "bold", 14, AMBER, AMBER, (0, 0, 0), 0, pad=1)
          for r_ in ("MAX", "AAA", "AA", "A", "B", "C", "D", "E")]
    rw_ = max(i.size[0] for i in rs)
    for k, im in enumerate(rs):
        c = Image.new("RGBA", (rw_, im.size[1]))
        c.paste(im, (rw_ - im.size[0], 0))
        a.add(f"rk_s{k}", c)
    return a


DIFF_COLORS = [(170, 175, 190), (90, 220, 120), (90, 170, 255), (255, 210, 80), (255, 90, 110),
               (190, 120, 255), (200, 200, 200)]


# the hi-speed panel (shown while START is held): a 0.5 .. 10.0 track
HS_W, HS_H = 96, 380
HS_TRACK = (52, 292)


def hs_panel_img(fonts):
    im = Image.new("RGBA", (HS_W, HS_H))
    d = ImageDraw.Draw(im)
    d.rounded_rectangle((0, 0, HS_W - 1, HS_H - 1), 8, fill=PANEL + (238,), outline=EDGE + (255,))
    d.rounded_rectangle((0, 0, HS_W - 1, 24), 8, fill=AMBER + (255,))
    d.rectangle((0, 14, HS_W - 1, 24), fill=AMBER + (255,))
    d.text((HS_W // 2, 12), "HI-SPEED", font=fonts.get("bold", 12), fill=(30, 20, 0), anchor="mm")
    t0, t1 = HS_TRACK
    tx = 36
    d.text((tx, t0 - 8), "10.0", font=fonts.get("semi", 12), fill=TEXT, anchor="mb")
    d.text((tx, t1 + 6), "0.5", font=fonts.get("semi", 12), fill=TEXT, anchor="mt")
    d.rounded_rectangle((tx - 4, t0, tx + 4, t1), 3, fill=(6, 7, 11, 255), outline=EDGE + (255,))
    for k in range(1, 10):
        y = t1 - (t1 - t0) * (k - 0.5) / 9.5
        d.line((tx + 8, y, tx + (16 if k % 5 == 0 else 12), y), fill=DIM + (255,), width=1)
    # the mode, written down the right edge
    lab = Image.new("RGBA", (t1 - t0, 18))
    ImageDraw.Draw(lab).text(((t1 - t0) // 2, 9), "FLOATING HI-SPEED", font=fonts.get("bold", 12),
                             fill=DIM, anchor="mm")
    im.alpha_composite(lab.rotate(90, expand=True), (HS_W - 26, t0))
    # the value box (the digits are skin numbers)
    d.rounded_rectangle((8, HS_H - 58, HS_W - 9, HS_H - 9), 5, fill=(6, 7, 11, 255),
                        outline=AMBER + (255,))
    d.text((HS_W // 2, HS_H - 50), "CURRENT", font=fonts.get("bold", 10), fill=AMBER, anchor="mt")
    return im


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
        # region relayout (portrait from a landscape design): list of
        # ((x, y, w, h) source rect, (x, y) target) -- see relayout()
        self.regions = None
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
            if key in ("white", "sep") or key.startswith("lane"):
                x, y, w, h = x + 4, y + 4, w - 8, h - 8
            elif key == "barline":
                x, y, w, h = x + 4, y + 7, w - 8, 1  # 1 px tall
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
            elif n == 11:
                v["zeropadding"] = 2  # dim zeros from the padding image
            if key == "n_combo_pg":
                v["cycle"] = 80  # same cycle as the j_pg image: flashes in sync
            self.j["value"].append(v)
        cell = w // n
        return iid, cell, h

    def text(self, ref, align=0):
        iid = f"text{ref}_{align}"
        if iid not in self._ids:
            self._ids.add(iid)
            # shrink to the destination width (beatoraja's default overflows)
            self.j["text"].append({"id": iid, "ref": ref, "align": align, "overflow": 1})
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

    def shift_of(self, x, y):
        """Translation of the region containing (x, y); None = dropped."""
        if self.regions is None:
            return (0, 0)
        for (sx, sy, sw, sh), (tx, ty) in self.regions:
            if sx <= x < sx + sw and sy <= y < sy + sh:
                return (tx - sx, ty - sy)
        return None

    def put(self, iid, x, y, w, h, timer=0, loop=0, op=None, blend=0, center=0, keys=None,
            fixed=False, **kw):
        if not fixed and self.regions is not None:
            px, py = (keys[-1][0], keys[-1][1]) if keys else (x, y)
            t = self.shift_of(px, py)
            if t is None:
                return None
            if keys:
                keys = [(k[0] + t[0], k[1] + t[1]) + tuple(k[2:]) for k in keys]
            x, y = x + t[0], y + t[1]
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
        """Text in the box x..x+w: beatoraja anchors centred text at x and
        right-aligned text at its end, so the anchor moves to the box's
        centre / right edge."""
        d = self.put(self.text(ref, align), x, y, w, size, r=color[0], g=color[1], b=color[2])
        if d and align:
            for k in d["dst"]:
                k["x"] += k["w"] * align // 2

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
          judge_at, translucent=False, part="all", tt=None):
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
                   lanes_out, tt)


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
               lanes_out, tt=None):
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
    # white (cover, 1/1000) and green numbers on the cover's lower edge
    cw, ch = sk.num("n_small", 14, 4)[1:]
    gw_ = sk.num("n_green", 313, 4)[1] * 4
    ny = top - ch - 10
    mid = fx + fw // 2
    for d in (sk.put(sk.num("n_small", 14, 4)[0], mid - 8 - cw * 4, ny, cw, ch, op=[271],
                     fixed=True),
              sk.put(sk.num("n_green", 313, 4)[0], mid + 8, ny - 1, gw_ // 4, 22, op=[271],
                     fixed=True)):
        d["offset"] = 4
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
    # keys / turntable (touch layouts leave them to the on-screen controls)
    for x, w, k, li in lanes_out if keys_h > 0 else []:
        on, _, _, _ = timer_ids(mode, li, player)
        if k == "s":
            if tt:
                # a turntable larger than its lane (centre x, centre y, size)
                cx_, cy_, s = tt
                tx, ty = cx_ - s // 2, cy_ - s // 2
            else:
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
    # timing (ms, + = early) and FAST / SLOW (not on PERFECT) above the judgement
    iid, cw, ch = sk.num("n_signed", 525 + player, 4)
    fw_ = sk.parts.items["fast"][2]
    x0 = jcx - (cw * 4 + 6 + fw_) // 2
    sk.put(iid, 0, 0, 0, 0, timer=timer, loop=-1, fixed=True, keys=[
        (x0, jtop - ch - 4, cw, ch, {"time": 0}),
        (x0, jtop - ch - 4, cw, ch, {"time": 500}),
    ])
    for key, op in (("fast", 1242), ("slow", 1243)):
        x, y, w, h = sk.parts.items[key]
        fx_ = x0 + cw * 4 + 6
        fy = jtop - 4 - (ch + h) // 2
        sk.put(sk.img(key), 0, 0, 0, 0, timer=timer, loop=-1, op=[op, -241],
               fixed=True, keys=[
                   (fx_, fy, w, h, {"time": 0}),
                   (fx_, fy, w, h, {"time": 500}),
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


SCORE_H, JUDGE_H, BPM_H = 148, 152, 92


def score_content(sk, img, f, x, y, w, h):
    """EX score, max combo, rate and personal best (with difference)."""
    r = x + w - 14
    big = "n_big" if w >= 260 else "n_mid"
    # the EX score row is taller (large digits); the others are compact
    top = y + 24 + max(0, (h - 148) // 2)
    rows = [top, top + 42, top + 68, top + 94]
    for i, (ry, s_) in enumerate(zip(rows, ["EX SCORE", "MAX COMBO", "RATE", "BEST"])):
        draw_label(img, f, x + 12, ry + (12 if i == 0 else 7), s_ if w >= 250 else s_.split()[-1],
                   13, TEXT)
    cw, ch = sk.num(big, 71, 5)[1:]
    sk.number(big, 71, 5, r - cw * 5, rows[0] + (38 - ch) // 2)
    cw, ch = sk.num("n_mid", 75, 5)[1:]
    sk.number("n_mid", 75, 5, r - cw * 5, rows[1])
    rate_number(sk, img, f, r, rows[2])
    sk.number("n_small", 150, 5, x + (100 if w >= 250 else 52), rows[3] + 4)
    sw = sk.num("n_signed", 152, 5)[1]
    sk.number("n_signed", 152, 5, r - sw * 5, rows[3] + 4)


def graph_bar(sk, ref, angle):
    gid = f"graph{ref}_{angle}"
    if gid not in sk._ids:
        sk._ids.add(gid)
        px, py, pw, ph = sk.parts.items["white"]
        sk.j["graph"].append({"id": gid, "src": "parts", "x": px + 4, "y": py + 4,
                              "w": pw - 8, "h": ph - 8, "angle": angle, "type": ref})
    return gid


def graph_content(sk, img, f, x, y, w, h):
    """Score graph: current rate and total progress against the A / AA / AAA
    lines; vertical bars when there is height for them, horizontal otherwise."""
    d = ImageDraw.Draw(img)
    draw_label(img, f, x + 12, y + 7, "SCORE GRAPH", 12)
    bars = ((110, TEAL, "NOW"), (111, PINK, "TOTAL"))
    if h >= 140:
        gx, gy, gw, gh = x + 16, y + 28, w - 64, h - 50
        for name, frac in (("A", 6 / 9), ("AA", 7 / 9), ("AAA", 8 / 9)):
            ly = gy + gh - int(gh * frac)
            d.line((gx, ly, gx + gw, ly), fill=(70, 80, 115, 255), width=1)
            draw_label(img, f, gx + gw + 6, ly, name, 11, anchor="lm")
        d.line((gx, gy + gh, gx + gw, gy + gh), fill=EDGE + (255,), width=1)
        bw = gw // 2 - 20
        for i, (ref, col, label) in enumerate(bars):
            bx = gx + 10 + i * (gw // 2)
            sk.put(graph_bar(sk, ref, 1), bx, gy, bw, gh, r=col[0], g=col[1], b=col[2], a=230)
            draw_label(img, f, bx + bw // 2, gy + gh + 4, label, 11, anchor="ma")
        return
    gx, gw = x + 70, w - 90
    bh = max(10, min(24, (h - 52) // 2))
    gy = y + 30 + ((h - 38) - bh * 2 - 10) // 2
    for name, frac in (("A", 6 / 9), ("AA", 7 / 9), ("AAA", 8 / 9)):
        lx = gx + int(gw * frac)
        d.line((lx, gy - 4, lx, gy + bh * 2 + 14), fill=(70, 80, 115, 255), width=1)
        draw_label(img, f, lx, y + 8, name, 11, anchor="ma")
    for i, (ref, col, label) in enumerate(bars):
        by = gy + i * (bh + 10)
        sk.put(graph_bar(sk, ref, 0), gx, by, gw, bh, r=col[0], g=col[1], b=col[2], a=230)
        draw_label(img, f, x + 12, by + bh // 2, label, 11, anchor="lm")


def score_panel(sk, img, f, x, y, w, h=SCORE_H, graph_h=0):
    """The score panel, with the score graph joined below it when graph_h > 0."""
    panel(img, (x, y, w, h + graph_h), "SCORE", f)
    score_content(sk, img, f, x, y, w, h)
    if graph_h:
        ImageDraw.Draw(img).line((x + 10, y + h, x + w - 10, y + h), fill=EDGE + (255,))
        graph_content(sk, img, f, x, y + h, w, graph_h)


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
    rh = min(24, (h - 42) // 6)
    top = y + 24 + ((h - 32) - rh * 6 - 4) // 2
    names = ["PERFECT", "GREAT", "GOOD", "BAD", "POOR"] if wide else ["PG", "GR", "GD", "BD", "PR"]
    for i, (name, ref) in enumerate(zip(names, (110, 111, 112, 113, 114))):
        ry = top + i * rh
        draw_label(img, f, x + 12, ry + 2, name, 13, TEXT)
        sk.number("n_small", ref, 4, tot_r - cw * 4, ry)
        sk.number("n_small", 410 + i * 2, keta, fast_r - cw * keta, ry, color=fast_c)
        sk.number("n_small", 411 + i * 2, keta, slow_r - cw * keta, ry, color=slow_c)
    ry = top + 5 * rh + 4
    d.line((x + 10, ry - 3, x + w - 10, ry - 3), fill=EDGE + (255,), width=1)
    draw_label(img, f, x + 12, ry + 2, "COMBO BREAK" if wide else "CB", 13, AMBER)
    sk.number("n_small", 425, 4, tot_r - cw * 4, ry, color=(255, 220, 140))
    sk.number("n_small", 423, keta, fast_r - cw * keta, ry, color=fast_c)
    sk.number("n_small", 424, keta, slow_r - cw * keta, ry, color=slow_c)


def bpm_panel(sk, img, f, x, y, w, h):
    """Current BPM large; min / max small and coloured with MIN, BPM, MAX labels.
    Wide panels put min and max beside the current value, narrow ones below it."""
    panel(img, (x, y, w, h), None, accent=AMBER)
    big = 1.0 if w >= 300 else 0.8
    cw, ch = sk.num("n_bpm", 160, 3)[1:]
    cw, ch = int(cw * big), int(ch * big)
    sw, sh = sk.num("n_bpm_s", 91, 3)[1:]
    cx = x + w // 2
    lx, rx = x + 14 + sw * 3 // 2, x + w - 14 - sw * 3 // 2
    hs_on_top = h >= 120
    if cw * 3 + sw * 6 + 36 <= w and not hs_on_top:
        row = y + (h - ch - 18) // 2
        base = row + ch - sh - 3
        ly = row + ch + 2
    else:
        row = y + 16 + (h - 16 - ch - sh - 18) // 2
        base = row + ch + 2
        ly = base + sh
    sk.number("n_bpm", 160, 3, cx - cw * 3 // 2, row, scale=big)
    sk.number("n_bpm_s", 91, 3, lx - sw * 3 // 2, base, color=(130, 190, 255))
    sk.number("n_bpm_s", 90, 3, rx - sw * 3 // 2, base, color=(255, 130, 180))
    draw_label(img, f, lx, ly, "MIN", 11, anchor="ma")
    draw_label(img, f, cx, row + ch + 2, "BPM", 12, AMBER, "bold", anchor="ma")
    draw_label(img, f, rx, ly, "MAX", 11, anchor="ma")
    # current scroll speed (hi-speed) in the top-right corner
    nw = sk.num("n_small", 310, 1)[1]
    hx = x + w - 12 - nw * 3 - 6
    sk.number("n_small", 310, 1, hx, y + 6, color=(200, 255, 200))
    sk.put(sk.img("white"), hx + nw + 1, y + 21, 3, 3, r=200, g=255, b=200)
    sk.number("n_small", 311, 2, hx + nw + 5, y + 6, zeropad=True, color=(200, 255, 200))
    draw_label(img, f, hx - 6, y + 16, "HS", 11, (120, 220, 140), "bold", anchor="rm")


# with touch controls under them: a wider canvas (the space above the
# controls on a 4:3 tablet) and only the lanes and the gauge
TOUCH_H = 528
TOUCH_LANE_H = TOUCH_H - 60


# --- landscape single play (7/5 keys) and 9 buttons ----------------------------
# The note field down one side with a large turntable under its scratch
# lane, the gauge and a LEVEL / GAUGE / HI-SPEED row beside it; beyond it a
# title bar, a large BGA with the judge, BPM and status panels under it, and
# the score graph with the score rows at the outer edge.
LAND_SP = {"7": (78, 46), "5": (80, 64), "9": (0, 44)}
SP_FX = 60
SP_LANE_H, SP_KEYS_Y, SP_KEYS_H, SP_GAUGE_Y, SP_INFO_Y = 540, 546, 72, 616, 690
GAUGE_BAR = 32


def hs_panel(sk, x, y, scale=1.0):
    """The hi-speed panel at (x, y), shown while START is held (timer 21)."""
    S = lambda v: int(round(v * scale))  # noqa: E731
    sk.put(sk.img("hs_panel"), x, y, S(HS_W), S(HS_H), timer=21)
    t0, t1 = HS_TRACK
    kx, ky, kw, kh = sk.parts.items["hs_knob"]
    kid = f"hs_knob{S(100)}"
    if kid not in sk._ids:
        sk._ids.add(kid)
        sk.j["slider"].append({"id": kid, "src": "parts", "x": kx, "y": ky, "w": kw, "h": kh,
                               "angle": 0, "range": S(t1 - t0), "type": 9400})
    sk.put(kid, x + S(36) - S(kw) // 2, y + S(t1) - S(kh) // 2, S(kw), S(kh), timer=21)
    # the value: integer . two decimals
    cw, ch = sk.num("n_mid", 310, 2)[1:]
    cw, ch = int(cw * scale), int(ch * scale)
    vy = y + S(HS_H - 36)
    vx = x + (S(HS_W) - cw * 4 - S(6)) // 2
    sk.number("n_mid", 310, 2, vx, vy, scale=scale, timer=21)
    sk.put(sk.img("white"), vx + cw * 2 + 1, vy + ch - S(6), S(4), S(4), timer=21, r=AMBER[0],
           g=AMBER[1], b=AMBER[2])
    sk.number("n_mid", 311, 2, vx + cw * 2 + S(6), vy, zeropad=True, scale=scale, timer=21)


def info_bar(sk, img, f, x, y, w, h):
    """LEVEL, gauge type and hi-speed under the gauge."""
    panel(img, (x, y, w, h))
    cy = y + h // 2
    draw_label(img, f, x + 10, cy, "LV", 11, anchor="lm")
    nw, nh = sk.num("n_green", 96, 2)[1:]
    sk.number("n_green", 96, 2, x + 30, cy - nh // 2, color=TEXT)
    gx = x + 30 + nw * 2 + 16
    for key, op in (("gt_normal", 118), ("gt_hard", 119), ("gt_hazard", 120), ("gt_easy", 121),
                    ("gt_aeasy", 124), ("gt_exhard", 125)):
        gw_, gh_ = sk.parts.items[key][2:]
        sk.put(sk.img(key), gx, cy - gh_ // 2, gw_, gh_, op=[op])
    cw, ch = sk.num("n_small", 310, 1)[1:]
    hx = x + w - 10 - cw * 3 - 5
    draw_label(img, f, hx - 8, cy, "HI-SPEED" if w >= 260 else "HS", 11, anchor="rm")
    sk.number("n_small", 310, 1, hx, cy - ch // 2, color=(200, 255, 200))
    sk.put(sk.img("white"), hx + cw + 1, cy + ch // 2 - 5, 3, 3, r=200, g=255, b=200)
    sk.number("n_small", 311, 2, hx + cw + 5, cy - ch // 2, zeropad=True, color=(200, 255, 200))


def gauge_block(sk, img, f, x, y, w, bar=22):
    """Gauge across the full width `w`, its value above the right end."""
    draw_label(img, f, x + 2, y + 12, "GROOVE GAUGE", 11)
    gauge_number(sk, x + w - gauge_number_width(sk), y)
    ImageDraw.Draw(img).rounded_rectangle((x - 2, y + 28, x + w + 1, y + 31 + bar), 4,
                                          fill=(6, 7, 11, 255), outline=EDGE + (255,))
    gauge(sk, x, y + 30, w, bar)


def title_bar(sk, img, f, x, y, w, h=56):
    """AUTO PLAY, title and artist, difficulty and level, time left."""
    panel(img, (x, y, w, h), accent=PINK)
    aw, ah = sk.parts.items["auto_tag"][2:]
    sk.put(sk.img("auto_tag"), x + 14, y + (h - ah) // 2, aw, ah, op=[33])
    right = 250
    tx, tw = x + 24 + aw, w - 24 - aw - right - 12
    sk.label_text(12, tx, y + 6, tw, 26, align=1)
    sk.label_text(16, tx, y + 34, tw, 15, align=1, color=DIM)
    rx = x + w - right
    for k in range(1, 6):
        bw, bh = sk.parts.items[f"diff{k}"][2:]
        sk.put(sk.img(f"diff{k}"), rx, y + 8, bw, bh, op=[150 + k])
    draw_label(img, f, rx + 8, y + 42, "LV", 11, anchor="lm")
    nw, nh = sk.num("n_green", 96, 2)[1:]
    sk.number("n_green", 96, 2, rx + 28, y + 42 - nh // 2, color=TEXT)
    draw_label(img, f, x + w - 14, y + 8, "TIME LEFT", 11, anchor="ra")
    cw, ch = sk.num("n_mid", 163, 2)[1:]
    t0 = x + w - 14 - cw * 4 - 10
    sk.number("n_mid", 163, 2, t0, y + 24)
    draw_label(img, f, t0 + cw * 2 + 5, y + 24 + ch // 2, ":", 20, TEXT, "bold", anchor="mm")
    sk.number("n_mid", 164, 2, t0 + cw * 2 + 10, y + 24, zeropad=True)


def status_panel(sk, img, f, x, y, w, h):
    """Last judgement timing, green / white number, judge offset, FPS (as many
    rows as fit)."""
    panel(img, (x, y, w, h))
    cw, ch = sk.num("n_small", 20, 4)[1:]
    gw = sk.num("n_green", 313, 4)[1]
    r = x + w - 14

    def timing(ry):
        sk.number("n_signed", 525, 4, r - 22 - cw * 4, ry)
        draw_label(img, f, r, ry + ch // 2, "ms", 11, anchor="rm")

    def green(ry):
        sk.number("n_green", 313, 4, r - gw * 4, ry - 1)
        sk.number("n_small", 14, 4, r - gw * 4 - 18 - cw * 4, ry)

    def offset(ry):
        sk.number("n_signed", 12, 4, r - 22 - cw * 4, ry)
        draw_label(img, f, r, ry + ch // 2, "ms", 11, anchor="rm")

    def fps(ry):
        sk.number("n_small", 20, 4, r - cw * 4, ry, color=DIM)

    wide = w >= 260
    rows = [("TIMING", timing), ("WHITE / GREEN" if wide else "W / GN", green),
            ("JUDGE OFFSET" if wide else "OFFSET", offset), ("FPS", fps)]
    n = max(1, min(len(rows), (h - 12) // 26))
    rh = (h - 12) // n
    for i, (label, fn) in enumerate(rows[:n]):
        ry = y + 6 + i * rh + (rh - ch) // 2
        if i:
            ImageDraw.Draw(img).line((x + 10, y + 6 + i * rh, x + w - 10, y + 6 + i * rh),
                                     fill=(35, 42, 62, 255))
        draw_label(img, f, x + 12, ry + ch // 2, label, 11, anchor="lm")
        fn(ry)


def pace_graph(sk, img, f, x, y, w, h):
    """Score graph: NOW, BEST and TARGET bars (faint: the whole chart) against
    the A / AA / AAA / MAX lines, and the difference to the next rank."""
    panel(img, (x, y, w, h), "SCORE GRAPH", f, accent=PINK)
    sw = sk.num("n_signed", 154, 5)[1]
    nx = x + w - 12 - sw * 5
    sk.number("n_signed", 154, 5, nx, y + 26)
    draw_label(img, f, x + 12, y + 29, "NEXT", 11)
    for k in range(8):
        rw, rh = sk.parts.items[f"rk_s{k}"][2:]
        # current rank AAA (op 200) -> next MAX, AA -> AAA, ...
        sk.put(sk.img(f"rk_s{k}"), x + 50, y + 28, rw, rh, op=[200 + k])
    d = ImageDraw.Draw(img)
    gx, gy, gw, gh = x + 12, y + 60, w - 46, h - 60 - 26
    for name, frac in (("A", 6 / 9), ("AA", 7 / 9), ("AAA", 8 / 9), ("MAX", 1.0)):
        ly = gy + gh - int(gh * frac)
        d.line((gx, ly, gx + gw, ly), fill=(60, 70, 102, 255), width=1)
        draw_label(img, f, gx + gw + 4, ly, name, 10, anchor="lm")
    d.line((gx, gy + gh, gx + gw, gy + gh), fill=EDGE + (255,), width=1)
    bars = ((111, None, BLUE, "NOW"), (112, 113, (70, 220, 130), "BEST"),
            (114, 115, RED, "TARGET"))
    gap = 10
    bw = (gw - gap * 4) // 3
    for i, (now, total, col, label) in enumerate(bars):
        bx = gx + gap + i * (bw + gap)
        if total:
            sk.put(graph_bar(sk, total, 1), bx, gy, bw, gh, r=col[0], g=col[1], b=col[2], a=60)
        sk.put(graph_bar(sk, now, 1), bx, gy, bw, gh, r=col[0], g=col[1], b=col[2], a=230)
        draw_label(img, f, bx + bw // 2, gy + gh + 6, label, 10, anchor="ma")


def pace_rows(sk, img, f, x, y, w, rh=44, gap=6):
    """NOW / BEST / TARGET EX scores, with the differences to the pacemakers."""
    d = ImageDraw.Draw(img)
    cw, ch = sk.num("n_mid", 71, 5)[1:]
    sw, sh = sk.num("n_signed", 152, 5)[1:]
    rows = (("NOW SCORE", 71, None, BLUE), ("BEST SCORE", 150, 152, (70, 220, 130)),
            ("TARGET", 151, 153, RED))
    for i, (label, ref, diff, col) in enumerate(rows):
        ry = y + i * (rh + gap)
        d.rounded_rectangle((x, ry, x + w - 1, ry + rh - 1), 6, fill=mix(PANEL, col, 0.14) + (240,),
                            outline=mix(EDGE, col, 0.5) + (255,))
        d.rounded_rectangle((x + 1, ry + 8, x + 4, ry + rh - 9), 1, fill=col + (255,))
        draw_label(img, f, x + 12, ry + 8, label, 11, TEXT, "bold")
        sk.number("n_mid", ref, 5, x + w - 10 - cw * 5, ry + (rh - ch) // 2)
        if diff:
            sk.number("n_signed", diff, 5, x + 12, ry + rh - sh - 4)


def combo_box(sk, img, f, x, y, w, h):
    """MAX COMBO, MISS and the score rate."""
    panel(img, (x, y, w, h))
    rows = [("MAX COMBO" if w >= 200 else "COMBO", 75), ("MISS", 76), ("RATE", None)]
    n = max(1, min(3, (h - 10) // 24))
    rh = (h - 10) // n
    key = "n_mid" if w >= 200 else "n_small"
    cw, ch = sk.num(key, 75, 5)[1:]
    for i, (label, ref) in enumerate(rows[:n]):
        ry = y + 5 + i * rh + (rh - ch) // 2
        draw_label(img, f, x + 12, ry + ch // 2, label, 11, anchor="lm")
        if ref is None:
            rate_number(sk, img, f, x + w - 10, ry, key)
        else:
            sk.number(key, ref, 5, x + w - 10 - cw * 5, ry)


def land_sp(fonts, parts, mode, side, touch=False):
    """7/5 keys and 9 buttons: [field | title, BGA, panels | score graph] for
    1P, mirrored for 2P. `touch`: the variant shown above on-screen controls
    (no keys row)."""
    W, H = 1280, (TOUCH_H if touch else 720)
    lane_h = TOUCH_LANE_H if touch else SP_LANE_H
    kind = {"7": 0, "5": 1, "9": 4}[mode]
    name = f"vibeBMS Default {mode}K{' 2P' if side else ''}{' touch' if touch else ''}"
    sk = Skin(kind, name, W, H, parts)
    bgname = {"9": "bg_land_9"}.get(mode, f"bg_land_{'2p' if side else '1p'}")
    bgname += "_touch.png" if touch else ".png"
    sk.source("bg", bgname)
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H)
    img = background(W, H)
    f = fonts
    sw, kw = LAND_SP[mode]
    kinds, scratch = lane_kinds(mode)
    fw = (sw if scratch else 0) + kw * len(kinds)
    fx = SP_FX if side == 0 else W - SP_FX - fw
    d = ImageDraw.Draw(img)
    d.rectangle((fx - 8, 0, fx + fw + 8, H), fill=(8, 9, 14, 255))
    # progress between the field and the panels
    px = fx + fw + 12 if side == 0 else fx - 24
    d.rounded_rectangle((px + 2, 4, px + 10, lane_h - 4), 3, fill=(24, 28, 42, 255))
    progress(sk, px, 0, lane_h)
    ls, _ = lanes(mode, fx, side, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(ls)])
    if touch:
        # the on-screen controls show the keys: lanes and gauge only
        field(sk, mode, 0, side, fx, 0, lane_h, sw, kw, lane_h, 0, 0.6)
        gauge_block(sk, img, f, fx, lane_h + 4, fw, GAUGE_BAR - 8)
    else:
        # a turntable larger than its lane at the outer corner; the gauge and
        # the info row take the width beside it
        tt = None
        gx, gw = fx, fw
        if scratch:
            size = 140
            tcx = fx + 12 if side == 0 else fx + fw - 12
            tt = (tcx, H - 8 - size // 2, size)
            edge = size // 2 + 10
            gx = tcx + edge if side == 0 else fx
            gw = fx + fw - gx if side == 0 else tcx - edge - fx
        field(sk, mode, 0, side, fx, 0, lane_h, sw, kw, SP_KEYS_Y, SP_KEYS_H, 0.6, tt=tt)
        gauge_block(sk, img, f, gx, SP_GAUGE_Y, gw, GAUGE_BAR)
        info_bar(sk, img, f, gx, SP_INFO_Y, gw, 24)
    # panels: [centre | graph] beyond the field (mirrored for 2P)
    cw_all = W - 8 - (fx + fw + 36) if side == 0 else fx - 36 - 8
    x0 = fx + fw + 36 if side == 0 else 8
    gw_ = 224
    cw = cw_all - gw_ - 8
    cx = x0 if side == 0 else x0 + gw_ + 8
    rx = x0 + cw + 8 if side == 0 else x0
    title_bar(sk, img, f, x0, 6, cw_all)
    top = 70
    bottom_h = 226 if not touch else 150
    by = H - 8 - bottom_h
    bh = by - 8 - top
    bw = min(cw, bh * 4 // 3)
    bga_frame(sk, img, cx + (cw - bw) // 2, top, bw, bh)
    jw = min(264, cw * 48 // 100)
    judge_panel(sk, img, f, cx, by, jw, bottom_h)
    sx, sw_ = cx + jw + 8, cw - jw - 8
    bpm_h = 104 if not touch else 84
    bpm_panel(sk, img, f, sx, by, sw_, bpm_h)
    status_panel(sk, img, f, sx, by + bpm_h + 8, sw_, bottom_h - bpm_h - 8)
    rows_h = 3 * 44 + 2 * 6
    box_h = 82 if not touch else 58
    graph_h = H - 8 - box_h - 8 - rows_h - 8 - top
    pace_graph(sk, img, f, rx, top, gw_, graph_h)
    pace_rows(sk, img, f, rx, top + graph_h + 8, gw_)
    combo_box(sk, img, f, rx, H - 8 - box_h, gw_, box_h)
    # the hi-speed panel against the field, over the BGA
    hs_x = fx + fw + 28 if side == 0 else fx - 28 - HS_W
    hs_panel(sk, hs_x, min(120, lane_h - HS_H - 8))
    ready(sk, fx + fw // 2, lane_h // 2)
    return sk, img, bgname


# --- landscape double play: the two fields around a gap that holds the
# hi-speed panel, turntables at the outer corners and the gauge between
# them; [title, BGA, judge, status] on the left and [time, score graph,
# scores] on the right, BPM and MAX COMBO in the bottom corners --------------
LAND_DP = {"7": (52, 42), "5": (60, 57)}
DP_LANE_H, DP_KEYS_Y, DP_KEYS_H, DP_GAP = 540, 546, 64, 100
DP_COL = 220


def time_box(sk, img, f, x, y, w, h):
    """Difficulty and level, time left."""
    panel(img, (x, y, w, h))
    for k in range(1, 6):
        bw, bh = sk.parts.items[f"diff{k}"][2:]
        sk.put(sk.img(f"diff{k}"), x + 10, y + 7, bw, bh, op=[150 + k])
    nw, nh = sk.num("n_green", 96, 2)[1:]
    draw_label(img, f, x + 12, y + h - 13, "LV", 11, anchor="lm")
    sk.number("n_green", 96, 2, x + 32, y + h - 13 - nh // 2, color=TEXT)
    draw_label(img, f, x + w - 12, y + 7, "TIME LEFT", 10, anchor="ra")
    cw, ch = sk.num("n_mid", 163, 2)[1:]
    t0 = x + w - 12 - cw * 4 - 10
    sk.number("n_mid", 163, 2, t0, y + h - ch - 5)
    draw_label(img, f, t0 + cw * 2 + 5, y + h - 5 - ch // 2, ":", 20, TEXT, "bold", anchor="mm")
    sk.number("n_mid", 164, 2, t0 + cw * 2 + 10, y + h - ch - 5, zeropad=True)


def land_dp(fonts, parts, mode, touch=False):
    W, H = 1280, (TOUCH_H if touch else 720)
    lane_h = TOUCH_LANE_H if touch else DP_LANE_H
    kind = {"7": 2, "5": 3}[mode]
    name = f"vibeBMS Default {'14' if mode == '7' else '10'}K{' touch' if touch else ''}"
    sk = Skin(kind, name, W, H, parts)
    bgname = "bg_land_dp_touch.png" if touch else "bg_land_dp.png"
    sk.source("bg", bgname)
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H)
    img = background(W, H)
    f = fonts
    d = ImageDraw.Draw(img)
    sw, kw = LAND_DP[mode]
    kinds, _ = lane_kinds(mode)
    fw = sw + kw * len(kinds)
    f1 = (W - fw * 2 - DP_GAP) // 2
    f2 = f1 + fw + DP_GAP
    d.rectangle((f1 - 8, 0, f2 + fw + 8, H), fill=(8, 9, 14, 255))
    l1, _ = lanes(mode, f1, 0, sw, kw)
    l2, _ = lanes(mode, f2, 1, sw, kw)
    note_def(sk, [(x, w, k, i) for i, (x, w, k) in enumerate(l1 + l2)])
    # touch: no key row (the on-screen controls show the keys)
    keys_y, keys_h = (lane_h, 0) if touch else (DP_KEYS_Y, DP_KEYS_H)
    # turntables larger than their lanes at the outer corners
    tts = [None, None]
    size = 120
    if not touch:
        tts = [(f1 + 20, H - 8 - size // 2, size), (f2 + fw - 20, H - 8 - size // 2, size)]
    for part in ("under", "over"):
        field(sk, mode, 0, 0, f1, 0, lane_h, sw, kw, keys_y, keys_h, 0.6, part=part, tt=tts[0])
        field(sk, mode, 1, 1, f2, 0, lane_h, sw, kw, keys_y, keys_h, 0.6, part=part, tt=tts[1])
    # progress in the gap
    gx = W // 2 - 6
    d.rounded_rectangle((gx + 2, 4, gx + 10, lane_h - 4), 3, fill=(24, 28, 42, 255))
    progress(sk, gx, 0, lane_h)
    # the gauge between the turntables, LEVEL / gauge type / HI-SPEED under it
    if touch:
        gauge_block(sk, img, f, f1, lane_h + 4, f2 + fw - f1, GAUGE_BAR - 8)
    else:
        gx0 = tts[0][0] + size // 2 + 10
        gx1 = tts[1][0] - size // 2 - 10
        gauge_block(sk, img, f, gx0, DP_KEYS_Y + DP_KEYS_H + 6, gx1 - gx0, GAUGE_BAR)
        info_bar(sk, img, f, gx0, H - 8 - 24, gx1 - gx0, 24)
    # outer columns (above the turntables)
    lx, rx = 8, W - 8 - DP_COL
    col_b = lane_h + (52 if touch else 44)
    header(sk, img, f, lx, 8, DP_COL, 86)
    bh = DP_COL * 3 // 4
    bga_frame(sk, img, lx, 102, DP_COL, bh)
    jy = 102 + bh + 8
    if touch:
        judge_panel(sk, img, f, lx, jy, DP_COL, col_b - jy)
    else:
        judge_panel(sk, img, f, lx, jy, DP_COL, 200)
        status_panel(sk, img, f, lx, jy + 208, DP_COL, col_b - jy - 208)
    time_box(sk, img, f, rx, 8, DP_COL, 54)
    rows_h = 3 * 44 + 2 * 6
    pace_graph(sk, img, f, rx, 70, DP_COL, col_b - rows_h - 8 - 70)
    pace_rows(sk, img, f, rx, col_b - rows_h, DP_COL)
    if not touch:
        # bottom corners, beside the turntables
        cw_ = tts[0][0] - size // 2 - 8 - lx
        bpm_panel(sk, img, f, lx, col_b + 8, cw_, H - 8 - col_b - 8)
        combo_box(sk, img, f, W - 8 - cw_, col_b + 8, cw_, H - 8 - col_b - 8)
    # the hi-speed panel in the gap between the fields
    scale = min(1.0, (DP_GAP - 4) / HS_W)
    hs_panel(sk, W // 2 - int(HS_W * scale) // 2, min(120, lane_h - int(HS_H * scale) - 8), scale)
    ready_y = lane_h * 5 // 12 if touch else 300
    ready(sk, f1 + fw // 2, ready_y)
    ready(sk, f2 + fw // 2, ready_y)
    return sk, img, bgname


# --- portrait single play: title, field, gauge, then SCORE | JUDGE | BPM -------
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
    # level and time at the right end of the title
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
    judge_panel(sk, img, f, 248, 1118, 224, 156)
    bpm_panel(sk, img, f, 480, 1118, 216, 156)
    ready(sk, fx + fw // 2, 500)
    return sk, img


# --- result screen ------------------------------------------------------------
def relayout(sk, regions):
    """Move whole regions of a landscape design (portrait variants)."""
    sk.regions = regions


def compose(img, regions, w, h):
    out = background(w, h)
    for (sx, sy, sw, sh), (tx, ty) in regions:
        out.paste(img.crop((sx, sy, sx + sw, sy + sh)), (tx, ty))
    return out


SELECT_PORTRAIT = [
    ((16, 16, 660, 334), (30, 16)),     # title, stage file, chart facts
    ((16, 358, 660, 140), (30, 358)),   # personal best
    ((16, 506, 660, 154), (30, 1062)),  # options
    ((16, 668, 660, 40), (30, 1224)),   # path
]
def graph_obj(sk, kind, gid, extra=None):
    lst = sk.j.setdefault(kind, [])
    if gid not in sk._ids:
        sk._ids.add(gid)
        d = {"id": gid}
        d.update(extra or {})
        lst.append(d)
    return gid


def fade_in(t0, x, y, w, h, dy=12, dur=250, **kw):
    """Keyframes: slide up and fade in starting at t0 ms."""
    return [
        (x, y + dy, w, h, dict(time=0, a=0, **kw)),
        (x, y + dy, w, h, dict(time=t0, a=0, **kw)),
        (x, y, w, h, dict(time=t0 + dur, a=255, **kw)),
    ]


RANKS = ["AAA", "AA", "A", "B", "C", "D", "E", "F"]


def res_header(sk, img, f, w):
    """The RESULT strip across the top: AUTO PLAY and the clock."""
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, w, 43), fill=(18, 21, 32, 255))
    d.line((0, 43, w, 43), fill=EDGE + (255,))
    d.text((16, 22), "RESULT", font=f.get("black_it", 22), fill=TEXT, anchor="lm")
    aw, ah = sk.parts.items["auto_tag"][2:]
    sk.put(sk.img("auto_tag"), 130, 22 - ah // 2, aw, ah, op=[33])
    cw, ch = sk.num("n_small", 24, 2)[1:]
    r = w - 16
    sk.number("n_small", 25, 2, r - cw * 2, 22 - ch // 2, zeropad=True)
    draw_label(img, f, r - cw * 2 - 4, 21, ":", 14, TEXT, anchor="rm")
    sk.number("n_small", 24, 2, r - cw * 4 - 8, 22 - ch // 2, zeropad=True)


def res_clear(sk, img, f, x, y, w, h):
    """Clear type (and the one before), DJ level."""
    panel(img, (x, y, w, h), "CLEAR TYPE", f, accent=RED)
    lw, lh = sk.parts.items["lamp0"][2:]
    lw, lh = lw * 4 // 5, lh * 4 // 5
    graph_obj(sk, "imageset", "clear", {"ref": 370, "images": [sk.img(f"lamp{k}") for k in range(11)]})
    sk.put("clear", 0, 0, 0, 0, keys=[
        (x + 4, y + 24, lw, lh, dict(time=0, a=0)),
        (x + 4, y + 24, lw, lh, dict(time=150, a=0)),
        (x + 12, y + 24, lw, lh, dict(time=450, a=255)),
    ], loop=450)
    sw, sh = sk.parts.items["lamp_s0"][2:]
    graph_obj(sk, "imageset", "clear_before",
              {"ref": 371, "images": [sk.img(f"lamp_s{k}") for k in range(11)]})
    draw_label(img, f, x + 14, y + h - 22, "BEFORE", 11, anchor="lm")
    sk.put("clear_before", x + 66, y + h - 22 - sh // 2, sw, sh)
    draw_label(img, f, x + w - 14, y + 7, "DJ LEVEL", 12, anchor="ra")
    rw, rh = sk.parts.items["rank0"][2:]
    rw, rh = rw * 17 // 20, rh * 17 // 20
    for k in range(8):
        sk.put(sk.img(f"rank{k}"), 0, 0, 0, 0, op=[300 + k],
               keys=fade_in(500, x + w - 8 - rw, y + h - 6 - rh, rw, rh), loop=750)


def res_scores(sk, img, f, x, y, w, h):
    """The DJ level ladder; before > now (difference) for EX score, rate,
    miss count and max combo; the target."""
    panel(img, (x, y, w, h))
    d = ImageDraw.Draw(img)
    draw_label(img, f, x + 12, y + 16, "DJ LEVEL", 11, anchor="lm")
    cw_ = (w - 100) // 8
    for i, r_ in enumerate(reversed(RANKS)):
        cx = x + 88 + i * cw_
        d.rounded_rectangle((cx, y + 8, cx + cw_ - 4, y + 25), 4, fill=(32, 38, 56, 255))
        d.text((cx + (cw_ - 4) // 2, y + 17), r_, font=f.get("bold", 11), fill=(86, 96, 126),
               anchor="mm")
        k = RANKS.index(r_)
        sk.put(sk.img(f"rk_cell{k}"), cx, y + 8, cw_ - 4, 17, op=[300 + k])
    rows = [("EX SCORE", 150, 71, 152), ("SCORE RATE", None, None, None),
            ("MISS COUNT", 176, 76, 178), ("MAX COMBO", 173, 75, 175), ("TARGET", None, 151, 153)]
    top = y + 34
    rh = (h - 40) // len(rows)
    mw, mh = sk.num("n_mid", 71, 5)[1:]
    sw, sh = sk.num("n_small", 150, 5)[1:]
    gw, gh = sk.num("n_signed", 152, 6)[1:]
    c_before = x + w * 40 // 100
    c_now = x + w * 76 // 100
    c_diff = x + w - 12
    for i, (label, before, now, diff) in enumerate(rows):
        ry = top + i * rh
        if i:
            d.line((x + 10, ry, x + w - 10, ry), fill=(35, 42, 62, 255))
        cy = ry + rh // 2
        draw_label(img, f, x + 12, cy, label, 11, anchor="lm")
        if before:
            sk.number("n_small", before, 5, c_before - sw * 5, cy - sh // 2, color=DIM)
        if before or label == "SCORE RATE":
            d.polygon([(c_before + 9, cy - 4), (c_before + 15, cy), (c_before + 9, cy + 4)],
                      fill=DIM + (255,))
        if label == "SCORE RATE":
            sk.number("n_small", 183, 3, c_before - sw * 5 - 2, cy - sh // 2, color=DIM)
            sk.put(sk.img("white"), c_before - sw * 2 - 4, cy + sh // 2 - 5, 3, 3, r=DIM[0],
                   g=DIM[1], b=DIM[2])
            sk.number("n_small", 184, 2, c_before - sw * 2, cy - sh // 2, zeropad=True, color=DIM)
            rate_number(sk, img, f, c_now + 14, cy - mh // 2)
            continue
        sk.number("n_mid", now, 5, c_now - mw * 5, cy - mh // 2)
        sk.number("n_signed", diff, 6, c_diff - gw * 6, cy - gh // 2)
    w_, h_ = sk.parts.items["newrec"][2:]
    ny = top + rh // 2 - h_ // 2
    sk.put(sk.img("newrec"), 0, 0, 0, 0, op=[330, 61], keys=[
        (x + 96, ny, w_, h_, dict(time=0, a=0)),
        (x + 96, ny, w_, h_, dict(time=1000, a=0)),
        (x + 96, ny, w_, h_, dict(time=1200, a=255)),
        (x + 96, ny, w_, h_, dict(time=1700, a=255)),
        (x + 96, ny, w_, h_, dict(time=1900, a=120)),
    ], loop=1200)


def res_timing(sk, img, f, x, y, w, h):
    """Average and spread of the timing, FAST / SLOW totals."""
    panel(img, (x, y, w, h))
    cw, ch = sk.num("n_small", 374, 3)[1:]
    gw = sk.num("n_signed", 374, 4)[1]
    r = x + w - 14
    rows = ("AVG TIMING", "STD DEV", "FAST / SLOW")
    rh = (h - 12) // 3
    for i, label in enumerate(rows):
        cy = y + 6 + i * rh + rh // 2
        if i:
            ImageDraw.Draw(img).line((x + 10, y + 6 + i * rh, x + w - 10, y + 6 + i * rh),
                                     fill=(35, 42, 62, 255))
        draw_label(img, f, x + 12, cy, label, 11, anchor="lm")
        if i < 2:
            ref = 374 if i == 0 else 376
            draw_label(img, f, r, cy, "ms", 11, anchor="rm")
            dx = r - 22 - cw * 2
            sk.number("n_small", ref + 1, 2, dx, cy - ch // 2, zeropad=True)
            sk.put(sk.img("white"), dx - 5, cy + ch // 2 - 5, 3, 3)
            if i == 0:
                sk.number("n_signed", ref, 4, dx - 7 - gw * 4, cy - ch // 2)
            else:
                sk.number("n_small", ref, 3, dx - 7 - cw * 3, cy - ch // 2)
        else:
            sk.number("n_small", 424, 5, r - cw * 5, cy - ch // 2, color=(255, 140, 150))
            draw_label(img, f, r - cw * 5 - 6, cy, "/", 12, DIM, anchor="rm")
            sk.number("n_small", 423, 5, r - cw * 5 - 16 - cw * 5, cy - ch // 2,
                      color=(140, 195, 255))


def res_info(sk, img, f, x, y, w, h, stage=True):
    """Stage file (when there is room), difficulty, title, chart facts, the
    play count and the RETRY / CONTINUE buttons."""
    d = ImageDraw.Draw(img)
    if stage:
        sh = w * 3 // 4
        # original art behind the stage file (charts without one)
        art = Image.new("RGBA", (w, sh), (14, 16, 28, 255))
        ad = ImageDraw.Draw(art)
        for i, (cx, cy, col) in enumerate(((0.3, 0.3, PINK), (0.75, 0.65, TEAL))):
            for r_ in range(int(w * 0.45), 0, -6):
                a_ = int(70 * (1 - r_ / (w * 0.45)))
                ad.ellipse((cx * w - r_, cy * sh - r_, cx * w + r_, cy * sh + r_),
                           fill=mix((14, 16, 28), col, a_ / 255) + (255,))
        for r_ in range(40, w, 50):
            ad.ellipse((w // 2 - r_, sh // 2 - r_, w // 2 + r_, sh // 2 + r_),
                       outline=(40, 44, 62, 255))
        m = Image.new("L", (w, sh))
        ImageDraw.Draw(m).rounded_rectangle((0, 0, w - 1, sh - 1), 8, fill=255)
        img.paste(art, (x, y), m)
        d.rounded_rectangle((x, y, x + w - 1, y + sh - 1), 8, outline=EDGE + (255,))
        sk.j["image"].append({"id": "stagefile", "ref": 100})
        sk._ids.add("stagefile")
        sk.put("stagefile", x + 2, y + 2, w - 4, sh - 4, op=[191])
        y, h = y + sh + 8, h - sh - 8
    panel(img, (x, y, w, h), accent=PINK)
    for k in range(1, 6):
        bw, bh = sk.parts.items[f"diff{k}"][2:]
        sk.put(sk.img(f"diff{k}"), x + 14, y + 14, bw, bh, op=[150 + k])
    draw_label(img, f, x + 134, y + 25, "LV", 11, anchor="lm")
    nw, nh = sk.num("n_green", 96, 2)[1:]
    sk.number("n_green", 96, 2, x + 154, y + 25 - nh // 2, color=TEXT)
    sk.label_text(13, x + 200, y + 16, w - 214, 16, align=2, color=DIM)
    sk.label_text(12, x + 14, y + 44, w - 28, 28)
    sk.label_text(16, x + 14, y + 78, w - 28, 16, color=DIM)
    ly = y + 104
    d.line((x + 14, ly, x + w - 14, ly), fill=EDGE + (255,))
    cw, ch = sk.num("n_small", 74, 5)[1:]
    facts = [("NOTES", lambda fx, fy: sk.number("n_small", 74, 5, fx, fy)),
             ("BPM", None), ("PLAY", lambda fx, fy: sk.number("n_small", 77, 5, fx, fy)),
             ("CLEAR", lambda fx, fy: sk.number("n_small", 78, 5, fx, fy))]
    for i, (label, fn) in enumerate(facts):
        fx = x + 14 + (i % 2) * (w // 2)
        fy = ly + 10 + (i // 2) * 26
        draw_label(img, f, fx, fy + ch // 2, label, 11, anchor="lm")
        if fn:
            fn(fx + 60, fy)
        else:
            sk.number("n_small", 91, 3, fx + 60, fy, color=(130, 190, 255))
            draw_label(img, f, fx + 60 + cw * 3 + 6, fy + ch // 2, "-", 12, TEXT, anchor="mm")
            sk.number("n_small", 90, 3, fx + 72 + cw * 3, fy, color=(255, 130, 180))
    res_buttons(sk, x + w - 14, y + h - 44)


def res_buttons(sk, r, y):
    """RETRY and CONTINUE ending at r."""
    for key, act, dx in (("btn_retry", 9500, 140 + 8 + 120), ("btn_continue", 9501, 140)):
        bw_, bh_ = sk.parts.items[key][2:]
        bid = key + "_act"
        if bid not in sk._ids:
            sk._ids.add(bid)
            x_, y_, w_, h_ = sk.parts.items[key]
            sk.j["image"].append({"id": bid, "src": "parts", "x": x_, "y": y_, "w": w_, "h": h_,
                                  "act": act})
        sk.put(bid, r - dx, y, bw_, bh_)


def res_graphs(sk, img, f, x, y, w, h):
    """Gauge, FAST / SLOW per section and the timing distribution."""
    panel(img, (x, y, w, h), "GRAPH DATA", f)
    d = ImageDraw.Draw(img)
    gx, gw = x + 12, w - 24
    parts_h = h - 32 - 3 * 30
    gh = [parts_h * 36 // 100, parts_h * 28 // 100]
    gh.append(parts_h - gh[0] - gh[1])
    ty = y + 30
    # gauge
    draw_label(img, f, gx, ty + 10, "GROOVE GAUGE", 11, anchor="lm")
    for key, op in (("gt_normal", 118), ("gt_hard", 119), ("gt_hazard", 120), ("gt_easy", 121),
                    ("gt_aeasy", 124), ("gt_exhard", 125)):
        gw_, gh_ = sk.parts.items[key][2:]
        sk.put(sk.img(key), gx + 104, ty + 10 - gh_ // 2, gw_, gh_, op=[op])
    gauge_number(sk, gx + gw - gauge_number_width(sk), ty - 4)
    ty += 30
    d.rectangle((gx, ty, gx + gw, ty + gh[0]), fill=(6, 7, 11, 255))
    sk.put(graph_obj(sk, "gaugegraph", "gaugegraph", {"style": "vibebms"}), gx + 2, ty + 2,
           gw - 4, gh[0] - 4)
    ty += gh[0] + 4
    # FAST / SLOW per section
    draw_label(img, f, gx, ty + 13, "FAST", 11, BLUE, "bold", anchor="lm")
    draw_label(img, f, gx + 38, ty + 13, "/", 11, DIM, anchor="lm")
    draw_label(img, f, gx + 48, ty + 13, "SLOW", 11, RED, "bold", anchor="lm")
    ty += 26
    d.rectangle((gx, ty, gx + gw, ty + gh[1]), fill=(6, 7, 11, 255))
    sk.put(graph_obj(sk, "judgegraph", "fsgraph", {"type": 2, "style": "vibebms"}), gx + 2,
           ty + 2, gw - 4, gh[1] - 4)
    ty += gh[1] + 4
    draw_label(img, f, gx, ty + 13, "TIMING  (FAST \u2190  \u2192 SLOW)", 11, anchor="lm")
    ty += 26
    d.rectangle((gx, ty, gx + gw, ty + gh[2]), fill=(6, 7, 11, 255))
    sk.put(graph_obj(sk, "timingdistributiongraph", "timing", {"style": "vibebms"}), gx + 2,
           ty + 2, gw - 4, gh[2] - 4)


def result_layout(fonts, parts, portrait=False):
    W, H = (720, 1280) if portrait else (1280, 720)
    sk = Skin(7, "vibeBMS Default Result", W, H, parts)
    sk.j.update({"input": 500, "fadeout": 400, "scene": 3600000})
    sk.source("bg", f"bg_result{'_portrait' if portrait else ''}.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, W, H, fixed=True)
    img = background(W, H)
    f = fonts
    res_header(sk, img, f, W)
    if portrait:
        res_info(sk, img, f, 8, 52, 704, 224, stage=False)
        res_clear(sk, img, f, 8, 284, 704, 140)
        res_scores(sk, img, f, 8, 432, 704, 200)
        judge_panel(sk, img, f, 8, 640, 400, 190)
        res_timing(sk, img, f, 416, 640, 296, 190)
        res_graphs(sk, img, f, 8, 838, 704, 434)
    else:
        res_clear(sk, img, f, 12, 56, 404, 150)
        res_scores(sk, img, f, 12, 214, 404, 196)
        judge_panel(sk, img, f, 12, 418, 404, 182)
        res_timing(sk, img, f, 12, 608, 404, 100)
        res_info(sk, img, f, 428, 56, 424, 652)
        res_graphs(sk, img, f, 864, 56, 404, 652)
    return sk, img


# --- music select ---------------------------------------------------------------
LAMP_COLORS = [(60, 64, 80), (200, 50, 70), (170, 110, 255), (200, 160, 255), (70, 210, 120),
               (60, 160, 255), (255, 80, 92), (255, 210, 60), (45, 226, 199), (255, 180, 230),
               (255, 255, 255)]


def select_layout(fonts, parts, portrait=False):
    W, H = 1280, 720
    sk = Skin(5, "vibeBMS Default Select", *((720, 1280) if portrait else (W, H)), parts)
    if portrait:
        relayout(sk, SELECT_PORTRAIT)
    sk.j.update({"input": 300, "scene": 3600000, "fadeout": 0})
    sk.source("bg", f"bg_select{'_portrait' if portrait else ''}.png")
    sk.put(sk.img("bg", sid="bg"), 0, 0, sk.w, sk.h, fixed=True)
    img = background(W, H)
    f = fonts
    d = ImageDraw.Draw(img)
    # --- song list (right) ---
    if portrait:
        slots, center, bh, gap = 12, 5, 44, 46
        lx, lw = 58, 632
        cy = 510 + center * gap
    else:
        slots, center, bh, gap = 17, 8, 44, 46
        lx, lw = 704, 560
        cy = H // 2 - bh // 2
    off, on = [], []
    for k in range(slots):
        y = cy + (k - center) * gap
        off.append({"id": "bar", "dst": [sk.key(lx, y, lw, bh, fixed=True, time=0, r=190, g=195,
                                                b=210)]})
        on.append({"id": "bar", "dst": [sk.key(lx - 28, y, lw + 28, bh, fixed=True, time=0)]})
    graph_obj(sk, "imageset", "bar", {"images": [sk.img(f"bar{k}") for k in range(8)]})
    sk.j["text"].append({"id": "bartext", "ref": 0, "align": 0, "overflow": 1})
    # sub-destinations: y-up offsets from the bar's bottom
    rel = lambda x, y, w, h, **kw: dict(time=0, x=x, y=bh - (y + h), w=w, h=h, **kw)
    level = []
    for k, col in enumerate(DIFF_COLORS):
        iid, cw, ch = sk.num("n_mid", 96, 2)
        level.append({"id": iid, "dst": [rel(lw - 28 - 2 * cw, 8, cw, ch, r=col[0], g=col[1],
                                                 b=col[2])]})
    lamp = []
    for k, col in enumerate(LAMP_COLORS):
        lamp.append({"id": sk.img("lampcell"), "dst": [rel(18, 6, 8, 32, r=col[0], g=col[1],
                                                           b=col[2])]})
    sk.j["songlist"] = {
        "id": "songlist", "center": center, "clickable": list(range(slots)),
        "listoff": off, "liston": on,
        "text": [{"id": "bartext", "dst": [rel(36, 11, lw - 120, 22)]}],
        "level": level, "lamp": lamp,
    }
    sk.put("songlist", 0, 0, sk.w, sk.h, fixed=True)
    cw_, ch_ = parts.items["barcursor"][2:]
    sk.put(sk.img("barcursor"), 0, 0, 0, 0, blend=2, loop=0, fixed=True, keys=[
        (lx - 28 - 10, cy - 4, lw + 28 + 20, bh + 8, dict(time=0, a=255)),
        (lx - 28 - 10, cy - 4, lw + 28 + 20, bh + 8, dict(time=600, a=140)),
        (lx - 28 - 10, cy - 4, lw + 28 + 20, bh + 8, dict(time=1200, a=255)),
    ])
    # --- left: selected chart ---
    header(sk, img, f, 16, 16, 660, 86)
    # stage file / banner (4:3)
    d.rounded_rectangle((16, 110, 336, 350), 6, fill=(0, 0, 0, 255), outline=EDGE + (255,))
    sk.j["image"].append({"id": "stagefile", "ref": 100})
    sk._ids.add("stagefile")
    sk.put("stagefile", 20, 114, 312, 234, op=[191])
    sk.j["image"].append({"id": "banner", "ref": 102})
    sk._ids.add("banner")
    sk.put("banner", 20, 114, 312, 234, op=[190, 193])
    # chart facts
    panel(img, (344, 110, 332, 240), "CHART", f)
    draw_label(img, f, 358, 146, "LEVEL", 13, TEXT)
    for k, col in enumerate(DIFF_COLORS[:6]):
        sk.number("n_big", 96, 2, 600, 134, color=col, op=[150 + k])
    draw_label(img, f, 358, 190, "BPM", 13, TEXT)
    sk.number("n_mid", 91, 3, 470, 180, color=(130, 190, 255))
    draw_label(img, f, 534, 190, "-", 14, TEXT)
    sk.number("n_mid", 90, 3, 548, 180, color=(255, 130, 180), op=[177])
    draw_label(img, f, 358, 228, "NOTES", 13, TEXT)
    sk.number("n_mid", 74, 5, 556, 218)
    draw_label(img, f, 358, 266, "LENGTH", 13, TEXT)
    sk.number("n_mid", 1163, 2, 566, 256)
    draw_label(img, f, 612, 266, ":", 16, TEXT, "bold", anchor="ma")
    sk.number("n_mid", 1164, 2, 620, 256, zeropad=True)
    lx2 = 358
    for key, op in (("lbl_ln", 173), ("lbl_rand", 179), ("lbl_bpm", 177), ("lbl_bga", 171)):
        w_, h_ = parts.items[key][2:]
        sk.put(sk.img(key), lx2, 310, w_, h_, op=[op, 2])
        lx2 += w_ + 6
    # personal best
    panel(img, (16, 358, 660, 140), "PERSONAL BEST", f, accent=AMBER)
    graph_obj(sk, "imageset", "clear", {"ref": 370, "images": [sk.img(f"lamp{k}") for k in range(11)]})
    lw_, lh_ = parts.items["lamp0"][2:]
    sk.put("clear", 26, 380, lw_ * 2 // 3, lh_ * 2 // 3, op=[2])
    rw, rh = parts.items["rank0"][2:]
    for k in range(8):
        sk.put(sk.img(f"rank{k}"), 676 - 16 - rw // 2, 362, rw // 2, rh // 2, op=[320 + k])
    for k, (label, x) in enumerate((("EX SCORE", 30), ("RATE", 200), ("MISS", 350), ("COMBO", 470))):
        draw_label(img, f, x, 448, label, 11)
    sk.number("n_mid", 71, 5, 30, 462, op=[2])
    sk.number("n_mid", 102, 3, 200, 462, op=[2])
    sk.put(sk.img("white"), 200 + 21 * 3 + 1, 484, 4, 4, op=[2])
    sk.number("n_mid", 103, 2, 200 + 21 * 3 + 6, 462, zeropad=True, op=[2])
    sk.number("n_mid", 76, 4, 350, 462, op=[2])
    sk.number("n_mid", 75, 5, 470, 462, op=[2])
    draw_label(img, f, 590, 448, "PLAYS", 11)
    sk.number("n_small", 77, 4, 590, 466, op=[2])
    # options: clickable buttons (vibeBMS option ids 9000 + i)
    panel(img, (16, 506, 660, 154), "OPTIONS", f)
    for i in range(10):
        bx, by = 26 + (i % 5) * 130, 530 + (i // 5) * 64
        bid = f"optbtn{i}"
        if bid not in sk._ids:
            sk._ids.add(bid)
            x_, y_, w_, h_ = parts.items["optbtn"]
            sk.j["image"].append({"id": bid, "src": "parts", "x": x_, "y": y_, "w": w_, "h": h_,
                                  "act": 9000 + i})
        sk.put(bid, bx, by, 124, 60)
        sk.label_text(9100 + i, bx + 6, by + 8, 112, 12, align=1, color=DIM)
        sk.label_text(9000 + i, bx + 6, by + 28, 112, 20, align=1)
    # footer
    panel(img, (16, 668, 660, 40))
    bw_, bh_ = parts.items["btn_back"][2:]
    if "back_act" not in sk._ids:
        sk._ids.add("back_act")
        x_, y_, w_, h_ = parts.items["btn_back"]
        sk.j["image"].append({"id": "back_act", "src": "parts", "x": x_, "y": y_, "w": w_, "h": h_,
                              "act": 9900})
    sk.put("back_act", 22, 674, bw_, bh_)
    sk.label_text(1000, 116, 678, 340, 18, color=DIM)
    draw_label(img, f, 664, 688, "F10 / : SEARCH    F5 AUTOPLAY", 11, DIM, anchor="rm")
    if portrait:
        img = compose(img, SELECT_PORTRAIT, 720, 1280)
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
    sk, img, bg = land_sp(fonts, parts, "9", 0)
    emit(sk, img, "play9.json", bg)
    for mode in ("7", "5", "9"):
        for side in ((0, 1) if mode != "9" else (0,)):
            sk, img, bg = land_sp(fonts, parts, mode, side, touch=True)
            emit(sk, img, f"play{mode}_touch{'_2p' if side else ''}.json", bg)
    for mode in ("7", "5", "9"):
        for side in ((0, 1) if mode != "9" else (0,)):
            emit(*port_sp(fonts, parts, mode, side),
                 f"play{mode}_portrait{'_2p' if side else ''}.json", "bg_port.png")
    for mode, name in (("7", "play14"), ("5", "play10")):
        for touch in (False, True):
            sk, img, bg = land_dp(fonts, parts, mode, touch)
            emit(sk, img, f"{name}{'_touch' if touch else ''}.json", bg)
    emit(*result_layout(fonts, parts), "result.json", "bg_result.png")
    emit(*select_layout(fonts, parts), "select.json", "bg_select.png")
    emit(*result_layout(fonts, parts, True), "result_portrait.json", "bg_result_portrait.png")
    emit(*select_layout(fonts, parts, True), "select_portrait.json", "bg_select_portrait.png")
    for name, img in bgs.items():
        img.convert("RGB").quantize(128).convert("RGB").save(OUT / name, optimize=True)
    total = sum(p.stat().st_size for p in OUT.iterdir())
    print(f"wrote {OUT} ({total // 1024} KiB)")


if __name__ == "__main__":
    sys.exit(main())
