"""Keyframed poses for attack animations: hold the wind-up, snap the strike, ease the recovery.

Sprite sheets show frames at a fixed rate (`duration / frames`), so timing lives in the poses: a
key per frame index, and the easing decides where in-between samples (sub-frame smear sampling,
fractional keys) fall. A pose maps bone -> (x, y, z) degrees, applied as rot_z @ rot_y @ rot_x in
the parent's frame (y: forward/back swing, z: yaw about the vertical, x: sideways), plus
"root" -> (dx, dy, dz) world offset of the pelvis and optional "root_rot" -> (x, y, z) degrees.
Keys inherit from the previous key, so each one lists only what changes.
"""

from __future__ import annotations

import math

from vox import rot_x, rot_y, rot_z

R = math.radians

EASE = {
    "lin": lambda k: k,
    "in": lambda k: k * k,  # accelerate into the key (strikes)
    "in3": lambda k: k * k * k,
    "out": lambda k: 1 - (1 - k) ** 2,  # decelerate into the key (wind-ups, settles)
    "out3": lambda k: 1 - (1 - k) ** 3,
    "io": lambda k: k * k * (3 - 2 * k),
}


def euler(v) -> "np.ndarray":  # noqa: F821
    x, y, z = v
    return rot_z(R(z)) @ rot_y(R(y)) @ rot_x(R(x))


def resolve(base: dict, keys: list) -> list:
    """[(pos, full pose, ease)] from cumulative key overrides."""
    out, pose = [], dict(base)
    for pos, over, ease in keys:
        pose = {**pose, **over}
        out.append((pos, pose, ease))
    return out


def blend(a: dict, b: dict, k: float) -> dict:
    keys = set(a) | set(b)
    zero = (0.0, 0.0, 0.0)
    return {n: tuple(p + (q - p) * k for p, q in zip(a.get(n, zero), b.get(n, zero))) for n in keys}


def keyed(frames: int, base: dict, keys: list):
    """fn(t) -> (rotations, root_offset, root_rot) for `sheet.render_all` (play_once sampling:
    frame f is t = f / (frames - 1)). `keys`: [(frame position, overrides, ease into this key)]."""
    full = resolve(base, keys)

    def pose_at(u: float) -> dict:
        if u <= full[0][0]:
            return full[0][1]
        for (p0, a, _), (p1, b, ease) in zip(full, full[1:]):
            if u <= p1:
                return blend(a, b, EASE[ease]((u - p0) / max(p1 - p0, 1e-6)))
        return full[-1][1]

    def fn(t: float):
        u = t * (frames - 1)
        if abs(u - round(u)) < 0.02:  # render_all samples t * 0.999
            u = round(u)
        pose = pose_at(u)
        rot = {n: euler(v) for n, v in pose.items() if n not in ("root", "root_rot")}
        root_rot = euler(pose["root_rot"]) if "root_rot" in pose else None
        return rot, pose.get("root", (0, 0, 0)), root_rot

    fn.frames = frames
    return fn


def retime(fn, frames: int, keys: list):
    """Re-times an existing fn(t) for `frames` play_once frames: `keys` = [(frame, source t)],
    linear in between, so a smooth source animation gets holds and snaps where the keys say."""

    def src(u: float) -> float:
        if u <= keys[0][0]:
            return keys[0][1]
        for (f0, t0), (f1, t1) in zip(keys, keys[1:]):
            if u <= f1:
                return t0 + (t1 - t0) * (u - f0) / max(f1 - f0, 1e-6)
        return keys[-1][1]

    def out(t: float):
        u = t * (frames - 1)
        if abs(u - round(u)) < 0.02:
            u = round(u)
        return fn(min(src(u), 0.999))

    return out
