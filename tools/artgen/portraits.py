"""Unit-frame portraits: close-up renders of our models on a dark vignette.

Usage (from repo root):  python -I tools/artgen/portraits.py

Writes `custom_assets/content/custom/portraits/portrait_custom_<model>.png` (80x80, the HUD uses
small portraits whole and cuts the circle itself). The client prefers these with `--art custom`.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import character  # noqa: E402
import creatures  # noqa: E402
import valefolk  # noqa: E402
from vox import BAYER4, COS_E, PX_PER_UNIT, rot_y  # noqa: E402

OUT = Path(__file__).resolve().parents[2] / "custom_assets" / "content" / "custom" / "portraits"
SIZE = 80
CANVAS = 512
# A 3/4 view toward the camera: halfway between sheet directions S (pi/4) and SE (0).
FACING = math.pi / 8


def background() -> np.ndarray:
    """Dark warm vignette with ordered dithering, like a dim portrait card."""
    y, x = np.indices((SIZE, SIZE))
    d = np.hypot(x - SIZE / 2 + 0.5, y - SIZE * 0.42) / (SIZE * 0.7)
    shade = np.clip(1.0 - d, 0, 1) ** 1.5
    level = np.clip(shade * 3 + (BAYER4[y % 4, x % 4] - 0.5) * 0.8, 0, 3).round().astype(int)
    ramp = np.array([[14, 10, 8], [28, 20, 15], [44, 32, 22], [62, 45, 30]], dtype=np.uint8)
    img = np.zeros((SIZE, SIZE, 4), dtype=np.uint8)
    img[..., :3] = ramp[level]
    img[..., 3] = 255
    return img


def portrait(model, focus_z: float, scale: float, pose=None) -> np.ndarray:
    """Renders `model` large and crops a square centred on height `focus_z` (model units)."""
    rot, off, root_rot = pose(0.0) if pose else ({}, (0, 0, 0), None)
    world = model.pose(rot, off, root_rot)
    foot = (CANVAS // 2, CANVAS - 40)
    img = model.render(world, FACING, CANVAS, foot, scale)
    cy = int(foot[1] - focus_z * COS_E * PX_PER_UNIT * scale)
    cx = foot[0]
    crop = img[cy - SIZE // 2 : cy + SIZE // 2, cx - SIZE // 2 : cx + SIZE // 2]
    out = background()
    a = crop[..., 3:4].astype(float) / 255
    out[..., :3] = (crop[..., :3] * a + out[..., :3] * (1 - a)).astype(np.uint8)
    return out


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    jobs = {
        # name: (model, focus height, render scale, pose)
        "adventurer": (character.build(), 1.56, 2.0, character.stance),
        "goblin": (creatures.goblin("dagger", "rag"), 1.5, 2.1, creatures.hunched(character.stance)),
        "goblin_charger": (creatures.goblin("spear", "red"), 1.5, 2.1, creatures.hunched(character.stance)),
        "spider": (creatures.spider(), 0.32, 2.1, None),
        "antlion_small": (creatures.antling(), 0.22, 2.9, None),
        # Duskhollow (valefolk.py)
        "glarewolf": (valefolk.glarewolf(), 0.7, 2.1, valefolk.wolf_portrait),
        "stooped": (valefolk.stooped(), 1.18, 2.3, valefolk.stooped_anims()[0][1]),
        "hollowed_warden": (valefolk.hollowed_warden(), 1.68, 1.9, valefolk.warden_anims()[0][1]),
        "cairnkeeper": (valefolk.cairnkeeper(), 1.58, 2.1, valefolk.keeper_anims()[0][1]),
        "lightworker": (valefolk.lightworker(), 1.52, 2.0, valefolk.worker_anims()[0][1]),
        "lowshade_guard": (valefolk.lowshade_guard(), 1.62, 2.0, valefolk.guard_anims()[0][1]),
    }
    only = sys.argv[1:]
    for name, (model, z, scale, pose) in jobs.items():
        if only and name not in only:
            continue
        Image.fromarray(portrait(model, z, scale, pose)).save(OUT / f"portrait_custom_{name}.png")
        print(f"portrait {name}")


if __name__ == "__main__":
    main()
