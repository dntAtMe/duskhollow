"""Builds a playable map entirely from our own art: `custom_glade`.

Usage (from repo root, after enviro.py):  python -I tools/artgen/mapgen.py

Writes:
- `custom_assets/maps/custom_glade.map` in the original `.map` format (docs/formats.md)
- `custom_assets/maps/custom_glade.spawns`: `entry x y orientation wander` per line, read by our server
- unique blended ground tiles for cells where terrain types meet
  (`custom_assets/content/custom/env/map/cg_glade_*.png`); pure cells reuse the periodic tiles.

Layout: a meadow ringed by forest, a winding dirt path crossing it, and a ruined cobble plaza with
broken walls where goblins camp.
"""

from __future__ import annotations

import json
import math
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import enviro  # noqa: E402
from enviro import PERIOD, TILE_H, TILE_W, diamond_coords, ground_texture, periodic_noise, shade_to_rgb  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
CA = ROOT / "custom_assets"
NAME = "custom_glade"
SIZE = 52
PLAZA = (32.0, 18.0)
POND = (13.0, 40.0)  # centre; elliptical, radii below
POND_R = (5.5, 3.8)
CAMP = (14, 15)  # hut centre cell (3x3 footprint)
FLAG_UNWALKABLE, FLAG_BLOCK = 0x20, 0x40


def path_y(x):
    return 27 + 7 * np.sin(x / 8.0) + 2.5 * np.sin(x / 3.1 + 1.0)


def terrain_at(x, y):
    """Ground type index per world point: 0 grass, 1 dirt, 2 cobble, 3 water."""
    wobble = periodic_noise(x / 4, y / 4, 2, 501, period=16) - 0.5
    on_path = np.abs(y - path_y(x)) < 1.5 + wobble * 1.6
    plaza = np.maximum(np.abs(x - PLAZA[0]), np.abs(y - PLAZA[1])) < 5.2 + wobble * 1.2
    pond = ((x - POND[0]) / POND_R[0]) ** 2 + ((y - POND[1]) / POND_R[1]) ** 2 < 1.0 + wobble * 0.5
    camp = np.hypot(x - CAMP[0] - 2, y - CAMP[1] - 2.5) < 3.2 + wobble  # trampled dirt yard
    return np.where(pond, 3, np.where(plaza, 2, np.where(on_path | camp, 1, 0)))


KINDS = ["grass", "dirt", "cobble", "water"]


def ground_tiles():
    """Per cell: texture filename (periodic tile or a baked blend)."""
    du, dv, inside = diamond_coords()
    py, px = np.indices((TILE_H, TILE_W))
    out_dir = CA / "content" / "custom" / "env" / "map"
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob("cg_glade_*.png"):
        old.unlink()
    names = {}
    baked = 0
    for y in range(SIZE):
        for x in range(SIZE):
            wx, wy = du + x, dv + y
            t = terrain_at(wx, wy)
            kinds = np.unique(t[inside])
            if len(kinds) == 1:
                names[(x, y)] = f"cg_{KINDS[kinds[0]]}_{x % PERIOD}{y % PERIOD}.png"
                continue
            img = np.zeros((TILE_H, TILE_W, 4), dtype=np.uint8)
            for k in kinds:
                ramp, shade = ground_texture(KINDS[k], wx % PERIOD, wy % PERIOD)
                rgb = shade_to_rgb(ramp, np.clip(shade, 0, 1), px, py)
                m = inside & (t == k)
                img[m, :3] = rgb[m]
            img[inside, 3] = 255
            name = f"cg_glade_{x}_{y}.png"
            Image.fromarray(img).save(out_dir / name)
            names[(x, y)] = name
            baked += 1
    blends = [n for n in names.values() if n.startswith("cg_glade_")]
    lines = "".join(f"{n} {TILE_W // 2} {TILE_H // 2}\n" for n in blends)
    (out_dir / "hotspots.txt").write_text(lines, newline="\n")
    print(f"ground: {SIZE * SIZE - baked} periodic cells, {baked} baked blends")
    return names


def near_camp(x, y):
    return abs(x - CAMP[0] - 1) <= 4 and abs(y - CAMP[1] - 1) <= 4


