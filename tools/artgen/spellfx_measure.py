"""Measures the LAYOUT of the original spell flipbooks (no pixels are kept).

Usage (from repo root, needs the extracted original assets):
    python -I tools/artgen/spellfx_measure.py "E:/path/to/assets"

For every `scripts/animation/*.sa` it records the script header (ratio, canvas size, delay, loop
range, frame numbers), whether the frames are opaque black-background (luma-keyed by the client)
or carry real alpha, and per frame only a coarse bounding box (canvas px, 2..98 % quantiles of the
lit pixels) and a brightness envelope (0..1). Plus one dominant colour per animation, used to pick
our school palette. `spellfx.py` draws its own art into those boxes so our effects sit exactly where
the originals did (`spell_visual_kit.spranim_x/y` positions the canvas and can't change).
Output: `tools/artgen/spellfx_layout.json` (committed, so spellfx.py runs without the originals).
"""

from __future__ import annotations

import colorsys
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

OUT = Path(__file__).with_name("spellfx_layout.json")


def parse_sa(text: str):
    kv, frames = {}, []
    for line in text.splitlines():
        line = line.strip()
        if "=" in line:
            k, v = line.split("=", 1)
            kv[k.strip()] = v.strip()
        elif line.count(",") == 2:
            frames.append(tuple(int(x) for x in line.split(",")))
    return kv, frames


def main(assets: Path):
    index = {}
    for line in (assets / "file_index.txt").read_text(encoding="utf-8", errors="replace").splitlines():
        parts = line.split("\t")
        if len(parts) == 2:
            index[parts[0].lower()] = parts[1]
    layout = {}
    for sa in sorted((assets / "scripts" / "animation").glob("*.sa")):
        kv, frames = parse_sa(sa.read_text(errors="replace"))
        lum, boxes_px, keyed_votes = [], [], []
        col_acc, col_w = np.zeros(3), 0.0
        for num, x, y in frames:
            rel = index.get(f"{kv['filename']}_{num}.png".lower())
            if rel is None:
                lum.append(None)
                continue
            im = np.asarray(Image.open(assets / rel).convert("RGBA")).astype(float) / 255.0
            rgb, a = im[..., :3], im[..., 3]
            keyed = bool(a.min() >= 1.0)
            keyed_votes.append(keyed)
            w = rgb.max(-1) if keyed else a * rgb.max(-1)
            lum.append((x, y, w))
            hot = w > 0.5 * max(w.max(), 1e-6)
            col_acc += (rgb[hot] * w[hot, None]).sum(0)
            col_w += w[hot].sum()
        peak = max((w.max() for e in lum if e for w in [e[2]]), default=1.0)
        energies = []
        for e in lum:
            if e is None:
                boxes_px.append(None)
                energies.append(0.0)
                continue
            x, y, w = e
            m = w > 0.12 * peak
            energies.append(float(w[w > 0.04 * peak].sum()))
            if m.sum() < 2:
                boxes_px.append(None)
                continue
            ys, xs = np.nonzero(m)
            qx = np.quantile(xs, [0.02, 0.98])
            qy = np.quantile(ys, [0.02, 0.98])
            boxes_px.append([int(x + qx[0]), int(y + qy[0]), int(x + qx[1] + 1), int(y + qy[1] + 1)])
        emax = max(energies) or 1.0
        c = col_acc / max(col_w, 1e-6)
        h, l, s = colorsys.rgb_to_hls(*c)
        layout[sa.name] = {
            "ratio": int(kv.get("ratio", 1)),
            "size": int(kv["size"]),
            "filename": kv["filename"],
            "delay": int(kv.get("delay", 50)),
            "loopstart": int(kv.get("loopstart", 0)),
            "loopend": int(kv.get("loopend", 0)),
            "frames": [f[0] for f in frames],
            "keyed": bool(keyed_votes) and sum(keyed_votes) * 2 > len(keyed_votes),
            "color": "#%02x%02x%02x" % tuple(int(v * 255) for v in c),
            "hue": round(h * 360), "sat": round(s, 2),
            "boxes": boxes_px,
            "env": [round(e / emax, 2) for e in energies],
        }
        print(sa.name, layout[sa.name]["color"], len(frames))
    OUT.write_text(json.dumps(layout, separators=(",", ":")) + "\n", newline="")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
