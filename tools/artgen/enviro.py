"""Environment art: ground tiles, walls and props in the same oldschool pre-rendered style.

Usage (from repo root):  python -I tools/artgen/enviro.py

Writes `custom_assets/content/custom/env/*.png` plus `custom_assets/env_manifest.json` (what exists,
used by mapgen.py). Pivots follow the engine's default for map sprites without a `sprite_hotspot`
row: horizontally centred, 16 px above the bottom (= centre of the cell's diamond).

Ground: each terrain type is a 4x4-cell periodic texture; tile (i, j) is cell (i, j) of it, so a map
that picks tile (x % 4, y % 4) is seamless and repeats only every 4 cells.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
from vox import BAYER4, OUTLINE, RAMP_TABLE, RAMP_IDS, Bone, Model, Prim  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets" / "content" / "custom" / "env"
PERIOD = 4  # cells

# --- periodic value noise ---------------------------------------------------------------


def periodic_noise(u, v, freq: int, seed: int, period: int = PERIOD):
    """Smooth value noise on a torus of `period` cells; `freq` lattice points per cell."""
    n = period * freq
    rng = np.random.default_rng(seed)
    lattice = rng.random((n, n))
    x, y = u * freq, v * freq
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    fx, fy = x - x0, y - y0
    sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = lattice[x0 % n, y0 % n]
    b = lattice[(x0 + 1) % n, y0 % n]
    c = lattice[x0 % n, (y0 + 1) % n]
    d = lattice[(x0 + 1) % n, (y0 + 1) % n]
    return (a * (1 - sx) + b * sx) * (1 - sy) + (c * (1 - sx) + d * sx) * sy


def fbm(u, v, seed, octaves=(2, 4, 8, 16), gains=(0.5, 0.25, 0.15, 0.1)):
    return sum(g * periodic_noise(u, v, f, seed + i) for i, (f, g) in enumerate(zip(octaves, gains))) / sum(gains)


def cells_noise(u, v, freq: int, seed: int, period: int = PERIOD):
    """Periodic Worley noise: (distance to nearest feature, distance to 2nd nearest)."""
    n = period * freq
    rng = np.random.default_rng(seed)
    pts = rng.random((n, n, 2))
    x, y = u * freq, v * freq
    xi, yi = np.floor(x).astype(int), np.floor(y).astype(int)
    d1 = np.full(x.shape, 9.0)
    d2 = np.full(x.shape, 9.0)
    for dx in (-1, 0, 1):
        for dy in (-1, 0, 1):
            cx, cy = xi + dx, yi + dy
            p = pts[cx % n, cy % n]
            d = np.hypot(cx + p[..., 0] - x, cy + p[..., 1] - y)
            d2 = np.where(d < d1, d1, np.minimum(d2, d))
            d1 = np.minimum(d1, d)
    return d1, d2


# --- ground tiles --------------------------------------------------------------------------

TILE_W, TILE_H = 64, 32


def diamond_coords():
    """Cell-space (u, v) in [0, 1) for each pixel of a 64x32 diamond, plus the inside mask."""
    py, px = np.indices((TILE_H, TILE_W)).astype(float)
    sx, sy = px - TILE_W / 2 + 0.5, py - TILE_H / 2 + 0.5
    u = sx / TILE_W + sy / TILE_H
    v = sy / TILE_H - sx / TILE_W
    inside = (np.abs(u) <= 0.5 + 1e-3) & (np.abs(v) <= 0.5 + 1e-3)
    return u + 0.5, v + 0.5, inside


def shade_to_rgb(ramp: np.ndarray, shade: np.ndarray, px, py):
    """Dither a 0..1 shade into a ramp (array of ramp ids per pixel)."""
    t = BAYER4[py.astype(int) % 4, px.astype(int) % 4]
    level = np.clip(shade * 4 + (t - 0.5) * 0.9, 0, 4).round().astype(int)
    return RAMP_TABLE[ramp, level]


GROUND_RAMPS = {
    "grass": ["#1a240f", "#2c3b17", "#40531f", "#5b6f2a", "#7d8f3c"],
    "dirt": ["#22170e", "#3a2817", "#543b22", "#6f5232", "#8e6d47"],
    "cobble": ["#1b1b1d", "#323234", "#4c4b4a", "#6a6764", "#8c8782"],
    "moss": ["#16200f", "#24341a", "#344a22", "#486131", "#5f7a40"],
}


def ramp_ids(name):
    return RAMP_IDS[name]


def register_ramps():
    """Adds ground ramps to vox's palette tables so `shade_to_rgb` can index them."""
    import vox

    for k, v in GROUND_RAMPS.items():
        if k not in vox.RAMPS:
            vox.RAMPS[k] = v
    vox.RAMP_RGB = {k: np.array([vox.hex_rgb(c) for c in v], dtype=np.uint8) for k, v in vox.RAMPS.items()}
    vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
    vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])
    globals()["RAMP_TABLE"] = vox.RAMP_TABLE
    globals()["RAMP_IDS"] = vox.RAMP_IDS


