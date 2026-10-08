"""Rendering animations to sprite sheets + sprite scripts in the original format.

Shared by character.py and creatures.py. A sprite script looks like `scripts/npc/*.txt`:
`image=`, then per animation `[name] frames= duration= type=` and
`frame=F,D,x,y,w,h,pivot_x,pivot_y` for 8 directions (docs/formats.md).
"""

from __future__ import annotations

import math
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets"

# Sheet direction order (engine): 0=W 1=NW 2=N 3=NE 4=E 5=SE 6=S 7=SW (screen).
# `ClientUnit::computeDirection` maps cell-space orientation k*pi/4 to sheet dir [5,6,7,0,1,2,3,4][k].
DIR_TO_ORIENTATION = {d: k * math.pi / 4 for k, d in enumerate([5, 6, 7, 0, 1, 2, 3, 4])}


def render_all(model, anims, frame: int, foot, scale: float):
    """anims: [(name, fn(t) -> (rotations, root_offset, root_rot), frames, ms, type)].
    Returns {name: [[rgba per dir] per frame]}."""
    out = {}
    for name, fn, frames, _, kind in anims:
        per_frame = []
        for f in range(frames):
            t = f / frames if kind == "looped" else f / max(frames - 1, 1)
            rot, off, root_rot = fn(t * 0.999)
            world = model.pose(rot, off, root_rot)
            per_frame.append([model.render(world, DIR_TO_ORIENTATION[d], frame, foot, scale) for d in range(8)])
        out[name] = per_frame
    return out


def trim(img: np.ndarray):
    ys, xs = np.nonzero(img[:, :, 3])
    if len(xs) == 0:
        return img[:1, :1], (0, 0)
    x0, x1, y0, y1 = xs.min(), xs.max() + 1, ys.min(), ys.max() + 1
    return img[y0:y1, x0:x1], (x0, y0)


def export(renders, anims, foot, image_name: str, script_path: Path, sheet_width: int = 1024):
    """Shelf-pack trimmed frames into one sheet (custom_assets/content/custom/<image_name>)
    and write the sprite script to `script_path`."""
    entries = []
    for name, *_ in anims:
        for f, dirs in enumerate(renders[name]):
            for d, img in enumerate(dirs):
                cropped, (x0, y0) = trim(img)
                entries.append((name, f, d, cropped, (foot[0] - x0, foot[1] - y0)))
    x = y = row_h = 0
    placed = []
    for e in sorted(entries, key=lambda e: -e[3].shape[0]):
        h, w = e[3].shape[:2]
        if x + w > sheet_width:
            x, y, row_h = 0, y + row_h + 1, 0
        placed.append((e, x, y))
        x += w + 1
        row_h = max(row_h, h)
    sheet = np.zeros((y + row_h + 1, sheet_width, 4), dtype=np.uint8)
    rects = {}
    for (name, f, d, img, pivot), px, py in placed:
        h, w = img.shape[:2]
        sheet[py : py + h, px : px + w] = img
        rects[(name, f, d)] = (px, py, w, h, pivot)

    img_dir = OUT / "content" / "custom"
    img_dir.mkdir(parents=True, exist_ok=True)
    script_path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(sheet).save(img_dir / image_name)

    lines = [f"image={image_name}", ""]
    for name, _, frames, duration, kind in anims:
        lines += [f"[{name}]", f"frames={frames}", f"duration={duration}ms", f"type={kind}"]
        for f in range(frames):
            for d in range(8):
                px, py, w, h, (pvx, pvy) = rects[(name, f, d)]
                lines.append(f"frame={f},{d},{px},{py},{w},{h},{pvx},{pvy}")
        lines.append("")
    script_path.write_text("\n".join(lines), newline="\n")
    print(f"  sheet {sheet.shape[1]}x{sheet.shape[0]} -> {image_name}")


def preview(renders, anims, frame: int, name: str):
    """custom_assets/preview/<name>_<anim>.png: rows = directions, columns = frames."""
    pdir = OUT / "preview"
    pdir.mkdir(parents=True, exist_ok=True)
    for anim, *_ in anims:
        frames = renders[anim]
        strip = Image.new("RGBA", (frame * len(frames), frame * 8), (58, 72, 40, 255))
        for f, dirs in enumerate(frames):
            for d, img in enumerate(dirs):
                strip.alpha_composite(Image.fromarray(img), (f * frame, d * frame))
        strip.save(pdir / f"{name}_{anim}.png")
