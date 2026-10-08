"""Duskhollow vale environment art: the crimson palette set used by `valemap.py`.

Not run on its own; `python -I tools/artgen/valemap.py` builds the map and calls into this module.

Look (docs/world.md, art rules): key light is crimson from (almost straight) above, shadows are
bruised violet, albedo stays dark, only fire glows. Every ramp below runs violet-black ->
material -> red-lit, so a top face reads as "lit by the Eye" and a wall face as "in shade".

Pieces:
- ground: 4x4-cell periodic textures (rust soil, ochre dust, dry grass, crimson grain beds,
  packed dirt lanes, charcoal scree, dark gorge floor, flagstones, still water) in three
  shadow levels (open sky, shade, deep shelter); `valemap.py` bakes per-cell blends where
  terrain kinds or shadow levels meet.
- cliffs: one sprite per rock cell column (unique where a face shows, else a periodic variant),
  carved from a domain-warped occupancy field so neighbouring columns join seamlessly; a column
  of the overhang wall also carries its share of the rock lip hanging over Lowshade.
- props (`make_props`): covered huts with deep eaves, roofed-lane segments, canopy shelters,
  grain clumps, lightworker poles, the rest cairn (stone cairn + brazier), lantern posts,
  the ruined Glare Gate (towers, walls, rubble), the Fallen Blade, dead trees, bones, boulders,
  terrace walls, crates/sacks/barrels, a broken cart, dead reeds.

Renders use the game's projection (`vox.project`); a sprite's pivot is wherever the anchor
cell's centre lands in the image (written to `hotspots.txt`), and its pixels are dithered in
screen-aligned 4x4 Bayer cells so neighbouring sprites and tiles share one dither grid.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import vox  # noqa: E402
from enviro import PERIOD, TILE_H, TILE_W, cells_noise, diamond_coords, fbm, periodic_noise  # noqa: E402
from vox import BAYER4, Bone, Model, Prim  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets" / "content" / "custom" / "vale"

# --- palette -------------------------------------------------------------------------------
# dark (violet shadow) -> light (crimson key); nothing near white, fire excepted.
VALE_RAMPS = {
    # materials
    "v_stone": ["#100b14", "#211826", "#372a32", "#543436", "#704340"],
    "v_ashlar": ["#0f0a10", "#1f161c", "#33242a", "#4c302f", "#663e38"],
    "v_scrub": ["#120b10", "#221616", "#382618", "#52361c", "#6a4622"],
    "v_wood": ["#130a10", "#241419", "#3c2119", "#5a311f", "#784329"],
    "v_dwood": ["#110b10", "#211619", "#372622", "#513427", "#69432f"],
    "v_thatch": ["#120b10", "#22151a", "#38221e", "#523022", "#6c3f27"],
    "v_turf": ["#110a10", "#21141a", "#37201e", "#4f2c22", "#683a28"],
    "v_tarp": ["#17080e", "#2f1019", "#4f1a1e", "#74261f", "#943523"],
    "v_tarp2": ["#140e16", "#272026", "#40332f", "#5a473b", "#735a46"],
    "v_metal": ["#0b0a0f", "#18151d", "#2a2429", "#433536", "#5e4645"],
    "v_rust": ["#120809", "#230d0f", "#3a1513", "#521f17", "#6a2a1a"],
    "v_bronze": ["#0f0b0b", "#1e1511", "#322416", "#48331a", "#5e421e"],
    "v_iron": ["#0d0b0e", "#1b1719", "#2b2425", "#3f3232", "#544140"],
    "v_bone": ["#191117", "#2f2328", "#52423b", "#74604b", "#927a5c"],
    "v_cloth": ["#130d16", "#231925", "#3a2531", "#523039", "#6c3c40"],
    "v_rag": ["#16070c", "#2c0d14", "#481418", "#661d1b", "#82291f"],
    "v_grain": ["#1b060c", "#350a14", "#591218", "#801d1b", "#a42c21"],
    "v_straw": ["#150e10", "#271a17", "#3e2a1c", "#583a21", "#724a27"],
    "v_dark": ["#07050a", "#0c090f", "#120d14", "#181118", "#1e151b"],
    "v_ember": ["#3a0c04", "#7a1f06", "#b8420c", "#e8741a", "#ffb347"],
    # ground
    "g_rust": ["#130a0e", "#25121a", "#3c1c1c", "#572a21", "#743a27"],
    "g_ochre": ["#190f12", "#2d1e1c", "#493022", "#664527", "#835b31"],
    "g_grass": ["#130c10", "#231818", "#392818", "#53391d", "#6c4b25"],
    "g_lane": ["#150e12", "#271a1c", "#3d2a26", "#55392f", "#6d493a"],
    "g_gorge": ["#0b070d", "#171016", "#25191f", "#362326", "#4a2f2c"],
    "g_flag": ["#110d14", "#231b22", "#392b30", "#523739", "#6a4643"],
    "g_water": ["#050409", "#0a0910", "#110e16", "#1b121a", "#28161c"],
    "g_eye": ["#100509", "#20070d", "#360b12", "#511217", "#6c1b1c"],
}
EMISSIVE = ("v_ember",)
OUTLINE_RGBA = (9, 5, 11, 255)


def register_ramps():
    for k, v in VALE_RAMPS.items():
        vox.RAMPS[k] = v
    vox.RAMP_RGB = {k: np.array([vox.hex_rgb(c) for c in v], dtype=np.uint8) for k, v in vox.RAMPS.items()}
    vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
    vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])


def rid(name: str) -> int:
    return vox.RAMP_IDS[name]


HOTSPOTS: list[str] = []
SPRITE_FX: list[str] = []

# --- light ---------------------------------------------------------------------------------

L_KEY = np.array([-0.25, -0.15, 1.0])
L_KEY = L_KEY / np.linalg.norm(L_KEY)  # the Eye: crimson, from above, a little from the north
L_FILL = np.array([1.0, -0.6, 0.35])
L_FILL = L_FILL / np.linalg.norm(L_FILL)  # weak bounce so wall faces keep some modelling


def lighting(n: np.ndarray, tone: np.ndarray) -> np.ndarray:
    return tone * (0.2 + 0.7 * np.clip(n @ L_KEY, 0, 1) + 0.16 * np.clip(n @ L_FILL, 0, 1))


# --- renderer --------------------------------------------------------------------------------


def render(P, N, C, name, outline=True, splat=2, seed=0, min_h=18, shade_mul=None):
    """Splats points (world coords relative to the anchor cell centre) into a sprite whose pivot
    (the anchor) sits on a multiple of 4 px. Normal length = tone. Saves `OUT/name`."""
    tone = np.linalg.norm(N, axis=1)
    n = N / np.maximum(tone, 1e-6)[:, None]
    shade = lighting(n, tone)
    if shade_mul is not None:
        shade = shade * shade_mul
    em = np.isin(C, [rid(e) for e in EMISSIVE])
    if em.any():
        rng = np.random.default_rng(seed)
        shade[em] = 0.45 + 0.55 * rng.random(int(em.sum())) * np.clip(n[em] @ np.array([0, 0, 1.0]) * 0.5 + 0.7, 0, 1)
    sx, sy, depth = vox.project(np.asarray(P, float))
    fx = int(math.ceil((-sx.min() + 3) / 4) * 4)
    fy = int(math.ceil((-sy.min() + 3) / 4) * 4)
    W = fx + int(math.ceil(sx.max())) + 3 + splat
    H = max(fy + int(math.ceil(sy.max())) + 3 + splat, fy + min_h)
    x = np.round(sx + fx).astype(int)
    y = np.round(sy + fy).astype(int)
    zbuf = np.full((H, W), -1e9)
    sh = np.zeros((H, W))
    rp = np.full((H, W), -1, dtype=int)
    order = np.argsort(depth)
    for dx in range(splat):
        for dy in range(splat):
            xx, yy = x[order] + dx, y[order] + dy
            ok = (xx >= 0) & (xx < W) & (yy >= 0) & (yy < H)
            xx, yy, dd, ll, cc = xx[ok], yy[ok], depth[order][ok], shade[order][ok], C[order][ok]
            closer = dd > zbuf[yy, xx]
            zbuf[yy[closer], xx[closer]] = dd[closer]
            sh[yy[closer], xx[closer]] = ll[closer]
            rp[yy[closer], xx[closer]] = cc[closer]
    mask = rp >= 0
    ty, tx = np.indices((H, W))
    thr = BAYER4[ty % 4, tx % 4]
    level = np.clip(sh * 4.0 + (thr - 0.5) * 0.9, 0, 4).round().astype(int)
    rgb = vox.RAMP_TABLE[np.clip(rp, 0, None), level]
    img = np.zeros((H, W, 4), dtype=np.uint8)
    img[mask, :3] = rgb[mask]
    img[mask, 3] = 255
    if outline:
        grown = mask.copy()
        grown[1:, :] |= mask[:-1, :]
        grown[:-1, :] |= mask[1:, :]
        grown[:, 1:] |= mask[:, :-1]
        grown[:, :-1] |= mask[:, 1:]
        img[grown & ~mask] = OUTLINE_RGBA
    # crop transparent margins (keep the pivot multiple-of-4 aligned)
    ys, xs = np.nonzero(img[:, :, 3])
    if len(xs):
        x0, y0 = (xs.min() // 4) * 4, (ys.min() // 4) * 4
        x1, y1 = xs.max() + 1, ys.max() + 1
        x0, y0 = min(x0, fx), min(y0, fy)
        x1, y1 = max(x1, fx + 1), max(y1, fy + 1)
        img = img[y0:y1, x0:x1]
        fx, fy = fx - x0, fy - y0
    OUT.mkdir(parents=True, exist_ok=True)
    Image.fromarray(img).save(OUT / name, optimize=True)
    HOTSPOTS.append(f"{name} {fx} {fy}")
    return name, (fx, fy), img


def screen_offset(p) -> tuple[int, int]:
    """Pixel offset of a world point (relative to the anchor cell centre) from the pivot."""
    sx, sy, _ = vox.project(np.asarray([p], float))
    return int(round(sx[0])), int(round(sy[0]))


def prims_points(prims, voxel=0.022, seed=0, jitter=0.0):
    m = Model([Bone("m", None, (0, 0, 0), prims)], voxel=voxel)
    P, N, C = (np.concatenate(a) for a in zip(*m.parts.values()))
    N = N.astype(float)
    if jitter:
        rng = np.random.default_rng(seed)
        N = N * (1 + (rng.random(len(P)) - 0.5) * jitter)[:, None]
    return P, N, C


def merge(*parts):
    return tuple(np.concatenate(a) for a in zip(*parts))


def hash01(*vals):
    s = 0.0
    for i, v in enumerate(vals):
        s = s + np.asarray(v, float) * (12.9898 + 31.7 * i)
    return (np.sin(s) * 43758.5453) % 1.0


# --- periodic 3D value noise (period PERIOD cells in x and y) ----------------------------------

_LAT: dict = {}


def vnoise3(x, y, z, freq: int, seed: int):
    n, m = PERIOD * freq, 97
    key = (freq, seed)
    if key not in _LAT:
        _LAT[key] = np.random.default_rng(seed).random((n, n, m)).astype(np.float32)
    lat = _LAT[key]
    xs, ys, zs = x * freq, y * freq, z * freq
    x0, y0, z0 = np.floor(xs).astype(int), np.floor(ys).astype(int), np.floor(zs).astype(int)
    fx, fy, fz = xs - x0, ys - y0, zs - z0
    fx, fy, fz = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy), fz * fz * (3 - 2 * fz)
    xa, xb = x0 % n, (x0 + 1) % n
    ya, yb = y0 % n, (y0 + 1) % n
    za, zb = z0 % m, (z0 + 1) % m
    c00 = lat[xa, ya, za] * (1 - fx) + lat[xb, ya, za] * fx
    c10 = lat[xa, yb, za] * (1 - fx) + lat[xb, yb, za] * fx
    c01 = lat[xa, ya, zb] * (1 - fx) + lat[xb, ya, zb] * fx
    c11 = lat[xa, yb, zb] * (1 - fx) + lat[xb, yb, zb] * fx
    return (c00 * (1 - fy) + c10 * fy) * (1 - fz) + (c01 * (1 - fy) + c11 * fy) * fz


# --- occupancy -> surface points ---------------------------------------------------------------


def _blur(f, passes=2):
    for _ in range(passes):
        for ax in range(3):
            f = (np.roll(f, 1, ax) + f + np.roll(f, -1, ax)) / 3.0
    return f


def occ_surface(occ, own, origin, voxel):
    """Surface voxels of `occ` that are also in `own`: positions and normals (from the blurred
    occupancy gradient), plus their grid indices."""
    inner = occ.copy()
    for ax in range(3):
        for s in (1, -1):
            inner &= np.roll(occ, s, ax)
    surf = own & occ & ~inner
    idx = np.nonzero(surf)
    f = _blur(occ.astype(np.float32))
    g = np.stack([(np.roll(f, -1, ax) - np.roll(f, 1, ax))[idx] for ax in range(3)], 1)
    n = -g
    ln = np.linalg.norm(n, axis=1)
    n = np.where(ln[:, None] > 1e-5, n / np.maximum(ln, 1e-6)[:, None], np.array([0, 0, 1.0]))
    P = np.asarray(origin, float) + np.stack(idx, 1) * voxel
    return P, n, idx


# --- ground ---------------------------------------------------------------------------------------

GROUND_KINDS = ["rust", "ochre", "grass", "field", "lane", "scree", "gorge", "flag", "water"]
SHADOW_MUL = (1.0, 0.66, 0.4)  # open sky, shade, deep shelter


def ground_texture(kind: str, u, v):
    """(ramp id, shade 0..1) per pixel at periodic world coords (u, v in [0, PERIOD))."""
    if kind == "rust":
        base = fbm(u, v, 301)
        d1, d2 = cells_noise(u, v, 3, 311)
        crack = (d2 - d1) < 0.045
        p1, _ = cells_noise(u, v, 6, 317)
        pebble = (p1 < 0.13) & (periodic_noise(u, v, 4, 319) > 0.6)
        shade = 0.34 + 0.42 * base + 0.1 * (periodic_noise(u, v, 16, 313) - 0.5)
        shade = np.where(crack, shade * 0.5, shade)
        shade = np.where(pebble, 0.75 - p1 * 2.5, shade)
        ramp = np.where(pebble, rid("v_stone"), rid("g_rust"))
        return ramp, shade
    if kind == "ochre":
        base = fbm(u, v, 321)
        fine = periodic_noise(u, v, 20, 325)
        patch = fbm(u, v, 327) < 0.4
        shade = 0.4 + 0.34 * base + 0.18 * (fine - 0.5)
        ramp = np.where(patch, rid("g_rust"), rid("g_ochre"))
        return ramp, shade
    if kind == "grass":
        base = fbm(u, v, 331)
        clumps = periodic_noise(u, v, 6, 333)
        blades = periodic_noise(u, v, 26, 335)
        streak = periodic_noise(u * 1.0, v * 1.0, 13, 337)
        shade = 0.3 + 0.42 * base + 0.3 * (blades - 0.5) + 0.12 * (streak - 0.5)
        ramp = np.where(clumps < 0.3, rid("g_rust"), rid("g_grass"))
        return ramp, shade
    if kind == "field":
        # crimson grain beds in rows along x, dark furrows between them
        row = (v * 2.0 + 0.15 * (periodic_noise(u, v, 4, 341) - 0.5)) % 1.0
        furrow = np.abs(row - 0.5) > 0.36
        stalk = periodic_noise(u, v, 28, 343)
        heads = periodic_noise(u, v, 40, 345) > 0.72
        base = fbm(u, v, 347)
        shade = 0.2 + 0.32 * stalk + 0.16 * base
        shade = np.where(heads, shade + 0.14, shade)
        shade = np.where(furrow, 0.14 + 0.16 * base, shade)
        ramp = np.where(furrow, rid("g_rust"), rid("v_grain"))
        return ramp, shade
    if kind == "lane":
        base = fbm(u, v, 351)
        fine = periodic_noise(u, v, 18, 353)
        p1, _ = cells_noise(u, v, 7, 355)
        pebble = (p1 < 0.1) & (periodic_noise(u, v, 5, 357) > 0.55)
        shade = 0.38 + 0.3 * base + 0.14 * (fine - 0.5)
        shade = np.where(pebble, 0.62 - p1 * 2.5, shade)
        ramp = np.where(pebble, rid("v_stone"), rid("g_lane"))
        return ramp, shade
    if kind == "scree":
        d1, d2 = cells_noise(u, v, 4, 361)
        stone = fbm(u, v, 363)
        gap = (d2 - d1) < 0.08
        shade = 0.3 + 0.35 * stone + 0.3 * np.clip(0.5 - d1, 0, 1)
        shade = np.where(gap, 0.1 + 0.1 * stone, shade)
        ramp = np.where(gap, rid("g_gorge"), rid("v_stone"))
        return ramp, shade
    if kind == "gorge":
        base = fbm(u, v, 371)
        p1, _ = cells_noise(u, v, 5, 373)
        pebble = (p1 < 0.11) & (periodic_noise(u, v, 4, 375) > 0.5)
        shade = 0.3 + 0.38 * base + 0.1 * (periodic_noise(u, v, 16, 377) - 0.5)
        shade = np.where(pebble, 0.6 - p1 * 2.0, shade)
        ramp = np.where(pebble, rid("v_stone"), rid("g_gorge"))
        return ramp, shade
    if kind == "flag":
        d1, d2 = cells_noise(u, v, 2, 381)
        edge = d2 - d1
        stone = fbm(u, v, 383)
        crack = np.abs(periodic_noise(u, v, 8, 385) - 0.5) < 0.025
        shade = 0.36 + 0.3 * stone + 0.22 * np.clip(edge * 3, 0, 1)
        mortar = edge < 0.06
        shade = np.where(mortar, 0.1 + 0.12 * stone, np.where(crack, shade * 0.55, shade))
        ramp = np.where(mortar & (stone > 0.5), rid("g_ochre"), rid("g_flag"))
        return ramp, shade
    if kind == "water":
        swell = fbm(u, v, 391, octaves=(1, 2, 4), gains=(0.5, 0.3, 0.2))
        ripple = periodic_noise(u, v * 2.0, 5, 393)
        shade = 0.3 + 0.3 * swell + 0.12 * (ripple - 0.5)
        return np.full(u.shape, rid("g_water")), shade
    raise ValueError(kind)


def dither_rgb(ramp, shade, px, py):
    t = BAYER4[py.astype(int) % 4, px.astype(int) % 4]
    level = np.clip(shade * 4 + (t - 0.5) * 0.9, 0, 4).round().astype(int)
    return vox.RAMP_TABLE[ramp, level]


_TILE_CACHE: dict = {}


def periodic_tile(kind: str, i: int, j: int, lvl: int) -> str:
    """Tile (i, j) of the kind's 4x4 periodic texture at shadow level `lvl` (made on demand)."""
    key = (kind, i, j, lvl)
    if key in _TILE_CACHE:
        return _TILE_CACHE[key]
    du, dv, inside = diamond_coords()
    py, px = np.indices((TILE_H, TILE_W))
    u, v = (du + i) % PERIOD, (dv + j) % PERIOD
    ramp, shade = ground_texture(kind, u, v)
    rgb = dither_rgb(ramp, np.clip(shade * SHADOW_MUL[lvl], 0, 1), px, py)
    img = np.zeros((TILE_H, TILE_W, 4), dtype=np.uint8)
    img[inside, :3] = rgb[inside]
    img[inside, 3] = 255
    name = f"cv_{kind}_{i}{j}_{lvl}.png"
    OUT.mkdir(parents=True, exist_ok=True)
    Image.fromarray(img).save(OUT / name, optimize=True)
    HOTSPOTS.append(f"{name} {TILE_W // 2} {TILE_H // 2}")
    _TILE_CACHE[key] = name
    return name


