"""Icon rendering toolkit: 2D signed-distance shapes, small 3D voxel objects, and a framed canvas.

Shared by `icons.py` (item + spell icons). Everything goes through `vox.compose`, so icons get the
same look as the sprites: per-pixel shade -> 5-shade palette ramp with 4x4 Bayer dithering and a
1 px dark outline.

Coordinates
- 2D shapes (`Sh`) live in canvas units: the icon spans [-1, 1] on both axes, Y points UP.
- 3D objects (`S3`) are modelled upright (z up, the viewer looks along +y, so -y is "front"),
  then turned with `view()` and auto-fitted into the icon by `Canvas.obj`.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import vox  # noqa: E402
from vox import compose, rot_x, rot_y, rot_z, sd_box, sd_capsule, sd_ellipsoid  # noqa: E402

# --- palette -----------------------------------------------------------------------------------

RAMPS = {
    # shared with gear.py (same values, so items match the paper-doll layers)
    "linen": ["#2b241b", "#4a3f30", "#6e5f48", "#968266", "#bba686"],
    "chain": ["#141517", "#2b2e33", "#4a4f56", "#727981", "#a1a8b0"],
    "plate": ["#1d2024", "#3e444b", "#67707a", "#9aa4ae", "#d4dbe1"],
    "mage": ["#160f22", "#2a1b40", "#422c63", "#5f4289", "#8364b2"],
    "redmark": ["#2a0606", "#4f0c0c", "#7a1612", "#a5241a", "#cf3a26"],
    "stone": ["#1c1c1f", "#34343a", "#4f4f55", "#6e6d70", "#918e8c"],
    # icon backgrounds by item quality (dark, so the item pops)
    "q1": ["#0e0e0e", "#181818", "#232323", "#2f2f2f", "#404040"],
    "q2": ["#120f0b", "#1d1913", "#29231b", "#383024", "#4c4130"],
    "q3": ["#0a130a", "#112011", "#183018", "#224222", "#305c2e"],
    "q4": ["#0a101c", "#101c30", "#182a48", "#223b62", "#335585"],
    "q5": ["#171005", "#271b08", "#3c2b0d", "#563e12", "#7a5a1c"],
    "q6": ["#130a1b", "#20112e", "#301a46", "#43265f", "#5f3787"],
    # spell schools
    "fire": ["#2a0a04", "#6b1a06", "#b8420c", "#f08a1c", "#ffd860"],
    "flame": ["#a33a08", "#e0701a", "#ffb030", "#ffe080", "#fff8d8"],
    "ice": ["#081830", "#1a4474", "#3a80bc", "#84c2ea", "#e4f8ff"],
    "snow": ["#4a7aa8", "#7aa8d0", "#a8cce8", "#d4ecfa", "#ffffff"],
    "holy": ["#2a1a04", "#704c10", "#c09028", "#f0d068", "#fff6cc"],
    "light": ["#a07a28", "#d0aa48", "#f0d478", "#fff0b0", "#ffffff"],
    "shadow": ["#10051a", "#2e0e48", "#58208a", "#9450c8", "#d0a0f0"],
    "void": ["#050208", "#0e0614", "#180a22", "#241030", "#301640"],
    "poison": ["#081604", "#1c3e0e", "#3a7418", "#76b430", "#c4ea78"],
    "arcane": ["#080a28", "#1a226a", "#3046aa", "#6686de", "#b8ccff"],
    "phys": ["#120e0a", "#261e16", "#3e3226", "#5a4a38", "#7e6a50"],
    "blood": ["#1e0204", "#4a060a", "#86101a", "#c42a2a", "#f06a5a"],
    # materials
    "copper": ["#2a1408", "#5a2c12", "#8e4a1e", "#c07034", "#eaa462"],
    "ruby": ["#2a0408", "#6a0c16", "#b01e28", "#e8504a", "#ffb4a4"],
    "sapphire": ["#040a2a", "#0c2470", "#1e4ab8", "#4c8ef0", "#b4dcff"],
    "emerald": ["#032010", "#0a4a22", "#16803c", "#3cc068", "#acf2bc"],
    "topaz": ["#2a1a02", "#6a4206", "#b07a10", "#e8b832", "#fff2a4"],
    "amethyst": ["#1a0626", "#42106a", "#7426b0", "#a85ae0", "#e2bcff"],
    "teal": ["#021c1e", "#06484c", "#108086", "#38c0c0", "#acf4ee"],
    "glass": ["#1a2226", "#3a4a52", "#6a8088", "#a8c0c8", "#eef8fa"],
    "paper": ["#3a2e1c", "#6e5a3a", "#a88e60", "#d8c49a", "#f6ecd0"],
    "bone": ["#2e2618", "#5e5236", "#948660", "#c8bc94", "#f2ecd4"],
    "meat": ["#2a0c06", "#5a1e10", "#8e3a1e", "#c0643a", "#e8a070"],
    "crust": ["#2e1a08", "#6a4012", "#a8701e", "#d8a040", "#f6d688"],
    "leaf": ["#0e1e06", "#224a10", "#3c7a1e", "#68b030", "#aae474"],
    "fur": ["#1e140c", "#3e2a1a", "#6a4a30", "#96704a", "#c29c72"],
    "chitin": ["#0e140a", "#222e16", "#3c4a24", "#5e6c36", "#8a9450"],
    "pink": ["#2a0a14", "#6a1430", "#b0285a", "#e05888", "#ffaccc"],
    "flesh": ["#2a1014", "#5e2830", "#96485a", "#c87888", "#f0b0b8"],
    "cloth_g": ["#0e1a0e", "#1e3a1c", "#305a2c", "#4a7e40", "#6ea860"],
    "cloth_b": ["#0c1424", "#1a2c4c", "#2c4a7a", "#4268a6", "#6890cc"],
    "dark": ["#060504", "#0e0c0a", "#181410", "#221c16", "#2e261e"],
    "cheese": ["#3a2a04", "#7a5a0a", "#c09418", "#ecc640", "#fff09a"],
}


def register(ramps: dict) -> None:
    """Adds ramps to vox's palette (keeping existing names) and rebuilds its lookup tables."""
    for k, v in ramps.items():
        vox.RAMPS.setdefault(k, v)
    vox.RAMP_RGB = {k: np.array([vox.hex_rgb(c) for c in v], dtype=np.uint8) for k, v in vox.RAMPS.items()}
    vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
    vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])