def props(rng, terrain):
    """Upright sprite + flags per cell."""
    man = json.loads((CA / "env_manifest.json").read_text())["upright"]
    pick = lambda kind: man[kind][rng.integers(len(man[kind]))]  # noqa: E731
    placed = {}
    for y in range(SIZE):
        for x in range(SIZE):
            edge = min(x, y, SIZE - 1 - x, SIZE - 1 - y)
            t = terrain[y, x]
            r = rng.random()
            if edge < 4:
                # Forest ring; the path may leave the map through it.
                if t == 1:
                    continue
                if r < 0.62 - edge * 0.08:
                    placed[(x, y)] = (pick("pine" if rng.random() < 0.55 else "tree"), FLAG_UNWALKABLE)
                elif r < 0.75:
                    placed[(x, y)] = (pick("bush"), 0)
                continue
            if t != 0 or near_camp(x, y):
                continue
            if r < 0.018:
                placed[(x, y)] = (pick("tree"), FLAG_UNWALKABLE)
            elif r < 0.028:
                placed[(x, y)] = (pick("pine"), FLAG_UNWALKABLE)
            elif r < 0.05:
                placed[(x, y)] = (pick("bush"), 0)
            elif r < 0.06:
                placed[(x, y)] = (pick("rock"), FLAG_UNWALKABLE)
    # Woodcutter's camp: hut (3x3, blocks movement), campfire, crates, barrels, a lamp by the path.
    hx, hy = CAMP
    for dx in (-1, 0, 1):
        for dy in (-1, 0, 1):
            placed.pop((hx + dx, hy + dy), None)
            placed[(hx + dx, hy + dy)] = (None, FLAG_UNWALKABLE | FLAG_BLOCK)  # footprint
    placed[(hx, hy)] = (man["hut"][0], FLAG_UNWALKABLE | FLAG_BLOCK)
    placed[(hx + 3, hy + 3)] = (man["campfire"][0], FLAG_UNWALKABLE)
    for (x, y), kind in (((hx - 2, hy + 2), "crate"), ((hx - 2, hy + 3), "crate"), ((hx + 2, hy - 2), "barrel"), ((hx + 3, hy - 2), "barrel")):
        placed[(x, y)] = (pick(kind), FLAG_UNWALKABLE)
    lx = 22
    placed[(lx, int(path_y(lx + 0.5)) - 2)] = (man["lamp"][0], FLAG_UNWALKABLE)
    # Fireflies over the pond banks (invisible .psi sprites carrying the original particle system).
    for _ in range(7):
        a = rng.uniform(0, 2 * math.pi)
        x, y = int(POND[0] + (POND_R[0] + 1.2) * math.cos(a)), int(POND[1] + (POND_R[1] + 1.2) * math.sin(a))
        if (x, y) not in placed and terrain[y, x] == 0:
            placed[(x, y)] = ("green_firefly.psi", 0)
    # Water can't be walked on.
    for y in range(SIZE):
        for x in range(SIZE):
            if terrain[y, x] == 3:
                placed.setdefault((x, y), (None, FLAG_UNWALKABLE))
    # Ruined walls around the plaza: a square ring with gaps and missing stones.
    px, py = int(PLAZA[0]), int(PLAZA[1])
    for d in range(-6, 7):
        for (x, y) in ((px + d, py - 6), (px + d, py + 6), (px - 6, py + d), (px + 6, py + d)):
            gap = abs(d) <= 1 or rng.random() < 0.25
            if not gap:
                placed[(x, y)] = (pick("wall"), FLAG_UNWALKABLE | FLAG_BLOCK)
            else:
                placed.pop((x, y), None)
    for (x, y) in ((px - 2, py - 2), (px + 2, py + 2)):
        placed[(x, y)] = (pick("wall"), FLAG_UNWALKABLE | FLAG_BLOCK)  # standing pillars
    return placed


def write_map(ground, placed):
    textures = sorted(set(ground.values()) | {p[0] for p in placed.values() if p[0]})
    tex_id = {t: i for i, t in enumerate(textures)}
    out = bytearray()
    out += struct.pack("<II", SIZE, len(textures))
    for t in textures:
        out += t.encode("ascii") + b"\0"
    out += struct.pack("<I", SIZE * SIZE)
    for y in range(SIZE):
        for x in range(SIZE):
            prop = placed.get((x, y))
            out += struct.pack("<IB", y * SIZE + x, prop[1] if prop else 0)
            out += struct.pack("<BII", 1, tex_id[ground[(x, y)]], 0)  # layer 0: ground
            out += b"\0"  # layer 1: decals (none)
            if prop and prop[0]:
                out += struct.pack("<BII", 1, tex_id[prop[0]], 0)  # layer 2: upright
            else:
                out += b"\0"
    out += struct.pack("<I", 0)  # no terrain textures (so no terrain pairs)
    out += struct.pack("<II", 0, 0)  # zones, areas
    (CA / "maps").mkdir(parents=True, exist_ok=True)
    (CA / "maps" / f"{NAME}.map").write_bytes(bytes(out))
    print(f"map {NAME}: {SIZE}x{SIZE}, {len(textures)} textures, {len(placed)} props")


def write_spawns(rng, terrain, placed):
    """Pit crawlers + thatch spinners (neutral) in the meadow, gnawers (hostile) camping in the ruins.

    Entries: data/npc_templates.txt (50020 Ditch Gnawer, 50021 Gnawer Rusher, 50022 Pit Crawler,
    50023 Thatch Spinner)."""
    lines = ["# entry x y orientation wander_distance (cells) -- read by dusk_server for custom maps"]

    def free(x, y):
        return (int(x), int(y)) not in placed and 4 <= x < SIZE - 4 and 4 <= y < SIZE - 4

    count = 0
    while count < 14:
        x, y = rng.uniform(5, SIZE - 5), rng.uniform(5, SIZE - 5)
        if free(x, y) and terrain[int(y), int(x)] == 0:
            entry = 50022 if count % 3 else 50023
            lines.append(f"{entry} {x:.1f} {y:.1f} {rng.uniform(0, 6.28):.2f} 4")
            count += 1
    for i in range(5):
        a = i * 2 * math.pi / 5
        x, y = PLAZA[0] + 3 * math.cos(a), PLAZA[1] + 3 * math.sin(a)
        if free(x, y):
            lines.append(f"{50021 if i % 2 else 50020} {x:.1f} {y:.1f} {a + math.pi:.2f} 2")
    (CA / "maps" / f"{NAME}.spawns").write_text("\n".join(lines) + "\n", newline="\n")


def main():
    enviro.main()  # make sure tiles/props exist and the palette is registered
    rng = np.random.default_rng(7)
    ys, xs = np.indices((SIZE, SIZE))
    terrain = terrain_at(xs + 0.5, ys + 0.5)
    ground = ground_tiles()
    placed = props(rng, terrain)
    write_map(ground, placed)
    write_spawns(rng, terrain, placed)


if __name__ == "__main__":
    main()