def ground_texture(kind: str, u, v):
    """(ramp id per pixel, shade 0..1) of a ground type at periodic world coords."""
    g = RAMP_IDS
    if kind == "grass":
        base = fbm(u, v, 11)
        clumps = periodic_noise(u, v, 6, 21)
        blades = periodic_noise(u * 1.0, v * 1.0, 24, 31)
        shade = 0.35 + 0.45 * base + 0.25 * (blades - 0.5) + 0.15 * (clumps - 0.5)
        ramp = np.where(clumps > 0.72, g["moss"], g["grass"])
        return ramp, shade
    if kind == "dirt":
        base = fbm(u, v, 41)
        d1, d2 = cells_noise(u, v, 5, 51)
        pebble = (d1 < 0.16) & (periodic_noise(u, v, 5, 61) > 0.55)
        shade = 0.3 + 0.45 * base - 0.12 * (d2 - d1 < 0.06)
        shade = np.where(pebble, 0.75 - d1 * 2.0, shade)
        ramp = np.where(pebble, g["cobble"], g["dirt"])
        return ramp, shade
    if kind == "cobble":
        d1, d2 = cells_noise(u, v, 3, 71)
        edge = d2 - d1
        stone = fbm(u, v, 81)
        shade = 0.35 + 0.35 * stone + 0.35 * np.clip(edge * 3, 0, 1) - 0.2 * (1 - np.clip(d1 * 2.5, 0, 1))
        mortar = edge < 0.07
        shade = np.where(mortar, 0.12 + 0.1 * stone, shade)
        ramp = np.where(mortar & (stone > 0.55), g["moss"], g["cobble"])
        return ramp, shade
    if kind == "water":
        # Slow swell + fine ripples; highlights where ripples crest.
        swell = fbm(u, v, 91, octaves=(1, 2, 4), gains=(0.5, 0.3, 0.2))
        ripple = periodic_noise(u * 1.0, v * 2.0, 6, 95)
        shade = 0.25 + 0.35 * swell + 0.25 * (ripple - 0.5)
        crest = ripple > 0.8
        shade = np.where(crest, 0.85, shade)
        return np.full(u.shape, g["water"]), shade
    raise ValueError(kind)


def blend_mask(kind_a, kind_b, u, v, seed):
    """Organic transition: noise threshold for mixing two ground types (used for path edges)."""
    return fbm(u, v, seed) > 0.5


HOTSPOTS: list[str] = []  # "name x y" lines -> env/hotspots.txt (engine pivot per sprite)


def make_ground(kind: str):
    du, dv, inside = diamond_coords()
    py, px = np.indices((TILE_H, TILE_W))
    names = []
    for i in range(PERIOD):
        for j in range(PERIOD):
            u, v = (du + i) % PERIOD, (dv + j) % PERIOD
            ramp, shade = ground_texture(kind, u, v)
            rgb = shade_to_rgb(ramp, np.clip(shade, 0, 1), px, py)
            img = np.zeros((TILE_H, TILE_W, 4), dtype=np.uint8)
            img[inside, :3] = rgb[inside]
            img[inside, 3] = 255
            name = f"cg_{kind}_{i}{j}.png"
            Image.fromarray(img).save(OUT / name)
            names.append(name)
            HOTSPOTS.append(f"{name} {TILE_W // 2} {TILE_H // 2}")
    return names


# --- upright pieces (voxel renders) ---------------------------------------------------------