register(RAMPS)


def rid(name: str) -> int:
    return vox.RAMP_IDS[name]


def rgb(name: str, level: int) -> tuple[int, int, int]:
    return tuple(int(c) for c in vox.RAMP_RGB[name][level])


# --- 2D shapes ------------------------------------------------------------------------------------


class Sh:
    """2D signed distance shape: `sh(X, Y)` -> distance (negative inside). | union, - cut, & intersect."""

    def __init__(self, f):
        self.f = f

    def __call__(self, X, Y):
        return self.f(X, Y)

    def __or__(self, o):
        return Sh(lambda X, Y: np.minimum(self.f(X, Y), o.f(X, Y)))

    def __sub__(self, o):
        return Sh(lambda X, Y: np.maximum(self.f(X, Y), -o.f(X, Y)))

    def __and__(self, o):
        return Sh(lambda X, Y: np.maximum(self.f(X, Y), o.f(X, Y)))

    def move(self, dx, dy):
        return Sh(lambda X, Y: self.f(X - dx, Y - dy))

    def rot(self, a, cx=0.0, cy=0.0):
        """Rotated counter-clockwise by `a` radians around (cx, cy)."""
        c, s = math.cos(a), math.sin(a)
        return Sh(lambda X, Y: self.f(cx + c * (X - cx) + s * (Y - cy), cy - s * (X - cx) + c * (Y - cy)))

    def scale(self, k, cx=0.0, cy=0.0):
        return Sh(lambda X, Y: self.f(cx + (X - cx) / k, cy + (Y - cy) / k) * k)

    def grow(self, r):
        return Sh(lambda X, Y: self.f(X, Y) - r)

    def shell(self, th):
        return Sh(lambda X, Y: np.abs(self.f(X, Y)) - th)

    def warp(self, fn):
        return Sh(lambda X, Y: self.f(*fn(X, Y)))

    def mirror_x(self):
        return Sh(lambda X, Y: self.f(np.abs(X), Y))


