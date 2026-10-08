"""Tiny voxel pre-renderer for oldschool isometric sprites.

Models are built from signed-distance primitives attached to bones, voxelized once,
posed per frame and splatted with a z-buffer through the game's own projection:
a cell is 64x32 px on screen, i.e. a 2:1 dimetric camera (yaw 45 deg, elevation 30 deg)
with 45.25 px per world unit horizontally and 39.2 px per unit vertically.
World units are map cells (~1 m); the model stands at the origin facing +x.

Look: per-voxel Lambert + ambient light, quantized to a fixed palette with 4x4 Bayer
dithering, plus a 1 px dark outline -- the Diablo / Ultima Online pre-rendered style.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field

import numpy as np

# --- camera -----------------------------------------------------------------

PX_PER_UNIT = 32.0 * math.sqrt(2.0)  # 45.25: one cell edge on screen
ELEV = math.radians(30.0)  # 2:1 diamonds
SIN_E, COS_E = math.sin(ELEV), math.cos(ELEV)
LIGHT = np.array([-0.45, 0.35, 0.82])  # from the upper left, like the original art
LIGHT = LIGHT / np.linalg.norm(LIGHT)


def project(p: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """World points (N,3) -> screen x, screen y (down), depth (bigger = closer)."""
    u = (p[:, 0] - p[:, 1]) / math.sqrt(2.0)
    v = (p[:, 0] + p[:, 1]) / math.sqrt(2.0)
    sx = u * PX_PER_UNIT
    sy = (v * SIN_E - p[:, 2] * COS_E) * PX_PER_UNIT
    depth = v * COS_E + p[:, 2] * SIN_E
    return sx, sy, depth


# --- rotations ----------------------------------------------------------------


def rot_x(a: float) -> np.ndarray:
    c, s = math.cos(a), math.sin(a)
    return np.array([[1, 0, 0], [0, c, -s], [0, s, c]])


def rot_y(a: float) -> np.ndarray:
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, 0, s], [0, 1, 0], [-s, 0, c]])


def rot_z(a: float) -> np.ndarray:
    c, s = math.cos(a), math.sin(a)
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]])


# --- signed distance primitives ---------------------------------------------------


def sd_box(p, center, half):
    q = np.abs(p - center) - half
    return np.linalg.norm(np.maximum(q, 0), axis=1) + np.minimum(q.max(axis=1), 0)


def sd_capsule(p, a, b, r):
    a, b = np.asarray(a, float), np.asarray(b, float)
    pa, ba = p - a, b - a
    h = np.clip((pa @ ba) / (ba @ ba), 0, 1)
    return np.linalg.norm(pa - h[:, None] * ba, axis=1) - r


def sd_ellipsoid(p, center, radii):
    q = (p - center) / radii
    k0 = np.linalg.norm(q, axis=1)
    k1 = np.linalg.norm(q / radii, axis=1)
    return k0 * (k0 - 1.0) / np.maximum(k1, 1e-6)


@dataclass
class Prim:
    kind: str  # box | capsule | ellipsoid
    args: tuple
    color: str  # palette ramp name

    def sdf(self, p):
        return {"box": sd_box, "capsule": sd_capsule, "ellipsoid": sd_ellipsoid}[self.kind](p, *self.args)


@dataclass
class Bone:
    name: str
    parent: str | None
    pivot: tuple[float, float, float]  # joint position in model space (rest pose)
    prims: list[Prim] = field(default_factory=list)


# --- palette ----------------------------------------------------------------------

# Earthy, slightly desaturated ramps (dark -> light), 5 shades each.
RAMPS = {
    "skin": ["#3a2418", "#6b3f2a", "#9c6446", "#c48a63", "#e2b48c"],
    "hair": ["#1c1410", "#33241a", "#4d3624", "#6a4c32", "#8a6844"],
    "leather": ["#24180f", "#3e2a19", "#5c3f25", "#7d5833", "#a07445"],
    "cloth": ["#1b1f26", "#2c3440", "#3f4b5c", "#56667a", "#73859b"],
    "red": ["#2a0e0c", "#4d1714", "#74221b", "#9b3424", "#c04f33"],
    "metal": ["#1e2024", "#3b3f45", "#5f656c", "#8c939a", "#c3c9cf"],
    "gold": ["#2e2008", "#5a3f10", "#8a6418", "#b98d2a", "#e6c25a"],
    "wood": ["#21160c", "#3b2814", "#58401f", "#77592c", "#987540"],
    "boot": ["#140d08", "#26190f", "#3a2717", "#523821", "#6b4b2d"],
}
OUTLINE = (12, 8, 6, 255)

BAYER4 = (
    np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], dtype=float) + 0.5
) / 16.0


def hex_rgb(h: str) -> tuple[int, int, int]:
    return int(h[1:3], 16), int(h[3:5], 16), int(h[5:7], 16)


RAMP_RGB = {k: np.array([hex_rgb(c) for c in v], dtype=np.uint8) for k, v in RAMPS.items()}
RAMP_IDS = {k: i for i, k in enumerate(RAMPS)}
RAMP_TABLE = np.stack([RAMP_RGB[k] for k in RAMPS])  # (ramps, 5, 3)


# --- model --------------------------------------------------------------------------


class Model:
    """Bones with voxelized primitives in rest pose."""

    def __init__(self, bones: list[Bone], voxel: float = 0.022):
        self.bones = {b.name: b for b in bones}
        self.order = [b.name for b in bones]  # parents first
        self.voxel = voxel
        self.parts = {}
        for b in bones:
            if b.prims:
                self.parts[b.name] = self._voxelize(b.prims)

    def _voxelize(self, prims: list[Prim]):
        """Surface voxels (positions, normals, ramp ids) of the union of prims."""
        pts, nrm, col = [], [], []
        for prim in prims:
            lo, hi = self._bounds(prim)
            axes = [np.arange(lo[i], hi[i] + self.voxel, self.voxel) for i in range(3)]
            g = np.stack(np.meshgrid(*axes, indexing="ij"), -1).reshape(-1, 3)
            d = prim.sdf(g)
            shell = g[(d <= 0) & (d > -1.6 * self.voxel)]
            if len(shell) == 0:
                continue
            e = self.voxel * 0.5
            n = np.stack(
                [
                    prim.sdf(shell + [e, 0, 0]) - prim.sdf(shell - [e, 0, 0]),
                    prim.sdf(shell + [0, e, 0]) - prim.sdf(shell - [0, e, 0]),
                    prim.sdf(shell + [0, 0, e]) - prim.sdf(shell - [0, 0, e]),
                ],
                -1,
            )
            n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-6)
            pts.append(shell)
            nrm.append(n)
            col.append(np.full(len(shell), RAMP_IDS[prim.color]))
        return np.concatenate(pts), np.concatenate(nrm), np.concatenate(col)

    @staticmethod
    def _bounds(prim: Prim):
        if prim.kind == "box":
            c, h = np.asarray(prim.args[0], float), np.asarray(prim.args[1], float)
            return c - h - 0.03, c + h + 0.03
        if prim.kind == "ellipsoid":
            c, r = np.asarray(prim.args[0], float), np.asarray(prim.args[1], float)
            return c - r - 0.03, c + r + 0.03
        a, b, r = np.asarray(prim.args[0], float), np.asarray(prim.args[1], float), prim.args[2]
        return np.minimum(a, b) - r - 0.03, np.maximum(a, b) + r + 0.03

    def pose(self, rotations: dict[str, np.ndarray], root_offset=(0, 0, 0), root_rot=None):
        """World transforms per bone: rotation about its pivot, chained through parents."""
        world = {}
        for name in self.order:
            b = self.bones[name]
            r_local = rotations.get(name, np.eye(3))
            pivot = np.asarray(b.pivot, float)
            if b.parent is None:
                r_root = root_rot if root_rot is not None else np.eye(3)
                rot = r_root @ r_local
                origin = r_root @ pivot + np.asarray(root_offset, float)
            else:
                pr, po = world[b.parent]
                parent_pivot = np.asarray(self.bones[b.parent].pivot, float)
                rot = pr @ r_local
                origin = po + pr @ (pivot - parent_pivot)
            world[name] = (rot, origin)
        return world

    def raster(self, world, facing: float, size: int = 128, foot=(64, 104), scale: float = 1.0):
        """Z-buffered raster of the posed model rotated by `facing` (cell-space radians), uniformly
        scaled by `scale` around the feet: (depth, light, ramp id) per pixel, ramp -1 = empty."""
        rz = rot_z(facing)
        all_p, all_n, all_c = [], [], []
        for name, (pts, nrm, col) in self.parts.items():
            rot, origin = world[name]
            pivot = np.asarray(self.bones[name].pivot, float)
            p = (pts - pivot) @ rot.T + origin
            all_p.append(p @ rz.T)
            all_n.append(nrm @ (rz @ rot).T)
            all_c.append(col)
        zbuf = np.full((size, size), -1e9)
        shade = np.zeros((size, size))
        ramp = np.full((size, size), -1, dtype=int)
        if not all_p:
            return zbuf, shade, ramp
        p, n, c = np.concatenate(all_p), np.concatenate(all_n), np.concatenate(all_c)
        p = p * scale

        sx, sy, depth = project(p)
        light = np.clip(n @ LIGHT, 0, 1) * 0.75 + 0.25 + np.clip(n[:, 2], 0, 1) * 0.08
        x = np.round(sx + foot[0]).astype(int)
        y = np.round(sy + foot[1]).astype(int)
        # Splat each voxel as a 2x2 block (voxel ~ 1 px; avoids holes when rotated).
        order = np.argsort(depth)
        for dx, dy in ((0, 0), (1, 0), (0, 1), (1, 1)):
            xx, yy = x[order] + dx, y[order] + dy
            ok = (xx >= 0) & (xx < size) & (yy >= 0) & (yy < size)
            xx, yy, dd, ll, cc = xx[ok], yy[ok], depth[order][ok], light[order][ok], c[order][ok]
            # depth-sorted ascending: with duplicate pixels the last (closest) write wins
            closer = dd > zbuf[yy, xx]
            zbuf[yy[closer], xx[closer]] = dd[closer]
            shade[yy[closer], xx[closer]] = ll[closer]
            ramp[yy[closer], xx[closer]] = cc[closer]
        return zbuf, shade, ramp

    def render(self, world, facing: float, size: int = 128, foot=(64, 104), scale: float = 1.0):
        """RGBA image (size x size) of the posed model; see `raster`."""
        _, shade, ramp = self.raster(world, facing, size, foot, scale)
        return compose(shade, ramp, ramp >= 0)


def compose(shade: np.ndarray, ramp: np.ndarray, mask: np.ndarray) -> np.ndarray:
    """Palette + ordered dithering for the pixels in `mask`, with a 1 px dark outline around them."""
    size_y, size_x = mask.shape
    ty, tx = np.indices((size_y, size_x))
    threshold = BAYER4[ty % 4, tx % 4]
    level = np.clip(shade * 4.0 + (threshold - 0.5) * 0.9, 0, 4).round().astype(int)
    rgb = RAMP_TABLE[np.clip(ramp, 0, None), level]
    img = np.zeros((size_y, size_x, 4), dtype=np.uint8)
    img[mask, :3] = rgb[mask]
    img[mask, 3] = 255
    grown = mask.copy()
    grown[1:, :] |= mask[:-1, :]
    grown[:-1, :] |= mask[1:, :]
    grown[:, 1:] |= mask[:, :-1]
    grown[:, :-1] |= mask[:, 1:]
    img[grown & ~mask] = OUTLINE
    return img
