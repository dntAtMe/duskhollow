"""Light textures for map lights (`light` lines of sprite_fx.txt, client `lights.rs`).

Usage (from repo root):  python -I tools/artgen/lightfx.py

Writes to `custom_assets/content/fx/`:
- `fx_light_glow.png` (422x193): the additive glow drawn on (and around) a light, tinted with the
  light's colour and scaled by its scale. White with an elliptical falloff (2.2 : 1, flattened like
  ground light in the isometric view), peak alpha ~0.38, Bayer-dithered alpha steps.
- `fx_light_mask.png` (1024x512): the light's cut-out in the map darkness. Black; alpha ~0.04 in
  the centre rising to 1 at the ellipse's edge (the darkness is multiplied by it), dithered.
- `custom_assets/preview/lightfx.png`: both over a dark ground (gitignored).
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
from vox import BAYER4  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets" / "content" / "custom" / "fx"
PREVIEW = ROOT / "custom_assets" / "preview"

GLOW_SIZE = (422, 193)
MASK_SIZE = (1024, 512)


def ellipse_r(w: int, h: int) -> np.ndarray:
    """Normalised elliptical distance from the centre: 0 in the middle, 1 at the image edge."""
    y, x = np.mgrid[0:h, 0:w] + 0.5
    return np.hypot((x - w / 2) / (w / 2), (y - h / 2) / (h / 2))


def dither(a: np.ndarray, levels: int) -> np.ndarray:
    """Quantise 0..1 to `levels` steps with 4x4 Bayer ordered dithering."""
    h, w = a.shape
    b = np.tile(BAYER4, (h // 4 + 1, w // 4 + 1))[:h, :w]
    return np.clip(np.floor(a * levels + b), 0, levels) / levels


def glow() -> np.ndarray:
    w, h = GLOW_SIZE
    r = ellipse_r(w, h)
    # Bright core easing into a long, faint skirt; exactly 0 at the edge.
    core = np.exp(-((r / 0.38) ** 2))
    skirt = np.clip(1 - r, 0, 1) ** 1.6
    a = 0.38 * (0.55 * core + 0.45 * skirt)
    a = dither(a, 48)
    a[r >= 1] = 0
    out = np.zeros((h, w, 4), np.uint8)
    out[..., :3] = 255
    out[..., 3] = (a * 255).round().astype(np.uint8)
    return out


def mask() -> np.ndarray:
    w, h = MASK_SIZE
    r = ellipse_r(w, h)
    a = 0.04 + 0.96 * np.clip(r, 0, 1) ** 1.25
    a = dither(a, 40)
    a[r >= 1] = 1
    out = np.zeros((h, w, 4), np.uint8)
    out[..., 3] = (a * 255).round().astype(np.uint8)
    return out


def preview(g: np.ndarray, m: np.ndarray) -> None:
    """The glow tinted ember on a dark crimson ground, and the mask cutting a 70 % darkness."""
    ground = np.zeros((MASK_SIZE[1], MASK_SIZE[0] + GLOW_SIZE[0] + 16, 3), float)
    ground[...] = (0.16, 0.06, 0.06)
    tint = np.array([0xE0, 0x64, 0x2A]) / 255.0
    ga = g[..., 3:4] / 255.0
    gh, gw = g.shape[:2]
    ground[:gh, :gw] += tint * ga
    dark = 0.7 * (m[..., 3] / 255.0)
    ground[:, gw + 16:] *= (1 - dark)[..., None]
    PREVIEW.mkdir(parents=True, exist_ok=True)
    Image.fromarray((np.clip(ground, 0, 1) * 255).astype(np.uint8)).save(PREVIEW / "lightfx.png")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    g, m = glow(), mask()
    Image.fromarray(g).save(OUT / "fx_light_glow.png", optimize=True)
    Image.fromarray(m).save(OUT / "fx_light_mask.png", optimize=True)
    preview(g, m)
    print("fx_light_glow.png, fx_light_mask.png ->", OUT)


if __name__ == "__main__":
    main()