def union(*shapes) -> Sh:
    out = shapes[0]
    for s in shapes[1:]:
        out = out | s
    return out


def circle(cx, cy, r) -> Sh:
    return Sh(lambda X, Y: np.hypot(X - cx, Y - cy) - r)


def ellipse(cx, cy, rx, ry) -> Sh:
    def f(X, Y):
        qx, qy = (X - cx) / rx, (Y - cy) / ry
        k0 = np.hypot(qx, qy)
        k1 = np.hypot(qx / rx, qy / ry)
        return k0 * (k0 - 1.0) / np.maximum(k1, 1e-6)

    return Sh(f)


def rect(cx, cy, hw, hh, a=0.0, rr=0.0) -> Sh:
    def f(X, Y):
        qx, qy = np.abs(X - cx) - (hw - rr), np.abs(Y - cy) - (hh - rr)
        return np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - rr

    s = Sh(f)
    return s.rot(a, cx, cy) if a else s


def seg(ax, ay, bx, by, r) -> Sh:
    def f(X, Y):
        px, py, ex, ey = X - ax, Y - ay, bx - ax, by - ay
        h = np.clip((px * ex + py * ey) / max(ex * ex + ey * ey, 1e-9), 0, 1)
        return np.hypot(px - ex * h, py - ey * h) - r

    return Sh(f)


def _round_cone(comps, a, b, r1, r2):
    """iq's round cone / uneven capsule, any dimension; comps = list of coordinate arrays."""
    ba = [bb - aa for aa, bb in zip(a, b)]
    l2 = sum(v * v for v in ba)
    rr = r1 - r2
    a2 = l2 - rr * rr
    il2 = 1.0 / l2
    pa = [c - aa for c, aa in zip(comps, a)]
    y = sum(p * v for p, v in zip(pa, ba))
    z = y - l2
    x2 = sum((p * l2 - v * y) ** 2 for p, v in zip(pa, ba))
    y2 = y * y * l2
    z2 = z * z * l2
    k = math.copysign(1.0, rr) * rr * rr * x2
    d_top = np.sqrt(x2 + z2) * il2 - r2
    d_bot = np.sqrt(x2 + y2) * il2 - r1
    d_mid = (np.sqrt(np.maximum(x2 * a2 * il2, 0)) + y * rr) * il2 - r1
    return np.where(np.sign(z) * a2 * z2 > k, d_top, np.where(np.sign(y) * a2 * y2 < k, d_bot, d_mid))


def taper(ax, ay, bx, by, ra, rb) -> Sh:
    """Capsule from (ax, ay) radius ra to (bx, by) radius rb."""
    return Sh(lambda X, Y: _round_cone([X, Y], (ax, ay), (bx, by), ra, rb))


def poly(pts) -> Sh:
    V = np.asarray(pts, float)

    def f(X, Y):
        d = (X - V[0, 0]) ** 2 + (Y - V[0, 1]) ** 2
        s = np.ones_like(X)
        j = len(V) - 1
        for i in range(len(V)):
            ex, ey = V[j] - V[i]
            wx, wy = X - V[i, 0], Y - V[i, 1]
            h = np.clip((wx * ex + wy * ey) / (ex * ex + ey * ey), 0, 1)
            d = np.minimum(d, (wx - ex * h) ** 2 + (wy - ey * h) ** 2)
            c1, c2, c3 = Y >= V[i, 1], Y < V[j, 1], ex * wy > ey * wx
            s = np.where((c1 & c2 & c3) | (~c1 & ~c2 & ~c3), -s, s)
            j = i
        return s * np.sqrt(d)

    return Sh(f)


