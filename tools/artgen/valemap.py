"""Builds the "First Gaze" demo map `duskhollow` (docs/demo-plan.md) from the crimson vale
art in `vale.py`.

Usage (from repo root):  python -I tools/artgen/valemap.py [--preview-only]

Writes:
- `custom_assets/content/vale/*.png` + `hotspots.txt` + `sprite_fx.txt` (tiles, cliffs, props)
- `custom_assets/maps/duskhollow.map` (original `.map` format, docs/formats.md)
- `.spawns` (`entry x y orientation wander`), `.cover` (`W H` + rows of `.`/`s`/`S`/`C`),
  `.markers` (`name x y radius`)
- `custom_assets/preview/duskhollow.png`: whole-map overview (painter's order, like the client)

Layout (cell x grows screen right-down, y grows left-down; "north" = screen up = small x + y):
- south: the gorge mouth at the bottom corner, arrival under open sky
- west-north-west: Lowshade under the great overhang (rock lip over x 7..12), 5 huts with deep
  eaves, a roofed lane from the hamlet gate to the cairn plaza, the rest cairn in the middle
- east: the Red Fields, three terraces of crimson grain with low dry-stone walls, 4 canopy
  shelters, lightworker poles
- north: the ruined Glare Gate (ring of broken walls, two towers flanking the north gap) around
  the boss arena; the Fallen Blade beyond it, in the top corner
- the still pool in a side pocket south-west of the vale's centre
Rock everywhere else: tall cliffs at the back (north-west / north-east), cliff heights limited
wherever a taller column would hide walkable ground behind it (so the front edges are low scree).
"""

from __future__ import annotations

import math
import struct
import sys
import time
from collections import deque
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import vale  # noqa: E402
from enviro import PERIOD, TILE_H, TILE_W, diamond_coords, periodic_noise  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
CA = ROOT / "custom_assets"
NAME = "duskhollow"
SIZE = 64
FLAG_UNWALKABLE, FLAG_BLOCK = 0x20, 0x40
KINDS = vale.GROUND_KINDS
K = {k: i for i, k in enumerate(KINDS)}

ARRIVAL = (54, 54)
CAIRN = (15, 34)
GATE = (16.5, 15.5)  # arena centre (world coords)
ARENA_R = 5.0
BLADE = (7, 7)  # anchor = front cell of the 2x2 footprint (6..7, 6..7)
POOL = (25.5, 51.0)
POOL_R = (3.1, 2.2)
XWALL = 6  # overhang wall column for Lowshade rows
LOW_Y = (23, 45)
HUTS = [(9, 27), (9, 34), (9, 41), (19, 28), (19, 41)]
LANE_X = (18, 24)  # roofed lane: walkable rows 33..35, back posts row 32, front posts row 36
LANE_Y = 33
CANOPIES = [(37, 10), (51, 10), (50, 19), (36, 27)]  # back corner of a 4x4 block
BEDS = [(9, 14), (18, 23), (27, 31)]  # grain bed rows (inclusive), x 35..55
TERRACE_ROWS = (16, 25)
LANES = [
    [(63.5, 63.5), (55, 55), (47, 47), (40, 41), (33, 36), (26, 34.5), (17, 34.5)],
    [(33, 36), (40, 33), (45, 28), (45.5, 14), (46, 8)],
    [(32, 33), (26, 26), (21, 21), (18.5, 17.5)],
    [(13.5, 12.5), (9.5, 9.5)],
]
CLIFF_LEVELS = (0.7, 1.3, 2.2, 3.2, 4.2, 5.2, 6.4)


# --- shape helpers --------------------------------------------------------------------------------