# --- cliffs ------------------------------------------------------------------------------------

CLIFF_VOXEL = 0.034


class Cliffs:
    """Rock mass of the map: `rock[y, x]` cells with top heights `top[y, x]`, plus overhang lips
    per row (`lip_x[y]` = wall column x, `lip_reach[y]` cells beyond it, `lip_bot[y]` underside)."""

    def __init__(self, rock, top, lip_x, lip_reach, lip_bot):
        self.rock, self.top = rock, top
        self.lip_x, self.lip_reach, self.lip_bot = lip_x, lip_reach, lip_bot
        self.size = rock.shape[0]

    def field(self, X, Y, Z):
        """Occupancy and owning cell of world points."""
        S = self.size
        zq = Z * 0.6
        wx = X + 0.62 * (vnoise3(X, Y, zq, 2, 11) - 0.5) + 0.22 * (vnoise3(X, Y, Z * 1.5, 5, 13) - 0.5)
        wy = Y + 0.62 * (vnoise3(X, Y, zq, 2, 12) - 0.5) + 0.22 * (vnoise3(X, Y, Z * 1.5, 5, 14) - 0.5)
        ix = np.clip(np.floor(wx).astype(int), 0, S - 1)
        iy = np.clip(np.floor(wy).astype(int), 0, S - 1)
        tn = vnoise3(wx, wy, np.zeros_like(wx), 3, 15) - 0.5
        top = self.top[iy, ix] + 0.55 * tn
        in_col = self.rock[iy, ix] & (Z < top) & (Z >= -0.05)
        owner_x, owner_y = ix.copy(), iy.copy()
        reach = self.lip_reach[iy]
        has_lip = reach > 0
        occ = in_col
        if has_lip.any():
            xw = self.lip_x[iy]
            ln = vnoise3(wx, wy, np.zeros_like(wx), 2, 16) - 0.5
            drip = 0.55 * np.clip((vnoise3(wx, wy, np.zeros_like(wx), 6, 17) - 0.6) * 3.0, 0, 1)
            wall_top = self.top[iy, np.clip(xw, 0, S - 1)] + 0.55 * tn
            bot = self.lip_bot[iy] + 0.5 * (vnoise3(wx, wy, Z * 0.3, 3, 18) - 0.5) - drip
            in_lip = (
                has_lip
                & (wx >= xw)
                & (wx < xw + 1 + reach + 1.2 * ln - 0.6 * np.clip((Z - bot) / 2.0, 0, 1))
                & (Z < wall_top)
                & (Z > bot)
            )
            in_lip &= ~in_col
            owner_x = np.where(in_lip, xw, owner_x)
            occ = in_col | in_lip
        return occ, owner_x, owner_y

    def render_cell(self, cx: int, cy: int, name: str):
        v = CLIFF_VOXEL
        reach = int(self.lip_reach[cy]) if self.lip_x[cy] == cx else 0
        zmax = float(self.top[cy, cx]) + 0.6
        if reach:
            zmax = max(zmax, float(self.top[cy, cx]) + 0.6)
        x0, x1 = cx - 0.7, cx + 1.7 + (reach + 1.0 if reach else 0)
        y0, y1 = cy - 0.7, cy + 1.7
        xs = np.arange(x0, x1, v)
        ys = np.arange(y0, y1, v)
        zs = np.arange(-0.1, zmax, v)
        X, Y, Z = np.meshgrid(xs, ys, zs, indexing="ij")
        occ, ox, oy = self.field(X, Y, Z)
        own = occ & (ox == cx) & (oy == cy)
        if not own.any():
            return None
        P, n, idx = occ_surface(occ, own, (x0, y0, -0.1), v)
        keep = P[:, 2] > -0.02
        P, n = P[keep], n[keep]
        # strata, cracks and dry scrub on the tops
        strata = 0.86 + 0.16 * np.sin(P[:, 2] * 8.5 + 4.0 * vnoise3(P[:, 0], P[:, 1], P[:, 2] * 0.4, 2, 21))
        crack = np.abs(vnoise3(P[:, 0], P[:, 1], P[:, 2] * 0.8, 6, 22) - 0.5) < 0.03
        rnd = hash01(P[:, 0] * 37.0, P[:, 1] * 41.0, P[:, 2] * 43.0)
        tone = strata * (0.9 + 0.22 * (rnd - 0.5))
        tone = np.where(crack, tone * 0.55, tone)
        is_top = n[:, 2] > 0.72
        scrub = is_top & (vnoise3(P[:, 0], P[:, 1], np.zeros(len(P)), 4, 23) > 0.63)
        # plateau tops stay dim so the vale floor reads as the lit place; faces carry the strata
        tone = np.where(is_top, tone * 0.6, tone * 1.08)
        C = np.where(scrub, rid("v_scrub"), rid("v_stone"))
        N = n * tone[:, None]
        P = P - np.array([cx + 0.5, cy + 0.5, 0.0])
        return render(P, N, C, name, outline=False, splat=2, min_h=8)