def star(cx, cy, ro, ri, n, a0=math.pi / 2) -> Sh:
    pts = []
    for i in range(2 * n):
        a = a0 + i * math.pi / n
        r = ro if i % 2 == 0 else ri
        pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
    return poly(pts)


def arc(cx, cy, r, a0, a1, th) -> Sh:
    """Ring segment between angles a0..a1 (radians, CCW), thickness 2*th, round caps."""
    mid, half = (a0 + a1) / 2, (a1 - a0) / 2
    sc, cc = math.sin(half), math.cos(half)

    def f(X, Y):
        # rotate so the arc is symmetric around +y (iq sdArc)
        c, s = math.cos(math.pi / 2 - mid), math.sin(math.pi / 2 - mid)
        px = c * (X - cx) - s * (Y - cy)
        py = s * (X - cx) + c * (Y - cy)
        px = np.abs(px)
        inside = cc * px > sc * py
        d_end = np.hypot(px - sc * r, py - cc * r)
        d_ring = np.abs(np.hypot(px, py) - r)
        return np.where(inside, d_end, d_ring) - th

    return Sh(f)


def polyline(pts, r) -> Sh:
    return union(*[seg(*pts[i], *pts[i + 1], r) for i in range(len(pts) - 1)])


def bbox2(sh: Sh, lim=2.0, step=0.01):
    """(x0, x1, y0, y1) of where sh < 0, sampled on a grid."""
    g = np.arange(-lim, lim, step)
    X, Y = np.meshgrid(g, g)
    m = sh(X, Y) < 0
    if not m.any():
        return (-0.1, 0.1, -0.1, 0.1)
    xs, ys = X[m], Y[m]
    return (xs.min() - step, xs.max() + step, ys.min() - step, ys.max() + step)


# --- 3D objects --------------------------------------------------------------------------------------


class S3:
    """3D primitive: sdf `f(P)` (P: (N,3)), bounds, palette ramp and surface options.
    spec: specular strength (metal, gems, glass); emit: fixed shade (glowing); tone: f(P) -> shade multiplier."""

    def __init__(self, f, lo, hi, ramp, spec=0.0, emit=None, tone=None):
        self.f, self.lo, self.hi = f, np.asarray(lo, float), np.asarray(hi, float)
        self.ramp, self.spec, self.emit, self.tone = ramp, spec, emit, tone

    def at(self, o=(0, 0, 0), R=None) -> "S3":
        """Placed copy: world = o + R @ local."""
        o = np.asarray(o, float)
        R = np.eye(3) if R is None else np.asarray(R, float)
        f, tone = self.f, self.tone
        c = np.array([[x, y, z] for x in (self.lo[0], self.hi[0]) for y in (self.lo[1], self.hi[1]) for z in (self.lo[2], self.hi[2])])
        w = c @ R.T + o
        t2 = (lambda P: tone((P - o) @ R)) if tone else None
        return S3(lambda P: f((P - o) @ R), w.min(0), w.max(0), self.ramp, self.spec, self.emit, t2)

    def opts(self, **kw) -> "S3":
        s = S3(self.f, self.lo, self.hi, self.ramp, self.spec, self.emit, self.tone)
        for k, v in kw.items():
            setattr(s, k, v)
        return s


def _v(a):
    return np.asarray(a, float)


def box(c, h, ramp, **kw):
    c, h = _v(c), _v(h)
    return S3(lambda P: sd_box(P, c, h), c - h, c + h, ramp, **kw)


def rbox(c, h, r, ramp, **kw):
    c, h = _v(c), _v(h)
    return S3(lambda P: sd_box(P, c, h - r) - r, c - h, c + h, ramp, **kw)


def cap(a, b, r, ramp, **kw):
    a, b = _v(a), _v(b)
    return S3(lambda P: sd_capsule(P, a, b, r), np.minimum(a, b) - r, np.maximum(a, b) + r, ramp, **kw)


def cone(a, b, ra, rb, ramp, **kw):
    """Round cone from a (radius ra) to b (radius rb)."""
    a, b = _v(a), _v(b)
    r = max(ra, rb)
    return S3(lambda P: _round_cone([P[:, 0], P[:, 1], P[:, 2]], a, b, ra, rb), np.minimum(a, b) - r, np.maximum(a, b) + r, ramp, **kw)