def seg_d(x, y, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = np.clip(((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy), 0, 1)
    return np.hypot(x - ax - t * dx, y - ay - t * dy)


def poly_d(x, y, pts):
    return np.minimum.reduce([seg_d(x, y, a, b) for a, b in zip(pts[:-1], pts[1:])])


def rect_d(x, y, x0, y0, x1, y1):
    return np.maximum.reduce([x0 - x, x - x1, y0 - y, y - y1])


def noise(x, y, freq, seed):
    return periodic_noise(np.asarray(x, float) / 4.0 % 16, np.asarray(y, float) / 4.0 % 16, freq, seed, period=16)


def walk_sdf(x, y):
    """< 0 inside the open vale (before per-cell overrides)."""
    d = [
        seg_d(x, y, (63.5, 63.5), (47, 47)) - 4.0,
        seg_d(x, y, (47, 47), (36, 39)) - 5.0,
        np.hypot(x - 32, y - 34) - 7.5,
        rect_d(x, y, 7, 23, 26, 46),
        rect_d(x, y, 33, 7, 58, 34),
        seg_d(x, y, (34, 36), (42, 28)) - 5.0,
        seg_d(x, y, (30, 30), (20, 20)) - 4.5,
        np.hypot(x - GATE[0], y - GATE[1]) - 7.8,
        seg_d(x, y, (12.5, 12.5), (8, 8)) - 2.6,
        np.hypot(x - 7.5, y - 7.5) - 4.2,
        np.hypot(x - 26, y - 50.5) - 5.5,
        seg_d(x, y, (35, 41), (28, 48)) - 2.6,
    ]
    return np.minimum.reduce(d) + (noise(x, y, 2, 701) - 0.5) * 1.8


def lip_reach(y):
    if not (LOW_Y[0] <= y <= LOW_Y[1]):
        return 0
    bump = math.sin(math.pi * (y - LOW_Y[0] + 0.5) / (LOW_Y[1] - LOW_Y[0] + 1))
    return max(1, int(round(1 + 5 * bump)))


def in_canopy(x, y):
    return any(cx <= x <= cx + 3 and cy <= y <= cy + 3 for cx, cy in CANOPIES)


# --- layout ----------------------------------------------------------------------------------------


def build_walk():
    ys, xs = np.indices((SIZE, SIZE))
    walk = walk_sdf(xs + 0.5, ys + 0.5) < 0
    walk[:, :1] = walk[:1, :] = False
    walk[:, -1:] = walk[-1:, :] = False
    for y in range(LOW_Y[0], LOW_Y[1] + 1):
        walk[y, : XWALL + 1] = False
        walk[y, XWALL + 1 : XWALL + 7] = True
    # keep only what is reachable from the arrival (4-connected)
    seen = np.zeros_like(walk)
    q = deque([ARRIVAL[::-1]])
    seen[ARRIVAL[1], ARRIVAL[0]] = True
    while q:
        y, x = q.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < SIZE and 0 <= nx < SIZE and walk[ny, nx] and not seen[ny, nx]:
                seen[ny, nx] = True
                q.append((ny, nx))
    return seen


def cliff_heights(rock, walk):
    """Tall at the back; a column never hides walkable ground behind it on screen."""
    ys, xs = np.nonzero(walk)
    wsum, wdiff = xs + ys, xs - ys
    top = np.zeros((SIZE, SIZE))
    for y, x in zip(*np.nonzero(rock)):
        s, d = x + y, x - y
        m = (wsum < s) & (np.abs(wdiff - d) <= 1.5)
        allowed = 5.2 if not m.any() else ((s - wsum[m]).min() - 1.0) / 2.45
        top[y, x] = max([lv for lv in CLIFF_LEVELS if lv <= allowed] or [CLIFF_LEVELS[0]])
    # the great overhang: its wall (and the rock behind it) stands higher than the rest
    for y in range(LOW_Y[0] - 1, LOW_Y[1] + 2):
        top[y, : XWALL + 1][rock[y, : XWALL + 1]] = CLIFF_LEVELS[-1]
    return top


def terrain_at(x, y):
    """Ground kind per world point."""
    wf = walk_sdf(x, y)
    n1 = noise(x, y, 3, 711)
    n2 = noise(x, y, 5, 713)
    t = np.full(np.shape(x), K["rust"])
    t = np.where(n1 > 0.58, K["ochre"], t)
    t = np.where(noise(x, y, 4, 715) > 0.62, K["grass"], t)
    t = np.where(np.hypot(x - ARRIVAL[0], y - ARRIVAL[1]) < 7 + 2 * n2, np.where(n1 > 0.4, K["ochre"], K["rust"]), t)
    in_fields = (x >= 33) & (x <= 58) & (y >= 7) & (y <= 34)
    t = np.where(in_fields & (n2 > 0.55), K["grass"], t)
    for y0, y1 in BEDS:
        bed = (x >= 35) & (x < 56) & (y >= y0) & (y < y1 + 1)
        t = np.where(bed, K["field"], t)
    for cx, cy in CANOPIES:
        t = np.where((x >= cx - 0.3) & (x < cx + 4.3) & (y >= cy - 0.3) & (y < cy + 4.3), np.where(n2 > 0.5, K["ochre"], K["lane"]), t)
    t = np.where(wf > -1.4 + 0.9 * n2, K["gorge"], t)
    t = np.where(wf > -0.15, K["scree"], t)
    lane = np.minimum.reduce([poly_d(x, y, p) for p in LANES])
    t = np.where(lane < 1.0 + 0.6 * (n2 - 0.5), K["lane"], t)
    plaza = np.hypot(x - CAIRN[0] - 0.5, y - CAIRN[1] - 0.5) < 2.7 + 0.5 * (n1 - 0.5)
    lane_roofed = (x >= LANE_X[0]) & (x < LANE_X[1] + 1) & (y >= LANE_Y - 1) & (y < LANE_Y + 4)
    t = np.where(plaza | lane_roofed, K["flag"], t)
    arena = (np.hypot(x - GATE[0], y - GATE[1]) < ARENA_R + 0.6) & (noise(x, y, 6, 717) > 0.3)
    t = np.where(arena, K["flag"], t)
    pool = ((x - POOL[0]) / POOL_R[0]) ** 2 + ((y - POOL[1]) / POOL_R[1]) ** 2 < 1.0 + 0.35 * (n2 - 0.5)
    t = np.where(pool, K["water"], t)
    return t


def shadow_grid(rock, top):
    """Shadow level per cell: 0 open sky, 1 shade, 2 deep shelter."""
    g = np.zeros((SIZE, SIZE))
    for y in range(SIZE):
        r = lip_reach(y)
        if r:
            g[y, XWALL + 1 : XWALL + 1 + r] = 2
            g[y, XWALL + 1 + r : XWALL + 3 + r] = np.maximum(g[y, XWALL + 1 + r : XWALL + 3 + r], 1)
    for y in range(1, SIZE):
        for x in range(1, SIZE):
            if not rock[y, x] and max(top[y - 1, x - 1], top[y, x - 1], top[y - 1, x]) >= 3.2:
                g[y, x] = max(g[y, x], 1)
    for hx, hy in HUTS:
        g[hy - 2 : hy + 3, hx - 2 : hx + 3] = np.maximum(g[hy - 2 : hy + 3, hx - 2 : hx + 3], 1)
        g[hy - 1 : hy + 2, hx - 1 : hx + 2] = 2
    g[LANE_Y : LANE_Y + 3, LANE_X[0] : LANE_X[1] + 1] = 2
    for cx, cy in CANOPIES:
        g[cy : cy + 4, cx : cx + 4] = np.maximum(g[cy : cy + 4, cx : cx + 4], 1)
    return g


def shadow_at(grid, x, y):
    """Per-point shadow level: bilinear grid + noise, quantized (dithered edges)."""
    gx, gy = np.clip(x - 0.5, 0, SIZE - 1.001), np.clip(y - 0.5, 0, SIZE - 1.001)
    x0, y0 = np.floor(gx).astype(int), np.floor(gy).astype(int)
    fx, fy = gx - x0, gy - y0
    x1, y1 = np.minimum(x0 + 1, SIZE - 1), np.minimum(y0 + 1, SIZE - 1)
    v = (grid[y0, x0] * (1 - fx) + grid[y0, x1] * fx) * (1 - fy) + (grid[y1, x0] * (1 - fx) + grid[y1, x1] * fx) * fy
    v = v + (noise(x, y, 12, 721) - 0.5) * 0.7
    return np.clip(np.round(v), 0, 2).astype(int)


# --- ground tiles ------------------------------------------------------------------------------------


def eye_smear(wx, wy):
    """The Eye's reflection in the still pool: a dull crimson almond with a dark rift."""
    ex, ey = POOL[0] + 0.3, POOL[1] - 0.2
    s = ((wx - ex) + (wy - ey)) / math.sqrt(2)  # screen-vertical
    t = ((wx - ex) - (wy - ey)) / math.sqrt(2)  # screen-horizontal
    smear = np.exp(-((t / 1.25) ** 2) - (s / 0.75) ** 2)
    streak = 0.65 + 0.5 * noise(wx * 2, wy * 2, 9, 731)
    smear = smear * streak * np.where(((s * 7) % 1.0) < 0.22, 0.6, 1.0)
    pupil = (np.abs(t) < 0.11 + 0.05 * (1 - np.abs(s) / 0.5)) & (np.abs(s) < 0.42)
    return smear, pupil


def ground_tiles(rock, sgrid):
    du, dv, inside = diamond_coords()
    py, px = np.indices((TILE_H, TILE_W))
    names = {}
    baked = 0
    for old in vale.OUT.glob("cv_dh_*.png"):
        old.unlink()
    for y in range(SIZE):
        for x in range(SIZE):
            if rock[y, x]:
                names[(x, y)] = vale.periodic_tile("scree", x % PERIOD, y % PERIOD, 1)
                continue
            wx, wy = du + x, dv + y
            t = terrain_at(wx, wy)
            s = shadow_at(sgrid, wx, wy)
            kinds, levels = np.unique(t[inside]), np.unique(s[inside])
            smear, pupil = (None, None)
            if K["water"] in kinds:
                smear, pupil = eye_smear(wx, wy)
            plain = smear is None or (smear[inside] < 0.3).all() and not pupil[inside].any()
            if len(kinds) == 1 and len(levels) == 1 and plain:
                names[(x, y)] = vale.periodic_tile(KINDS[kinds[0]], x % PERIOD, y % PERIOD, int(levels[0]))
                continue
            img = np.zeros((TILE_H, TILE_W, 4), dtype=np.uint8)
            mul = np.asarray(vale.SHADOW_MUL)[s]
            for k in kinds:
                ramp, shade = vale.ground_texture(KINDS[k], wx % PERIOD, wy % PERIOD)
                shade = shade * mul
                if KINDS[k] == "water" and smear is not None:
                    eye = smear > 0.3
                    ramp = np.where(eye, vale.rid("g_eye"), ramp)
                    shade = np.where(eye, 0.2 + 0.55 * smear, shade)
                    ramp = np.where(pupil, vale.rid("g_water"), ramp)
                    shade = np.where(pupil, 0.04, shade)
                rgb = vale.dither_rgb(ramp, np.clip(shade, 0, 1), px, py)
                m = inside & (t == k)
                img[m, :3] = rgb[m]
            img[inside, 3] = 255
            name = f"cv_dh_{x}_{y}.png"
            Image.fromarray(img).save(vale.OUT / name, optimize=True)
            vale.HOTSPOTS.append(f"{name} {TILE_W // 2} {TILE_H // 2}")
            names[(x, y)] = name
            baked += 1
    print(f"ground: {SIZE * SIZE - baked} periodic cells, {baked} baked blends")
    return names


# --- cliffs (parallel) ----------------------------------------------------------------------------

_CLIFFS = None


def _init_worker(args):
    global _CLIFFS
    vale.register_ramps()
    _CLIFFS = vale.Cliffs(*args)


def _cliff_job(job):
    cx, cy, name = job
    vale.HOTSPOTS.clear()
    r = _CLIFFS.render_cell(cx, cy, name)
    return (cx, cy, name if r else None, vale.HOTSPOTS[-1] if r else None)


def cliff_sprites(rock, top):
    lip_x = np.full(SIZE, -1)
    lip_r = np.zeros(SIZE, dtype=int)
    lip_b = np.zeros(SIZE)
    for y in range(SIZE):
        r = lip_reach(y)
        if r:
            lip_x[y] = XWALL
            # The rock lip itself only reaches ~2/3 of the sheltered strip and hangs high (>= 4):
            # anything lower or deeper would be drawn over the hamlet (the lip of a row further
            # down-screen sits in front of the cells behind it). Its shadow does the rest.
            lip_r[y] = max(1, round(r * 0.7))
            lip_b[y] = 4.0 + 0.4 * (1 - r / 6)
    for old in vale.OUT.glob("cv_cliff_*.png"):
        old.unlink()
    for old in vale.OUT.glob("cv_rock_*.png"):
        old.unlink()
    jobs, assign, variants = [], {}, {}
    for y, x in zip(*np.nonzero(rock)):
        interior = lip_x[y] != x
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                ny, nx = min(max(y + dy, 0), SIZE - 1), min(max(x + dx, 0), SIZE - 1)
                if not rock[ny, nx] or top[ny, nx] < top[y, x]:
                    interior = False
        if interior:
            key = (x % PERIOD, y % PERIOD, CLIFF_LEVELS.index(top[y, x]))
            if key not in variants:
                variants[key] = f"cv_rock_{key[0]}{key[1]}_{key[2]}.png"
                jobs.append((x, y, variants[key]))
            assign[(x, y)] = variants[key]
        else:
            name = f"cv_cliff_{x}_{y}.png"
            jobs.append((x, y, name))
            assign[(x, y)] = name
    print(f"cliffs: {len(jobs)} renders ({len(variants)} periodic variants) for {len(assign)} rock cells")
    t0 = time.time()
    done = {}
    with ProcessPoolExecutor(initializer=_init_worker, initargs=((rock, top, lip_x, lip_r, lip_b),)) as ex:
        for cx, cy, name, hot in ex.map(_cliff_job, jobs, chunksize=4):
            if name:
                done[name] = True
                vale.HOTSPOTS.append(hot)
    print(f"cliffs rendered in {time.time() - t0:.0f}s")
    return {c: n for c, n in assign.items() if n in done}


# --- props ------------------------------------------------------------------------------------------


def place_props(rng, walk, rock, top, cliffs, props):
    placed = {}  # (x, y) -> (sprite or None, flags)
    pick = lambda kind: props[kind][rng.integers(len(props[kind]))]  # noqa: E731
    for (x, y), name in cliffs.items():
        placed[(x, y)] = (name, FLAG_UNWALKABLE | (FLAG_BLOCK if top[y, x] >= 1.3 else 0))
    for y, x in zip(*np.nonzero(rock)):
        placed.setdefault((x, y), (None, FLAG_UNWALKABLE | FLAG_BLOCK))
    reserved = set()  # cells kept clear (lanes, plaza, arena centre, marker spots)
    ys, xs = np.indices((SIZE, SIZE))
    lane = np.minimum.reduce([poly_d(xs + 0.5, ys + 0.5, p) for p in LANES])
    for y, x in zip(*np.nonzero(lane < 1.3)):
        reserved.add((x, y))

    def free(x, y):
        return 0 <= x < SIZE and 0 <= y < SIZE and walk[y, x] and (x, y) not in placed and (x, y) not in reserved

    # Lowshade: huts (3x3 footprint, deep eaves), roofed lane, cairn, lanterns, clutter
    for i, (hx, hy) in enumerate(HUTS):
        for dx in (-1, 0, 1):
            for dy in (-1, 0, 1):
                placed[(hx + dx, hy + dy)] = (None, FLAG_UNWALKABLE | FLAG_BLOCK)
        placed[(hx, hy)] = (props["hut"][i % len(props["hut"])], FLAG_UNWALKABLE | FLAG_BLOCK)
    for x in range(LANE_X[0], LANE_X[1] + 1):
        roof = pick("lane_roof_post") if x % 2 == 0 else pick("lane_roof")
        placed[(x, LANE_Y - 1)] = (roof, FLAG_UNWALKABLE)
        if x % 2 == 0:
            placed[(x, LANE_Y + 3)] = (props["lane_post"][0], FLAG_UNWALKABLE)
        for dy in range(3):
            reserved.add((x, LANE_Y + dy))
    placed[CAIRN] = (props["cairn"][0], FLAG_UNWALKABLE)
    for dx in range(-2, 3):
        for dy in range(-2, 3):
            if (dx, dy) != (0, 0):
                reserved.add((CAIRN[0] + dx, CAIRN[1] + dy))
    for c in ((17, 31), (17, 37), (25, 32), (25, 36), (12, 25), (12, 44)):
        placed[c] = (props["lantern"][0], FLAG_UNWALKABLE)
    for c, kind in (
        ((11, 26), "crate"), ((11, 30), "sacks"), ((7, 31), "woodpile"), ((21, 26), "crate"),
        ((21, 43), "barrel"), ((11, 43), "sacks"), ((7, 44), "crate"), ((7, 37), "barrel"),
        ((22, 30), "woodpile"), ((7, 24), "barrel"),
    ):
        if free(*c):
            placed[c] = (pick(kind), FLAG_UNWALKABLE)

    # Red Fields: canopies, terrace walls, poles, grain
    for cx, cy in CANOPIES:
        placed[(cx, cy)] = (props["canopy"][(cx + cy) % 2], FLAG_UNWALKABLE)
        for c in ((cx + 3, cy), (cx, cy + 3), (cx + 3, cy + 3)):
            placed[c] = (props["canopy_post"][0], FLAG_UNWALKABLE)
        for dx in range(4):
            for dy in range(4):
                reserved.add((cx + dx, cy + dy))
    for ty in TERRACE_ROWS:
        for x in range(34, 57):
            if free(x, ty) and x % 9 not in (0, 1) and rng.random() > 0.12:
                placed[(x, ty)] = (pick("terrace"), FLAG_UNWALKABLE)
    for c in ((42, 12), (48, 21), (34, 20), (54, 29), (47, 31), (40, 22)):
        if free(*c):
            placed[c] = (pick("pole"), FLAG_UNWALKABLE)
    for y0, y1 in BEDS:
        for y in range(y0, y1 + 1):
            if y % 2:
                continue
            for x in range(35, 56):
                if free(x, y) and rng.random() < 0.8:
                    placed[(x, y)] = (pick("grain"), 0)

    # Glare Gate: ring of broken walls with gaps north (towers) and south (the way in)
    gx, gy = GATE
    north, south = math.radians(225), math.radians(45)
    for y in range(SIZE):
        for x in range(SIZE):
            d = math.hypot(x + 0.5 - gx, y + 0.5 - gy)
            if not (6.0 <= d < 7.0) or not walk[y, x]:
                continue
            a = math.atan2(y + 0.5 - gy, x + 0.5 - gx) % (2 * math.pi)
            if abs((a - north + math.pi) % (2 * math.pi) - math.pi) < math.radians(24):
                continue
            if abs((a - south + math.pi) % (2 * math.pi) - math.pi) < math.radians(32):
                continue
            back = x + y < gx + gy
            if back and rng.random() < 0.8:
                placed[(x, y)] = (props["gate_wall_tall"][rng.integers(2)], FLAG_UNWALKABLE | FLAG_BLOCK)
            elif not back and rng.random() < 0.6:
                placed[(x, y)] = (pick("gate_wall_low"), FLAG_UNWALKABLE)
    dn = np.array([-1, -1]) / math.sqrt(2)
    pp = np.array([1, -1]) / math.sqrt(2)
    towers = []
    for k, side in enumerate((1, -1)):
        c = np.array(GATE) + 6.3 * dn + side * 2.9 * pp
        x0, y0 = int(round(c[0])) - 1, int(round(c[1])) - 1  # footprint x0..x0+1, y0..y0+1
        for dx in (0, 1):
            for dy in (0, 1):
                placed[(x0 + dx, y0 + dy)] = (None, FLAG_UNWALKABLE | FLAG_BLOCK)
        placed[(x0 + 1, y0 + 1)] = (props["gate_tower"][k], FLAG_UNWALKABLE | FLAG_BLOCK)
        towers.append((x0 + 1, y0 + 1))
    for c in ((14, 17), (19, 13)):
        if free(*c):
            placed[c] = (props["bones"][1], 0)
    # the Fallen Blade (2x2 footprint, anchored at the front cell)
    bx, by = BLADE
    for dx in (-1, 0):
        for dy in (-1, 0):
            placed[(bx + dx, by + dy)] = (None, FLAG_UNWALKABLE | FLAG_BLOCK)
    placed[BLADE] = (props["blade"][0], FLAG_UNWALKABLE | FLAG_BLOCK)

    # the still pool: water is not walkable; reeds and stones on the banks
    ys, xs = np.indices((SIZE, SIZE))
    pool = terrain_at(xs + 0.5, ys + 0.5) == K["water"]
    for y, x in zip(*np.nonzero(pool & walk)):
        placed[(x, y)] = (None, FLAG_UNWALKABLE)
    for c in ((22, 49), (28, 53), (23, 53), (29, 49)):
        if free(*c):
            placed[c] = (pick("reeds"), 0)
    for c, kind in (((21, 51), "boulder"), ((30, 51), "dead_tree")):
        if free(*c):
            placed[c] = (pick(kind), FLAG_UNWALKABLE)

    # arrival + scatter: dead trees, boulders at cliff feet, bones, the broken cart
    for c, kind, fl in (
        ((57, 52), "cart", FLAG_UNWALKABLE), ((50, 56), "dead_tree", FLAG_UNWALKABLE),
        ((58, 56), "bones", 0), ((47, 51), "dead_tree", FLAG_UNWALKABLE), ((52, 49), "boulder", FLAG_UNWALKABLE),
    ):
        if free(*c):
            placed[c] = (props[kind][0] if kind == "cart" else pick(kind), fl)
    reserved.update({(ARRIVAL[0] + dx, ARRIVAL[1] + dy) for dx in (-1, 0, 1) for dy in (-1, 0, 1)})
    wf = walk_sdf(xs + 0.5, ys + 0.5)
    for y in range(SIZE):
        for x in range(SIZE):
            if not free(x, y):
                continue
            r = rng.random()
            lowshade = XWALL < x <= 26 and LOW_Y[0] - 1 <= y <= LOW_Y[1] + 1
            fields = 33 <= x <= 58 and 7 <= y <= 34
            arena = math.hypot(x + 0.5 - gx, y + 0.5 - gy) < 6
            if wf[y, x] > -1.6 and r < 0.12 and not lowshade:
                placed[(x, y)] = (pick("boulder"), FLAG_UNWALKABLE)
            elif r < 0.02 and not lowshade and not arena:
                placed[(x, y)] = (pick("dead_tree"), FLAG_UNWALKABLE)
            elif r < 0.035 and not lowshade:
                placed[(x, y)] = (pick("bones"), 0)
            elif fields and r < 0.05:
                placed[(x, y)] = (pick("bones"), 0)
    return placed, towers


# --- sidecars -----------------------------------------------------------------------------------------


def cover_grid(walk):
    g = np.full((SIZE, SIZE), ".", dtype="<U1")
    for y in range(SIZE):
        r = lip_reach(y)
        if r:
            g[y, XWALL + 1 : XWALL + 1 + r] = "S"
            g[y, XWALL + 1 + r] = "s"
    for hx, hy in HUTS:
        for dx in range(-2, 3):
            for dy in range(-2, 3):
                c = (hy + dy, hx + dx)
                if max(abs(dx), abs(dy)) <= 1:
                    g[c] = "S"
                elif g[c] == ".":
                    g[c] = "s"
    g[LANE_Y : LANE_Y + 3, LANE_X[0] : LANE_X[1] + 1] = "S"
    for row in (LANE_Y - 1, LANE_Y + 3):
        for x in range(LANE_X[0], LANE_X[1] + 1):
            if g[row, x] == ".":
                g[row, x] = "s"
    for cx, cy in CANOPIES:
        blk = g[cy : cy + 4, cx : cx + 4]
        blk[blk == "."] = "s"
    g[CAIRN[1], CAIRN[0]] = "C"
    return g


def write_cover(g):
    lines = [f"{SIZE} {SIZE}"] + ["".join(row) for row in g]
    (CA / "maps" / f"{NAME}.cover").write_text("\n".join(lines) + "\n", newline="\n")


MARKERS = [
    ("arrival", ARRIVAL[0] + 0.5, ARRIVAL[1] + 0.5, 3),
    ("lowshade", 14.5, 34.5, 9),
    ("red_fields", 45.0, 20.0, 12),
    ("glare_gate", GATE[0], GATE[1], ARENA_R),
    ("fallen_blade", 9.5, 9.5, 3),
    ("still_pool", POOL[0], POOL[1], 4),
]


def write_markers():
    lines = ["# name x y radius (cells) -- demo landmarks, see docs/demo-plan.md"]
    lines += [f"{n} {x:.1f} {y:.1f} {r:g}" for n, x, y, r in MARKERS]
    (CA / "maps" / f"{NAME}.markers").write_text("\n".join(lines) + "\n", newline="\n")


SPAWNS = [
    # glarewolves (50001) in three packs harrying the lightworkers, the alpha (50002) apart
    (50001, 41.5, 19.5, 0.8, 3), (50001, 43.0, 20.5, 2.4, 3), (50001, 42.0, 21.8, 4.0, 3),
    (50001, 49.5, 15.5, 1.2, 3), (50001, 51.0, 17.5, 3.0, 3), (50001, 50.5, 14.0, 5.1, 3),
    (50001, 47.0, 29.5, 0.3, 3), (50001, 48.5, 30.5, 2.0, 3),
    (50002, 54.5, 24.5, 3.9, 2),
    # Ysolde, the cairnkeeper, beside the cairn facing the lane
    (50010, 16.5, 35.5, 0.0, 0),
    # lightworkers near the canopies
    (50011, 41.5, 13.5, 2.3, 2), (50011, 49.5, 23.5, 4.2, 2), (50011, 40.5, 29.5, 0.9, 2),
    # Lowshade guards at the hamlet's east edge, looking out
    (50012, 26.5, 31.5, 0.0, 0), (50012, 26.5, 37.5, 0.0, 0),
]


def write_spawns(placed):
    lines = ["# entry x y orientation wander_distance (cells) -- read by dusk_server for custom maps"]
    for e, x, y, o, w in SPAWNS:
        c = (int(x), int(y))
        if c in placed and placed[c][1] & FLAG_UNWALKABLE:
            print(f"warning: spawn {e} at {c} is on an unwalkable cell")
        lines.append(f"{e} {x:.1f} {y:.1f} {o:.2f} {w}")
    (CA / "maps" / f"{NAME}.spawns").write_text("\n".join(lines) + "\n", newline="\n")


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
            out += struct.pack("<BII", 1, tex_id[ground[(x, y)]], 0)
            out += b"\0"
            if prop and prop[0]:
                out += struct.pack("<BII", 1, tex_id[prop[0]], 0)
            else:
                out += b"\0"
    out += struct.pack("<I", 0)
    out += struct.pack("<II", 0, 0)
    (CA / "maps").mkdir(parents=True, exist_ok=True)
    (CA / "maps" / f"{NAME}.map").write_bytes(bytes(out))
    print(f"map {NAME}: {SIZE}x{SIZE}, {len(textures)} textures, {sum(1 for p in placed.values() if p[0])} uprights")


def check_paths(placed):
    blocked = {c for c, p in placed.items() if p[1] & FLAG_UNWALKABLE}
    start = ARRIVAL
    seen = {start}
    q = deque([start])
    while q:
        x, y = q.popleft()
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            c = (x + dx, y + dy)
            if 0 <= c[0] < SIZE and 0 <= c[1] < SIZE and c not in seen and c not in blocked:
                seen.add(c)
                q.append(c)
    for n, x, y, r in MARKERS:
        ok = (int(x), int(y)) in seen or any(math.hypot(c[0] + 0.5 - x, c[1] + 0.5 - y) <= r for c in seen)
        print(f"  reachable {n}: {ok}")
    for e, x, y, *_ in SPAWNS:
        if (int(x), int(y)) not in seen:
            print(f"  warning: spawn {e} at ({x}, {y}) unreachable")


# --- overview preview -------------------------------------------------------------------------------


def preview(ground, placed, cover):
    hot = {}
    for line in vale.HOTSPOTS:
        n, a, b = line.split()
        hot[n] = (int(a), int(b))
    margin_top = 560
    W, H = SIZE * 64 + 64, SIZE * 32 + margin_top + 64
    ox, oy = W // 2, margin_top
    img = Image.new("RGBA", (W, H), (6, 4, 8, 255))
    cache = {}

    def get(n):
        if n not in cache:
            cache[n] = Image.open(vale.OUT / n).convert("RGBA")
        return cache[n]

    def cell_screen(x, y):
        return ox + (x - y) * 32, oy + (x + y + 1) * 16

    for y in range(SIZE):
        for x in range(SIZE):
            sx, sy = cell_screen(x, y)
            img.alpha_composite(get(ground[(x, y)]), (sx - 32, sy - 16))
    order = sorted((c for c, p in placed.items() if p[0]), key=lambda c: (c[0] + c[1], c[0]))
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    gl = np.zeros((H, W), dtype=np.float32)
    for x, y in order:
        n = placed[(x, y)][0]
        sx, sy = cell_screen(x, y)
        px, py = hot.get(n, (0, 0))
        img.alpha_composite(get(n), (sx - px, sy - py))
        if n.startswith(("cv_cairn", "cv_lantern")):  # rough stand-in for the client's light glow
            r = 150 if "cairn" in n else 80
            cy = sy - (20 if "cairn" in n else 55)
            yy, xx = np.ogrid[0:H, 0:W]
            y0, y1, x0, x1 = max(cy - r, 0), min(cy + r, H), max(sx - r, 0), min(sx + r, W)
            dist = np.hypot(xx[:, x0:x1] - sx, yy[y0:y1] - cy)
            gl[y0:y1, x0:x1] += np.clip(1 - dist / r, 0, 1) ** 2 * (0.55 if "cairn" in n else 0.35)
    arr = np.asarray(img).astype(np.float32)
    arr[:, :, 0] += gl * 226
    arr[:, :, 1] += gl * 88
    arr[:, :, 2] += gl * 34
    img = Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8))
    del glow
    from PIL import ImageDraw

    d = ImageDraw.Draw(img)
    for e, x, y, *_ in SPAWNS:
        sx, sy = ox + (x - y) * 32, oy + (x + y) * 16
        col = {50001: (230, 60, 40), 50002: (255, 120, 40), 50010: (240, 200, 90), 50011: (200, 160, 120), 50012: (150, 150, 220)}[e]
        d.ellipse((sx - 6, sy - 6, sx + 6, sy + 6), outline=col, width=3)
    for n, x, y, r in MARKERS:
        sx, sy = ox + (x - y) * 32, oy + (x + y) * 16
        d.ellipse((sx - r * 45, sy - r * 22.6, sx + r * 45, sy + r * 22.6), outline=(120, 110, 140), width=2)
        d.text((sx + 8, sy - 8), n, fill=(200, 190, 210))
    out = CA / "preview"
    out.mkdir(parents=True, exist_ok=True)
    img.convert("RGB").save(out / f"{NAME}_full.png")
    img.resize((W // 2, H // 2), Image.LANCZOS).convert("RGB").save(out / f"{NAME}.png")
    # cover overlay (small)
    cov = Image.new("RGB", (SIZE * 8, SIZE * 8))
    cd = ImageDraw.Draw(cov)
    colors = {".": (90, 30, 30), "s": (150, 110, 60), "S": (40, 40, 90), "C": (255, 160, 40)}
    for y in range(SIZE):
        for x in range(SIZE):
            c = colors[cover[y, x]]
            if (x, y) in placed and placed[(x, y)][1] & FLAG_UNWALKABLE:
                c = tuple(v // 3 for v in c)
            cd.rectangle((x * 8, y * 8, x * 8 + 7, y * 8 + 7), fill=c)
    cov.save(out / f"{NAME}_cover.png")
    print(f"preview: {out / (NAME + '.png')}")


# --- main ----------------------------------------------------------------------------------------------


def main():
    vale.register_ramps()
    vale.OUT.mkdir(parents=True, exist_ok=True)
    for old in vale.OUT.glob("cv_*.png"):
        old.unlink()
    vale.HOTSPOTS.clear()
    vale.SPRITE_FX.clear()
    t0 = time.time()
    props = vale.make_props()
    print(f"props: {sum(len(v) for v in props.values())} sprites in {time.time() - t0:.0f}s")
    walk = build_walk()
    rock = ~walk
    top = cliff_heights(rock, walk)
    cliffs = cliff_sprites(rock, top)
    sgrid = shadow_grid(rock, top)
    ground = ground_tiles(rock, sgrid)
    rng = np.random.default_rng(17)
    placed, _ = place_props(rng, walk, rock, top, cliffs, props)
    cover = cover_grid(walk)
    write_map(ground, placed)
    write_cover(cover)
    write_markers()
    write_spawns(placed)
    check_paths(placed)
    (vale.OUT / "hotspots.txt").write_text("\n".join(vale.HOTSPOTS) + "\n", newline="\n")
    (vale.OUT / "sprite_fx.txt").write_text(
        "# particles <sprite> <system> <x> <y>       (data/particles.txt; offset from the sprite's top-left)\n"
        "# light <sprite> <rrggbbaa> <x> <y> <ground 0/1> <top 0/1> <scale>  (offset from the cell)\n"
        + "\n".join(vale.SPRITE_FX)
        + "\n",
        newline="\n",
    )
    (vale.OUT / "roofs.txt").write_text(
        "# roof <sprite prefix> <dx> <dy>: the sprite (placed on its back cell) roofs the cells up to +dx,+dy.
"
        "# Drawn above anything under it, faded while the player stands beneath.
"
        f"roof cv_laneroof 0 {3}
"
        "roof cv_canopy_ 3 3
",
        newline="
",
    )
    preview(ground, placed, cover)
    total = sum(p.stat().st_size for p in vale.OUT.glob("*.png"))
    print(f"vale art: {len(list(vale.OUT.glob('*.png')))} PNGs, {total / 1e6:.1f} MB; total {time.time() - t0:.0f}s")


if __name__ == "__main__":
    main()
