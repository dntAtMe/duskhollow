"""Front-end (main menu, options, game menu) art in the "iron & oak" look of `ui.py`.

Every piece is drawn for 9-slicing by the client (`crates/dusk_client/src/menu/`), so the
border widths below are repeated there (`SLICE_*`):

- `menu_button_{idle,hover,press,disabled}.png` (220x38, border 9): oxblood leather plate in an
  iron rim; hover gets a brass rim and a faint ember glow, press darkens and sinks.
- `menu_panel.png` (112x112, border 18): dark stained oak under an iron frame, brass corner
  plates with iron rivets. `menu_panel_dark.png`: the same frame, near-black body (overlays).
- `menu_field{,_focus}.png` (64x30, border 7): recessed text well; brass rim when focused.
- `menu_slider_track.png` (64x12, border 5), `menu_slider_fill.png` (16x6, ember),
  `menu_slider_knob{,_hover}.png` (14x24).
- `menu_check_{off,on}.png` (22x22): small well, an ember-lit brass stud when on.
- `menu_class_{1..4}.png` (76x76): class medallions (iron ring, crimson field, emblem in bone
  and brass): 1 sword over a round shield (the Vanguard), 2 a flame in cupped hands (the
  Emberwright), 3 crossed knives and a hook (the Cutthroat), 4 a mace before a cairn fire
  (the Ashpriest). `menu_class_ring_sel.png`: the selection ring drawn over a medallion.
- `menu_rule.png` (360x14): bronze rule with a central lozenge, under the title.

Output: `assets/content/ui/` (our own names, always available).

    python -I tools/artgen/menu_ui.py              # write the set
    python -I tools/artgen/menu_ui.py --preview    # also assets/preview/menu_sheet.png
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import ui  # noqa: E402
from ui import OUTLINE, Canvas, _tri, band, grow, leather, metal, rivet, vnoise, well, wood  # noqa: E402

ui.OUT = ui.ROOT / "assets" / "content" / "ui"
ui.RAMPS["crimson"] = ["#1e0306", "#43070d", "#6e0f16", "#9c1f22", "#c94a3a"]
ui.RAMPS["bronze"] = ["#1f1308", "#3d2610", "#5e3d1a", "#83582a", "#a8773e"]
for k in ("crimson", "bronze"):
    ui.TABLE[k] = np.array([ui._rgb(c) for c in ui.RAMPS[k]], dtype=np.uint8)


# --- buttons ----------------------------------------------------------------------------------


def button(state: str):
    w, h = 220, 38
    c = Canvas(w, h)
    hover, press, off = state == "hover", state == "press", state == "disabled"
    m = c.chamfer(0, 0, w, h, 3)
    base = 0.56 if hover else 0.30 if press else 0.22 if off else 0.44
    s = leather(c, 41, base)
    # a soft horizontal sheen across the upper third (darker when pressed)
    yy = (c.yy - 2) / (h - 4)
    s = s + (0.10 if not press else 0.03) * np.exp(-((yy - 0.28) ** 2) / 0.03) - 0.10 * yy
    if hover:  # ember glow rising from the bottom edge
        s = s + 0.12 * np.clip(yy - 0.4, 0, 1)
    c.paint(m, "iron" if off else "leather", s)
    inner = band(c, 0, 0, w, h, 3, "brass" if hover else "iron", 43, raised=not press, base=0.5, mask=m)
    # stitched seam inside the rim
    dash = ((c.xx // 2) % 3 == 0) & (c.xx >= 12) & (c.xx < w - 12)
    for yy_ in (6, h - 7):
        c.paint(inner & dash & (c.yy == yy_), "parch", 0.22 if not hover else 0.34)
    for x in (9.5, w - 9.5):
        rivet(c, x, h / 2, 2.0, "brass" if not off else "iron")
    c.save(f"menu_button_{state}")


# --- panels -----------------------------------------------------------------------------------


def panel(name: str, dark: bool):
    n, border = 112, 18
    c = Canvas(n, n)
    outer = c.chamfer(0, 0, n, n, 4)
    inner = c.rect(border - 8, border - 8, n - 2 * (border - 8), n - 2 * (border - 8))
    if dark:
        s = 0.16 + 0.08 * (vnoise(n, n, 9, 9, 5) - 0.5)
        c.paint(inner, "well", s + 0.15)
    else:
        s = wood(c, 51, horizontal=True, plank=22, base=0.30)
        c.paint(inner, "oak", s)
    # inner shadow along the frame
    d, _ = ui._sides(c, 10, 10, n - 10, n - 10)
    shade = inner & (d < 4)
    c.px[shade, :3] = (c.px[shade, :3] * (0.55 + 0.11 * d[shade, None])).astype(np.uint8)
    band(c, 0, 0, n, n, 10, "iron", 52, mask=outer & ~inner, base=0.36)
    # brass corner plates
    for cx, cy in ((0, 0), (n - 18, 0), (0, n - 18), (n - 18, n - 18)):
        plate = c.chamfer(cx, cy, cx + 18, cy + 18, 3) & outer
        band(c, cx, cy, cx + 18, cy + 18, 2, "brass", 53, base=0.55, mask=plate)
        c.paint(plate & c.rect(cx + 2, cy + 2, 14, 14), "brass", 0.48 + 0.12 * (vnoise(n, n, 3, 3, 54) - 0.5))
        rivet(c, cx + 9, cy + 9, 2.4, "iron")
    # mid-edge rivets (they tile along the sliced edges)
    for t in (n / 2,):
        for x, y in ((t, 5), (t, n - 5), (5, t), (n - 5, t)):
            rivet(c, x, y, 1.5, "iron")
    c.save(name)


# --- text field, slider, checkbox -------------------------------------------------------------


def field(name: str, focus: bool):
    w, h = 64, 30
    c = Canvas(w, h)
    m = c.rect(0, 0, w, h)
    band(c, 0, 0, w, h, 3, "brass" if focus else "iron", 61, raised=False, base=0.5, mask=m)
    inner = c.rect(3, 3, w - 6, h - 6)
    s = 0.18 + 0.2 * (c.yy - 3) / (h - 6) + 0.06 * (vnoise(h, w, 5, 5, 62) - 0.5)
    d, lit = ui._sides(c, 3, 3, w - 3, h - 3)
    s = np.where(lit & (d < 2), s - 0.2, s)
    c.paint(inner, "well", s)
    c.save(name)


def slider():
    w, h = 64, 12
    c = Canvas(w, h)
    well(c, 2, 2, w - 4, h - 4)
    c.save("menu_slider_track")

    f = Canvas(16, 6)
    s = np.array([0.95, 0.8, 0.65, 0.5, 0.38, 0.28])[f.yy]
    f.paint(np.ones((6, 16), bool), "ember", s)
    f.save("menu_slider_fill")

    for state in ("", "_hover"):
        k = Canvas(14, 24)
        m = k.chamfer(0, 0, 14, 24, 2)
        k.paint(m, "brass", metal(k, 71, 0.62 if state else 0.5))
        band(k, 0, 0, 14, 24, 2, "brass", 72, base=0.6 if state else 0.5, mask=m)
        for y in (9, 12, 15):  # grip ridges
            k.solid(k.rect(4, y, 6, 1), OUTLINE)
            k.paint(k.rect(4, y + 1, 6, 1), "brass", 0.85)
        k.save(f"menu_slider_knob{state}")


def checkbox():
    for on in (False, True):
        c = Canvas(22, 22)
        well(c, 3, 3, 16, 16)
        if on:
            d = c.dist(11, 11)
            glow = (d <= 7.5) & c.rect(3, 3, 16, 16)
            c.paint(glow, "ember", np.clip(0.55 - d / 14, 0, 1))
            stud = d <= 4.5
            nx, ny = (c.xx + 0.5 - 11) / 4.5, (c.yy + 0.5 - 11) / 4.5
            c.paint(stud, "brass", np.clip(0.7 - 0.35 * (nx + ny), 0.1, 1))
            c.solid((d > 4.5) & (d <= 5.5), OUTLINE)
        c.save(f"menu_check_{'on' if on else 'off'}")


# --- class medallions -------------------------------------------------------------------------

M = 76
CX = CY = M / 2


def medallion_base(c: Canvas, seed: int):
    d = c.dist(CX, CY)
    ring = (d <= 36) & (d > 29)
    field = d <= 29
    ang = np.arctan2(c.yy + 0.5 - CY, c.xx + 0.5 - CX)
    # crimson field, lit from above (the Eye's light), darker at the rim
    s = 0.62 - 0.45 * (d / 29) + 0.14 * (-np.sin(ang)) * (d / 29) + 0.1 * (vnoise(M, M, 4, 4, seed) - 0.5)
    c.paint(field, "crimson", np.clip(s - 0.12, 0, 1))
    # iron ring with a top-left light, bevel
    rs = metal(c, seed + 1, 0.42) + 0.22 * (-np.sin(ang + math.pi / 4)) * ((d - 29) / 7)
    rs = np.where(d > 34.5, rs - 0.2, np.where(d < 30.5, rs - 0.25, rs + 0.05))
    c.paint(ring, "iron", rs)
    c.solid((d > 36) & (d <= 37.2), OUTLINE)
    c.solid((d > 28.2) & (d <= 29.2), OUTLINE)
    for a in range(4):
        t = math.pi / 4 + a * math.pi / 2
        rivet(c, CX + 32.5 * math.cos(t), CY + 32.5 * math.sin(t), 1.9, "brass")
    return field


def poly_mask(c: Canvas, pts) -> np.ndarray:
    m = np.zeros((c.h, c.w), dtype=bool)
    for i in range(1, len(pts) - 1):
        m |= _tri(c, pts[0], pts[i], pts[i + 1])
    return m


def seg_mask(c: Canvas, a, b, r) -> np.ndarray:
    px, py = c.xx + 0.5, c.yy + 0.5
    ax, ay = a
    bx, by = b
    vx, vy = bx - ax, by - ay
    t = np.clip(((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy), 0, 1)
    return np.hypot(px - ax - t * vx, py - ay - t * vy) <= r


def stamp(c: Canvas, m: np.ndarray, ramp: str, base: float, field: np.ndarray, seed: int):
    """Emblem shape: top-lit gradient, dark outline, drop shadow onto the field."""
    shadow = np.zeros_like(m)
    shadow[2:, 1:] = m[:-2, :-1]
    sh = shadow & ~m & field
    c.px[sh, :3] = (c.px[sh, :3] * 0.4).astype(np.uint8)
    c.outline(m)
    ys = c.yy[m]
    top, bot = (ys.min(), ys.max()) if ys.size else (0, 1)
    s = base + 0.28 * (1 - (c.yy - top) / max(bot - top, 1)) + 0.08 * (vnoise(c.h, c.w, 2.5, 2.5, seed) - 0.5)
    c.paint(m, ramp, np.clip(s, 0, 1))


def class_1(c, field):
    """Vanguard: a round shield with a sword driven down behind it."""
    shield = c.dist(38, 40) <= 17
    stamp(c, shield, "bronze", 0.3, field, 82)
    rim = shield & (c.dist(38, 40) > 14.5)
    c.paint(rim, "iron", 0.55 - 0.3 * (c.yy - 23) / 34)
    for a in range(6):
        t = a * math.pi / 3 + math.pi / 6
        rivet(c, 38 + 15.7 * math.cos(t), 40 + 15.7 * math.sin(t), 1.0, "brass")
    blade = poly_mask(c, [(38, 6), (41.5, 12), (41.5, 46), (34.5, 46), (34.5, 12)])
    stamp(c, blade, "bone", 0.5, field, 81)
    c.solid(c.rect(37, 12, 2, 32) & blade, (90, 84, 74))  # fuller
    guard = c.rect(27, 46, 22, 4)
    grip = c.rect(36, 50, 4, 9) | (c.dist(38, 61) <= 3)
    stamp(c, guard | grip, "brass", 0.42, field, 83)


def class_2(c, field):
    """Emberwright: a tongue of flame rising from cupped hands."""
    flame = np.zeros((M, M), bool)
    px, py = c.xx + 0.5, c.yy + 0.5
    for cx, cy, rx, ry in ((38, 36, 11, 15), (33, 30, 6, 12), (43, 27, 5, 11)):
        flame |= ((px - cx) / rx) ** 2 + ((py - cy) / ry) ** 2 <= 1
    flame |= _tri(c, (38, 8), (30, 30), (46, 30)) | _tri(c, (46, 13), (41, 27), (49, 26))
    flame &= py < 47
    stamp(c, flame, "ember", 0.45, field, 91)
    core = (((px - 38) / 5.5) ** 2 + ((py - 39) / 8) ** 2 <= 1)
    c.paint(core, "amber", 0.85 - 0.3 * (py - 31) / 16)
    hands = poly_mask(c, [(18, 42), (24, 40), (31, 47), (45, 47), (52, 40), (58, 42), (52, 56), (24, 56)])
    stamp(c, hands, "bone", 0.36, field, 92)
    c.solid(c.rect(37, 48, 2, 8), OUTLINE)


def class_3(c, field):
    """Cutthroat: two crossed knives and a hook."""
    hook = (c.dist(38, 22) <= 9) & (c.dist(38, 22) > 6) & (c.yy + 0.5 < 26)
    hook |= seg_mask(c, (38, 13), (38, 9), 1.5)
    k1 = poly_mask(c, [(17, 18), (22, 17), (45, 42), (42, 45)]) | seg_mask(c, (46, 47), (55, 56), 2.6)
    k2 = poly_mask(c, [(59, 18), (54, 17), (31, 42), (34, 45)]) | seg_mask(c, (30, 47), (21, 56), 2.6)
    stamp(c, hook, "iron", 0.5, field, 101)
    stamp(c, k1, "bone", 0.45, field, 102)
    stamp(c, k2, "bone", 0.45, field, 103)
    stamp(c, seg_mask(c, (40, 49), (48, 41), 1.6) | seg_mask(c, (36, 49), (28, 41), 1.6), "brass", 0.45, field, 104)


def class_4(c, field):
    """Ashpriest: a flanged mace standing before a cairn brazier."""
    stones = np.zeros((M, M), bool)
    for x, y, r in ((27, 55, 6), (38, 57, 6.5), (49, 55, 6), (32, 47, 5), (44, 47, 5)):
        stones |= c.dist(x, y) <= r
    stones &= field
    stamp(c, stones, "stone" if "stone" in ui.TABLE else "iron", 0.32, field, 111)
    fire = _tri(c, (38, 20), (29, 42), (47, 42)) | (c.dist(38, 38) <= 7)
    fire |= _tri(c, (31, 27), (28, 40), (35, 40)) | _tri(c, (46, 25), (41, 40), (49, 40))
    stamp(c, fire & (c.yy < 43), "ember", 0.45, field, 112)
    haft = seg_mask(c, (52, 18), (52, 58), 1.8)
    head = poly_mask(c, [(52, 9), (58, 15), (58, 24), (52, 28), (46, 24), (46, 15)])
    stamp(c, haft, "bronze", 0.4, field, 113)
    stamp(c, head, "iron", 0.45, field, 114)


def medallions():
    for i, draw in enumerate((class_1, class_2, class_3, class_4), start=1):
        c = Canvas(M, M)
        field = medallion_base(c, 120 + i)
        draw(c, field)
        # a dithered vignette inside the ring keeps the emblem readable
        d = c.dist(CX, CY)
        rim = field & (d > 24)
        dark = rim & (c.bayer < (d - 24) / 6)
        c.px[dark, :3] = (c.px[dark, :3] * 0.6).astype(np.uint8)
        c.save(f"menu_class_{i}")

    # selection ring: brass halo with ember ticks, transparent centre
    c = Canvas(M + 8, M + 8)
    cx = cy = (M + 8) / 2
    d = c.dist(cx, cy)
    ring = (d <= 41) & (d > 37.5)
    ang = np.arctan2(c.yy + 0.5 - cy, c.xx + 0.5 - cx)
    c.paint(ring, "brass", 0.62 + 0.25 * (-np.sin(ang + math.pi / 4)))
    c.solid((d > 41) & (d <= 42), OUTLINE)
    c.solid((d > 36.5) & (d <= 37.5), OUTLINE)
    for a in range(8):
        t = a * math.pi / 4
        tick = c.dist(cx + 39.3 * math.cos(t), cy + 39.3 * math.sin(t)) <= 2.2
        c.paint(tick, "ember", 0.85)
    c.save("menu_class_ring_sel")


# --- ornaments --------------------------------------------------------------------------------


def rule():
    w, h = 360, 14
    c = Canvas(w, h)
    cy = h / 2
    fade = np.clip(1 - np.abs(c.xx + 0.5 - w / 2) / (w / 2), 0, 1)
    line = c.rect(0, cy - 1, w, 2) & (c.bayer < fade * 1.6)
    c.paint(line, "bronze", 0.35 + 0.5 * fade)
    c.solid(c.rect(0, cy + 1, w, 1) & line[np.r_[1:h, 0], :] & ~line, OUTLINE)
    loz = np.abs(c.xx + 0.5 - w / 2) / 9 + np.abs(c.yy + 0.5 - cy) / 6 <= 1
    c.outline(loz)
    c.paint(loz, "brass", 0.75 - 0.4 * (c.yy - 1) / 12)
    inner = np.abs(c.xx + 0.5 - w / 2) / 4 + np.abs(c.yy + 0.5 - cy) / 2.6 <= 1
    c.paint(inner, "crimson", 0.7)
    for side in (-1, 1):
        rivet(c, w / 2 + side * 22, cy, 1.6, "brass")
    c.save("menu_rule")


def contact_sheet():
    ui.PREVIEW.mkdir(parents=True, exist_ok=True)
    ims = [Image.open(ui.OUT / f"{n}.png") for n in ui.SAVED]
    W, x, y, rowh = 900, 8, 8, 0
    pos = []
    for im in ims:
        if x + im.width > W:
            x, y, rowh = 8, y + rowh + 8, 0
        pos.append((x, y))
        x += im.width + 8
        rowh = max(rowh, im.height)
    sheet = Image.new("RGBA", (W, y + rowh + 8), (24, 14, 14, 255))
    for im, p in zip(ims, pos):
        sheet.alpha_composite(im.convert("RGBA"), p)
    sheet = sheet.resize((sheet.width * 2, sheet.height * 2), Image.NEAREST)
    sheet.save(ui.PREVIEW / "menu_sheet.png")


def main():
    if "stone" not in ui.TABLE:
        ui.RAMPS["stone"] = ["#1c1c1f", "#34343a", "#4f4f55", "#6e6d70", "#918e8c"]
        ui.TABLE["stone"] = np.array([ui._rgb(c) for c in ui.RAMPS["stone"]], dtype=np.uint8)
    for s in ("idle", "hover", "press", "disabled"):
        button(s)
    panel("menu_panel", dark=False)
    panel("menu_panel_dark", dark=True)
    field("menu_field", False)
    field("menu_field_focus", True)
    slider()
    checkbox()
    medallions()
    rule()
    print(f"{len(ui.SAVED)} images -> {ui.OUT}")
    if "--preview" in sys.argv:
        contact_sheet()


if __name__ == "__main__":
    main()