def ell(c, rad, ramp, **kw):
    c, rad = _v(c), _v(rad)
    return S3(lambda P: sd_ellipsoid(P, c, rad), c - rad, c + rad, ramp, **kw)


def sphere(c, r, ramp, **kw):
    c = _v(c)
    return S3(lambda P: np.linalg.norm(P - c, axis=1) - r, c - r, c + r, ramp, **kw)


def cyl(a, b, r, ramp, **kw):
    """Capped cylinder between a and b."""
    a, b = _v(a), _v(b)

    def f(P):
        ba = b - a
        pa = P - a
        baba = ba @ ba
        paba = pa @ ba
        x = np.linalg.norm(pa * baba - np.outer(paba, ba), axis=1) - r * baba
        y = np.abs(paba - baba * 0.5) - baba * 0.5
        x2, y2 = x * x, y * y * baba
        d = np.where(np.maximum(x, y) < 0, -np.minimum(x2, y2), np.where(x > 0, x2, 0) + np.where(y > 0, y2, 0))
        return np.sign(d) * np.sqrt(np.abs(d)) / baba

    return S3(f, np.minimum(a, b) - r, np.maximum(a, b) + r, ramp, **kw)


def torus(c, R, r, ramp, **kw):
    """Torus around the z axis through c (ring in the xy plane)."""
    c = _v(c)

    def f(P):
        q = P - c
        return np.hypot(np.hypot(q[:, 0], q[:, 1]) - R, q[:, 2]) - r

    return S3(f, c - [R + r, R + r, r], c + [R + r, R + r, r], ramp, **kw)


def lathe(sh: Sh, ramp, **kw):
    """Surface of revolution around z of a 2D profile given in (radius, z)."""
    x0, x1, z0, z1 = bbox2(sh)
    rmax = max(abs(x0), abs(x1))

    def f(P):
        return sh(np.hypot(P[:, 0], P[:, 1]), P[:, 2])

    return S3(f, (-rmax, -rmax, z0), (rmax, rmax, z1), ramp, **kw)


def ext(sh: Sh, t, ramp, rnd=0.0, **kw):
    """2D profile in the x-z plane extruded +-t along y, edges rounded by `rnd`."""
    x0, x1, z0, z1 = bbox2(sh)

    def f(P):
        d2 = sh(P[:, 0], P[:, 2]) + rnd
        wy = np.abs(P[:, 1]) - (t - rnd)
        return np.minimum(np.maximum(d2, wy), 0) + np.hypot(np.maximum(d2, 0), np.maximum(wy, 0)) - rnd

    return S3(f, (x0, -t, z0), (x1, t, z1), ramp, **kw)


def convex(planes, ramp, lim=1.5, bounds=None, facet=0.0, **kw):
    """Convex polyhedron: intersection of half-spaces n.p <= d (n normalised here); bounds are
    sampled on a grid in [-lim, lim]^3 unless given. `facet` > 0 gives every face its own
    brightness, the oldschool way to make cut gems sparkle."""
    N = np.array([np.asarray(n, float) / np.linalg.norm(n) for n, _ in planes])
    D = np.array([d / np.linalg.norm(n) for n, d in planes])

    def f(P):
        return (P @ N.T - D).max(axis=1)

    if facet and "tone" not in kw:
        kw["tone"] = lambda P: 1 + facet * (((P @ N.T - D).argmax(axis=1) * 0.618) % 1 - 0.5) * 2
    if bounds is not None:
        return S3(f, bounds[0], bounds[1], ramp, **kw)
    g = np.linspace(-lim, lim, 61)
    G = np.stack(np.meshgrid(g, g, g, indexing="ij"), -1).reshape(-1, 3)
    inside = G[f(G) <= 0]
    lo, hi = (inside.min(0) - 0.06, inside.max(0) + 0.06) if len(inside) else ((-lim,) * 3, (lim,) * 3)
    return S3(f, lo, hi, ramp, **kw)


