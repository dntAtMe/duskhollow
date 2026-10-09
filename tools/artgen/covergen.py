"""Writes the gaze cover sidecar (`maps/<name>.cover`, docs/demo-plan.md) for a custom map by
looking at its upright props: canopies (trees, pines) and walls cast shade, a hut's eaves are
deep shelter, a campfire is a rest cairn.

Usage (from repo root):  python -I tools/artgen/covergen.py [glade]

Cover chars: `.` open sky, `s` shade, `S` deep shelter, `C` rest cairn (cell centre).
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAPS = ROOT / "custom_assets" / "maps"

# prop name prefix -> (kind, radius in cells)
RULES = [
    ("cg_tree_", "s", 1.6),
    ("cg_pine_", "s", 1.2),
    ("cg_wall_", "s", 1.0),
    ("cg_rock_", "s", 0.0),
    ("cg_hut_", "hut", 0.0),
    ("cg_campfire_", "C", 0.0),
]
RANK = {".": 0, "s": 1, "S": 2, "C": 3}


def read_map(path: Path):
    """(size, {(x, y): upright texture name}) from a `.map` file (docs/formats.md)."""
    b = path.read_bytes()
    size, ntex = struct.unpack_from("<II", b, 0)
    off = 8
    names = []
    for _ in range(ntex):
        end = b.index(b"\0", off)
        names.append(b[off:end].decode("ascii"))
        off = end + 1
    (ncells,) = struct.unpack_from("<I", b, off)
    off += 4
    upright = {}
    for _ in range(ncells):
        idx, _flags = struct.unpack_from("<IB", b, off)
        off += 5
        layers = []
        for _ in range(3):
            present = b[off]
            off += 1
            if present:
                tex, _ = struct.unpack_from("<II", b, off)
                off += 8
                layers.append(names[tex])
            else:
                layers.append(None)
        if layers[2]:
            upright[(idx % size, idx // size)] = layers[2]
    return size, upright


def build(size, upright):
    grid = [["."] * size for _ in range(size)]

    def put(x, y, c):
        if 0 <= x < size and 0 <= y < size and RANK[c] > RANK[grid[y][x]]:
            grid[y][x] = c

    for (x, y), name in upright.items():
        rule = next((r for r in RULES if name.startswith(r[0])), None)
        if not rule:
            continue
        kind, radius = rule[1], rule[2]
        if kind == "hut":
            # 3x3 footprint centred here: roofed eaves (deep shelter) one cell around it, shade beyond.
            for dy in range(-3, 4):
                for dx in range(-3, 4):
                    ring = max(abs(dx), abs(dy))
                    put(x + dx, y + dy, "S" if ring <= 2 else "s")
            continue
        if kind == "C":
            put(x, y, "C")
            continue
        r = int(radius + 0.999)
        for dy in range(-r, r + 1):
            for dx in range(-r, r + 1):
                if dx * dx + dy * dy <= radius * radius + 0.01:
                    put(x + dx, y + dy, "s")
    return grid


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else "glade"
    size, upright = read_map(MAPS / f"{name}.map")
    grid = build(size, upright)
    out = MAPS / f"{name}.cover"
    text = f"{size} {size}\n" + "\n".join("".join(row) for row in grid) + "\n"
    out.write_text(text, newline="\n")
    counts = {c: text.count(c) for c in ".sSC"}
    print(f"{out.relative_to(ROOT)}: {size}x{size} {counts}")


if __name__ == "__main__":
    main()
