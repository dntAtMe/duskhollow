"""Weapon smears: the arc a blade sweeps between two frames, as its own sprite layer.

Oldschool motion trails: the swept surface of the blade (from `base` to `tip`, in the weapon
bone's rest frame) is sampled between the previous frame and this one, splatted like voxels with
the game's projection, depth-tested against the body (a swing passing behind the back is hidden)
and drawn in 3 dithered bands from dark oxblood (oldest) to dull bone (newest), thinning to just
the tip at its tail. Frames where the blade barely moves stay empty.

The layer is exported as `<model>_smear` next to the unit's script with the same attack
animations plus an empty `stance` (so any other animation shows nothing); the client draws it
over the unit (`unit::SMEAR`).
"""

from __future__ import annotations

import copy

import numpy as np

import sheet
import vox

# dark oxblood -> crimson -> rust -> dull bone (no near-white: docs/world.md)
COLORS = np.array([(0, 0, 0), (74, 16, 14), (134, 34, 24), (176, 92, 62), (196, 160, 122)], dtype=np.uint8)
MIN_TRAVEL = 0.32  # world units the tip must move between frames to leave a smear
SPAN = 1.0  # frames of motion covered by one smear (1 = since the previous frame)
DEPTH_EPS = 0.04


def blade_from_part(model, bone: str = "sword", base_frac: float = 0.3):
    """(base, tip) of a weapon hanging down from its bone pivot in rest pose (lowest voxel = tip)."""
    pts = model.parts[bone][0]
    tip = pts[np.argmin(pts[:, 2])]
    pivot = np.asarray(model.bones[bone].pivot, float)
    return pivot + (tip - pivot) * base_frac, tip


def _world(world, bone_pivot, bone: str, pts: np.ndarray):
    rot, origin = world[bone]
    return (pts - bone_pivot) @ rot.T + origin


def _sample(model, fn, frames: int, u: float, bone: str, base, tip, along: np.ndarray):
    t = u / max(frames - 1, 1)
    rot, off, root_rot = fn(t)
    world = model.pose(rot, off, root_rot)
    pts = base[None, :] + (tip - base)[None, :] * along[:, None]
    return _world(world, np.asarray(model.bones[bone].pivot, float), bone, pts)


def tip_travel(model, fn, frames: int, f: int, bone: str, base, tip) -> float:
    a = _sample(model, fn, frames, f - 1, bone, base, tip, np.array([1.0]))
    b = _sample(model, fn, frames, f, bone, base, tip, np.array([1.0]))
    return float(np.linalg.norm(a - b))


def render_frame(model, fn, frames, f, bone, base, tip, facing, size, foot, scale, body_z=None, hide=None):
    """RGBA smear for frame `f` (motion from f - SPAN to f); `hide`: pixels to leave clear (the
    weapon itself)."""
    steps, along_n = 160, 24
    img = np.zeros((size, size, 4), np.uint8)
    inten = np.zeros((size, size))
    zbuf = np.full((size, size), -1e9)
    rz = vox.rot_z(facing)
    for i in range(steps + 1):
        w = i / steps  # 0 = oldest, 1 = newest
        u = f - SPAN * (1 - w)
        a0 = 0.95 - 0.65 * w**0.7  # tail: only the tip; head: most of the blade
        along = np.linspace(a0, 1.0, along_n)
        p = _sample(model, fn, frames, u, bone, base, tip, along) @ rz.T * scale
        sx, sy, depth = vox.project(p)
        x = np.round(sx + foot[0]).astype(int)
        y = np.round(sy + foot[1]).astype(int)
        for dx, dy in ((0, 0), (1, 0), (0, 1), (1, 1)):
            xx, yy = x + dx, y + dy
            ok = (xx >= 0) & (xx < size) & (yy >= 0) & (yy < size)
            xx, yy, dd = xx[ok], yy[ok], depth[ok]
            np.maximum.at(inten, (yy, xx), w)
            np.maximum.at(zbuf, (yy, xx), dd)
    mask = inten > 0
    if body_z is not None:
        mask &= ~(body_z > zbuf + DEPTH_EPS)
    if hide is not None:
        mask &= ~hide
    ty, tx = np.indices((size, size))
    thr = vox.BAYER4[ty % 4, tx % 4]
    level = np.clip(np.floor(inten * 3.4 + (thr - 0.5) * 1.1 + 0.6), 0, 4).astype(int)
    mask &= level > 0
    img[mask, :3] = COLORS[level[mask]]
    img[mask, 3] = 255
    return img


def render_layer(model, anims, frame: int, foot, scale: float, base, tip, hits: dict, bone: str = "sword",
                 smear_anims=("swing", "swing2", "swing3"), depth_model=None, verbose=False):
    """{anim: [[rgba per dir] per frame]} for the smear layer (`stance` + the attack anims): the
    strike frame (`hits`) and the follow-through frame after it, if the blade moves enough."""
    empty = np.zeros((frame, frame, 4), np.uint8)
    out = {"stance": [[empty] * 8]}
    depth_model = depth_model or model
    weapon = copy.copy(model)
    weapon.parts = {bone: model.parts[bone]}
    base, tip = np.asarray(base, float), np.asarray(tip, float)
    for name, fn, frames, _, _ in anims:
        if name not in smear_anims:
            continue
        per = []
        for f in range(frames):
            travel = tip_travel(model, fn, frames, f, bone, base, tip) if f > 0 else 0.0
            if verbose:
                print(f"  {name} f{f}: tip travel {travel:.2f}")
            if travel < MIN_TRAVEL or f not in (hits.get(name), hits.get(name, -9) + 1):
                per.append([empty] * 8)
                continue
            t = f / max(frames - 1, 1)
            rot, off, rr = fn(t * 0.999)
            world = depth_model.pose(rot, off, rr)
            dirs = []
            for d in range(8):
                facing = sheet.DIR_TO_ORIENTATION[d]
                bz, _, br = depth_model.raster(world, facing, frame, foot, scale)
                bz = np.where(br >= 0, bz, -1e9)
                _, _, wr = weapon.raster(world, facing, frame, foot, scale)
                dirs.append(render_frame(model, fn, frames, f, bone, base, tip, facing, frame, foot, scale, bz, wr >= 0))
            per.append(dirs)
        out[name] = per
    return out


def anims_for(anims, renders):
    """The anim table rows the smear layer has (same frames/durations as the unit's)."""
    rows = [("stance", None, 1, 100, "play_once")]
    rows += [a for a in anims if a[0] in renders and a[0] != "stance"]
    return rows
