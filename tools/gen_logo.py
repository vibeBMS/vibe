#!/usr/bin/env python3
"""Generate the vibeBMS logo and icons.

The mark is a turntable (with a sound wave on it) and the first two keys of
a 1P controller: key 1 (white) beside the disc, key 2 (blue) raised. The
wordmark is "vibe" + "BMS" in Inter (SIL OFL 1.1), converted to outlines so
the SVGs need no font.

    pip install fonttools cairosvg
    python3 tools/gen_logo.py [--fonts DIR]   # DIR holds Inter-ExtraBold.otf, Inter-SemiBold.otf

Writes assets/logo/ (SVGs and PNG icons), the web page's icons, the window
icon, the Android launcher icons and the iOS home screen icons.
"""

import argparse
import math
import os

import cairosvg
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BG = "#0b0d14"
TEAL = "#2de2c7"
BLUE = "#3f8cff"
RED = "#ff4d5e"
WHITE = "#e8ecf2"
INK = "#141824"
TEAL_ON_LIGHT = "#1a9e8b"


def mark(fg):
    """The mark in a 120x120 box."""
    cx, cy, r, sw = 38, 60, 29, 7
    pts = []
    w, amp = r * 1.4, r * 0.3
    for i in range(61):
        t = i / 60
        x = cx - w / 2 + w * t
        y = cy + math.sin(t * math.pi * 3) * amp * math.sin(t * math.pi)
        pts.append(f"{x:.2f},{y:.2f}")
    a = -math.pi / 4
    return (
        f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{fg}" stroke-width="{sw}"/>'
        f'<circle cx="{cx}" cy="{cy}" r="{r * 0.7:.1f}" fill="none" stroke="{fg}" stroke-opacity=".25" stroke-width="2"/>'
        f'<circle cx="{cx + r * math.cos(a):.2f}" cy="{cy + r * math.sin(a):.2f}" r="{sw * 0.85:.2f}" fill="{RED}"/>'
        f'<polyline points="{" ".join(pts)}" fill="none" stroke="{TEAL}" stroke-width="6" '
        'stroke-linecap="round" stroke-linejoin="round"/>'
        f'<rect x="79" y="50" width="14" height="40" rx="3.5" fill="{fg}"/>'
        f'<rect x="97" y="30" width="14" height="40" rx="3.5" fill="{BLUE}"/>'
    )


class Face:
    def __init__(self, path):
        self.font = TTFont(path)
        self.glyphs = self.font.getGlyphSet()
        self.cmap = self.font.getBestCmap()
        self.upm = self.font["head"].unitsPerEm

    def run(self, text, x, baseline, size, tracking):
        """SVG path data for `text` and its advance (tracking in em)."""
        s = size / self.upm
        d = []
        for i, ch in enumerate(text):
            g = self.glyphs[self.cmap[ord(ch)]]
            pen = SVGPathPen(self.glyphs)
            g.draw(TransformPen(pen, (s, 0, 0, -s, x, baseline)))
            d.append(pen.getCommands())
            x += g.width * s
            if i < len(text) - 1:
                x += tracking * size
        return " ".join(d), x

    def top(self, ch, size):
        """Height of a glyph above the baseline."""
        pen = BoundsPen(self.glyphs)
        self.glyphs[self.cmap[ord(ch)]].draw(pen)
        return pen.bounds[3] * size / self.upm


def lockup(bold, semi, fg, sub):
    """Mark + wordmark, laid out as on the approved candidate sheet."""
    size, baseline = 80, 89
    word, x = bold.run("vibe", 137.5, baseline, size, -0.045)
    small = 27.5
    # "BMS" sits on the top of the tall letters
    bms_base = baseline - bold.top("b", size) + semi.top("B", small)
    bms, x = semi.run("BMS", x + 10, bms_base, small, 0.18)
    width = math.ceil(x + 4)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} 120" width="{width * 2}" height="240">'
        f"<title>vibeBMS</title>{mark(fg)}"
        f'<path d="{word}" fill="{fg}"/><path d="{bms}" fill="{sub}"/></svg>'
    )


def icon(radius=26):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" width="512" height="512">'
        f'<rect width="120" height="120" rx="{radius}" fill="{BG}"/>{mark(WHITE)}</svg>'
    )


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text + "\n")


def png(svg, path, size):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    cairosvg.svg2png(bytestring=svg.encode(), write_to=path, output_width=size, output_height=size)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fonts", default="/usr/share/fonts/opentype/inter")
    args = ap.parse_args()
    bold = Face(os.path.join(args.fonts, "Inter-ExtraBold.otf"))
    semi = Face(os.path.join(args.fonts, "Inter-SemiBold.otf"))

    out = os.path.join(ROOT, "assets", "logo")
    write(os.path.join(out, "logo-dark.svg"), lockup(bold, semi, WHITE, TEAL))
    write(os.path.join(out, "logo-light.svg"), lockup(bold, semi, INK, TEAL_ON_LIGHT))
    write(
        os.path.join(out, "mark.svg"),
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120">{mark(WHITE)}</svg>',
    )
    rounded = icon()
    write(os.path.join(out, "icon.svg"), rounded)
    for n in (512, 256, 128, 64, 32):
        png(rounded, os.path.join(out, f"icon-{n}.png"), n)

    # web page: favicon, and a full-bleed square for iOS (which rounds it)
    write(os.path.join(ROOT, "web", "icon.svg"), rounded)
    png(icon(radius=0), os.path.join(ROOT, "web", "apple-touch-icon.png"), 180)
    # window icon
    png(rounded, os.path.join(ROOT, "crates", "vibebms", "assets", "icon.png"), 128)
    # Android launcher
    res = os.path.join(ROOT, "crates", "vibebms-android", "res")
    for dpi, n in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        png(rounded, os.path.join(res, f"mipmap-{dpi}", "ic_launcher.png"), n)
    # iOS home screen (full-bleed: iOS rounds it; named as Info.plist lists them)
    square = icon(radius=0)
    for name, n in (("AppIcon60x60@2x", 120), ("AppIcon60x60@3x", 180),
                    ("AppIcon76x76@2x", 152), ("AppIcon83.5x83.5@2x", 167)):
        png(square, os.path.join(ROOT, "platform", "ios", f"{name}.png"), n)
    print("wrote", out)


if __name__ == "__main__":
    main()