def rough(s: S3, amp, freq=6.0, seed=0) -> S3:
    """Lumpy surface: sdf displaced by a few seeded sine waves."""
    rng = np.random.default_rng(seed)
    dirs = rng.normal(size=(5, 3))
    dirs /= np.linalg.norm(dirs, axis=1, keepdims=True)
    ph = rng.uniform(0, 2 * math.pi, 5)
    f = s.f

    def g(P):
        n = sum(np.sin(P @ dirs[i] * freq * (1 + 0.37 * i) + ph[i]) for i in range(5)) / 5
        return f(P) + amp * n

    return S3(g, s.lo - amp, s.hi + amp, s.ramp, s.spec, s.emit, s.tone)


def gem_cut(c, r, ramp, crown=0.45, pav=0.9, n=8, table=0.55, rot=0.0, **kw):
    """Brilliant-ish faceted gem (girdle radius r): table, n crown facets, n pavilion facets."""
    planes = [((0, 0, 1), crown * r * table)]
    for i in range(n):
        a = rot + 2 * math.pi * i / n
        d = (math.cos(a), math.sin(a))
        planes.append(((d[0], d[1], 0.0), r))  # girdle
        planes.append(((d[0] * crown, d[1] * crown, 1.0 - table * 0.3), r * crown))
        a2 = a + math.pi / n
        planes.append(((math.cos(a2) * pav, math.sin(a2) * pav, -0.62), r * pav))
    s = convex(planes, ramp, lim=r * 1.6, spec=kw.pop("spec", 0.9), facet=kw.pop("facet", 0.3), **kw)
    return s.at(c)


def facet_rock(c, r, ramp, n=14, seed=0, squash=(1, 1, 1), **kw):
    """Rough-cut stone: intersection of randomly oriented planes around a centre."""
    rng = np.random.default_rng(seed)
    planes = []
    for _ in range(n):
        v = rng.normal(size=3)
        v /= np.linalg.norm(v)
        v = v / np.asarray(squash, float)
        planes.append((v, r * rng.uniform(0.75, 1.0) * np.linalg.norm(v)))
    return convex(planes, ramp, lim=r * 2.2, facet=kw.pop("facet", 0.2), **kw).at(c)


# tones (shade multipliers, evaluated on local points)


def t_wood(P):
    return 1 + 0.12 * np.sin(P[:, 2] * 48 + 4 * np.sin(P[:, 0] * 9 + P[:, 1] * 7))


def t_grain(freq=40, amp=0.12, axis=0):
    return lambda P: 1 + amp * np.sin(P[:, axis] * freq + 3 * np.sin(P[:, (axis + 1) % 3] * 7))


def t_chain(P):
    k = (np.floor(P[:, 0] / 0.07) + np.floor(P[:, 1] / 0.07) + np.floor(P[:, 2] / 0.07)) % 2
    return 0.78 + 0.38 * k


def t_stripes(freq=14, amp=0.25, axis=2):
    return lambda P: 1 - amp * (np.sin(P[:, axis] * freq) > 0.6)


def t_noise(amp=0.15, freq=18, seed=0):
    rng = np.random.default_rng(seed)
    dirs = rng.normal(size=(3, 3))
    ph = rng.uniform(0, 6.28, 3)
    return lambda P: 1 + amp * sum(np.sin(P @ dirs[i] * freq + ph[i]) for i in range(3)) / 3


# --- 3D rendering ------------------------------------------------------------------------------------

LIGHT3 = np.array([-0.55, -0.5, 0.67])  # from upper left, in front
LIGHT3 = LIGHT3 / np.linalg.norm(LIGHT3)


def view(diag=0.0, tilt=20.0, yaw=0.0) -> np.ndarray:
    """Object -> view rotation (degrees): yaw about z, tilt the top toward the viewer, then rotate in
    the screen plane (+diag turns the object's top to the right)."""
    return rot_y(math.radians(diag)) @ rot_x(math.radians(tilt)) @ rot_z(math.radians(yaw))


