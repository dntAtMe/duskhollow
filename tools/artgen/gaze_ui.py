"""Gaze HUD art (docs/demo-plan.md), in the "iron & oak" look of `ui.py`:

- `gaze_eye_{lidded,half,open}.png` (32x20): the Eye glyph for the strain bar and the minimap,
  lid closing over a crimson iris with a pitch-black rift for a pupil.
- `gaze_strain_frame.png` (196x16): recessed iron well around the strain bar (fill area at
  (4, 4) size 188x8, see `GAZE_FILL` in `crates/dusk_client/src/gaze.rs`).
- `gaze_strain_fill.png` (188x8), `gaze_corruption_fill.png` (188x3): bar fills.

Output: `assets/content/ui/` (our own names, always available).

    python -I tools/artgen/gaze_ui.py
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import ui  # noqa: E402
from ui import OUTLINE, Canvas, grow, vnoise, well  # noqa: E402

ui.OUT = ui.ROOT / "assets" / "content" / "ui"
ui.RAMPS["crimson"] = ["#1e0306", "#43070d", "#6e0f16", "#9c1f22", "#c94a3a"]
ui.RAMPS["bruise"] = ["#140a1a", "#2a1236", "#462052", "#653070", "#8a4a8e"]
ui.RAMPS["lid"] = ["#1a120e", "#33241a", "#4d3828", "#6b5038", "#8c6c4c"]
for k in ("crimson", "bruise", "lid"):
    ui.TABLE[k] = np.array([ui._rgb(c) for c in ui.RAMPS[k]], dtype=np.uint8)

EYE_W, EYE_H = 32, 20


def eye(name: str, k: float, rays: float):
    """`k`: aperture (1 = wide open, ~0.4 = half-lidded); `rays`: length of the godly rays."""
    c = Canvas(EYE_W, EYE_H)
    cx, cy, w, h = 16.0, 10.0, 12.5, 6.5
    dx = (c.xx + 0.5 - cx) / w
    f = np.clip(1 - dx * dx, 0, None)
    y = c.yy + 0.5
    almond = (np.abs(dx) <= 1) & (y >= cy - h * f) & (y <= cy + h * f)
    # Rays: thin spokes behind the eye, faint crimson.
    if rays > 0:
        ang = np.arctan2(c.yy + 0.5 - cy, (c.xx + 0.5 - cx) * 0.8)
        r = c.dist(cx, cy)
        spoke = np.abs(((ang / (np.pi / 6)) + 0.5) % 1 - 0.5) < 0.12
        m = spoke & (r > 8.5) & (r < 8.5 + rays) & ~almond
        c.paint(m, "crimson", 0.55 - 0.35 * (r - 8.5) / rays)
    c.outline(almond)
    # Aperture: between the lower lid and the drooping upper lid.
    top = cy + h * f - 2 * h * f * k
    aperture = almond & (y >= top)
    lid = almond & ~aperture
    s = 0.55 - 0.3 * (c.yy - (cy - h)) / (2 * h) + 0.15 * (vnoise(EYE_H, EYE_W, 3, 3, 7) - 0.5)
    c.paint(lid, "lid", s)
    # Sclera: dull bone, darker towards the corners.
    sclera = aperture
    c.paint(sclera, "bone", 0.45 - 0.35 * np.abs(dx))
    r = c.dist(cx, cy + 0.5)
    iris = aperture & (r <= 5.2)
    c.paint(iris, "crimson", 0.95 - 0.12 * r)
    rift = aperture & (np.abs(c.xx + 0.5 - cx) <= 1.0) & (np.abs(c.yy + 0.5 - cy - 0.5) <= 4.0)
    c.solid(rift, (0, 0, 0))
    # Lid edge: a dark crease along the upper lid.
    crease = aperture & ~grow(aperture & ~grow(lid))
    c.solid(crease & (y < cy + 0.5), OUTLINE)
    c.save(name)


def strain_frame():
    c = Canvas(196, 16)
    well(c, 4, 4, 188, 8, rim=2)
    c.save("gaze_strain_frame")


def fill(name: str, w: int, h: int, ramp: str, seed: int):
    c = Canvas(w, h)
    t = c.yy / max(h - 1, 1)
    s = 0.8 - 0.5 * t + 0.1 * (vnoise(h, w, 18, 2, seed) - 0.5)
    c.paint(np.ones((h, w), bool), ramp, s)
    if h > 3:
        c.solid(c.yy == h - 1, (0, 0, 0), 140)
    c.save(name)


def main():
    eye("gaze_eye_lidded", 0.38, 0.0)
    eye("gaze_eye_half", 0.68, 2.0)
    eye("gaze_eye_open", 1.0, 4.0)
    strain_frame()
    fill("gaze_strain_fill", 188, 8, "crimson", 41)
    fill("gaze_corruption_fill", 188, 3, "bruise", 42)
    print(f"{len(ui.SAVED)} images -> {ui.OUT}")


if __name__ == "__main__":
    main()