# --- props -------------------------------------------------------------------------------------


def tone_bands(P, N, axis_vals, width, base=0.82, amp=0.3, seed=0.0):
    t = base + amp * hash01(np.floor(axis_vals / width), seed)
    return N * t[:, None]


def hut(seed: int):
    """Covered hut, 3x3 footprint (origin = centre cell), timber walls on a stone footing and a
    heavy low hip roof whose eaves reach past the footprint."""
    rng = np.random.default_rng(seed)
    half, wall_top = 1.08, 1.32
    eave = 1.78 + 0.06 * rng.random()
    prims = [
        Prim("box", ((0, 0, 0.13), (half + 0.07, half + 0.07, 0.13)), "v_stone"),
        Prim("box", ((0, 0, wall_top / 2 + 0.1), (half, half, wall_top / 2 - 0.02)), "v_wood"),
    ]
    for sx in (-1, 1):
        for sy in (-1, 1):
            prims.append(Prim("capsule", ((sx * half, sy * half, 0.1), (sx * half, sy * half, wall_top + 0.1), 0.075), "v_dwood"))
    prims.append(Prim("box", ((half + 0.015, 0.25, 0.62), (0.03, 0.3, 0.5)), "v_dark"))  # door (+x)
    prims.append(Prim("box", ((half + 0.05, 0.25, 1.15), (0.06, 0.4, 0.04)), "v_dwood"))  # lintel
    prims.append(Prim("box", ((-0.35, half + 0.015, 0.85), (0.22, 0.03, 0.14)), "v_dark"))  # window (+y)
    prims.append(Prim("box", ((-0.35, half + 0.04, 0.85), (0.2, 0.02, 0.12)), "v_cloth"))  # rag over it
    steps = 16
    roof = []
    for k in range(steps):
        h = eave * (1 - k / steps) + 0.1
        roof.append(Prim("box", ((0, 0, wall_top + 0.1 + k * 0.062), (h, h * (0.94 if k > 8 else 1.0), 0.05)), "v_thatch"))
    roof.append(Prim("box", ((0, 0, wall_top + 0.12 + steps * 0.062), (0.32, 0.2, 0.07)), "v_turf"))
    P1, N1, C1 = prims_points(prims, 0.026, seed, 0.25)
    # vertical planks on the walls
    wall = (C1 == rid("v_wood"))
    along = np.where(np.abs(N1[:, 0]) > np.abs(N1[:, 1]), P1[:, 1], P1[:, 0])
    t = 0.75 + 0.4 * hash01(np.floor(along / 0.19), seed)
    t = np.where((along % 0.19) < 0.03, 0.5, t)
    N1 = np.where(wall[:, None], N1 * t[:, None], N1)
    P2, N2, C2 = prims_points(roof, 0.026, seed + 1, 0.35)
    # thatch streaks running down the slopes + some turf patches
    streak = 0.68 + 0.32 * hash01(np.round(P2[:, 0] * 30), np.round(P2[:, 1] * 30), seed)
    N2 = N2 * streak[:, None]
    turf = vnoise3(P2[:, 0] + 8, P2[:, 1] + 8, P2[:, 2], 2, 31 + seed) > 0.62
    C2 = np.where(turf, rid("v_turf"), C2)
    return merge((P1, N1, C1), (P2, N2, C2))