def voxelize(prims, vs, dilate=0.35):
    """Surface voxels of each primitive; `dilate` (in voxels) keeps parts thinner than a voxel."""
    pts, nrm, ramp, spec, emit, tone = [], [], [], [], [], []
    for pr in prims:
        lo, hi = pr.lo - 2 * vs, pr.hi + 2 * vs
        axes = [np.arange(lo[i], hi[i] + vs, vs) for i in range(3)]
        g = np.stack(np.meshgrid(*axes, indexing="ij"), -1).reshape(-1, 3)
        d = pr.f(g)
        sh = g[(d <= dilate * vs) & (d > -1.6 * vs)]
        if len(sh) == 0:
            continue
        e = vs * 0.5
        n = np.stack([pr.f(sh + o) - pr.f(sh - o) for o in (np.array([e, 0, 0]), np.array([0, e, 0]), np.array([0, 0, e]))], -1)
        n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-9)
        pts.append(sh)
        nrm.append(n)
        ramp.append(np.full(len(sh), rid(pr.ramp)))
        spec.append(np.full(len(sh), pr.spec))
        emit.append(np.full(len(sh), -1.0 if pr.emit is None else pr.emit))
        tone.append(pr.tone(sh) if pr.tone else np.ones(len(sh)))
    if not pts:
        z = np.zeros((0, 3))
        return z, z, np.zeros(0, int), np.zeros(0), np.zeros(0), np.zeros(0)
    return tuple(np.concatenate(a) for a in (pts, nrm, ramp, spec, emit, tone))


def render3d(prims, rot, n, fit=0.84, off=(0.0, 0.0), scale=None, ambient=0.2):
    """Z-buffered orthographic render: (shade, ramp, mask, px_per_unit). The object is scaled so its
    larger screen extent is `fit * n` px (or uses `scale` px/unit) and centred, shifted by `off`
    (canvas units)."""
    if scale is None:
        lo = np.min([p.lo for p in prims], 0)
        hi = np.max([p.hi for p in prims], 0)
        coarse = voxelize(prims, float(np.max(hi - lo)) / 60, dilate=1.0)[0] @ rot.T
        w = np.ptp(coarse[:, 0]) + 1e-6
        h = np.ptp(coarse[:, 2]) + 1e-6
        scale = fit * n / max(w, h)
    pts, nrm, ramp, spec, emit, tone = voxelize(prims, 0.5 / scale)
    q, nn = pts @ rot.T, nrm @ rot.T
    sx, sy, depth = q[:, 0] * scale, -q[:, 2] * scale, -q[:, 1]
    sx += n / 2 - (sx.min() + sx.max()) / 2 + off[0] * n / 2
    sy += n / 2 - (sy.min() + sy.max()) / 2 - off[1] * n / 2
    lam = np.clip(nn @ LIGHT3, 0, 1)
    refl = 2 * (nn @ LIGHT3)[:, None] * nn - LIGHT3
    sp = np.clip(-refl[:, 1], 0, 1) ** 10 * spec
    shade = (ambient + (1 - ambient) * lam) * tone + sp
    glow = emit >= 0
    shade[glow] = emit[glow] + 0.25 * (lam[glow] - 0.5) + sp[glow] * 0.5
    x, y = np.floor(sx).astype(int), np.floor(sy).astype(int)
    ok = (x >= 0) & (x < n) & (y >= 0) & (y < n)
    order = np.argsort(depth[ok])
    xx, yy = x[ok][order], y[ok][order]
    zb = np.full((n, n), -1e9)
    S = np.zeros((n, n))
    Rm = np.full((n, n), -1, int)
    S[yy, xx] = shade[ok][order]
    Rm[yy, xx] = ramp[ok][order]
    zb[yy, xx] = depth[ok][order]
    return S, Rm, Rm >= 0, scale


# --- canvas ------------------------------------------------------------------------------------------

L2 = np.array([-0.55, 0.55, 0.63])  # 2D light: upper left (Y up)
L2 = L2 / np.linalg.norm(L2)