UPRIGHT_W, UPRIGHT_H = 128, 192
UPRIGHT_FOOT = (UPRIGHT_W // 2, UPRIGHT_H - 16)


def jitter_shade(model: Model, seed: int, amount: float):
    """Per-voxel brightness noise baked into normals' length -> texture (bark, leaves, stone)."""
    rng = np.random.default_rng(seed)
    for name, (p, n, c) in model.parts.items():
        scale = 1.0 + (rng.random(len(p)) - 0.5) * amount
        model.parts[name] = (p, n * scale[:, None], c)


def render_static(model: Model, size: int = UPRIGHT_H) -> np.ndarray:
    """Square render with the model's origin (cell centre) at (size/2, size-16)."""
    world = model.pose({})
    return model.render(world, 0.0, size=size, foot=(size // 2, size - 16))


def trim_keep_pivot(img: np.ndarray) -> np.ndarray:
    """Trim transparent margins while keeping the pivot at (w/2, h-16)."""
    a = img[:, :, 3] > 0
    ys, xs = np.nonzero(a)
    cx = img.shape[1] // 2
    half = int(max(cx - xs.min(), xs.max() + 1 - cx)) + 1
    top = int(ys.min())
    return img[top:, cx - half : cx + half]


def tree(seed: int) -> Model:
    """Broadleaf: short thick trunk with a couple of limbs and a lumpy canopy."""
    rng = np.random.default_rng(seed)
    h = 1.0 + rng.random() * 0.35
    lean = (0.06 * rng.standard_normal(), 0.06 * rng.standard_normal())
    top = (lean[0], lean[1], h)
    prims = [Prim("capsule", ((0, 0, 0), top, 0.14), "wood")]
    for k in range(3):  # roots
        a = k * 2.1 + rng.random()
        prims.append(Prim("capsule", ((0, 0, 0.15), (0.32 * math.cos(a), 0.32 * math.sin(a), 0.0), 0.07), "wood"))
    for k in range(2):  # limbs
        a = rng.random() * 2 * math.pi
        prims.append(Prim("capsule", (top, (lean[0] + 0.45 * math.cos(a), lean[1] + 0.45 * math.sin(a), h + 0.45), 0.06), "wood"))
    for k in range(12 + int(rng.random() * 5)):
        a, r = rng.random() * 2 * math.pi, rng.random() ** 0.6 * 0.6
        c = (lean[0] + r * math.cos(a), lean[1] + r * math.sin(a), h + 0.35 + rng.random() * 0.7 - r * 0.3)
        s = 0.3 + rng.random() * 0.2
        prims.append(Prim("ellipsoid", (c, (s, s, s * 0.85)), "leaf"))
    m = Model([Bone("tree", None, (0, 0, 0), prims)], voxel=0.03)
    jitter_shade(m, seed, 0.9)
    return m


def pine(seed: int) -> Model:
    rng = np.random.default_rng(seed)
    h = 2.2 + rng.random() * 0.8
    prims = [Prim("capsule", ((0, 0, 0), (0, 0, h), 0.08), "wood")]
    tiers = 5
    for k in range(tiers):
        z = 0.5 + k * (h - 0.4) / tiers
        r = 0.75 * (1 - k / (tiers + 0.5)) + 0.1
        prims.append(Prim("ellipsoid", ((0, 0, z + 0.15), (r, r, 0.28)), "pine"))
    m = Model([Bone("pine", None, (0, 0, 0), prims)], voxel=0.03)
    jitter_shade(m, seed, 0.8)
    return m


def rock(seed: int) -> Model:
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(3 + int(rng.random() * 3)):
        c = (rng.normal(0, 0.15), rng.normal(0, 0.15), 0.1 + rng.random() * 0.15)
        s = 0.2 + rng.random() * 0.25
        prims.append(Prim("ellipsoid", (c, (s * 1.2, s, s * 0.8)), "stone"))
    m = Model([Bone("rock", None, (0, 0, 0), prims)], voxel=0.022)
    jitter_shade(m, seed, 0.6)
    return m


def bush(seed: int) -> Model:
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(5 + int(rng.random() * 3)):
        c = (rng.normal(0, 0.18), rng.normal(0, 0.18), 0.2 + rng.random() * 0.2)
        s = 0.16 + rng.random() * 0.14
        prims.append(Prim("ellipsoid", (c, (s, s, s * 0.85)), "leaf"))
    m = Model([Bone("bush", None, (0, 0, 0), prims)], voxel=0.022)
    jitter_shade(m, seed, 0.9)
    return m


def wall_block(seed: int) -> Model:
    """Full-cell stone block, 1.4 units tall, brick courses baked in via per-voxel shade."""
    m = Model([Bone("wall", None, (0, 0, 0), [Prim("box", ((0, 0, 0.7), (0.5, 0.5, 0.7)), "stone")])], voxel=0.02)
    rng = np.random.default_rng(seed)
    for name, (p, n, c) in m.parts.items():
        z = p[:, 2]
        course = np.floor(z / 0.2)
        along = np.where(np.abs(n[:, 0]) > np.abs(n[:, 1]), p[:, 1], p[:, 0])
        offset = (course % 2) * 0.17
        brick_x = np.floor((along + offset) / 0.34)
        mortar = ((z % 0.2) < 0.03) | (((along + offset) % 0.34) < 0.035)
        brick_tone = 0.85 + 0.3 * ((np.sin(brick_x * 12.9898 + course * 78.233) * 43758.5453) % 1.0)
        tone = np.where(mortar, 0.45, brick_tone) * (1 + (rng.random(len(p)) - 0.5) * 0.25)
        top = n[:, 2] > 0.7
        tone = np.where(top, 0.55 + (rng.random(len(p)) - 0.5) * 0.3, tone)
        m.parts[name] = (p, n * tone[:, None], c)
    return m


def hut(seed: int) -> Model:
    """Log hut on a 3x3-cell footprint (2.4 x 2.4 units) with a door and a stepped thatch roof."""
    rng = np.random.default_rng(seed)
    P = Prim
    half, wall_h = 1.15, 1.25
    prims = [P("box", ((0, 0, wall_h / 2), (half, half, wall_h / 2)), "wood")]
    # Stepped gable roof along x: ever narrower slabs in y, overhanging the walls.
    steps = 7
    for k in range(steps):
        w = (half + 0.25) * (1 - k / steps)
        prims.append(P("box", ((0, 0, wall_h + 0.1 + k * 0.13), (half + 0.3, w, 0.075)), "thatch"))
    prims.append(P("box", ((half + 0.01, 0.0, 0.5), (0.02, 0.28, 0.5)), "boot"))  # door (faces +x)
    prims.append(P("box", ((0.0, -half - 0.01, 0.75), (0.25, 0.02, 0.18)), "hair"))  # window (faces -y)
    prims.append(P("box", ((0.5, half - 0.2, wall_h + 0.6), (0.12, 0.12, 0.45)), "stone"))  # chimney
    m = Model([Bone("hut", None, (0, 0, 0), prims)], voxel=0.03)
    # Log courses on the walls, straw streaks on the roof.
    for name, (p, n, c) in m.parts.items():
        z = p[:, 2]
        is_wall = (c == RAMP_IDS["wood"]) & (np.abs(n[:, 2]) < 0.5)
        log = 0.75 + 0.35 * np.abs(np.sin(z / 0.16 * math.pi))
        straw = 0.85 + 0.3 * ((np.sin(p[:, 0] * 41.0 + p[:, 1] * 7.0) * 43758.5) % 1.0)
        tone = np.where(is_wall, log, np.where(c == RAMP_IDS["thatch"], straw, 1.0))
        tone = tone * (1 + (rng.random(len(p)) - 0.5) * 0.2)
        m.parts[name] = (p, n * tone[:, None], c)
    return m


def campfire(seed: int) -> Model:
    """Ring of stones around crossed logs; the flames are the `campfire` particles."""
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(9):
        a = k * 2 * math.pi / 9 + rng.random() * 0.2
        prims.append(Prim("ellipsoid", ((0.3 * math.cos(a), 0.3 * math.sin(a), 0.05), (0.08, 0.07, 0.06)), "stone"))
    for a in (0.3, 1.9, 3.5):
        d = (0.22 * math.cos(a), 0.22 * math.sin(a))
        prims.append(Prim("capsule", ((-d[0], -d[1], 0.06), (d[0], d[1], 0.16), 0.045), "wood"))
    prims.append(Prim("ellipsoid", ((0, 0, 0.03), (0.15, 0.15, 0.03)), "ember"))  # embers
    m = Model([Bone("fire", None, (0, 0, 0), prims)], voxel=0.016)
    jitter_shade(m, seed, 0.5)
    return m


def crate(seed: int) -> Model:
    s = 0.22 + 0.04 * np.random.default_rng(seed).random()
    prims = [Prim("box", ((0, 0, s), (s, s, s)), "wood")]
    for z in (0.04, 2 * s - 0.04):  # banding
        prims.append(Prim("box", ((0, 0, z), (s + 0.01, s + 0.01, 0.025)), "boot"))
    m = Model([Bone("crate", None, (0, 0, 0), prims)], voxel=0.016)
    jitter_shade(m, seed, 0.4)
    return m


def barrel(seed: int) -> Model:
    prims = [Prim("ellipsoid", ((0, 0, 0.3), (0.21, 0.21, 0.36)), "wood")]
    for z in (0.1, 0.46):
        prims.append(Prim("ellipsoid", ((0, 0, z), (0.215, 0.215, 0.025)), "metal"))
    m = Model([Bone("barrel", None, (0, 0, 0), prims)], voxel=0.016)
    jitter_shade(m, seed, 0.4)
    return m


def lamp(seed: int) -> Model:
    prims = [
        Prim("capsule", ((0, 0, 0), (0, 0, 1.7), 0.04), "metal"),
        Prim("capsule", ((0, 0, 1.65), (0.25, 0, 1.65), 0.025), "metal"),
        Prim("box", ((0.25, 0, 1.48), (0.07, 0.07, 0.1)), "gold"),  # lantern
        Prim("box", ((0, 0, 0.03), (0.12, 0.12, 0.03)), "stone"),
    ]
    return Model([Bone("lamp", None, (0, 0, 0), prims)], voxel=0.016)


SPRITE_FX: list[str] = []  # lines for env/sprite_fx.txt, see main()


def make_upright(name: str, model: Model, size: int = UPRIGHT_H) -> str:
    img = trim_keep_pivot(render_static(model, size))
    fname = f"cg_{name}.png"
    Image.fromarray(img).save(OUT / fname)
    HOTSPOTS.append(f"{fname} {img.shape[1] // 2} {img.shape[0] - 16}")
    return fname


def main():
    import vox

    vox.RAMPS.update({
        "leaf": ["#13200d", "#203716", "#30501f", "#47692b", "#64843a"],
        "pine": ["#0f1a12", "#18291b", "#223b26", "#2f4f33", "#416642"],
        "stone": ["#1c1c1f", "#34343a", "#4f4f55", "#6e6d70", "#918e8c"],
        "water": ["#0b1820", "#122a36", "#1b3f4f", "#2a5a69", "#5f8f96"],
        "thatch": ["#2a2110", "#46371a", "#665026", "#8a6d35", "#ad8c4a"],
        "ember": ["#3a0c04", "#7a1f06", "#b8420c", "#e8741a", "#ffb347"],
    })
    register_ramps()
    OUT.mkdir(parents=True, exist_ok=True)
    HOTSPOTS.clear()
    SPRITE_FX.clear()
    manifest = {"ground": {}, "upright": {}}
    for kind in ("grass", "dirt", "cobble", "water"):
        manifest["ground"][kind] = make_ground(kind)
        print(f"ground {kind}: {PERIOD * PERIOD} tiles")
    for kind, fn, n in (
        ("tree", tree, 4),
        ("pine", pine, 3),
        ("rock", rock, 4),
        ("bush", bush, 3),
        ("wall", wall_block, 1),
        ("crate", crate, 2),
        ("barrel", barrel, 2),
    ):
        manifest["upright"][kind] = [make_upright(f"{kind}_{i}", fn(100 + i * 7 + len(kind))) for i in range(n)]
        print(f"upright {kind}: {n}")
    manifest["upright"]["hut"] = [make_upright("hut_0", hut(5), size=384)]
    fire = make_upright("campfire_0", campfire(3))
    lamp_name = make_upright("lamp_0", lamp(1))
    manifest["upright"]["campfire"] = [fire]
    manifest["upright"]["lamp"] = [lamp_name]
    # Effects (sprite_fx.txt, docs/visuals.md): particles offset from the sprite's top-left; light
    # offset from the cell's render position.
    fw, fh = Image.open(OUT / fire).size
    SPRITE_FX.append(f"particles {fire} campfire {fw // 2} {fh - 16 - 6}")
    SPRITE_FX.append(f"particles {fire} cairn_sparks {fw // 2} {fh - 16 - 10}")
    SPRITE_FX.append(f"light {fire} e25822c8 0 16 1 0 1.0")
    lw, lh = Image.open(OUT / lamp_name).size
    SPRITE_FX.append(f"particles {lamp_name} lantern_embers {lw // 2 + 11} {lh - 16 - 66}")
    SPRITE_FX.append(f"light {lamp_name} e25822c8 11 -60 1 0 0.8")
    # Ember-moths over the glade pond: mapgen.py places invisible `green_firefly.psi` sprites.
    SPRITE_FX.append("particles green_firefly.psi fireflies 0 -32")
    SPRITE_FX.append("light green_firefly.psi c8822e90 -5 20 1 0 0.5")
    print("upright hut, campfire, lamp")
    (ROOT / "custom_assets" / "env_manifest.json").write_text(json.dumps(manifest, indent=1), newline="\n")
    (OUT / "hotspots.txt").write_text("\n".join(HOTSPOTS) + "\n", newline="\n")
    (OUT / "sprite_fx.txt").write_text(
        "# particles <sprite> <system> <x> <y>       (data/particles.txt; offset from the sprite's top-left)\n"
        "# light <sprite> <rrggbbaa> <x> <y> <ground 0/1> <top 0/1> <scale>  (offset from the cell)\n"
        + "\n".join(SPRITE_FX)
        + "\n",
        newline="\n",
    )


if __name__ == "__main__":
    main()