def lane_roof(xseed: int, post: bool):
    """Segment of the roofed lane (lane runs along +x). Anchor = back post cell; the roof spans
    y in [0, 4] (posts at y=0 and y=4), sloping down toward the front."""
    prims = []
    if post:
        prims.append(Prim("capsule", ((0, 0, 0), (0, 0, 2.38), 0.07), "v_dwood"))
    prims.append(Prim("box", ((0, 0, 2.34), (0.53, 0.07, 0.07)), "v_dwood"))  # back beam
    prims.append(Prim("box", ((0, 4.0, 1.92), (0.53, 0.07, 0.07)), "v_dwood"))  # front beam
    P1, N1, C1 = prims_points(prims, 0.024, xseed, 0.3)
    # shingled plank roof: surface points
    us = np.arange(-0.52, 0.52, 0.018)
    vs = np.arange(-0.35, 4.4, 0.018)
    U, V = np.meshgrid(us, vs, indexing="ij")
    U, V = U.ravel(), V.ravel()
    slope = 0.105
    Z = 2.46 - slope * V + 0.04 * ((V * 3.2) % 1.0)  # shingle courses
    P2 = np.stack([U, V, Z], 1)
    n = np.stack([np.zeros_like(U), np.full_like(U, slope), np.ones_like(U)], 1)
    n = n / np.linalg.norm(n, axis=1)[:, None]
    plank = np.floor((U + 0.52) / 0.26)
    t = 0.72 + 0.38 * hash01(plank, np.floor(V * 3.2), xseed)
    t = np.where(((V * 3.2) % 1.0) > 0.9, t * 0.6, t)
    P2b = P2 - np.array([0, 0, 0.06])  # thickness, seen at the front edge
    front = V > 4.3
    P3 = np.concatenate([P2, P2b[front]])
    N3 = np.concatenate([n * t[:, None], np.tile([0, 1.0, 0.2], (int(front.sum()), 1)) * 0.6])
    thatch = vnoise3(U + xseed, V, np.zeros_like(U), 2, 41) > 0.6
    C3 = np.concatenate([np.where(thatch, rid("v_turf"), rid("v_wood")), np.full(int(front.sum()), rid("v_wood"))])
    return merge((P1, N1, C1), (P3, N3, C3))


def post(height: float, seed: int, cap=True):
    prims = [Prim("capsule", ((0, 0, 0), (0, 0, height), 0.07), "v_dwood")]
    prims.append(Prim("ellipsoid", ((0, 0, 0.04), (0.16, 0.14, 0.07)), "v_stone"))
    if cap:
        prims.append(Prim("box", ((0, 0, height - 0.05), (0.1, 0.1, 0.05)), "v_dwood"))
    return prims_points(prims, 0.022, seed, 0.3)