class Canvas:
    """An icon: dithered radial background in one ramp, painted layers on top, then a frame."""

    def __init__(self, n, ramp, lo=0.06, hi=0.52, center=(0.0, 0.15), spread=1.35):
        self.n = n
        yy, xx = np.indices((n, n))
        self.X = (xx + 0.5) / n * 2 - 1
        self.Y = 1 - (yy + 0.5) / n * 2
        d = np.hypot(self.X - center[0], self.Y - center[1]) / spread
        self.bg = lo + (hi - lo) * np.clip(1 - d, 0, 1) ** 1.3
        self.bg_ramp = ramp
        self.layers = []
        self.px = 2.0 / n  # canvas units per pixel

    # background effects
    def glow(self, sh: Sh, amount=0.35, radius=0.4):
        d = sh(self.X, self.Y)
        self.bg = self.bg + amount * np.clip(1 - np.maximum(d, 0) / radius, 0, 1) ** 2

    def shadow(self, mask, dx=1, dy=2, amount=0.22):
        m = np.zeros_like(mask)
        m[dy:, dx:] = mask[: self.n - dy, : self.n - dx]
        self.bg = self.bg - amount * m

    # layers
    def put(self, shade, ramp, mask, outline=True):
        r = np.full(mask.shape, rid(ramp)) if isinstance(ramp, str) else ramp
        img = compose(shade, r, mask)
        if not outline:
            img[~mask] = 0
        self.layers.append(img)
        return mask

    def paint(self, sh: Sh, ramp, mode="lit", bevel=0.14, lo=0.35, hi=1.0, base=0.0, outline=True, shade=None):
        """Fill a 2D shape. mode lit: bevelled relief lit from the upper left; glow: brighter toward
        the inside (flames, magic); flat: constant `lo`."""
        d = sh(self.X, self.Y)
        mask = d < 0
        if not mask.any():
            return mask
        if shade is None:
            if mode == "lit":
                shade = lit2d(d, bevel, self.px)
            elif mode == "glow":
                shade = lo + (hi - lo) * np.clip(-d / bevel, 0, 1) ** 0.8
            else:
                shade = np.full(d.shape, lo)
        return self.put(shade + base, ramp, mask, outline)

    def obj(self, prims, rot, fit=0.84, off=(0.0, 0.0), shadow=True, outline=True, scale=None, ambient=0.2):
        S, R, M, _ = render3d(prims, rot, self.n, fit, off, scale, ambient)
        if shadow:
            self.shadow(M)
        self.put(S, R, M, outline)
        return M

    def image(self) -> np.ndarray:
        out = compose(self.bg, np.full(self.bg.shape, rid(self.bg_ramp)), np.ones(self.bg.shape, bool))
        for img in self.layers:
            a = img[..., 3] > 0
            out[a] = img[a]
        return out

    def finish(self, light, dark, outer=(10, 8, 6)) -> np.ndarray:
        """Final RGBA with a 2 px frame: dark outer line, bevelled inner line (light top/left)."""
        img = self.image()
        n = self.n
        img[1, 1 : n - 1, :3] = light
        img[1 : n - 1, 1, :3] = light
        img[n - 2, 1 : n - 1, :3] = dark
        img[1 : n - 1, n - 2, :3] = dark
        img[0, :, :3] = outer
        img[n - 1, :, :3] = outer
        img[:, 0, :3] = outer
        img[:, n - 1, :3] = outer
        img[..., 3] = 255
        return img


def lit2d(d, bevel, px):
    """Shade of a bevelled 2D shape: height rises over `bevel` units from the edge, lit by L2."""
    t = np.clip(-d / bevel, 0, 1)
    H = (1 - (1 - t) ** 2) * (bevel / px)
    gy, gx = np.gradient(H)
    nx, ny, nz = -gx, gy, np.ones_like(H)
    ln = np.sqrt(nx * nx + ny * ny + 1)
    lam = np.clip((nx * L2[0] + ny * L2[1] + nz * L2[2]) / ln, 0, 1)
    return 0.22 + 0.78 * lam
