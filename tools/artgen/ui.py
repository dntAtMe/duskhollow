"""Our own UI skin: "iron & oak".

Every interface image the client loads is regenerated from scratch here, in the same
oldschool look as the sprites (5-shade ramps + 4x4 ordered dithering, dark outlines):
dark stained oak panels, blackened iron bands with brass rivets and corner plates,
oxblood leather title bars with stitching, recessed iron-rimmed wells, and hand-made
5x7 pixel lettering for the few labels the original art has baked in.

The client positions everything with the ORIGINAL images' measured geometry
(`hud.rs`, `spells_ui.rs`, `items_ui.rs`, `chat.rs`, `minimap.rs`, `nameplates.rs`),
so each image keeps the original pixel size and puts its wells / bar tracks / buttons /
holes exactly where the client expects them; the numbers are repeated next to each
generator below.

Output: `custom_assets/content/ui/<original name>.png`; with `--art custom`
the client resolves these instead of the originals (same bare file name).

    python -I tools/artgen/ui.py              # write the override set
    python -I tools/artgen/ui.py --preview    # also a contact sheet in custom_assets/preview
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets" / "content" / "override" / "ui"
PREVIEW = ROOT / "custom_assets" / "preview"

# --- palette -----------------------------------------------------------------------------

BAYER4 = (np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], dtype=float) + 0.5) / 16.0

# 5 shades each, dark -> light.
RAMPS = {
    "oak": ["#0f0a07", "#1a120c", "#261a11", "#352517", "#4a341f"],
    "iron": ["#0c0d0f", "#1c1e21", "#313438", "#4d5157", "#787d84"],
    "brass": ["#2a1b08", "#56390f", "#87621d", "#b78f36", "#e5c566"],
    "leather": ["#1a0806", "#300e0a", "#4a1611", "#672118", "#8a3122"],
    "well": ["#040303", "#0a0807", "#120f0d", "#1b1714", "#27211d"],
    "parch": ["#3b2c19", "#7a633e", "#b49a68", "#dcc799", "#f5e8c6"],
    "hp": ["#2a0605", "#5a0e0b", "#8c1913", "#be2f1e", "#e45a3a"],
    "mp": ["#05122a", "#0b2858", "#15498c", "#2a70c0", "#5c9de4"],
    "xp": ["#1a0a22", "#371446", "#5a236c", "#86399a", "#b766c6"],
    "green": ["#07200b", "#113f17", "#1d6426", "#338c38", "#5cb653"],
    "amber": ["#2a1604", "#5a320a", "#8e5612", "#c48420", "#efb84a"],
    "bone": ["#1f1d1a", "#3d3a35", "#625e56", "#8f8a7f", "#c2bcae"],
    "ember": ["#200403", "#4a0a06", "#7c150b", "#b52c12", "#ef5a22"],
}
OUTLINE = (10, 7, 5)


def _rgb(h: str) -> tuple[int, int, int]:
    return int(h[1:3], 16), int(h[3:5], 16), int(h[5:7], 16)


TABLE = {k: np.array([_rgb(c) for c in v], dtype=np.uint8) for k, v in RAMPS.items()}


# --- noise -------------------------------------------------------------------------------


def vnoise(h: int, w: int, sx: float, sy: float, seed: int) -> np.ndarray:
    """Smooth value noise in 0..1 with lattice spacing (sx, sy) pixels."""
    rng = np.random.default_rng(seed)
    lat = rng.random((int(h / sy) + 3, int(w / sx) + 3))
    yy, xx = np.indices((h, w)).astype(float)
    x, y = xx / sx, yy / sy
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = x - x0, y - y0
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = lat[y0, x0], lat[y0, x0 + 1]
    c, d = lat[y0 + 1, x0], lat[y0 + 1, x0 + 1]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


def fbm(h: int, w: int, s: float, seed: int, stretch: float = 1.0) -> np.ndarray:
    out = np.zeros((h, w))
    for i, g in enumerate((0.5, 0.3, 0.2)):
        k = s / (2**i)
        out += g * vnoise(h, w, max(k, 1.0), max(k * stretch, 1.0), seed + 17 * i)
    return out


# --- canvas ------------------------------------------------------------------------------


class Canvas:
    def __init__(self, w: int, h: int):
        self.w, self.h = w, h
        self.px = np.zeros((h, w, 4), dtype=np.uint8)
        yy, xx = np.indices((h, w))
        self.yy, self.xx = yy, xx
        self.bayer = BAYER4[yy % 4, xx % 4]

    # masks ------------------------------------------------------------------------
    def rect(self, x: float, y: float, w: float, h: float) -> np.ndarray:
        return (self.xx >= x) & (self.xx < x + w) & (self.yy >= y) & (self.yy < y + h)

    def disc(self, cx: float, cy: float, r: float) -> np.ndarray:
        return self.dist(cx, cy) <= r

    def dist(self, cx: float, cy: float) -> np.ndarray:
        return np.hypot(self.xx + 0.5 - cx, self.yy + 0.5 - cy)

    def chamfer(self, x0, y0, x1, y1, c: int) -> np.ndarray:
        """Rectangle [x0,x1) x [y0,y1) with corners cut at 45 degrees."""
        m = self.rect(x0, y0, x1 - x0, y1 - y0)
        dx = np.minimum(self.xx - x0, x1 - 1 - self.xx)
        dy = np.minimum(self.yy - y0, y1 - 1 - self.yy)
        return m & (dx + dy >= c)

    # painting ---------------------------------------------------------------------
    def paint(self, mask, ramp: str, shade, alpha=255):
        shade = np.broadcast_to(np.asarray(shade, dtype=float), (self.h, self.w))
        lvl = np.clip(shade * 4 + (self.bayer - 0.5) * 0.9, 0, 4).round().astype(int)
        col = TABLE[ramp][lvl]
        self.px[mask, :3] = col[mask]
        a = np.broadcast_to(np.asarray(alpha), (self.h, self.w))
        self.px[mask, 3] = a[mask]

    def solid(self, mask, color, alpha=255):
        self.px[mask, :3] = color[:3]
        self.px[mask, 3] = alpha

    def outline(self, mask, color=OUTLINE, alpha=255):
        """Dark 1 px ring around `mask` (4-neighbourhood), only on pixels outside it."""
        g = grow(mask)
        self.solid(g & ~mask, color, alpha)

    def save(self, name: str):
        OUT.mkdir(parents=True, exist_ok=True)
        Image.fromarray(self.px).save(OUT / f"{name}.png", optimize=True)
        SAVED.append(name)


SAVED: list[str] = []


def grow(mask, n: int = 1):
    g = mask.copy()
    for _ in range(n):
        h = g.copy()
        h[1:, :] |= g[:-1, :]
        h[:-1, :] |= g[1:, :]
        h[:, 1:] |= g[:, :-1]
        h[:, :-1] |= g[:, 1:]
        g = h
    return g


# --- materials ---------------------------------------------------------------------------


def wood(c: Canvas, seed: int, horizontal: bool = False, plank: int = 24, base: float = 0.42):
    """Dark oak planks: grain streaks along the plank, seams, per-plank tone, knots."""
    h, w = c.h, c.w
    across = (c.yy if horizontal else c.xx).astype(float)
    rng = np.random.default_rng(seed)
    idx = np.floor(across / plank).astype(int)
    tone = rng.uniform(-0.07, 0.07, idx.max() + 2)[idx]
    phase = rng.uniform(0, 50, idx.max() + 2)[idx]
    sx, sy = (40.0, 2.5) if horizontal else (2.5, 40.0)
    grain = vnoise(h, w, sx, sy, seed + 1)
    wobble = vnoise(h, w, sx * 2, sy * 2, seed + 2)
    streak = np.sin(across * 1.7 + wobble * 7 + phase)
    shade = base + tone + 0.16 * (grain - 0.5) + 0.06 * streak
    shade += 0.05 * (vnoise(h, w, 9, 9, seed + 3) - 0.5)
    pos = across - idx * plank
    shade = np.where(pos < 1, 0.02, shade)  # seam
    shade = np.where((pos >= 1) & (pos < 2), shade + 0.12, shade)  # lit edge
    # a few knots
    for _ in range(max(1, (h * w) // 9000)):
        kx, ky = rng.uniform(0, w), rng.uniform(0, h)
        r = rng.uniform(1.5, 3.0)
        dx, dy = (c.xx + 0.5 - kx), (c.yy + 0.5 - ky)
        d = np.hypot(dx / (2.2 if horizontal else 1.0), dy / (1.0 if horizontal else 2.2))
        shade = np.where(d < r, 0.08, np.where(d < r + 1.5, shade - 0.1, shade))
    return shade


def metal(c: Canvas, seed: int, base: float = 0.5):
    s = base + 0.22 * (fbm(c.h, c.w, 10, seed) - 0.5)
    pits = vnoise(c.h, c.w, 1.5, 1.5, seed + 9) > 0.93
    return np.where(pits, s - 0.25, s)


def leather(c: Canvas, seed: int, base: float = 0.48):
    return base + 0.28 * (fbm(c.h, c.w, 7, seed) - 0.5) + 0.08 * (vnoise(c.h, c.w, 1.2, 1.2, seed + 5) - 0.5)


# --- building blocks ---------------------------------------------------------------------


def _sides(c: Canvas, x0, y0, x1, y1):
    """For each pixel: distance to the nearest edge of the rect and whether that edge is lit
    (top / left) or shaded (bottom / right)."""
    dt, dl = c.yy - y0, c.xx - x0
    db, dr = y1 - 1 - c.yy, x1 - 1 - c.xx
    d = np.minimum(np.minimum(dt, dl), np.minimum(db, dr))
    lit = np.minimum(dt, dl) <= np.minimum(db, dr)
    return d, lit


def band(c: Canvas, x0, y0, x1, y1, t: int, ramp="iron", seed=0, raised=True, base=0.5, mask=None):
    """Bevelled frame band of thickness `t` inside rect [x0,x1) x [y0,y1): dark outer and
    inner pixel, lit top/left (raised) or bottom/right (recessed)."""
    outer = c.rect(x0, y0, x1 - x0, y1 - y0) if mask is None else mask
    inner = c.rect(x0 + t, y0 + t, x1 - x0 - 2 * t, y1 - y0 - 2 * t)
    m = outer & ~inner
    d, lit = _sides(c, x0, y0, x1, y1)
    s = metal(c, seed, base) if ramp == "iron" else np.full((c.h, c.w), base)
    if ramp == "brass":
        s = base + 0.12 * (vnoise(c.h, c.w, 3, 3, seed) - 0.5)
    up = lit if raised else ~lit
    s = np.where(up, s + 0.2, s - 0.18)
    s = np.where(d == 1, np.where(up, s + 0.15, s - 0.05), s)
    c.paint(m, ramp, s)
    c.solid(m & (d == 0), OUTLINE)
    if t >= 3:
        c.solid(m & (d == t - 1), OUTLINE)
    return inner


def rivet(c: Canvas, cx: float, cy: float, r: float = 2.0, ramp="brass"):
    d = c.dist(cx, cy)
    m = d <= r
    nx, ny = (c.xx + 0.5 - cx) / max(r, 0.1), (c.yy + 0.5 - cy) / max(r, 0.1)
    s = 0.55 - 0.45 * (nx + ny) / 1.4
    c.paint(m, ramp, np.clip(s, 0.05, 1.0))
    c.solid((d > r) & (d <= r + 1.0), OUTLINE)


def well(c: Canvas, x, y, w, h, rim: int = 2, ramp="well"):
    """Recessed slot: inner rect (x, y, w, h) exact, iron rim of `rim` px outside it
    (shadowed top/left, lit bottom/right) and a dark interior with an inner shadow."""
    x0, y0, x1, y1 = x - rim, y - rim, x + w + rim, y + h + rim
    band(c, x0, y0, x1, y1, rim, "iron", seed=x * 7 + y, raised=False, base=0.45)
    c.solid(c.rect(x0, y0, x1 - x0, y1 - y0) & (_sides(c, x0, y0, x1, y1)[0] == 0), OUTLINE)
    inner = c.rect(x, y, w, h)
    d, lit = _sides(c, x, y, x + w, y + h)
    s = 0.22 + 0.25 * (c.yy - y) / max(h, 1) + 0.1 * (vnoise(c.h, c.w, 5, 5, x + y) - 0.5)
    s = np.where(lit & (d < 2), s - 0.25, s)
    c.paint(inner, ramp, s)


def panel(c: Canvas, x0, y0, x1, y1, seed: int, border: int = 6, alpha=255, rivets=True, plank=26):
    """Window body: oak planks inside a riveted iron frame with brass corner plates."""
    outer = c.chamfer(x0, y0, x1, y1, 3)
    inner = c.rect(x0 + border, y0 + border, x1 - x0 - 2 * border, y1 - y0 - 2 * border)
    s = wood(c, seed, plank=plank)
    d, _ = _sides(c, x0 + border, y0 + border, x1 - border, y1 - border)
    s = s - 0.18 * np.exp(-np.maximum(d, 0) / 6.0)
    c.paint(inner, "oak", s, alpha)
    band(c, x0, y0, x1, y1, border, "iron", seed + 1, mask=outer)
    # corner plates
    for cx, cy in ((x0, y0), (x1 - 13, y0), (x0, y1 - 13), (x1 - 13, y1 - 13)):
        plate = c.chamfer(cx, cy, cx + 13, cy + 13, 2) & outer
        band(c, cx, cy, cx + 13, cy + 13, 2, "brass", seed + 2, base=0.55, mask=plate)
        c.paint(plate & c.rect(cx + 2, cy + 2, 9, 9), "brass", 0.5 + 0.1 * (vnoise(c.h, c.w, 3, 3, seed) - 0.5))
        rivet(c, cx + 6.5, cy + 6.5, 1.8, "iron")
    if rivets:
        mid = border / 2
        for edge_len, horiz in ((x1 - x0, True), (y1 - y0, False)):
            n = max(0, int((edge_len - 40) / 56))
            for i in range(1, n + 1):
                t = 13 + (edge_len - 26) * i / (n + 1)
                if horiz:
                    rivet(c, x0 + t, y0 + mid, 1.3, "iron")
                    rivet(c, x0 + t, y1 - mid, 1.3, "iron")
                else:
                    rivet(c, x0 + mid, y0 + t, 1.3, "iron")
                    rivet(c, x1 - mid, y0 + t, 1.3, "iron")
    return inner


def leather_band(c: Canvas, x0, y0, x1, y1, seed: int, base=0.5, stitch=True):
    m = c.rect(x0, y0, x1 - x0, y1 - y0)
    s = leather(c, seed, base) + 0.18 * (1 - np.abs((c.yy - y0) / max(y1 - y0 - 1, 1) - 0.4) * 2) * 0.5
    c.paint(m, "leather", s)
    c.solid(m & (c.yy == y0), OUTLINE)
    c.solid(m & (c.yy == y1 - 1), OUTLINE)
    c.paint(m & (c.yy == y0 + 1), "leather", 0.85)
    c.paint(m & (c.yy == y1 - 2), "leather", 0.12)
    if stitch and y1 - y0 >= 12:
        dash = ((c.xx // 2) % 3 == 0) & (c.xx >= x0 + 6) & (c.xx < x1 - 6)
        for yy in (y0 + 3, y1 - 4):
            c.paint(m & dash & (c.yy == yy), "parch", 0.35)


def title_bar(c: Canvas, x0, y0, x1, y1, text: str, seed: int, scale=2, cx=None):
    leather_band(c, x0, y0, x1, y1, seed)
    for x in (x0 + 7, x1 - 8):
        rivet(c, x + 0.5, (y0 + y1) / 2, 2.2)
    if text:
        draw_text(c, text, (x0 + x1) / 2 if cx is None else cx, (y0 + y1) / 2, scale, "parch", bright=1.0)


def tab_strip(c: Canvas, x0, y0, x1, y1, labels, seed: int):
    m = c.rect(x0, y0, x1 - x0, y1 - y0)
    c.paint(m, "iron", metal(c, seed, 0.28))
    c.solid(m & (c.yy == y0), OUTLINE)
    c.paint(m & (c.yy == y0 + 1), "iron", 0.62)
    c.solid(m & (c.yy == y1 - 1), OUTLINE)
    c.paint(m & (c.yy == y1 - 2), "iron", 0.15)
    for text, cx in labels:
        draw_text(c, text, cx, (y0 + y1) / 2, 2, "parch", bright=0.62)
        for side in (-1, 1):
            rivet(c, cx + side * (text_width(text, 2) / 2 + 9), (y0 + y1) / 2, 1.3, "brass")


def button_plate(c: Canvas, x, y, w, h, label: str | None, state: str, seed: int, ramp="leather"):
    """Raised leather button with an iron rim; hover brightens (brass rim), press darkens and
    shifts the label 1 px."""
    hover, press = state == "hover", state == "press"
    m = c.chamfer(x, y, x + w, y + h, 2)
    base = 0.62 if hover else 0.35 if press else 0.5
    s = leather(c, seed, base) if ramp == "leather" else metal(c, seed, base)
    c.paint(m, ramp, s)
    band(c, x, y, x + w, y + h, 2, "brass" if hover else "iron", seed, raised=not press, base=0.5, mask=m)
    if label:
        off = 1 if press else 0
        draw_text(c, label, x + w / 2 + off, y + h / 2 + off, 2, "parch", bright=1.0 if hover else 0.8)


# --- pixel lettering ---------------------------------------------------------------------

GLYPHS = {
    "A": [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"],
    "B": ["#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "],
    "C": [" ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### "],
    "D": ["#### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#### "],
    "E": ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"],
    "F": ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "],
    "G": [" ### ", "#   #", "#    ", "# ###", "#   #", "#   #", " ### "],
    "H": ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"],
    "I": ["###", " # ", " # ", " # ", " # ", " # ", "###"],
    "J": ["  ###", "   # ", "   # ", "   # ", "   # ", "#  # ", " ##  "],
    "K": ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"],
    "L": ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"],
    "M": ["#   #", "## ##", "# # #", "# # #", "#   #", "#   #", "#   #"],
    "N": ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"],
    "O": [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
    "P": ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "],
    "Q": [" ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"],
    "R": ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"],
    "S": [" ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "],
    "T": ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "],
    "U": ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
    "V": ["#   #", "#   #", "#   #", "#   #", " # # ", " # # ", "  #  "],
    "W": ["#   #", "#   #", "#   #", "# # #", "# # #", "## ##", "#   #"],
    "X": ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"],
    "Y": ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "],
    "Z": ["#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"],
    " ": ["   "] * 7,
}


def text_width(text: str, scale: int) -> int:
    return sum((len(GLYPHS[ch][0]) + 1) * scale for ch in text) - scale


def text_mask(c: Canvas, text: str, cx: float, cy: float, scale: int) -> np.ndarray:
    w, h = text_width(text, scale), 7 * scale
    x = int(round(cx - w / 2))
    y = int(round(cy - h / 2))
    m = np.zeros((c.h, c.w), dtype=bool)
    for ch in text:
        g = GLYPHS[ch]
        for gy, row in enumerate(g):
            for gx, v in enumerate(row):
                if v == "#":
                    m[y + gy * scale : y + (gy + 1) * scale, x + gx * scale : x + (gx + 1) * scale] = True
        x += (len(g[0]) + 1) * scale
    return m


def draw_text(c: Canvas, text: str, cx: float, cy: float, scale: int, ramp="parch", bright=1.0):
    """Blocky caps with a top-lit gradient, dark outline and a soft drop shadow."""
    m = text_mask(c, text, cx, cy, scale)
    top = c.yy[m].min() if m.any() else 0
    shadow = np.zeros_like(m)
    shadow[2:, 2:] = grow(m)[:-2, :-2]
    c.px[shadow & ~grow(m), :3] = (c.px[shadow & ~grow(m), :3] * 0.45).astype(np.uint8)
    c.outline(m)
    s = bright * (1.0 - 0.55 * (c.yy - top) / (7 * scale))
    c.paint(m, ramp, s)


# --- icons (drawn at 4x on a PIL canvas, then thresholded) -------------------------------


def icon_mask(size: int, draw_fn, rotate: float = 0.0) -> np.ndarray:
    big = Image.new("L", (size * 4, size * 4), 0)
    draw_fn(ImageDraw.Draw(big), size * 4 / 100.0)
    if rotate:
        big = big.rotate(rotate, resample=Image.BILINEAR)
    small = big.resize((size, size), Image.BOX)
    return np.array(small) > 110


def _poly(d, k, pts, fill=255):
    d.polygon([(x * k, y * k) for x, y in pts], fill=fill)


def _ell(d, k, box, fill=255, width=None):
    b = [v * k for v in box]
    if width:
        d.ellipse(b, outline=fill, width=int(width * k))
    else:
        d.ellipse(b, fill=fill)


def _rect(d, k, box, fill=255):
    d.rectangle([v * k for v in box], fill=fill)


def ic_head(d, k):
    _ell(d, k, (18, 8, 82, 70))
    _rect(d, k, (18, 38, 82, 88))
    _rect(d, k, (26, 44, 74, 53), 0)
    _rect(d, k, (45, 44, 55, 74), 0)


def ic_neck(d, k):
    _ell(d, k, (22, 4, 78, 60), width=6)
    _poly(d, k, [(50, 54), (64, 72), (50, 96), (36, 72)])


def ic_chest(d, k):
    _poly(d, k, [(18, 14), (38, 8), (50, 20), (62, 8), (82, 14), (88, 42), (76, 46), (76, 92), (24, 92), (24, 46), (12, 42)])


def ic_ranged(d, k):
    b = [12 * k, 4 * k, 72 * k, 96 * k]
    d.arc(b, -75, 75, fill=255, width=int(8 * k))
    a0, a1 = math.radians(-75), math.radians(75)
    cx, cy, rx, ry = 42, 50, 30, 46
    p0 = (cx + rx * math.cos(a0), cy + ry * math.sin(a0))
    p1 = (cx + rx * math.cos(a1), cy + ry * math.sin(a1))
    d.line([(p0[0] * k, p0[1] * k), (p1[0] * k, p1[1] * k)], fill=255, width=int(3 * k))


def ic_hands(d, k):
    _rect(d, k, (30, 42, 74, 92))
    for x0, top in ((30, 14), (42, 8), (53, 10), (64, 18)):
        _rect(d, k, (x0, top, x0 + 9, 48))
    _poly(d, k, [(32, 56), (14, 38), (8, 46), (28, 74)])


def ic_weapon(d, k):
    _poly(d, k, [(46, 16), (50, 4), (54, 16), (54, 62), (46, 62)])
    _rect(d, k, (30, 62, 70, 69))
    _rect(d, k, (46, 69, 54, 86))
    _ell(d, k, (43, 85, 57, 98))


def ic_ring(d, k):
    _ell(d, k, (24, 32, 76, 88), width=9)
    _poly(d, k, [(50, 10), (62, 22), (50, 36), (38, 22)])


def ic_offhand(d, k):
    _poly(d, k, [(14, 10), (86, 10), (86, 46), (50, 96), (14, 46)])


def ic_belt(d, k):
    _rect(d, k, (4, 40, 96, 62))
    _rect(d, k, (36, 32, 64, 70), 0)
    d.rectangle([36 * k, 32 * k, 64 * k, 70 * k], outline=255, width=int(6 * k))


def ic_legs(d, k):
    _poly(d, k, [(24, 8), (76, 8), (82, 94), (58, 94), (50, 40), (42, 94), (18, 94)])


def ic_feet(d, k):
    _poly(d, k, [(28, 8), (56, 8), (56, 66), (92, 76), (92, 94), (28, 94)])


EQUIP_ICONS = [ic_head, ic_neck, ic_chest, ic_ranged, ic_hands, ic_weapon, ic_ring, ic_ring, ic_offhand, ic_belt, ic_legs, ic_feet]


def stamp_icon(c: Canvas, mask: np.ndarray, x: int, y: int, ramp="iron", base=0.38):
    m = np.zeros((c.h, c.w), dtype=bool)
    m[y : y + mask.shape[0], x : x + mask.shape[1]] = mask
    top = y
    s = base + 0.2 * (1 - (c.yy - top) / mask.shape[0]) + 0.08 * (vnoise(c.h, c.w, 3, 3, x) - 0.5)
    c.outline(m)
    c.paint(m, ramp, s)


# --- pieces ------------------------------------------------------------------------------


def toolbar_base():
    """861x69. 12 wells, inner 37x37 at (153 + 46 i, 13) (`spells_ui.rs` SLOT_*)."""
    c = Canvas(861, 69)
    x0, x1, y0, y1 = 128, 734, 5, 68
    body = c.chamfer(x0, y0, x1, y1, 4)
    s = wood(c, 3, horizontal=True, plank=21)
    c.paint(body, "oak", s)
    band(c, x0, y0, x1, y1, 4, "iron", 4, mask=body)
    # scrolled iron end brackets
    for side in (-1, 1):
        ex = x0 if side < 0 else x1
        cx = ex + side * 10
        m = (c.disc(cx, 36.5, 22) & ~c.disc(cx + side * 9, 36.5, 15)) & (side * (c.xx + 0.5 - ex) >= -4)
        m &= (c.yy >= 10) & (c.yy < 64)
        c.paint(m, "iron", metal(c, 5, 0.5) + 0.25 * (36.5 - c.yy) / 26)
        c.outline(m)
        rivet(c, cx + side * 14, 36.5, 3.0)
        rivet(c, ex - side * 9, 12, 1.8)
        rivet(c, ex - side * 9, 61, 1.8)
    for i in range(12):
        x = 153 + 46 * i
        well(c, x, 13, 37, 37)
        if i < 11:
            rivet(c, x + 41.5, 31.5, 1.4, "iron")
    c.save("toolbar_base")


def castbar():
    """323x40. Icon 22x22 at (6, 8); fill 276x4 at (36, 24); label at (40, 4)."""
    c = Canvas(323, 40)
    body = c.chamfer(0, 1, 323, 39, 3)
    c.paint(body, "oak", wood(c, 11, horizontal=True, plank=13) - 0.05)
    band(c, 0, 1, 323, 39, 3, "iron", 12, mask=body)
    well(c, 6, 8, 22, 22)
    well(c, 36, 24, 276, 4)
    rivet(c, 318.5, 9.5, 1.5)
    c.save("castbar")

    f = Canvas(276, 4)
    s = np.array([0.95, 0.75, 0.55, 0.35])[f.yy] + 0.1 * (vnoise(4, 276, 6, 1, 13) - 0.5)
    f.paint(np.ones((4, 276), bool), "brass", s)
    f.save("castbar_fill")


FRAME_W, FRAME_H = 372, 116


def unit_frame(reverse: bool):
    """372x116. Portrait circle (r 39, drawn on top by the client) at (43, 73) / (328, 73);
    HP track 296x28 at (74, 39) / (3, 39); MP track 265x22 at (84, 70) / (23, 70)."""
    c = Canvas(FRAME_W, FRAME_H)
    pc = (328.0, 73.0) if reverse else (43.0, 73.0)
    hp = (3, 39, 296, 28) if reverse else (74, 39, 296, 28)
    mp = (23, 70, 265, 22) if reverse else (84, 70, 265, 22)
    px0, px1 = (0, 312) if reverse else (60, 372)
    py0, py1 = 9, 101
    body = c.chamfer(px0, py0, px1, py1, 4)
    c.paint(body, "oak", wood(c, 21 + reverse, horizontal=True, plank=15))
    band(c, px0, py0, px1, py1, 3, "iron", 22, mask=body)
    # bar tracks: recessed wells exactly under the bars (rim 1 px between HP and MP)
    for x, y, w, h in (hp, mp):
        well(c, x, y, w, h, rim=1)
    rivet(c, (px1 - 7.5) if not reverse else (px0 + 7.5), 15.5, 1.8)
    rivet(c, (px1 - 7.5) if not reverse else (px0 + 7.5), 95.5, 1.8)
    # portrait socket: black disc + iron ring with brass studs
    d = c.dist(*pc)
    c.solid(d <= 39.6, (6, 5, 4))
    ring = (d > 39.6) & (d <= 44.5)
    ang = np.arctan2(c.yy + 0.5 - pc[1], c.xx + 0.5 - pc[0])
    s = 0.5 + 0.28 * np.cos(ang + math.radians(135)) + 0.12 * (vnoise(c.h, c.w, 3, 3, 7) - 0.5)
    c.paint(ring, "iron", s)
    c.solid((d > 44.5) & (d <= 45.6), OUTLINE)
    c.solid((d > 39.6) & (d <= 40.4), OUTLINE)
    for a in (-90, -30, 30, 150, 210):
        rivet(c, pc[0] + 42 * math.cos(math.radians(a)), pc[1] + 42 * math.sin(math.radians(a)), 1.6)
    c.save("unit_frame_reverse" if reverse else "unit_frame")


def bar_fill(name: str, w: int, h: int, ramp: str, seed: int, hole=None):
    """Bar image: top-lit gradient + horizontal streaks, 1 px dark ends; `hole` (cx, cy, r)
    in bar coordinates is cut out where the portrait ring overlaps the bar."""
    c = Canvas(w, h)
    t = c.yy / (h - 1)
    s = 0.78 - 0.55 * t + 0.25 * np.exp(-((c.yy - 2) ** 2) / 3.0)
    s += 0.12 * (vnoise(h, w, 22, 2, seed) - 0.5) + 0.05 * (vnoise(h, w, 4, 1, seed + 1) - 0.5)
    m = np.ones((h, w), bool)
    if hole:
        m &= c.dist(hole[0], hole[1]) > hole[2]
    c.paint(m, ramp, s)
    c.solid(m & (c.yy == h - 1), (0, 0, 0), 120)
    c.save(name)


def unit_frame_bars():
    hole = 44.6
    bar_fill("unit_frame_hp", 296, 28, "hp", 31, (43 - 74, 73 - 39, hole))
    bar_fill("unit_frame_mp", 265, 22, "mp", 32, (43 - 84, 73 - 70, hole))
    bar_fill("unit_frame_hp_reverse", 296, 28, "hp", 33, (328 - 3, 73 - 39, hole))
    bar_fill("unit_frame_mp_reverse", 265, 22, "mp", 34, (328 - 23, 73 - 70, hole))


def level_badge():
    """33x33 round badge (level number drawn on top, centred)."""
    c = Canvas(33, 33)
    d = c.dist(16.5, 16.5)
    ang = np.arctan2(c.yy + 0.5 - 16.5, c.xx + 0.5 - 16.5)
    c.paint(d <= 15.5, "brass", 0.5 + 0.35 * np.cos(ang + math.radians(135)))
    c.paint(d <= 12.5, "iron", 0.18 + 0.15 * (c.yy / 33))
    c.solid((d > 12.5) & (d <= 13.4), OUTLINE)
    c.solid((d > 15.5) & (d <= 16.5), OUTLINE)
    c.save("unit_frame_level_bg")


def rank_ring(name: str, boss: bool):
    """136x140 ring around the target portrait, centre (65.5, 75.5); open on the left
    (where the frame meets the portrait) and at the bottom, like the original."""
    c = Canvas(136, 140)
    cx, cy = 65.5, 75.5
    d = c.dist(cx, cy)
    ang = np.degrees(np.arctan2(c.yy + 0.5 - cy, c.xx + 0.5 - cx))
    arc = ((ang > -128) & (ang < 68)) | ((ang > 96) & (ang < 156))
    ramp = "iron" if boss else "brass"
    glow_ramp = "ember" if boss else "amber"
    spikes = np.zeros_like(d, dtype=bool)
    n1, n2 = (9, 3) if boss else (10, 3)
    angles = np.concatenate([np.linspace(-116, 56, n1), np.linspace(108, 146, n2)])
    hook = 9 if boss else 12  # tips swept clockwise: thorns rather than cog teeth
    for i, a in enumerate(angles):
        L = (17 if boss else 10) * (1.0 if i % 2 == 0 else 0.65)
        ar, tr = math.radians(a), math.radians(a + hook)
        tip = (cx + (50 + L) * math.cos(tr), cy + (50 + L) * math.sin(tr))
        half = math.radians(5 if boss else 4)
        b0 = (cx + 48 * math.cos(ar - half), cy + 48 * math.sin(ar - half))
        b1 = (cx + 48 * math.cos(ar + half), cy + 48 * math.sin(ar + half))
        spikes |= _tri(c, tip, b0, b1)
    # band tapers to points at the arc ends
    ends = [(-128, 68), (96, 156)]
    to_end = np.full(d.shape, 0.0)
    for a0, a1 in ends:
        inside = (ang > a0) & (ang < a1)
        to_end = np.where(inside, np.minimum(ang - a0, a1 - ang), to_end)
    thick = 3.2 * np.clip(to_end / 18.0, 0.15, 1.0)
    band_m = arc & (np.abs(d - 47.5) <= thick)
    shape = band_m | (spikes & arc_ext(ang))
    # glow
    g = grow(shape, 3) & ~shape
    c.paint(g, glow_ramp, 0.55, 110)
    g2 = grow(shape, 1) & ~shape
    c.paint(g2, glow_ramp, 0.8, 200)
    s = 0.5 + 0.3 * np.cos(np.radians(ang) + math.radians(135)) + 0.1 * (vnoise(140, 136, 3, 3, 9) - 0.5)
    s = np.where(spikes & ~band_m, s + 0.12, s)
    if not boss:  # twisted-rope band
        twist = np.sin(np.radians(ang) * 40 + (d - 47.5) * 1.3)
        s = np.where(band_m, s + 0.18 * np.sign(twist), s)
    c.outline(shape)
    c.paint(shape, ramp, s)
    # studs on the band
    for a in np.linspace(-118, 58, 6 if boss else 8):
        rivet(c, cx + 47.5 * math.cos(math.radians(a)), cy + 47.5 * math.sin(math.radians(a)), 1.5, "brass" if boss else "iron")
    c.save(name)


def arc_ext(ang):
    return ((ang > -132) & (ang < 72)) | ((ang > 92) & (ang < 160))


def _tri(c: Canvas, a, b, d) -> np.ndarray:
    px, py = c.xx + 0.5, c.yy + 0.5

    def side(p0, p1):
        return (p1[0] - p0[0]) * (py - p0[1]) - (p1[1] - p0[1]) * (px - p0[0])

    s0, s1, s2 = side(a, b), side(b, d), side(d, a)
    return ((s0 >= 0) & (s1 >= 0) & (s2 >= 0)) | ((s0 <= 0) & (s1 <= 0) & (s2 <= 0))


def xp_bar():
    """633x12, drawn full for the fill and tinted dark by the client for the track."""
    c = Canvas(633, 12)
    m = c.chamfer(0, 0, 633, 12, 2)
    s = 0.85 - 0.6 * c.yy / 11 + 0.1 * (vnoise(12, 633, 30, 2, 41) - 0.5)
    c.paint(m, "xp", s)
    c.solid(m & ((c.yy == 0) | (c.yy == 11) | (c.xx == 0) | (c.xx == 632)), OUTLINE)
    ticks = m & (((c.xx - 316) % 63) == 0) & (c.yy > 0) & (c.yy < 11) & (c.xx > 2) & (c.xx < 630)
    c.solid(ticks, OUTLINE, 200)
    c.save("xp_bar")


def nameplates():
    """102x12; fill well x 2..100, y 2..10 (`nameplates.rs`)."""
    for name, ramp in (("nameplate_bg", None), ("nameplate_hp", "hp"), ("nameplate_hp_party", "green")):
        c = Canvas(102, 12)
        allm = np.ones((12, 102), bool)
        c.solid(allm, OUTLINE)
        rim = c.rect(1, 1, 100, 10)
        d, lit = _sides(c, 1, 1, 101, 11)
        c.paint(rim, "iron", np.where(lit, 0.6, 0.3))
        inner = c.rect(2, 2, 98, 8)
        if ramp:
            c.paint(inner, ramp, 0.85 - 0.6 * (c.yy - 2) / 7)
        else:
            c.paint(inner, "well", 0.25 + 0.1 * (c.yy - 2) / 7)
        c.save(name)


def abilities():
    """474x592. Title band, Spells / Actions tabs (labels centred near x 183 / 288, y 88),
    list area (36, 102) 401x470 (`spells_ui.rs`)."""
    c = Canvas(474, 592)
    panel(c, 6, 8, 468, 588, 51)
    title_bar(c, 12, 14, 462, 66, "ABILITIES", 52)
    tab_strip(c, 12, 67, 462, 108, [("SPELLS", 183), ("ACTIONS", 288)], 53)
    c.save("abilities")


def abilities_slot():
    """401x70 list row; icon well 40x40 at (18, 14); name at (72, 12), text at (72, 34)."""
    c = Canvas(401, 70)
    m = c.chamfer(0, 2, 401, 68, 3)
    c.paint(m, "leather", leather(c, 61, 0.22) + 0.1 * (1 - c.yy / 70))
    band(c, 0, 2, 401, 68, 2, "iron", 62, mask=m)
    c.paint(c.rect(72, 31, 315, 1), "leather", 0.45)
    well(c, 18, 14, 40, 40)
    rivet(c, 391.5, 10.5, 1.4)
    rivet(c, 391.5, 59.5, 1.4)
    c.save("abilities_slot_idle")


def inventory():
    """364x436. 7x7 wells, inner 37 at (28 + 45 c, 76 + 45 r); gold text at (52, 400)."""
    c = Canvas(364, 436)
    panel(c, 8, 10, 356, 428, 71)
    title_bar(c, 14, 14, 350, 58, "INVENTORY", 72)
    for r in range(7):
        for col in range(7):
            well(c, 28 + 45 * col, 76 + 45 * r, 37, 37)
    # gold coin by the gold counter
    coin(c, 39.5, 408.5, 5.0)
    c.save("inventory")


def coin(c: Canvas, cx, cy, r):
    d = c.dist(cx, cy)
    ang = np.arctan2(c.yy + 0.5 - cy, c.xx + 0.5 - cx)
    c.paint(d <= r, "brass", 0.62 + 0.3 * np.cos(ang + math.radians(135)))
    c.paint((d <= r - 2) & (d > r - 3), "brass", 0.35)
    c.solid((d > r) & (d <= r + 1), OUTLINE)


EQUIP_TOPS = [179, 238, 296, 354, 412, 471]


def equipment():
    """603x571. Slots: 41 px frames at x 46 / 288, tops EQUIP_TOPS (inner 39 at +1); close
    button (552, 30) 26x26; Inventory button (22, 532) 120x30; stats column from x 385."""
    c = Canvas(603, 571)
    panel(c, 8, 12, 596, 566, 81)
    title_bar(c, 14, 16, 590, 66, "CHARACTER", 82)
    close_x(c, 552, 30, 26, "idle", 83)
    tab_strip(c, 14, 67, 590, 108, [("GENERAL", 157), ("COMBAT", 296), ("SKILLS", 431)], 84)
    # divider between the paper doll and the stats column
    div = c.rect(359, 116, 4, 440)
    c.paint(div, "iron", metal(c, 85, 0.4) + np.where(c.xx == 360, 0.2, 0.0))
    c.solid(div & ((c.xx == 359) | (c.xx == 362)), OUTLINE)
    rivet(c, 361, 120, 1.6)
    rivet(c, 361, 552, 1.6)
    for s in range(12):
        col, row = (46, s) if s < 6 else (288, s - 6)
        x, y = col + 1, EQUIP_TOPS[row] + 1
        well(c, x, y, 39, 39)
        stamp_icon(c, icon_mask(27, EQUIP_ICONS[s], rotate=45 if s == 5 else 0), x + 6, y + 6)
    draw_text(c, "PROGRESSION", 190, 229, 2, "parch", bright=0.7)
    draw_text(c, "EXPERIENCE", 190, 287, 2, "parch", bright=0.7)
    button_plate(c, 22, 532, 120, 30, "INVENTORY", "idle", 86)
    c.save("equipment")


def close_x(c: Canvas, x, y, size, state, seed):
    button_plate(c, x, y, size, size, None, state, seed, ramp="iron")
    k = size
    m = np.zeros((c.h, c.w), bool)
    for i in range(int(k * 0.28), int(k * 0.72)):
        for t in (0, 1):
            m[y + i, x + i + t] = True
            m[y + i, x + k - 1 - i - t] = True
    c.outline(m)
    c.paint(m, "parch", 0.8)


def loot_window():
    """214x263. Title 5..30 with close button (180, 8) 24x24; list (14, 44) 186x168;
    Take All button (50, 222) 116x30."""
    c = Canvas(214, 263)
    panel(c, 3, 2, 211, 261, 91, border=5)
    title_bar(c, 8, 6, 206, 32, "LOOT", 92, cx=100)
    close_x(c, 181, 8, 22, "idle", 93)
    well(c, 13, 40, 188, 175, rim=2)
    button_plate(c, 50, 222, 116, 30, "TAKE ALL", "idle", 94)
    c.save("loot_window")


def chat():
    """537x237. Log (16, 10) 484x184, input row (18, 201) 420x30, scroll buttons at x 510,
    Enter button (447, 196) 82x40 (`chat.rs`)."""
    c = Canvas(537, 237)
    panel(c, 0, 0, 537, 237, 101, border=5, alpha=240)
    well(c, 14, 8, 488, 188, rim=2)
    # translucent log: let the world show a little through the text area
    m = c.rect(14, 8, 488, 188)
    c.px[m, 3] = 242
    well(c, 515, 30, 7, 145, rim=2)
    well(c, 16, 202, 424, 28, rim=2)
    c.save("game_chat_backdrop")

    def arrows(name, w, h, kinds):
        for state in ("idle", "hover", "press"):
            b = Canvas(w, h)
            m = b.chamfer(0, 0, w, h, 2)
            base = 0.6 if state == "hover" else 0.3 if state == "press" else 0.45
            b.paint(m, "iron", metal(b, 111, base))
            band(b, 0, 0, w, h, 1, "iron", 112, raised=state != "press", mask=m)
            off = 1 if state == "press" else 0
            am = np.zeros((h, w), bool)
            for up, y0 in kinds:
                for i in range(4):
                    yy = y0 + (i if up else 3 - i) + off
                    am[yy, 8 - i : 9 + i] = True
            b.outline(am)
            b.paint(am, "brass" if state == "hover" else "parch", 0.85 if state == "hover" else 0.6)
            b.save(f"{name}_{state}")

    arrows("game_chat_fullup", 17, 18, [(True, 4), (True, 10)])
    arrows("game_chat_up", 17, 11, [(True, 3)])
    arrows("game_chat_down", 17, 11, [(False, 3)])
    arrows("game_chat_fulldown", 17, 17, [(False, 3), (False, 9)])

    for state in ("idle", "hover", "press"):
        b = Canvas(82, 40)
        button_plate(b, 3, 5, 76, 30, None, state, 113)
        off = 1 if state == "press" else 0
        am = np.zeros((40, 82), bool)
        am[20 + off : 22 + off, 28 + off : 54 + off] = True  # shaft
        am[13 + off : 22 + off, 52 + off : 54 + off] = True  # riser
        for i in range(6):
            am[21 - i + off : 21 + i + 1 + off, 22 + i + off] = True  # head
        b.outline(am)
        b.paint(am, "parch", 0.95 if state == "hover" else 0.75)
        b.save(f"game_chat_enter_{state}")


def saybox():
    """Speech bubble 9-slice: 4 px corners, 1 px edges, 1x1 centre (`chat.rs`)."""
    c = Canvas(9, 9)
    m = c.chamfer(0, 0, 9, 9, 2)
    c.solid(m, (22, 17, 13), 235)
    edge = m & ~c.chamfer(1, 1, 8, 8, 2)
    c.solid(edge, (150, 118, 62), 255)
    px = c.px
    pieces = {
        "topleft": px[0:4, 0:4], "topacross": px[0:4, 4:5], "topright": px[0:4, 5:9],
        "leftup": px[4:5, 0:4], "center": px[4:5, 4:5], "rightup": px[4:5, 5:9],
        "bottomleft": px[5:9, 0:4], "bottomacross": px[5:9, 4:5], "bottomright": px[5:9, 5:9],
    }
    OUT.mkdir(parents=True, exist_ok=True)
    for k, v in pieces.items():
        Image.fromarray(np.ascontiguousarray(v)).save(OUT / f"saybox_{k}.png")
        SAVED.append(f"saybox_{k}")


def minimap():
    """241x293 frame; live map view (6, 46) 230x227 covered by the client; zoom button
    (99.5, 250) 42x41. `miniamp_decal.png` 230x227: only its alpha is used (inverted into a
    dark vignette)."""
    c = Canvas(241, 293)
    # crest plate on top
    top = c.chamfer(14, 6, 227, 44, 6)
    c.paint(top, "oak", wood(c, 121, horizontal=True, plank=12))
    band(c, 14, 6, 227, 44, 3, "iron", 122, mask=top)
    leather_band(c, 40, 15, 201, 36, 123)
    # compass diamond
    dm = (np.abs(c.xx + 0.5 - 120.5) + np.abs(c.yy + 0.5 - 25.5) * 1.6) <= 12
    c.outline(dm)
    c.paint(dm, "brass", 0.75 - 0.4 * (c.xx > 120) - 0.15 * (c.yy > 25))
    for x in (24, 216):
        rivet(c, x + 0.5, 25.5, 2.4)
    # window frame (view rect itself black)
    c.solid(c.rect(6, 46, 230, 227), (0, 0, 0))
    m = c.chamfer(1, 41, 241, 278, 3)
    band(c, 1, 41, 241, 278, 5, "iron", 124, mask=m)
    for (cx, cy) in ((1, 41), (228, 41), (1, 265), (228, 265)):
        plate = c.chamfer(cx, cy, cx + 13, cy + 13, 2)
        band(c, cx, cy, cx + 13, cy + 13, 2, "brass", 125, base=0.55, mask=plate)
        c.paint(plate & c.rect(cx + 2, cy + 2, 9, 9), "brass", 0.5)
        rivet(c, cx + 6.5, cy + 6.5, 1.8, "iron")
    # bracket under the zoom button
    br = c.chamfer(92, 262, 150, 293, 5)
    c.paint(br, "iron", metal(c, 126, 0.4))
    c.outline(br)
    c.solid(c.rect(6, 46, 230, 216), (0, 0, 0))
    c.save("minimap")

    d = Canvas(230, 227)
    e = np.minimum(np.minimum(d.xx + 0.5, 230 - d.xx - 0.5), np.minimum(d.yy + 0.5, 227 - d.yy - 0.5))
    e = e + 6 * (vnoise(227, 230, 9, 9, 127) - 0.5)
    a = np.clip((e - 2) / 16, 0, 1)
    a = np.clip(a * 4 + (d.bayer - 0.5) * 0.9, 0, 4).round() / 4  # dithered 5-step fade
    d.px[..., :3] = 128
    d.px[..., 3] = (a * 255).astype(np.uint8)
    d.save("miniamp_decal")

    for state in ("idle", "hover", "press"):
        b = Canvas(42, 41)
        dd = b.dist(21, 20.5)
        ang = np.arctan2(b.yy + 0.5 - 20.5, b.xx + 0.5 - 21)
        rim = "brass" if state == "hover" else "iron"
        b.paint(dd <= 17, rim, 0.5 + 0.3 * np.cos(ang + math.radians(135 if state != "press" else -45)))
        base = 0.55 if state == "hover" else 0.25 if state == "press" else 0.4
        b.paint(dd <= 13.5, "oak", base + 0.15 * (1 - b.yy / 41))
        b.solid((dd > 13.5) & (dd <= 14.4), OUTLINE)
        b.solid((dd > 17) & (dd <= 18), OUTLINE)
        off = 1 if state == "press" else 0
        # magnifier glyph
        lens = (np.abs(b.dist(19 + off, 18.5 + off) - 5.2) <= 1.2)
        handle = (np.abs((b.xx - 24 - off) - (b.yy - 23 - off)) <= 1) & (b.xx >= 23 + off) & (b.xx <= 29 + off)
        g = lens | handle
        b.outline(g)
        b.paint(g, "parch", 0.9 if state == "hover" else 0.7)
        b.save(f"minimap_button_{state}")

    for name, ramp in (("minimap_enemy", "hp"), ("minimap_neutral", "amber"), ("minimap_friendly", "green"), ("minimap_dead", "bone")):
        b = Canvas(18, 18)
        dd = b.dist(9, 9)
        b.paint(dd <= 6.5, "iron", 0.35)
        ang = np.arctan2(b.yy + 0.5 - 9, b.xx + 0.5 - 9)
        b.paint(dd <= 4.8, ramp, 0.55 + 0.25 * np.cos(ang + math.radians(135)) - 0.2 * dd / 5)
        b.paint(b.dist(7.5, 7.5) <= 1.2, ramp, 1.0)
        b.solid((dd > 6.5) & (dd <= 7.5), OUTLINE)
        b.save(name)


def gold_pouch():
    """32x32 loot marker / loot-list gold icon."""
    c = Canvas(32, 32)
    body = (c.dist(16, 21) <= 9.5) | _tri(c, (16, 8), (8, 18), (24, 18))
    neck = c.rect(12, 9, 8, 4)
    m = body | neck
    c.outline(m)
    c.paint(m, "leather", 0.55 + 0.3 * np.cos(np.arctan2(c.yy - 21, c.xx - 16) + math.radians(135)) * (c.dist(16, 21) / 10))
    c.paint(c.rect(11, 12, 10, 2), "brass", 0.6)  # cord
    c.solid(c.rect(11, 12, 10, 2) & (c.xx == 20), OUTLINE)
    for (x, y) in ((9, 27), (21, 28), (25, 24)):
        coin(c, x + 0.5, y + 0.5, 2.6)
    c.save("gossip_gold_pouch")


def portraits():
    """Faction placeholders (portraits/portrait_{friendly,hostile,grey}.png), same sizes."""
    for name, (w, h), ramp in (("portrait_friendly", (78, 77), "green"), ("portrait_hostile", (80, 80), "hp"),
                               ("portrait_grey", (78, 78), "bone")):
        c = Canvas(w, h)
        cx, cy, r = w / 2, h / 2, min(w, h) / 2 - 1
        d = c.dist(cx, cy)
        bg = d <= r
        c.paint(bg, ramp, 0.42 - 0.35 * d / r)
        # hooded figure
        head = c.dist(cx, cy - 6) <= 13
        hood = _tri(c, (cx, cy - 26), (cx - 17, cy + 2), (cx + 17, cy + 2))
        shoulders = (np.hypot((c.xx + 0.5 - cx) / 30, (c.yy + 0.5 - (cy + 34)) / 22) <= 1)
        fig = (head | hood | shoulders) & bg
        c.paint(fig, "iron", 0.12 + 0.12 * (1 - (c.yy - cy + 26) / 60))
        face = (c.dist(cx, cy - 3) <= 7.5) & bg
        c.solid(face, (4, 3, 2))
        for ex in (cx - 3, cx + 3):
            c.paint(c.dist(ex, cy - 4) <= 1.0, ramp, 0.95)
        c.solid((d > r) & (d <= r + 1), OUTLINE)
        c.save(name)


def contact_sheet():
    PREVIEW.mkdir(parents=True, exist_ok=True)
    ims = [Image.open(OUT / f"{n}.png") for n in SAVED if not n.startswith("saybox")]
    W, x, y, rowh = 1300, 6, 6, 0
    pos = []
    for im in ims:
        if x + im.width > W:
            x, y, rowh = 6, y + rowh + 6, 0
        pos.append((x, y))
        x += im.width + 6
        rowh = max(rowh, im.height)
    sheet = Image.new("RGBA", (W, y + rowh + 6), (70, 92, 60, 255))
    for im, p in zip(ims, pos):
        sheet.alpha_composite(im.convert("RGBA"), p)
    sheet.save(PREVIEW / "ui_sheet.png")


def main():
    toolbar_base()
    castbar()
    unit_frame(False)
    unit_frame(True)
    unit_frame_bars()
    level_badge()
    rank_ring("unit_frame_elite", boss=False)
    rank_ring("unit_frame_boss", boss=True)
    xp_bar()
    nameplates()
    abilities()
    abilities_slot()
    inventory()
    equipment()
    loot_window()
    chat()
    saybox()
    minimap()
    gold_pouch()
    portraits()
    print(f"{len(SAVED)} images -> {OUT}")
    if "--preview" in sys.argv:
        contact_sheet()


if __name__ == "__main__":
    main()