def canopy(seed: int, striped: bool):
    """Canopy shelter, 4x4 cells: anchor = back corner post (0, 0); tarp to (3, 3)."""
    rng = np.random.default_rng(seed)
    P1, N1, C1 = post(2.2, seed)
    ropes = [Prim("capsule", ((0, 0, 2.15), (-0.55, -0.55, 0.02), 0.012), "v_straw")]
    P1r, N1r, C1r = prims_points(ropes, 0.016, seed)
    us = np.arange(-0.12, 3.12, 0.017)
    U, V = np.meshgrid(us, us, indexing="ij")
    U, V = U.ravel(), V.ravel()
    sag = 0.32 + 0.05 * rng.random()

    def z_of(u, v):
        wr = periodic_noise(u % 4, v % 4, 3, 51 + seed) - 0.5
        return 2.2 - sag * np.sin(np.pi * np.clip(u, 0, 3) / 3) * np.sin(np.pi * np.clip(v, 0, 3) / 3) + 0.05 * wr

    Z = z_of(U, V)
    e = 0.01
    dzdu = (z_of(U + e, V) - z_of(U - e, V)) / (2 * e)
    dzdv = (z_of(U, V + e) - z_of(U, V - e)) / (2 * e)
    n = np.stack([-dzdu, -dzdv, np.ones_like(U)], 1)
    n = n / np.linalg.norm(n, axis=1)[:, None]
    patch = periodic_noise(U % 4, V % 4, 3, 61 + seed) > 0.7
    if striped:
        band = np.floor(U / 0.6) % 2
        C = np.where(band == 1, rid("v_tarp2"), rid("v_tarp"))
    else:
        C = np.where(patch, rid("v_tarp2"), rid("v_tarp"))
    weave = 0.72 + 0.2 * hash01(np.round(U * 40), np.round(V * 40), seed)
    weave = np.where(patch & striped, weave * 0.75, weave)
    P2 = np.stack([U, V, Z], 1)
    N2 = n * weave[:, None]
    # ragged valance on the two front edges (+x and +y), hanging 0.12..0.35
    parts = [(P1, N1, C1), (P1r, N1r, C1r), (P2, N2, C)]
    for axis in (0, 1):
        s = np.arange(-0.1, 3.12, 0.017)
        hang = 0.14 + 0.2 * periodic_noise(s % 4, np.full_like(s, axis + 0.5), 6, 71 + seed)
        zz = np.arange(0, 0.36, 0.017)
        S_, ZZ = np.meshgrid(s, zz, indexing="ij")
        H_ = np.repeat(hang[:, None], len(zz), 1)
        ok = ZZ < H_
        S_, ZZ = S_[ok], ZZ[ok]
        edge = 3.12
        if axis == 0:
            pts = np.stack([np.full_like(S_, edge), S_, z_of(np.full_like(S_, 3.0), S_) - ZZ], 1)
            nn = np.tile([1.0, 0.0, 0.1], (len(S_), 1))
        else:
            pts = np.stack([S_, np.full_like(S_, edge), z_of(S_, np.full_like(S_, 3.0)) - ZZ], 1)
            nn = np.tile([0.0, 1.0, 0.1], (len(S_), 1))
        cc = np.full(len(S_), rid("v_tarp" if striped else "v_tarp2"))
        parts.append((pts, nn * 0.8, cc))
    return merge(*parts)


def grain_clump(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    for row in (-0.22, 0.2):
        for k in range(11):
            x = -0.42 + k * 0.085 + rng.normal(0, 0.02)
            y = row + rng.normal(0, 0.05)
            h = 0.5 + rng.random() * 0.3
            lx, ly = rng.normal(0.05, 0.05), rng.normal(0.02, 0.05)
            prims.append(Prim("capsule", ((x, y, 0), (x + lx, y + ly, h), 0.013), "v_straw"))
            prims.append(Prim("ellipsoid", ((x + lx * 1.05, y + ly * 1.05, h + 0.04), (0.03, 0.03, 0.07)), "v_grain"))
    return prims_points(prims, 0.012, seed, 0.4)


def lightworker_pole(seed: int):
    """Scarecrow-ish lightworker marker: pole, crossbar, sack head, wide straw hat, rag veil."""
    rng = np.random.default_rng(seed)
    lean = rng.normal(0, 0.05, 2)
    top = (lean[0], lean[1], 2.0)
    prims = [
        Prim("capsule", ((0, 0, 0), top, 0.05), "v_dwood"),
        Prim("capsule", ((lean[0] * 0.8, lean[1] * 0.8 - 0.55, 1.55), (lean[0] * 0.8, lean[1] * 0.8 + 0.55, 1.6), 0.035), "v_dwood"),
        Prim("ellipsoid", ((top[0], top[1], 1.82), (0.13, 0.13, 0.16)), "v_tarp2"),
        Prim("ellipsoid", ((top[0], top[1], 2.0), (0.42, 0.42, 0.035)), "v_straw"),
        Prim("ellipsoid", ((top[0], top[1], 2.06), (0.17, 0.17, 0.12)), "v_straw"),
        Prim("ellipsoid", ((0, 0, 0.04), (0.18, 0.16, 0.06)), "v_stone"),
    ]
    for k in range(7):  # rag strips hanging from the crossbar
        y = -0.5 + k * 0.16 + rng.normal(0, 0.02)
        ln = 0.35 + rng.random() * 0.45
        prims.append(Prim("box", ((lean[0] * 0.8 + 0.03, lean[1] * 0.8 + y, 1.55 - ln / 2), (0.015, 0.07, ln / 2)), "v_rag" if k % 3 else "v_cloth"))
    prims.append(Prim("capsule", ((0.05, 0.32, 1.5), (0.12, 0.42, 1.18), 0.02), "v_metal"))  # hanging sickle
    return prims_points(prims, 0.02, seed, 0.35)


def cairn(seed: int):
    """Rest cairn: stacked flat stones with an iron brazier on top; returns points and the
    brazier coal centre (for the fire particles / light)."""
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(8):
        a = k * 2 * math.pi / 8 + rng.random() * 0.3
        prims.append(Prim("ellipsoid", ((0.48 * math.cos(a), 0.48 * math.sin(a), 0.1), (0.2, 0.17, 0.12)), "v_stone"))
    layers = [(0.0, 0.42, 0.18, 0.2), (0.35, 0.36, 0.16, 0.18), (0.63, 0.3, 0.13, 0.15)]
    for z, r, rr, hh in layers:
        n = 6
        for k in range(n):
            a = k * 2 * math.pi / n + rng.random() * 0.5 + z
            prims.append(Prim("ellipsoid", ((r * 0.55 * math.cos(a), r * 0.55 * math.sin(a), z + hh), (rr * 1.3, rr, hh * 0.75)), "v_stone"))
    prims.append(Prim("ellipsoid", ((0, 0, 0.92), (0.34, 0.3, 0.1)), "v_stone"))  # cap slab
    prims.append(Prim("ellipsoid", ((0, 0, 1.12), (0.33, 0.33, 0.13)), "v_iron"))  # brazier bowl
    prims.append(Prim("ellipsoid", ((0, 0, 1.24), (0.36, 0.36, 0.03)), "v_iron"))  # rim
    prims.append(Prim("ellipsoid", ((0, 0, 1.26), (0.27, 0.27, 0.05)), "v_ember"))  # coals
    for k in range(3):  # little offerings / bones at the foot
        a = 0.8 + k * 0.7
        prims.append(Prim("capsule", ((0.75 * math.cos(a), 0.75 * math.sin(a), 0.03), (0.75 * math.cos(a) + 0.15, 0.75 * math.sin(a) + 0.06, 0.03), 0.025), "v_bone"))
    return prims_points(prims, 0.018, seed, 0.45), (0.0, 0.0, 1.3)


def lantern_post(seed: int):
    prims = [
        Prim("capsule", ((0, 0, 0), (0, 0, 1.95), 0.05), "v_dwood"),
        Prim("capsule", ((0, 0, 1.85), (0.36, 0.0, 1.85), 0.03), "v_dwood"),
        Prim("capsule", ((0.36, 0, 1.85), (0.36, 0, 1.68), 0.008), "v_iron"),
        Prim("box", ((0.36, 0, 1.6), (0.06, 0.06, 0.07)), "v_ember"),
        Prim("box", ((0.36, 0, 1.69), (0.08, 0.08, 0.02)), "v_iron"),
        Prim("box", ((0.36, 0, 1.51), (0.08, 0.08, 0.02)), "v_iron"),
        Prim("ellipsoid", ((0, 0, 0.04), (0.16, 0.14, 0.06)), "v_stone"),
    ]
    for sx in (-1, 1):
        for sy in (-1, 1):
            prims.append(Prim("capsule", ((0.36 + sx * 0.065, sy * 0.065, 1.51), (0.36 + sx * 0.065, sy * 0.065, 1.69), 0.012), "v_iron"))
    return prims_points(prims, 0.014, seed, 0.3), (0.36, 0.0, 1.6)


def dead_tree(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    h = 1.6 + rng.random() * 0.9
    lean = rng.normal(0, 0.12, 2)
    pts = [(0, 0, 0), (lean[0] * 0.4, lean[1] * 0.4, h * 0.5), (lean[0], lean[1], h)]
    radii = [0.16, 0.11, 0.07]
    for a, b, r in zip(pts[:-1], pts[1:], radii):
        prims.append(Prim("capsule", (a, b, r), "v_dwood"))
    for k in range(4):  # roots
        a = k * 1.6 + rng.random()
        prims.append(Prim("capsule", ((0, 0, 0.18), (0.42 * math.cos(a), 0.42 * math.sin(a), -0.02), 0.06), "v_dwood"))

    def branch(p, d, length, r, depth):
        q = (p[0] + d[0] * length, p[1] + d[1] * length, p[2] + d[2] * length)
        prims.append(Prim("capsule", (p, q, r), "v_dwood"))
        if depth == 0:
            return
        for _ in range(2):
            nd = np.array(d) + rng.normal(0, 0.55, 3)
            nd[2] = abs(nd[2]) * 0.6 + 0.15
            nd = nd / np.linalg.norm(nd)
            branch(q, tuple(nd), length * 0.62, r * 0.62, depth - 1)

    for k in range(3 + int(rng.random() * 2)):
        a = rng.random() * 2 * math.pi
        z0 = h * (0.55 + rng.random() * 0.45)
        t = z0 / h
        p = (lean[0] * t, lean[1] * t, z0)
        d = np.array([math.cos(a), math.sin(a), 0.5 + rng.random() * 0.5])
        branch(p, tuple(d / np.linalg.norm(d)), 0.55 + rng.random() * 0.3, 0.05, 2)
    return prims_points(prims, 0.018, seed, 0.6)


def boulder(seed: int, size=1.0):
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(3 + int(rng.random() * 4)):
        c = (rng.normal(0, 0.16) * size, rng.normal(0, 0.16) * size, (0.08 + rng.random() * 0.18) * size)
        s = (0.18 + rng.random() * 0.24) * size
        prims.append(Prim("ellipsoid", (c, (s * 1.2, s, s * 0.75)), "v_stone"))
    P, N, C = prims_points(prims, 0.022, seed, 0.6)
    return P, N * 0.8, C


def bones(seed: int, kind: str):
    rng = np.random.default_rng(seed)
    prims = []
    if kind == "ribs":  # an old beast's ribcage
        for k in range(6):
            x = -0.6 + k * 0.24
            h = 0.55 + 0.25 * math.sin(math.pi * (k + 0.5) / 6)
            for side in (-1, 1):
                pts = [(x, side * (0.1 + 0.4 * math.sin(math.pi * t / 4)), h * math.sin(math.pi * t / 4 + 0.6) * (1 - t / 6)) for t in range(5)]
                for a, b in zip(pts[:-1], pts[1:]):
                    prims.append(Prim("capsule", (a, b, 0.03), "v_bone"))
        prims.append(Prim("capsule", ((-0.75, 0, 0.62), (0.75, 0, 0.5), 0.05), "v_bone"))
        prims.append(Prim("ellipsoid", ((0.95, 0.15, 0.12), (0.22, 0.13, 0.12)), "v_bone"))
    elif kind == "skull":
        prims.append(Prim("ellipsoid", ((0, 0, 0.09), (0.13, 0.1, 0.09)), "v_bone"))
        prims.append(Prim("ellipsoid", ((0.1, 0, 0.05), (0.08, 0.07, 0.05)), "v_bone"))
        prims.append(Prim("ellipsoid", ((0.1, 0.045, 0.11), (0.02, 0.025, 0.025)), "v_dark"))
        prims.append(Prim("ellipsoid", ((0.1, -0.045, 0.11), (0.02, 0.025, 0.025)), "v_dark"))
        for k in range(4):
            a = rng.random() * math.pi
            c = (rng.normal(0.1, 0.25), rng.normal(0.1, 0.25))
            prims.append(Prim("capsule", ((c[0], c[1], 0.03), (c[0] + 0.3 * math.cos(a), c[1] + 0.3 * math.sin(a), 0.03), 0.028), "v_bone"))
    else:  # scattered bones and a broken spear
        for k in range(6):
            a = rng.random() * math.pi
            c = (rng.normal(0, 0.25), rng.normal(0, 0.25))
            ln = 0.15 + rng.random() * 0.25
            prims.append(Prim("capsule", ((c[0], c[1], 0.03), (c[0] + ln * math.cos(a), c[1] + ln * math.sin(a), 0.03), 0.025), "v_bone"))
        prims.append(Prim("capsule", ((-0.4, 0.3, 0.02), (0.3, -0.2, 0.25), 0.02), "v_dwood"))
        prims.append(Prim("capsule", ((0.3, -0.2, 0.25), (0.38, -0.26, 0.31), 0.03), "v_metal"))
    return prims_points(prims, 0.014, seed, 0.4)


def terrace_wall(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    for row, z in ((0, 0.1), (1, 0.27), (2, 0.42)):
        for k in range(4 - (row == 2)):
            x = -0.45 + k * 0.3 + (row % 2) * 0.15 + rng.normal(0, 0.03)
            prims.append(Prim("ellipsoid", ((x, rng.normal(0, 0.04), z), (0.17, 0.2, 0.1)), "v_stone"))
    return prims_points(prims, 0.018, seed, 0.6)


def crate(seed: int):
    s = 0.22 + 0.04 * np.random.default_rng(seed).random()
    prims = [Prim("box", ((0, 0, s), (s, s, s)), "v_wood")]
    for z in (0.04, 2 * s - 0.04):
        prims.append(Prim("box", ((0, 0, z), (s + 0.01, s + 0.01, 0.025)), "v_dwood"))
    return prims_points(prims, 0.016, seed, 0.4)


def sacks(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(3):
        c = (rng.normal(0, 0.18), rng.normal(0, 0.18))
        prims.append(Prim("ellipsoid", ((c[0], c[1], 0.17), (0.17, 0.15, 0.2)), "v_tarp2"))
        prims.append(Prim("ellipsoid", ((c[0], c[1], 0.38), (0.05, 0.05, 0.05)), "v_straw"))
    return prims_points(prims, 0.016, seed, 0.4)


def barrel(seed: int):
    prims = [Prim("ellipsoid", ((0, 0, 0.3), (0.21, 0.21, 0.36)), "v_wood")]
    for z in (0.1, 0.46):
        prims.append(Prim("ellipsoid", ((0, 0, z), (0.215, 0.215, 0.025)), "v_iron"))
    return prims_points(prims, 0.016, seed, 0.4)


def woodpile(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    for layer in range(3):
        for k in range(4 - layer):
            y = -0.3 + k * 0.2 + layer * 0.1
            prims.append(Prim("capsule", ((-0.35, y, 0.08 + layer * 0.15), (0.35, y + rng.normal(0, 0.03), 0.08 + layer * 0.15), 0.075), "v_wood"))
    return prims_points(prims, 0.016, seed, 0.5)


def cart(seed: int):
    """Broken cart: tilted bed, one wheel off and lying flat."""
    prims = [
        Prim("box", ((0, 0, 0.35), (0.7, 0.4, 0.04)), "v_wood"),
        Prim("box", ((0, 0.4, 0.5), (0.7, 0.03, 0.15)), "v_wood"),
        Prim("box", ((0, -0.4, 0.5), (0.7, 0.03, 0.15)), "v_dwood"),
        Prim("capsule", ((0.7, 0.2, 0.33), (1.4, 0.15, 0.02), 0.04), "v_dwood"),
        Prim("capsule", ((0.7, -0.2, 0.33), (1.35, -0.3, 0.02), 0.04), "v_dwood"),
        Prim("ellipsoid", ((-0.3, 0.45, 0.28), (0.3, 0.04, 0.3)), "v_dwood"),
        Prim("ellipsoid", ((-0.9, -0.7, 0.04), (0.3, 0.3, 0.035)), "v_dwood"),
        Prim("ellipsoid", ((-0.9, -0.7, 0.05), (0.06, 0.06, 0.04)), "v_iron"),
    ]
    P, N, C = prims_points(prims, 0.018, seed, 0.5)
    # tilt: the cart rests on its broken side
    a = math.radians(9)
    R = np.array([[1, 0, 0], [0, math.cos(a), -math.sin(a)], [0, math.sin(a), math.cos(a)]])
    P = P @ R.T
    N = N @ R.T
    P[:, 2] -= P[:, 2].min() - 0.0
    return P, N, C


def reeds(seed: int):
    rng = np.random.default_rng(seed)
    prims = []
    for k in range(16):
        x, y = rng.normal(0, 0.22, 2)
        h = 0.4 + rng.random() * 0.5
        prims.append(Prim("capsule", ((x, y, 0), (x + rng.normal(0, 0.08), y + rng.normal(0, 0.08), h), 0.012), "v_straw" if k % 3 else "v_bone"))
    return prims_points(prims, 0.012, seed, 0.5)


def gate_wall(seed: int, height: float):
    """One cell of the Glare Gate's ruined wall: ashlar courses with a broken, jagged top."""
    rng = np.random.default_rng(seed)
    prims = []
    for qx in (-0.25, 0.25):
        for qy in (-0.25, 0.25):
            h = max(0.35, height * (0.6 + 0.45 * rng.random()))
            prims.append(Prim("box", ((qx, qy, h / 2), (0.26, 0.26, h / 2)), "v_ashlar"))
    for k in range(3):  # fallen blocks at the foot
        c = (rng.uniform(-0.45, 0.45), rng.uniform(0.2, 0.55))
        prims.append(Prim("box", ((c[0], c[1], 0.09), (0.13, 0.1, 0.09)), "v_ashlar"))
    P, N, C = prims_points(prims, 0.02, seed, 0.2)
    z = P[:, 2]
    course = np.floor(z / 0.24)
    along = np.where(np.abs(N[:, 0]) > np.abs(N[:, 1]), P[:, 1], P[:, 0])
    off = (course % 2) * 0.2
    mortar = ((z % 0.24) < 0.035) | (((along + off) % 0.4) < 0.035)
    t = np.where(mortar, 0.45, 0.8 + 0.35 * hash01(np.floor((along + off) / 0.4), course, seed))
    top = N[:, 2] > 0.7
    t = np.where(top, 0.75, t)
    return P, N * t[:, None], C


def gate_tower(seed: int, banner: bool):
    """Ruined 2x2 gate tower (origin = centre of the footprint) with a jagged broken crown,
    arrow slits and, optionally, a torn oxblood banner."""
    rng = np.random.default_rng(seed)
    prims = []
    for qx in (-0.48, 0.48):
        for qy in (-0.48, 0.48):
            h = 3.0 + rng.random() * 1.4
            prims.append(Prim("box", ((qx, qy, h / 2), (0.5, 0.5, h / 2)), "v_ashlar"))
    prims.append(Prim("box", ((0, 0, 0.2), (1.08, 1.08, 0.2)), "v_ashlar"))  # plinth
    for k in range(6):  # rubble
        a = rng.uniform(-0.4, 2.0)
        r = 1.25 + rng.random() * 0.4
        prims.append(Prim("box", ((r * math.cos(a), r * math.sin(a), 0.1), (0.16, 0.12, 0.1)), "v_ashlar"))
    P, N, C = prims_points(prims, 0.024, seed, 0.2)
    z = P[:, 2]
    course = np.floor(z / 0.28)
    along = np.where(np.abs(N[:, 0]) > np.abs(N[:, 1]), P[:, 1], P[:, 0])
    off = (course % 2) * 0.25
    mortar = ((z % 0.28) < 0.035) | (((along + off) % 0.5) < 0.04)
    t = np.where(mortar, 0.45, 0.8 + 0.35 * hash01(np.floor((along + off) / 0.5), course, seed))
    # arrow slits on the visible faces
    slit = (np.abs(along - 0.0) < 0.05) & (((z > 1.3) & (z < 1.9)) | ((z > 2.4) & (z < 2.9))) & (N[:, 2] < 0.3)
    face_x = (N[:, 0] > 0.7) & (P[:, 0] > 0.9)
    face_y = (N[:, 1] > 0.7) & (P[:, 1] > 0.9)
    slit &= face_x | face_y
    C = np.where(slit, rid("v_dark"), C)
    t = np.where(N[:, 2] > 0.7, 0.72, t)
    parts = [(P, N * t[:, None], C)]
    if banner:
        prims = [Prim("capsule", ((0.98, -0.6, 3.2), (0.98, 0.6, 3.2), 0.03), "v_iron")]
        for k in range(6):
            y = -0.5 + k * 0.2
            ln = 0.6 + rng.random() * 1.0 if k not in (3,) else 0.3
            prims.append(Prim("box", ((1.02, y, 3.2 - ln / 2), (0.015, 0.09, ln / 2)), "v_rag"))
        parts.append(prims_points(prims, 0.018, seed + 3, 0.4))
    return merge(*parts)


def fallen_blade():
    """The Fallen Blade: a giant tarnished sword driven into the earth, leaning back, with a
    cracked mound around it. Origin = the blade's entry point."""
    v = 0.045
    tilt = math.radians(13)
    lean_dir = np.array([-0.85, -0.35, 0.0])
    lean_dir /= np.linalg.norm(lean_dir)
    axis = np.array([0, 0, math.cos(tilt)]) + lean_dir * math.sin(tilt)
    axis /= np.linalg.norm(axis)
    t = math.radians(30)
    ew = math.cos(t) * np.array([1, -1, 0]) / math.sqrt(2) + math.sin(t) * np.array([1, 1, 0]) / math.sqrt(2)
    ew = ew - axis * (ew @ axis)
    ew /= np.linalg.norm(ew)
    ed = np.cross(axis, ew)
    L = 7.0  # visible blade length
    top = axis * (L + 3.0)
    lo = np.minimum(0, top) - 2.2
    hi = np.maximum(0, top) + 2.2
    lo[2], hi[2] = -0.2, L + 3.4
    xs, ys, zs = (np.arange(lo[i], hi[i], v) for i in range(3))
    X, Y, Z = np.meshgrid(xs, ys, zs, indexing="ij")
    Q = np.stack([X, Y, Z], -1)
    s = Q @ axis
    w = Q @ ew
    d = Q @ ed
    # blade: tapers into the ground, diamond section with a fuller, nicked edges
    hw = 0.36 + 0.26 * np.clip(s / L, 0, 1)
    nick = 0.16 * np.clip((vnoise3(s * 0.7, np.sign(w) * 1.3 + 2, np.zeros_like(s), 3, 81) - 0.58) * 4, 0, 1)
    hw_eff = hw - nick
    rel = np.abs(w) / np.maximum(hw, 1e-3)
    td = 0.13 * (1 - rel ** 1.3) + 0.018
    fuller = (np.abs(w) < 0.09) & (s > 0.4) & (s < L - 0.6)
    td = np.where(fuller, td - 0.045, td)
    blade = (s > -0.3) & (s < L) & (np.abs(w) < hw_eff) & (np.abs(d) < td)
    # crossguard (drooping ends), grip, pommel
    gw = 1.85
    gs = L + 0.2 - 0.25 * (w / gw) ** 2
    guard = (np.abs(w) < gw) & (np.abs(s - gs) < 0.2 + 0.06 * (np.abs(w) > gw - 0.3)) & (np.abs(d) < 0.24)
    grip_r = 0.17 + 0.025 * (np.sin((s - L) * 22) > 0.3)
    grip = (s > L + 0.35) & (s < L + 2.3) & (np.hypot(w, d) < grip_r)
    pc = L + 2.62
    pommel = ((w / 0.42) ** 2 + ((s - pc) / 0.34) ** 2 + (d / 0.2) ** 2) < 1.0
    pommel_hole = ((w / 0.16) ** 2 + ((s - pc) / 0.13) ** 2) < 1.0
    pommel &= ~pommel_hole
    # mound of torn earth and heaved slabs around the entry point
    r = np.hypot(X, Y)
    mound_h = 0.55 * np.exp(-(r / 1.3) ** 2) + 0.25 * (vnoise3(X, Y, np.zeros_like(X), 3, 82) - 0.5) * np.exp(-(r / 2.0) ** 2)
    mound = (Z < mound_h) & (mound_h > 0.08) & (r < 2.1)
    slab_n = vnoise3(X * 0.8, Y * 0.8, Z * 0.5, 2, 83)
    slabs = (r > 0.7) & (r < 1.9) & (Z < 0.9 * np.clip((slab_n - 0.55) * 4, 0, 1)) & (Z >= 0)
    occ = blade | guard | grip | pommel | mound | slabs
    occ &= Z >= -0.05
    P, n, idx = occ_surface(occ, occ, lo, v)
    keep = P[:, 2] > -0.02
    P, n = P[keep], n[keep]
    idx = tuple(i[keep] for i in idx)
    sel = lambda m: m[idx]  # noqa: E731
    C = np.full(len(P), rid("v_metal"))
    rust = vnoise3(P[:, 0] * 0.6 + 7, P[:, 1] * 0.6, P[:, 2] * 0.6, 3, 84) > 0.6
    C = np.where(sel(blade) & rust, rid("v_rust"), C)
    C = np.where(sel(guard) | sel(pommel), rid("v_bronze"), C)
    C = np.where(sel(guard) & rust, rid("v_rust"), C)
    C = np.where(sel(grip), rid("v_cloth"), C)
    earth = sel(mound | slabs) & ~sel(blade)
    C = np.where(earth, np.where(sel(slabs), rid("v_stone"), rid("g_rust")), C)
    rnd = hash01(P[:, 0] * 53.0, P[:, 1] * 59.0, P[:, 2] * 61.0)
    tone = 0.88 + 0.22 * (rnd - 0.5)
    # pitted, tarnished metal: darker blotches; the edge bevel catches a little more light
    pit = vnoise3(P[:, 0] * 2 + 3, P[:, 1] * 2, P[:, 2] * 2, 4, 85) > 0.62
    tone = np.where(sel(blade) & pit, tone * 0.7, tone)
    bevel = sel(blade) & (sel(rel) > 0.75)
    tone = np.where(bevel, tone * 1.18, tone)
    N = n * tone[:, None]
    # rags tied to the guard, hanging down
    rng = np.random.default_rng(86)
    rag_parts = []
    for k in range(4):
        ww = -1.5 + k * 0.95 + rng.normal(0, 0.1)
        base = axis * (L + 0.1) + ew * ww
        ln = 0.8 + rng.random() * 1.1
        prims = [Prim("box", ((base[0], base[1], base[2] - ln / 2), (0.1, 0.1, ln / 2)), "v_rag")]
        rag_parts.append(prims_points(prims, 0.03, 87 + k, 0.4))
    return merge((P, N, C), *rag_parts)


# --- prop catalog -----------------------------------------------------------------------------


def make_props():
    """Renders every fixed prop; returns {key: [filenames]} and fills SPRITE_FX."""
    out: dict[str, list[str]] = {}

    def add(key, name, pts, **kw):
        fname = f"cv_{name}.png"
        render(*pts, fname, **kw)
        out.setdefault(key, []).append(fname)
        return fname

    for i in range(2):
        add("hut", f"hut_{i}", hut(11 + i * 5), splat=2)
    for i in range(2):
        add("lane_roof_post", f"laneroof_p{i}", lane_roof(3 + i, True))
        add("lane_roof", f"laneroof_{i}", lane_roof(7 + i, False))
    add("lane_post", "lanepost_0", post(1.95, 5))
    add("canopy", "canopy_0", canopy(21, True), splat=2)
    add("canopy", "canopy_1", canopy(22, False), splat=2)
    add("canopy_post", "canopypost_0", post(2.2, 23))
    for i in range(4):
        add("grain", f"grain_{i}", grain_clump(31 + i))
    for i in range(2):
        add("pole", f"pole_{i}", lightworker_pole(41 + i))
    pts, coal = cairn(51)
    name = add("cairn", "cairn_0", pts)
    ox, oy = screen_offset(coal)
    fx, fy = (int(v) for v in HOTSPOTS[-1].split()[1:])
    SPRITE_FX.append(f"psi {name} campfire.psi {fx + ox} {fy + oy + 2}")
    SPRITE_FX.append(f"light {name} e25822c8 {ox} {oy + 30} 1 0 1.6")
    pts, lan = lantern_post(61)
    name = add("lantern", "lantern_0", pts)
    ox, oy = screen_offset(lan)
    fx, fy = (int(v) for v in HOTSPOTS[-1].split()[1:])
    SPRITE_FX.append(f"psi {name} small_light_embers.psi {fx + ox} {fy + oy}")
    SPRITE_FX.append(f"light {name} e25822c8 {ox} {oy} 1 0 0.8")
    for i in range(4):
        add("dead_tree", f"deadtree_{i}", dead_tree(71 + i))
    for i in range(5):
        add("boulder", f"boulder_{i}", boulder(81 + i, 0.8 + 0.25 * (i % 3)))
    add("bones", "bones_ribs", bones(91, "ribs"))
    add("bones", "bones_skull", bones(92, "skull"))
    add("bones", "bones_pile", bones(93, "pile"))
    for i in range(3):
        add("terrace", f"terrace_{i}", terrace_wall(101 + i))
    for i in range(2):
        add("crate", f"crate_{i}", crate(111 + i))
    add("sacks", "sacks_0", sacks(115))
    add("barrel", "barrel_0", barrel(116))
    add("woodpile", "woodpile_0", woodpile(117))
    add("cart", "cart_0", cart(118))
    for i in range(2):
        add("reeds", f"reeds_{i}", reeds(121 + i))
    for i, h in enumerate((2.3, 2.0, 1.3, 0.75, 0.5)):
        add("gate_wall_tall" if h > 1.5 else "gate_wall_low", f"gatewall_{i}", gate_wall(131 + i, h))
    add("gate_tower", "gatetower_0", gate_tower(141, True))
    add("gate_tower", "gatetower_1", gate_tower(142, False))
    add("blade", "fallen_blade", fallen_blade(), splat=3)
    return out
