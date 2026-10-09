"""Our own spell visual effects: `.sa` flipbooks and the particle texture atlas.

Usage (from repo root):
    python -I tools/artgen/spellfx.py              # all flipbooks + fx_particles.png + previews
    python -I tools/artgen/spellfx.py cast_001.sa  # only some flipbooks (+ previews)
    python -I tools/artgen/spellfx.py --atlas      # only the particle atlas

Everything is drawn procedurally from scratch: glowing sigils, noise flames,
particle bursts, rings, light columns, beams, lightning, slashes, whirlwinds... Each effect is a
float intensity field mapped through a short per-school palette ramp (black -> dark -> mid ->
bright -> white-hot) with 4x4 Bayer ordered dithering, i.e. the oldschool "limited palette" look.

Placement: kits place a flipbook's canvas by `anim_x/anim_y` (`data/spell_visuals.txt`). Each
`<name>.sa` keeps its ratio, canvas size, delay, loop range and frame count from
`spellfx_layout.json` (our layout data: per-frame bounding boxes, a brightness envelope and a
school ramp per flipbook), and each frame is drawn into its box. Frames are rendered at world
resolution (canvas / ratio) and blown up by `ratio` with nearest neighbour, so they stay crisp at
the client's 1/sqrt(ratio) draw scale.

Output:
- `assets/scripts/animation/<name>.sa`, `filename=sfx_<name>`:
- `assets/content/spellfx/sfx_*_<n>.png`: trimmed, palettized frames on opaque
  black (the client luma-keys those = additive look). Quest markers / arrows / the obelisk are
  drawn with real alpha instead.
- `assets/content/fx/fx_particles.png`: 128x128 atlas of 16 white 32x32 particle
  sprites (`sprite=<cell>` in `data/particles.txt`), alpha in 5 dithered steps.
- `assets/preview/spellfx_*.png`: contact sheets (gitignored).
"""

from __future__ import annotations

import json
import math
import sys
import zlib
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).parent))
from vox import BAYER4  # noqa: E402
from enviro import periodic_noise  # noqa: E402
import paths  # noqa: E402

ROOT = paths.ROOT
LAYOUT = Path(__file__).with_name("spellfx_layout.json")
OUT_FRAMES = paths.SPELLFX
OUT_SA = paths.ANIMATION
OUT_ATLAS = paths.FX / "fx_particles.png"
PREVIEW = paths.PREVIEW
TAU = 2 * math.pi

# --- palettes -------------------------------------------------------------------------------
# One ramp per school, black first (= transparent after luma keying), white-hot last.
RAMPS = {
    "fire": ["#000000", "#3a0a04", "#7a1a06", "#c23d0a", "#f07a14", "#ffc23a", "#fff2a8"],
    "frost": ["#000000", "#06163a", "#0d3a78", "#1f72b8", "#4fb0e8", "#a6e4ff", "#f0fbff"],
    "holy": ["#000000", "#2e1a04", "#6b4610", "#b0801c", "#e8bc3c", "#ffe68a", "#fffbe0"],
    "shadow": ["#000000", "#1a0628", "#3e0f5e", "#6c2a9a", "#a259d6", "#d6a2f5", "#f6e6ff"],
    "nature": ["#000000", "#06200a", "#10461a", "#23802c", "#4fc040", "#a8ec6c", "#effcc8"],
    "pink": ["#000000", "#2a0620", "#5e1048", "#a02478", "#e050a8", "#ff9ad4", "#ffe6f4"],
    "arcane": ["#000000", "#0a0a3a", "#1c1f80", "#3a4cc8", "#6f8af0", "#b4c4ff", "#f0f4ff"],
    "cyan": ["#000000", "#032222", "#085050", "#128a84", "#2ccabc", "#8af0e0", "#e6fffa"],
    "blood": ["#000000", "#2a0404", "#5c0a0a", "#9a1414", "#d83028", "#ff7a5c", "#ffd8c8"],
    "earth": ["#000000", "#1e1208", "#3e2814", "#6a4624", "#9a6c3a", "#c89a62", "#f0d4a0"],
    # Duskhollow palette (docs/world.md: dull bone, rust, ash; nothing near-white) for the
    # flipbooks of our spell kits (data/spell_visuals.txt).
    "bone": ["#000000", "#1e1410", "#3e2c22", "#6a5040", "#9a7c62", "#c4a888", "#e0ccaa"],
    "rust": ["#000000", "#2a0c06", "#561a0c", "#8a3414", "#bc5a22", "#e08a48", "#f0b880"],
    "ash": ["#000000", "#140c14", "#2c1e2a", "#4a3646", "#6e5466", "#94788a", "#b8a0ac"],
    # untinted grey: effects the kit recolours with `sprcolor` (aura_001, *_003d, *_008d...)
    "white": ["#000000", "#1c1c1c", "#444444", "#747474", "#a8a8a8", "#d8d8d8", "#ffffff"],
}
K = 7  # ramp length


def _rgb(h):
    return int(h[1:3], 16), int(h[3:5], 16), int(h[5:7], 16)


RAMP_RGB = {k: [_rgb(c) for c in v] for k, v in RAMPS.items()}


def auto_ramp(hue: float, sat: float) -> str:
    """School ramp for a layout entry's dominant colour."""
    if sat < 0.25:
        return "white"
    for upto, name in ((15, "blood"), (38, "fire"), (65, "holy"), (150, "nature"), (190, "cyan"),
                       (222, "frost"), (255, "arcane"), (295, "shadow"), (345, "pink"), (361, "blood")):
        if hue < upto:
            return name
    return "white"


# --- noise ----------------------------------------------------------------------------------


def vnoise(x, y, seed):
    """Smooth value noise, lattice spacing 1 (wraps every 256)."""
    return periodic_noise(np.asarray(x, float), np.asarray(y, float), 1, seed, period=256)


def fbm(x, y, seed, octaves=3):
    tot, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        tot = tot + amp * vnoise(x * 2**o, y * 2**o, seed + o)
        norm += amp
        amp *= 0.5
    return tot / norm


# --- drawing primitives on float fields (F[y, x]) -------------------------------------------


def _win(F, x0, y0, x1, y1):
    H, W = F.shape
    a, b = max(int(math.floor(x0)), 0), min(int(math.ceil(x1)) + 1, W)
    c, d = max(int(math.floor(y0)), 0), min(int(math.ceil(y1)) + 1, H)
    if a >= b or c >= d:
        return None
    ys, xs = np.mgrid[c:d, a:b] + 0.5
    return (slice(c, d), slice(a, b)), xs, ys


def dot(F, x, y, r, amp, sharp=1.0):
    """Soft disc: amp * (1 - d/r)^sharp, added."""
    r = max(r, 0.6)
    w = _win(F, x - r, y - r, x + r, y + r)
    if w is None or amp <= 0:
        return
    sl, xs, ys = w
    F[sl] += amp * np.clip(1 - np.hypot(xs - x, ys - y) / r, 0, 1) ** sharp


def glow(F, x, y, r, amp):
    """Gaussian blob (sigma r), added."""
    r = max(r, 0.5)
    w = _win(F, x - 2.5 * r, y - 2.5 * r, x + 2.5 * r, y + 2.5 * r)
    if w is None or amp <= 0:
        return
    sl, xs, ys = w
    F[sl] += amp * np.exp(-((xs - x) ** 2 + (ys - y) ** 2) / (r * r))


def seg(F, x0, y0, x1, y1, w, a0, a1=None):
    """Line with soft width w, amplitude a0 -> a1 along it, max-blended (polylines don't double)."""
    a1 = a0 if a1 is None else a1
    w = max(w, 0.6)
    win = _win(F, min(x0, x1) - w, min(y0, y1) - w, max(x0, x1) + w, max(y0, y1) + w)
    if win is None:
        return
    sl, xs, ys = win
    dx, dy = x1 - x0, y1 - y0
    l2 = dx * dx + dy * dy or 1e-6
    s = np.clip(((xs - x0) * dx + (ys - y0) * dy) / l2, 0, 1)
    d = np.hypot(xs - (x0 + s * dx), ys - (y0 + s * dy))
    F[sl] = np.maximum(F[sl], (a0 + (a1 - a0) * s) * np.clip(1 - d / w, 0, 1))


def poly(F, pts, w, amps):
    for i in range(len(pts) - 1):
        seg(F, pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1], w, amps[i], amps[i + 1])


def ellipse(X, Y, cx, cy, rx, ry):
    """(normalized radius, angle) of every pixel relative to an ellipse; angle 0 = right, y down."""
    u, v = (X - cx) / max(rx, 0.5), (Y - cy) / max(ry, 0.5)
    return np.hypot(u, v), np.arctan2(v, u)


def band(rho, scale, w):
    """Ring profile around rho == 1, width w in px (scale = px per unit rho)."""
    return np.clip(1 - np.abs(rho - 1) * scale / max(w, 0.6), 0, 1)


def smooth(t):
    t = min(max(t, 0.0), 1.0)
    return t * t * (3 - 2 * t)


# --- one animation --------------------------------------------------------------------------


class Fx:
    """World-resolution canvas + the layout's per-frame boxes (smoothed) and envelope."""

    def __init__(self, name: str, lay: dict):
        self.name, self.lay = name, lay
        self.r = r = lay["ratio"]
        self.W = W = -(-lay["size"] // r)
        self.n = len(lay["frames"])
        self.seed = zlib.crc32(name.encode()) & 0xFFFF
        self.rng = np.random.default_rng(self.seed)
        self.Y, self.X = np.mgrid[0:W, 0:W] + 0.5
        self.B = np.tile(BAYER4, (W // 4 + 1, W // 4 + 1))[:W, :W]
        raw = [None if b is None else np.array(b, float) / r for b in lay["boxes"]]
        self.boxes = []
        for i in range(self.n):
            if raw[i] is None:
                self.boxes.append(None)
                continue
            near = [raw[j] for j in range(max(0, i - 1), min(self.n, i + 2)) if raw[j] is not None]
            b = np.mean(near, axis=0) * 0.5 + raw[i] * 0.5
            cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
            self.boxes.append((cx, cy, max((b[2] - b[0]) / 2, 1.5), max((b[3] - b[1]) / 2, 1.5)))
        env = np.array(lay["env"] or [1.0] * self.n, float)
        self.env = np.clip(env / max(env.max(), 1e-6), 0, 1) ** 0.5
        live = [b for b in raw if b is not None]
        u = (min(b[0] for b in live), min(b[1] for b in live), max(b[2] for b in live), max(b[3] for b in live))
        self.union = ((u[0] + u[2]) / 2, (u[1] + u[3]) / 2, (u[2] - u[0]) / 2, (u[3] - u[1]) / 2)
        # full ordered dithering on big effects; tiny ones (ratio-4 runes ~12 px) read better crisp
        self.dither = 1.0 if min(self.union[2], self.union[3]) >= 9 else 0.45
        self.first = next(i for i, b in enumerate(self.boxes) if b is not None)
        self.last = max(i for i, b in enumerate(self.boxes) if b is not None)

    def t(self, i):
        """0..1 over the frames that show something."""
        return (i - self.first) / max(self.last - self.first, 1)

    def field(self):
        return np.zeros((self.W, self.W))

    def quant(self, F, gamma=0.8):
        # gamma < 1: luma keying makes dark ramp entries nearly transparent over the bright
        # world, so mid tones are pushed up a little to keep effects as present as intended
        """Float field -> ramp index 0..K-1 with ordered dithering."""
        F = np.clip(F, 0, 1.0) ** gamma
        idx = np.floor(F * (K - 1) + 0.5 + (self.B - 0.5) * self.dither).astype(np.int16)
        idx[F < 0.035] = 0
        return np.clip(idx, 0, K - 1)


# --- effect families --------------------------------------------------------------------------
# Each takes (fx, **params) and returns one entry per frame: a list of (field, ramp) layers
# (None = empty frame). Shapes are drawn in the frame's box, normalized so the box gives
# position/scale and the family adds the internal motion.


def fam_sigil(fx, ramp, sides=6, spin=1.0, full=False):
    """Casting rune: rings, star polygon, ticks, hot core; ignites, spins, burns out."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        R = min(hw, hh) * 0.95
        F = fx.field()
        rho, th = ellipse(fx.X, fx.Y, cx, cy, R, R * (0.98 if not full else 0.9))
        lw = max(0.7, R * 0.06)
        a = spin * t * math.pi * 0.9
        F += band(rho, R, lw) * (0.75 + 0.25 * np.cos(th * 3 + t * 9))
        if R > 7:
            F += band(rho / 0.68, R * 0.68, lw * 0.8) * 0.7
            # rune ticks between the rings, counter-rotating
            tick = (np.cos((th - a * 1.6) * 16) > 0.55) & (rho > 0.74) & (rho < 0.92)
            F += tick * 0.55
        # star polygon (two interleaved triangles for 6, a triangle for 3...)
        verts = [(cx + R * 0.9 * math.cos(a + k * TAU / sides - math.pi / 2),
                  cy + R * 0.9 * math.sin(a + k * TAU / sides - math.pi / 2)) for k in range(sides)]
        step = 2 if sides >= 5 else 1
        if R < 7:  # tiny rune: ring + three spokes reads better than a full star at ~12 px
            sides, step = 3, 1
            verts = [(cx + R * 0.85 * math.cos(a + k * TAU / 3 - math.pi / 2),
                      cy + R * 0.85 * math.sin(a + k * TAU / 3 - math.pi / 2)) for k in range(3)]
        for k in range(sides):
            p, q = verts[k], verts[(k + step) % sides]
            seg(F, p[0], p[1], q[0], q[1], lw, 0.85)
        glow(F, cx, cy, R * (0.28 + 0.12 * math.sin(t * 12)), 0.9)
        if full:  # big version: flame tongues licking out of the circle
            n = fbm(fx.X / (R * 0.18) + fx.seed, fx.Y / (R * 0.18) - t * 6, fx.seed + 3)
            flame = np.clip(1.25 - rho, 0, 1) * (n - 0.35) * 2.2 * smooth(min(t * 3, 1) * 1.2 - 0.1)
            F = np.maximum(F, np.clip(flame, 0, 1) * (1 - 0.6 * t))
        F *= e * (1.15 - 0.35 * t)
        out.append([(F, ramp)])
    return out


def fam_orb(fx, ramp, texture=True, glint=True, ring=False, sparks=0):
    """Glowing orb: hot core, mottled body, rim, rotating 4-point glint, optional ring/sparks."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        R = (hw + hh) / 2
        F = fx.field()
        rho, th = ellipse(fx.X, fx.Y, cx, cy, R * 0.8, R * 0.8)
        body = np.clip(1 - rho, 0, 1) ** 0.7
        if texture:
            n = fbm(fx.X / max(R * 0.3, 1) + t * 2, fx.Y / max(R * 0.3, 1) - t, fx.seed)
            body *= 0.55 + 0.7 * n
        F += body * 0.75 + band(rho, R * 0.8, max(0.7, R * 0.08)) * 0.45
        glow(F, cx, cy, R * 0.3, 0.6)
        if glint:
            a = t * 1.5
            L = R * (1.1 + 0.3 * math.sin(t * 9))
            for k in range(4):
                ang = a + k * math.pi / 2
                seg(F, cx, cy, cx + L * math.cos(ang), cy + L * math.sin(ang), 0.9, 0.9, 0.0)
        if ring:
            rr = R * (0.6 + 0.6 * t)
            rr_rho, _ = ellipse(fx.X, fx.Y, cx, cy, rr, rr * 0.9)
            F += band(rr_rho, rr, 0.9) * 0.6 * (1 - t)
        for k in range(sparks):
            ang = t * 4 + k * TAU / sparks
            dot(F, cx + R * 1.05 * math.cos(ang), cy + R * 0.7 * math.sin(ang), 1.2, 0.8)
        out.append([(F * e, ramp)])
    return out


def fam_burst(fx, ramp, kind="fire", blobs=18, embers=24, burnout=0.9, carpet=0):
    """Noise-flame explosion: a churning cluster of hot blobs that cools, plus flying embers;
    `burnout` = how fast the flames erode, `carpet` = cinders glowing on after the blast."""
    rng = fx.rng
    cu, cv = rng.uniform(-0.95, 0.95, carpet), rng.uniform(-0.2, 0.95, carpet)
    cb = rng.uniform(0.2, 0.5, carpet)
    pos = rng.uniform(-0.55, 0.55, (blobs, 2))
    vel = rng.normal(0, 0.25, (blobs, 2)) + np.array([0, -0.25])
    rad = rng.uniform(0.25, 0.5, blobs)
    phase = rng.uniform(0, 1, blobs)
    eang = rng.uniform(0, TAU, embers)
    espd = rng.uniform(0.6, 1.4, embers)
    ebirth = rng.uniform(0.15, 0.6, embers)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        p = pos + vel * t * 0.6
        for k in range(blobs):
            heat = max(0.0, 1 - t * (0.9 + 0.4 * phase[k])) if kind == "fire" else 0.8
            glow(F, cx + p[k, 0] * hw, cy + p[k, 1] * hh, rad[k] * (hw + hh) / 2 * (0.8 + 0.4 * t), 0.35 + 0.65 * heat)
        sc = max((hw + hh) / 2 * 0.35, 1.0)
        n = fbm(fx.X / sc, fx.Y / sc + t * 5, fx.seed + 1)
        if kind == "fire":
            F = (1 - np.exp(-F)) * (0.3 + 1.15 * n) * (1.3 - 0.6 * t)
            F *= np.clip((n + 0.9 - t * burnout) * 2.5, 0, 1)  # burns out into ragged patches
        elif kind == "puff":
            F = (1 - np.exp(-F * 1.2)) * (0.3 + 1.0 * n) * 0.85
        else:  # spores: dotty
            F *= (n > 0.5) * 0.9 + 0.2
        for k in range(embers):
            age = t - ebirth[k]
            if age <= 0 or age > 0.5:
                continue
            d = 0.6 + age * espd[k] * 1.6
            dot(F, cx + math.cos(eang[k]) * d * hw, cy + math.sin(eang[k]) * d * hh + age * hh * 0.4,
                0.9, 0.9 * (1 - age * 2))
        for k in range(carpet):  # smouldering cinders left on the ground
            if t > cb[k]:
                a = (0.5 + 0.5 * math.sin(t * 25 + k)) * min((t - cb[k]) * 6, 1) * max(0.0, 1.15 - t)
                dot(F, cx + cu[k] * hw, cy + cv[k] * hh, 1.3, a, 0.7)
        out.append([(F * e, ramp)])
    return out


def fam_ring(fx, ramp, width=0.2, fill=0.15, runes=False, loop=False):
    """Elliptic ring fitted to the box: front brighter, shimmering; `runes` = ground sigil."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        ph = TAU * i / fx.n if loop else t * 5
        F = fx.field()
        rw = max(0.8, hh * width)
        rho, th = ellipse(fx.X, fx.Y, cx, cy, hw - rw / 2, hh - rw / 2)
        front = 0.65 + 0.35 * np.sin(th)
        shimmer = 0.75 + 0.25 * np.cos(th * 7 + ph * (1 if loop else 1.5))
        F += band(rho, hh, rw) * front * shimmer
        F += (rho < 1) * fill * (0.5 + rho)
        if runes:
            inner, _ = ellipse(fx.X, fx.Y, cx, cy, (hw - rw) * 0.7, (hh - rw) * 0.7)
            F += band(inner, hh * 0.7, 0.8) * 0.6
            tick = (np.cos((th + ph) * 10) > 0.6) & (rho > 0.72) & (rho < 0.92)
            F += tick * 0.5
        out.append([(F * e, ramp)])
    return out


def fam_column(fx, ramp, sparks=0, jet=False, rocks=0, base_ramp=None):
    """Light cylinder rising from a ground ellipse; optional sparkles, water jet or rock shards."""
    rng = fx.rng
    su = rng.uniform(-0.95, 0.95, sparks)
    sv = rng.uniform(0, 1, sparks)
    ssp = rng.uniform(0.6, 1.4, sparks)
    ru = rng.uniform(-0.7, 0.7, rocks)
    rsz = rng.uniform(0.12, 0.25, rocks)
    rsp = rng.uniform(0.5, 1.0, rocks)
    rph = rng.uniform(0, 0.3, rocks)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        top, bottom = cy - hh, cy + hh
        ery = min(hw * 0.32, hh * 0.35)
        by = bottom - ery
        F = fx.field()
        u = (fx.X - cx) / hw
        inside = (np.abs(u) <= 1) & (fx.Y >= top) & (fx.Y <= by + ery * np.sqrt(np.clip(1 - u * u, 0, 1)))
        vfade = np.clip((fx.Y - top) / max(by - top, 1), 0, 1) ** 1.6
        limb = 0.45 + 0.55 * np.abs(u) ** 2
        streak = fbm(u * 3 + 7, (fx.Y - top) / max(hh, 1) * 1.2 + t * 3, fx.seed)
        F += inside * np.clip(vfade * 1.4 + (streak - 0.5) * 0.6, 0, 1) * limb * (0.6 + 0.7 * streak)
        rho, th = ellipse(fx.X, fx.Y, cx, by, hw, ery)
        F += band(rho, ery, 1.0) * (0.55 + 0.45 * np.sin(th))
        layers = [(F, ramp)]
        for k in range(sparks):
            v = (sv[k] + t * ssp[k]) % 1.0
            x, y = cx + su[k] * hw * 0.9, by - v * (by - top)
            a = math.sin(v * math.pi) * (0.6 + 0.4 * math.sin(k * 3 + t * 20))
            dot(F, x, y, 1.0, a)
            if a > 0.7 and hw > 8:
                seg(F, x - 2, y, x + 2, y, 0.7, a * 0.6)
                seg(F, x, y - 2, x, y + 2, 0.7, a * 0.6)
        if jet:
            glow(F, cx, by - (by - top) * 0.5, hw * 0.25, 0.0)
            core = np.exp(-((fx.X - cx) / max(hw * 0.25, 0.8)) ** 2) * (fx.Y >= top) * (fx.Y <= by)
            F += core * (0.6 + 0.4 * fbm(fx.X / 2, fx.Y / 3 + t * 8, fx.seed + 5))
        if rocks:
            R = fx.field()
            for k in range(rocks):
                v = smooth((t - rph[k]) * 1.6) * rsp[k]
                x, y = cx + ru[k] * hw, by - v * (by - top) * 0.85
                s = rsz[k] * hw
                ang = t * 3 + k
                pts = [(x + s * 0.6 * math.cos(ang + q * math.pi / 2) * (1.6 if q % 2 else 1),
                        y + s * math.sin(ang + q * math.pi / 2) * (1.6 if q % 2 == 0 else 1)) for q in range(4)]
                _fill_poly(R, pts, 0.5, fx)
                poly(R, pts[:3], 0.8, [1.0, 1.0, 1.0])  # lit upper edges
                poly(R, pts[2:] + pts[:1], 0.7, [0.7, 0.7, 0.7])
            R *= e * (1.1 - 0.4 * t)
            layers.append((R, base_ramp or "earth"))
        layers[0] = (F * e, ramp)
        out.append(layers)
    return out


def _fill_poly(F, pts, amp, fx):
    """Convex polygon fill (max-blended)."""
    xs, ys = [p[0] for p in pts], [p[1] for p in pts]
    win = _win(F, min(xs), min(ys), max(xs), max(ys))
    if win is None:
        return
    sl, X, Y = win
    inside = np.ones(X.shape, bool)
    sign = None
    for k in range(len(pts)):
        (x0, y0), (x1, y1) = pts[k], pts[(k + 1) % len(pts)]
        c = (x1 - x0) * (Y - y0) - (y1 - y0) * (X - x0)
        s = np.sign(np.sum(c)) if sign is None else sign
        sign = s
        inside &= c * s >= 0
    F[sl] = np.maximum(F[sl], inside * amp)


def fam_dome(fx, ramp):
    """Shield bubble: fresnel rim, dim centre, highlight, latitude shimmer bands."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        rho, th = ellipse(fx.X, fx.Y, cx, cy, hw, hh)
        v = (fx.Y - cy) / hh
        inside = rho < 1
        fres = 0.12 + 0.88 * np.clip(rho, 0, 1) ** 5
        bands = (np.sin(v * 9 - t * 14) > 0.75) * 0.25
        hl = np.exp(-(((fx.X - (cx - hw * 0.4)) / (hw * 0.25)) ** 2 + ((fx.Y - (cy - hh * 0.45)) / (hh * 0.18)) ** 2))
        F += inside * (fres + bands * (1 - rho) + hl * 0.6)
        out.append([(F * e, ramp)])
    return out


def fam_sphere(fx, ramp, meridians=7):
    """Wire-frame bubble: rotating meridian arcs (back ones dim) and a rim."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        R = max(hw, hh)
        F = fx.field()
        rho, th = ellipse(fx.X, fx.Y, cx, cy, R, R)
        v = np.clip((fx.Y - cy) / R, -0.999, 0.999)
        half = R * np.sqrt(1 - v * v)
        s = (fx.X - cx) / np.maximum(half, 0.5)
        inside = rho < 1
        for k in range(meridians):
            m = (k / meridians + t * 0.35) * math.pi  # longitude 0..pi wraps
            m = (m % math.pi) - math.pi / 2
            d = np.abs(s - math.sin(m)) * half
            F = np.maximum(F, inside * np.clip(1 - d / 0.9, 0, 1) * (0.35 + 0.65 * math.cos(m)))
        F += band(rho, R, 1.0) * 0.8
        out.append([(F * e, ramp)])
    return out


def fam_sparkle(fx, ramp, count=14, core=0.4, rise=0.3, star=False):
    """Twinkling star glints scattered in the box (or one big rotating star with `star`)."""
    rng = fx.rng
    pu, pv = rng.uniform(-1, 1, count), rng.uniform(-1, 1, count)
    birth, life = rng.uniform(-0.2, 0.8, count), rng.uniform(0.25, 0.5, count)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        R = (hw + hh) / 2
        if core:
            glow(F, cx, cy, R * 0.45, core)
        if star:
            a = t * 1.2
            for k in range(8):
                L = R * (1.0 if k % 2 == 0 else 0.5) * (0.85 + 0.15 * math.sin(t * 10 + k))
                ang = a + k * math.pi / 4
                seg(F, cx, cy, cx + L * math.cos(ang), cy + L * math.sin(ang), 1.0 if k % 2 == 0 else 0.7, 1.0, 0.0)
            glow(F, cx, cy, R * 0.18, 1.0)
        for k in range(count):
            age = (t - birth[k]) / life[k]
            if not 0 < age < 1:
                continue
            a = math.sin(age * math.pi)
            x, y = cx + pu[k] * hw, cy + pv[k] * hh - age * rise * hh
            dot(F, x, y, 1.0, a)
            arm = 1 + 2.5 * a * min(R / 10, 1.5)
            if a > 0.5:
                seg(F, x - arm, y, x + arm, y, 0.7, a * 0.8)
                seg(F, x, y - arm, x, y + arm, 0.7, a * 0.8)
        out.append([(F * e, ramp)])
    return out


def fam_flake(fx, ramp, arms=6, shards=0):
    """Snowflake / hex rune: branched arms, slowly turning; `shards` converge first (ice)."""
    rng = fx.rng
    sa, sr = rng.uniform(0, TAU, shards), rng.uniform(0.7, 1.0, shards)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        R = min(hw, hh)
        F = fx.field()
        lw = max(0.7, R * 0.07)
        if shards and t < 0.5:
            k = smooth(t * 2)
            for j in range(shards):
                d = sr[j] * (1 - k) * 1.0 + 0.1
                x, y = cx + math.cos(sa[j]) * d * hw, cy + math.sin(sa[j]) * d * hh
                ux, uy = math.cos(sa[j]), math.sin(sa[j])
                seg(F, x, y, x + ux * R * 0.25, y + uy * R * 0.25, lw, 0.9, 0.2)
            glow(F, cx, cy, R * 0.3 * k, 0.7 * k)
        else:
            k = smooth((t - (0.5 if shards else 0)) * (2 if shards else 3))
            a = t * 0.8
            for j in range(arms):
                ang = a + j * TAU / arms - math.pi / 2
                ux, uy = math.cos(ang), math.sin(ang)
                L = R * k
                seg(F, cx, cy, cx + ux * L, cy + uy * L, lw, 1.0, 0.6)
                for f in (0.5, 0.75):
                    bx, by = cx + ux * L * f, cy + uy * L * f
                    for s in (-1, 1):
                        ba = ang + s * math.pi / 3
                        seg(F, bx, by, bx + math.cos(ba) * L * 0.28, by + math.sin(ba) * L * 0.28, lw * 0.8, 0.8, 0.3)
                if arms <= 4:
                    dot(F, cx + ux * L, cy + uy * L, max(1.2, R * 0.16), 1.0)
            glow(F, cx, cy, R * 0.35, 0.8)
            if shards:
                flash = max(0.0, 1 - abs(t - 0.55) * 6)
                glow(F, cx, cy, R * 0.8, flash)
        out.append([(F * e, ramp)])
    return out


def fam_beam(fx, ramp):
    """Light beam slanting down from the sky onto a flare on the ground."""
    ucx, ucy, uhw, uhh = fx.union
    sx, sy = ucx + uhw * 0.9, ucy - uhh
    ix, iy = ucx - uhw * 0.25, ucy + uhh * 0.7
    rng = fx.rng
    sp_a, sp_s = rng.uniform(math.pi * 1.05, math.pi * 1.95, 20), rng.uniform(0.3, 1.0, 20)
    out = []
    for i in range(fx.n):
        if fx.boxes[i] is None:
            out.append(None)
            continue
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        w = uhw * 0.22 * smooth(t * 4) * (1 - smooth((t - 0.55) * 3))
        if w > 0.3:
            dx, dy = ix - sx, iy - sy
            L = math.hypot(dx, dy)
            # distance from the beam axis, beam widening towards the ground
            s = np.clip(((fx.X - sx) * dx + (fx.Y - sy) * dy) / (L * L), 0, 1)
            d = np.hypot(fx.X - (sx + s * dx), fx.Y - (sy + s * dy))
            ww = w * (0.4 + 0.6 * s)
            n = fbm(s * 6 - t * 10, d / np.maximum(ww, 0.5) * 1.5 + 3, fx.seed)
            F += np.clip(1 - d / ww, 0, 1) ** 0.8 * (0.55 + 0.6 * n) * (0.65 + 0.35 * s)
        k = smooth((t - 0.2) * 3) * (1 - smooth((t - 0.7) * 3.3))
        glow(F, ix, iy, uhw * 0.25 * (0.5 + k), k * 1.1)
        rho, _ = ellipse(fx.X, fx.Y, ix, iy, uhw * 0.5 * (0.3 + t), uhh * 0.12 * (0.3 + t))
        F += band(rho, uhh * 0.12, 1.0) * k * 0.7
        for j in range(20):
            age = t - 0.35 - j * 0.01
            if 0 < age < 0.5:
                d = age * sp_s[j] * uhw * 1.4
                dot(F, ix + math.cos(sp_a[j]) * d, iy + math.sin(sp_a[j]) * d * 0.7 + age * age * uhh, 0.9, 1 - age * 2)
        out.append([(F * e, ramp)])
    return out


def _bolt(rng, x0, y0, x1, y1, depth, rough):
    pts = [(x0, y0), (x1, y1)]
    for d in range(depth):
        new = [pts[0]]
        for (ax, ay), (bx, by) in zip(pts, pts[1:]):
            L = math.hypot(bx - ax, by - ay)
            mx, my = (ax + bx) / 2, (ay + by) / 2
            nx, ny = -(by - ay) / (L or 1), (bx - ax) / (L or 1)
            o = rng.normal(0, rough * L)
            new += [(mx + nx * o, my + ny * o), (bx, by)]
        pts = new
    return pts


def fam_bolt(fx, ramp):
    """Lightning strike: jagged forking bolt (re-rolled every 2 frames), impact glow, crackle."""
    ucx, ucy, uhw, uhh = fx.union
    out = []
    for i in range(fx.n):
        if fx.boxes[i] is None:
            out.append(None)
            continue
        t, e = fx.t(i), fx.env[i]
        rng = np.random.default_rng(fx.seed + i // 2)
        F = fx.field()
        ix, iy = ucx + uhw * 0.3, ucy + uhh * 0.85
        if t < 0.35:
            pts = _bolt(rng, ucx - uhw * 0.5 + rng.uniform(-2, 2), ucy - uhh, ix, iy, 5, 0.22)
            poly(F, pts, 2.2, [0.55] * len(pts))
            poly(F, pts, 1.0, [1.0] * len(pts))
            for _ in range(2):
                j = int(rng.integers(len(pts) // 4, len(pts) * 3 // 4))
                bx, by = pts[j]
                br = _bolt(rng, bx, by, bx + rng.uniform(-0.5, 0.5) * uhw, by + rng.uniform(0.2, 0.5) * uhh, 3, 0.25)
                poly(F, br, 0.9, [0.8] * len(br))
        k = 1 - smooth((t - 0.2) * 1.5)
        glow(F, ix, iy, uhw * 0.25, k)
        if 0.3 < t < 0.9:
            for _ in range(2):
                a = rng.uniform(0, TAU)
                L = rng.uniform(0.2, 0.5) * uhw * k
                br = _bolt(rng, ix, iy, ix + math.cos(a) * L, iy + math.sin(a) * L * 0.6, 2, 0.3)
                poly(F, br, 0.8, [0.8 * k] * len(br))
        out.append([(F * e, ramp)])
    return out


def fam_slash(fx, ramp, kind="line", a0=-0.5, a1=3.5):
    """Weapon swing: a straight streak (`line`) or a sweeping crescent (`arc`) with a fading trail."""
    out = []
    ucx, ucy, uhw, uhh = fx.union
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        if kind == "line":
            x0, y0, x1, y1 = cx - hw, cy + hh, cx + hw, cy - hh
            seg(F, x0, y0, (x0 + x1) / 2, (y0 + y1) / 2, 2.0, 0.0, 0.6)
            seg(F, (x0 + x1) / 2, (y0 + y1) / 2, x1, y1, 2.0, 0.6, 0.1)
            seg(F, x0, y0, x1, y1, 0.9, 1.0)
        else:
            head = a0 + (a1 - a0) * smooth(t * 1.3)
            trail = 2.2
            rho, th = ellipse(fx.X, fx.Y, ucx, ucy, uhw * 0.85, uhh * 0.85)
            sgn = 1 if a1 > a0 else -1
            behind = ((head - th) * sgn) % TAU
            k = np.clip(1 - behind / trail, 0, 1) * (behind < trail)
            width = np.maximum(0.04 + 0.16 * k, 1.4 / max(uhw * 0.85, 1) * k)  # >= ~1.4 px
            F += np.clip(1 - np.abs(rho - 1) / width, 0, 1) * k ** 1.3
            F += np.clip(1 - np.abs(rho - 0.97) / 0.03, 0, 1) * (k > 0.6) * 0.5
        out.append([(F * e, ramp)])
    return out


def fam_swirl(fx, ramp, arms=2, orbs=0, star=True):
    """Spiral arms whirling around a star flare; `orbs` = ring of spinning orbs instead."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        R = (hw + hh) / 2
        F = fx.field()
        if orbs:
            for k in range(orbs):
                ang = t * 5 + k * TAU / orbs
                dot(F, cx + math.cos(ang) * hw * 0.82, cy + math.sin(ang) * hh * 0.82, max(1.0, R * 0.16), 1.0, 0.7)
            glow(F, cx, cy, R * 0.4, max(0.0, t - 0.5) * 1.6)
        else:
            for a in range(arms):
                pts, amps = [], []
                for s in np.linspace(0, 1, 24):
                    ang = a * TAU / arms + 3.0 * s + t * 7
                    rr = R * (0.15 + 0.85 * s)
                    pts.append((cx + math.cos(ang) * rr * hw / R, cy + math.sin(ang) * rr * hh / R))
                    amps.append(s ** 0.6)
                poly(F, pts, max(0.8, R * 0.09), amps)
        if star:
            L = R * 0.6
            for k in range(4):
                ang = k * math.pi / 2 + t
                seg(F, cx, cy, cx + L * math.cos(ang), cy + L * math.sin(ang), 0.8, 1.0, 0.0)
            glow(F, cx, cy, R * 0.2, 0.9)
        out.append([(F * e, ramp)])
    return out


def fam_rays(fx, ramp, count=22, ring=False):
    """Radial spark streaks shooting out of a central flash (shatter / firework)."""
    rng = fx.rng
    ang = rng.uniform(0, TAU, count)
    spd = rng.uniform(0.6, 1.0, count)
    ln = rng.uniform(0.2, 0.45, count)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        for k in range(count):
            a = ang[k] + (t * 0.8 if ring else 0)
            if ring:
                r1, r0 = 1.0, 0.6
            else:
                r1 = 0.35 + 0.65 * spd[k]
                r0 = max(r1 - ln[k] * (1.2 - t), 0.05)
            ux, uy = math.cos(a), math.sin(a)
            seg(F, cx + ux * r0 * hw, cy + uy * r0 * hh, cx + ux * r1 * hw, cy + uy * r1 * hh, 0.8, 0.15, 1.0)
        glow(F, cx, cy, (hw + hh) * 0.2, max(0.0, 1 - t * 2.5))
        out.append([(F * e, ramp)])
    return out


def fam_spirit(fx, ramp):
    """Wailing spirit: a ghostly head with hollow eyes and a waving tail, rising."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        hr = min(hw, hh * 0.6)
        hx, hy = cx, cy - hh + hr
        rho, _ = ellipse(fx.X, fx.Y, hx, hy, hr * 0.85, hr)
        F += np.clip(1 - rho, 0, 1) ** 0.5 * 0.9
        v = (fx.Y - hy) / max(hh * 2 - hr, 1)
        wob = np.sin(v * 8 - t * 12) * hw * 0.25 * v
        tail = np.clip(1 - np.abs(fx.X - hx - wob) / (hr * 0.8 * (1 - v)), 0, 1) * (v > 0) * (v < 1)
        F += tail * (1 - v) * 0.7 * (0.6 + 0.6 * fbm(fx.X / 2, fx.Y / 2 + t * 6, fx.seed))
        for s in (-1, 1):  # eyes
            ex, ey = hx + s * hr * 0.35, hy - hr * 0.05
            F[np.hypot(fx.X - ex, (fx.Y - ey) * 0.8) < max(hr * 0.2, 0.8)] = 0
        F[np.hypot((fx.X - hx) * 1.3, fx.Y - (hy + hr * 0.45)) < max(hr * 0.18, 0.7)] *= 0.2
        out.append([(F * e, ramp)])
    return out


def fam_angel(fx, ramp):
    """Radiant winged figure: glowing body, halo and two feathered wings unfolding."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        head = (cx, cy - hh * 0.55)
        hr = max(hh * 0.12, 1.0)
        dot(F, head[0], head[1], hr, 1.0, 0.5)
        seg(F, cx, head[1] + hr, cx, cy + hh * 0.9, max(hw * 0.12, 0.9), 1.0, 0.2)
        rho, _ = ellipse(fx.X, fx.Y, head[0], head[1] - hr * 1.6, hr * 1.6, hr * 0.5)
        F += band(rho, hr * 0.5, 0.8) * 0.8
        open_ = smooth(t * 2.5)
        sh = (cx, head[1] + hr * 1.6)
        for s in (-1, 1):
            for f in range(5):
                ang = -math.pi / 2 + s * (0.5 + open_ * (0.35 + f * 0.28))
                L = hh * (0.75 - f * 0.08) * (0.4 + 0.6 * open_)
                ex, ey = sh[0] + math.cos(ang) * L * hw / hh * 1.6, sh[1] + math.sin(ang) * L
                seg(F, sh[0], sh[1], ex, ey, max(0.8, hw * 0.06), 0.9, 0.25)
        glow(F, cx, cy, hh * 0.5, 0.35)
        out.append([(F * e, ramp)])
    return out


def fam_wisps(fx, ramp, count=3):
    """Sinuous ribbons drifting upward (wind / nature)."""
    rng = fx.rng
    ph = rng.uniform(0, TAU, count)
    off = rng.uniform(-0.6, 0.6, count)
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        for k in range(count):
            pts, amps = [], []
            for s in np.linspace(0, 1, 18):
                y = cy + hh - s * 2 * hh
                x = cx + (off[k] + 0.45 * math.sin(s * 5 + ph[k] + t * 8)) * hw
                pts.append((x, y))
                amps.append(math.sin(s * math.pi) * 0.9)
            poly(F, pts, max(0.8, hw * 0.12), amps)
        out.append([(F * e, ramp)])
    return out


def fam_whirl(fx, ramp, levels=7):
    """Whirlwind funnel: stacked dashed ellipses spinning, wide at the top."""
    out = []
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        for k in range(levels):
            v = -1 + 2 * (k + 0.5) / levels
            rx = hw * (0.35 + 0.6 * (1 - v) / 2)
            ry = max(rx * 0.3, 0.8)
            y = cy + v * (hh - ry)
            x = cx + math.sin(t * 6 + k) * hw * 0.08
            rho, th = ellipse(fx.X, fx.Y, x, y, rx, ry)
            dash = (np.sin(th * 2 + t * 22 + k * 0.9) > -0.2)
            F = np.maximum(F, band(rho, ry, 0.9) * dash * (0.55 + 0.45 * np.sin(th)))
        out.append([(F * e, ramp)])
    return out


def fam_comet(fx, ramp, tail=3.0):
    """Moving glowing head with a tail behind it (from the box track), bursting at the end."""
    out = []
    cs = [None if b is None else (b[0], b[1]) for b in fx.boxes]
    for i in range(fx.n):
        b = fx.boxes[i]
        if b is None:
            out.append(None)
            continue
        cx, cy, hw, hh = b
        t, e = fx.t(i), fx.env[i]
        F = fx.field()
        prev = next((cs[j] for j in range(i - 2, -1, -1) if cs[j] is not None), None)
        R = min(hw, hh) * 0.6
        if prev is not None and math.hypot(cx - prev[0], cy - prev[1]) > 0.5:
            dx, dy = cx - prev[0], cy - prev[1]
            L = math.hypot(dx, dy)
            ux, uy = dx / L, dy / L
            for s in np.linspace(0, 1, 10):
                glow(F, cx - ux * s * R * tail, cy - uy * s * R * tail, R * (1 - 0.6 * s), 0.6 * (1 - s))
            glow(F, cx, cy, R * 0.7, 0.9)
        else:  # stopped: dissolve into a sparkling puff
            n = fbm(fx.X / max(R * 0.5, 1), fx.Y / max(R * 0.5, 1) - t * 4, fx.seed)
            rho, _ = ellipse(fx.X, fx.Y, cx, cy, hw, hh)
            F += np.clip(1 - rho, 0, 1) * (n > 0.55) * 0.9
        out.append([(F * e, ramp)])
    return out


def fam_portal(fx, ramp, diamond=False):
    """Looping gateway: a thick shimmering ring (or iso diamond) with a swirling interior."""
    out = []
    ucx, ucy, uhw, uhh = fx.union
    for i in range(fx.n):
        if fx.boxes[i] is None:
            out.append(None)
            continue
        ph = TAU * i / fx.n
        e = fx.env[i]
        F = fx.field()
        if diamond:
            d = np.abs(fx.X - ucx) / uhw + np.abs(fx.Y - ucy) / uhh
            F += np.clip(1 - np.abs(d - 0.9) * uhh / 1.2, 0, 1) * (0.7 + 0.3 * math.sin(ph))
            F += (d < 0.9) * np.clip(np.sin(d * 9 - ph * 2), 0, 1) * 0.35
        else:
            rho, th = ellipse(fx.X, fx.Y, ucx, ucy, uhw * 0.85, uhh * 0.92)
            F += band(rho, uhw, uhw * 0.12) * (0.75 + 0.25 * np.cos(th * 3 - ph))
            sw = np.sin(th * 3 + rho * 8 - ph * 2)
            F += (rho < 0.92) * np.clip(sw, 0, 1) * 0.35 * rho
        out.append([(F * e, ramp)])
    return out


def fam_ringburst(fx, ramp):
    """Ground ring expanding, then a burst of embers (first half / second half)."""
    ring = fam_ring(fx, ramp, width=0.25, fill=0.0)
    burst = fam_burst(fx, ramp, blobs=10, embers=20)
    return [r if (r is None or fx.t(i) < 0.5) else burst[i] for i, r in enumerate(ring)]


def fam_storm(fx, ramp):
    """Thundercloud: churning puff with lightning crackling inside."""
    puff = fam_burst(fx, ramp, kind="puff", blobs=12, embers=0)
    out = []
    for i, layers in enumerate(puff):
        if layers is None:
            out.append(None)
            continue
        F = layers[0][0]
        cx, cy, hw, hh = fx.boxes[i]
        rng = np.random.default_rng(fx.seed + i)
        if i % 3 != 2:
            pts = _bolt(rng, cx + rng.uniform(-0.6, 0.6) * hw, cy - hh * 0.5,
                        cx + rng.uniform(-0.6, 0.6) * hw, cy + hh * 0.6, 4, 0.25)
            poly(F, pts, 0.9, [fx.env[i]] * len(pts))
        out.append([(F, ramp)])
    return out


# --- real-alpha sprites -------------------------------------------------------------------------

GLYPHS = {
    "!": [".####.", ".####.", ".####.", ".####.", "..##..", "..##..", "..##..", "......", "..##..", "..##.."],
    "?": [".####.", "##..##", "##..##", "....##", "...##.", "..##..", "..##..", "......", "..##..", "..##.."],
}


def _outline_rgba(rgb, mask, outline=(18, 12, 6)):
    """RGBA with a 1 px dark outline around mask."""
    m = mask.astype(bool)
    grow = m.copy()
    for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
        grow |= np.roll(np.roll(m, dy, 0), dx, 1)
    out = np.zeros(m.shape + (4,), np.uint8)
    out[grow & ~m] = (*outline, 255)
    out[m, :3] = rgb[m]
    out[m, 3] = 255
    return out


def sprite_glyph(fx, ch, ramp_hexes):
    """Quest marker (!/?): pixel-font glyph, vertical light->dark ramp, outline, gentle bob."""
    rows = GLYPHS[ch]
    ucx, ucy, uhw, uhh = fx.union
    scale = max(1, int(round(uhh * 2 / (len(rows) + 2))))
    g = np.array([[c == "#" for c in r] for r in rows])
    g = np.kron(g, np.ones((scale, scale), bool))
    ramp = np.array([_rgb(h) for h in ramp_hexes])
    out = []
    for i in range(fx.n):
        bob = int(round(math.sin(TAU * i / fx.n) * max(1, scale // 2)))
        H, W = g.shape
        img = np.zeros((fx.W, fx.W), bool)
        x0, y0 = int(round(ucx - W / 2)), int(round(ucy - H / 2)) + bob
        img[max(y0, 0):y0 + H, max(x0, 0):x0 + W] = g[max(-y0, 0):, max(-x0, 0):][: fx.W - max(y0, 0), : fx.W - max(x0, 0)]
        shade = np.clip(1 - (fx.Y - y0) / H, 0, 1) * 0.8 + 0.25 * (fx.X - x0 < W / 2)
        idx = np.clip(np.floor(shade * (len(ramp) - 1) + fx.B - 0.25), 0, len(ramp) - 1).astype(int)
        out.append(_outline_rgba(ramp[idx].astype(np.uint8), img))
    return out


def sprite_arrow(fx, angle):
    """Arrow pointing at `angle` (rad, 0 = east, y down): steel head, wooden shaft, white fletching."""
    ucx, ucy, uhw, uhh = fx.union
    L = max(uhw, uhh) * 0.95
    ux, uy = math.cos(angle), math.sin(angle)
    F = fx.field()
    seg(F, ucx - ux * L, ucy - uy * L, ucx + ux * L * 0.6, ucy + uy * L * 0.6, 1.5, 1.0)
    rgb = np.zeros((fx.W, fx.W, 3), np.uint8)
    mask = F > 0.3
    rgb[mask] = (122, 84, 46)
    head = np.hypot(fx.X - (ucx + ux * L * 0.7), fx.Y - (ucy + uy * L * 0.7)) < 1.8
    rgb[head] = (200, 206, 214)
    fl = (np.hypot(fx.X - (ucx - ux * L * 0.8), fx.Y - (ucy - uy * L * 0.8)) < 1.6) & ~head
    rgb[fl] = (236, 232, 220)
    return [_outline_rgba(rgb, mask | head | fl)]


def sprite_obelisk(fx):
    """Return obelisk: tapered stone needle, lit from the left, rune pulsing blue."""
    ucx, ucy, uhw, uhh = fx.union
    top, bot = ucy - uhh, ucy + uhh
    stone = np.array([_rgb(h) for h in ("#2a2830", "#46434f", "#67636f", "#8c8896", "#b4b0bc")])
    rune = np.array([_rgb(h) for h in RAMPS["frost"][2:]])
    out = []
    for i in range(fx.n):
        v = np.clip((fx.Y - top) / (bot - top), 0, 1)
        half = uhw * (0.3 + 0.6 * v)
        u = (fx.X - ucx) / np.maximum(half, 0.5)
        mask = (np.abs(u) <= 1) & (fx.Y >= top + uhh * 0.15) & (fx.Y <= bot)
        tip = (fx.Y < top + uhh * 0.15) & (fx.Y >= top) & (np.abs(fx.X - ucx) <= (fx.Y - top) / (uhh * 0.15) * half)
        mask |= tip
        light = np.where(u < 0, 0.75 - 0.2 * u, 0.45 - 0.35 * u) * (0.85 + 0.15 * v)
        idx = np.clip(np.floor(light * (len(stone) - 1) + fx.B - 0.5), 0, len(stone) - 1).astype(int)
        rgb = stone[idx]
        k = 0.5 + 0.5 * math.sin(TAU * i / fx.n)
        rr = (np.abs(fx.X - ucx) < max(1, half.mean() * 0.15)) & (v > 0.3) & (v < 0.7) & (np.sin(v * 40) > -0.3)
        ri = np.clip(np.floor(k * (len(rune) - 1) + fx.B), 0, len(rune) - 1).astype(int)
        rgb[rr & mask] = rune[ri][rr & mask]
        out.append(_outline_rgba(rgb.astype(np.uint8), mask))
    return out


# --- which effect is what ---------------------------------------------------------------------
# name -> (family, params); ramp=None picks the school ramp from the layout's dominant colour.

ARROWS = {"east": 0, "south_east": 0.45, "south": math.pi / 2, "south_west": math.pi - 0.45, "west": math.pi,
          "north_west": math.pi + 0.45, "north": -math.pi / 2, "north_east": -0.45}

SPEC: dict[str, tuple] = {
    # casting runes on the hand / under the caster
    **{f"cast_00{k}.sa": (fam_sigil, {}) for k in range(1, 10)},
    "cast_001.sa": (fam_sigil, {"ramp": "fire"}),
    "cast_007.sa": (fam_sigil, {"ramp": "white"}),  # recoloured by some kits
    "cast_001_full.sa": (fam_sigil, {"full": True, "ramp": "fire"}),
    "cast_004_full.sa": (fam_sigil, {"full": True, "sides": 8}),
    "magic_003.sa": (fam_sigil, {"sides": 4, "spin": -2.0}),
    # shields / bubbles
    "blue_bubble_01.sa": (fam_sphere, {}),
    "yellow_bubble_01.sa": (fam_sphere, {}),
    "yellow_bubble_01_solid.sa": (fam_sphere, {}),
    **{n: (fam_dome, {}) for n in ("magic_007.sa", "magic_007_solid.sa", "magic_007a.sa", "magic_007b.sa",
                                     "magic_007c.sa", "magic_007d.sa")},
    # light columns
    **{n: (fam_column, {}) for n in ("magic_008.sa", "magic_008a.sa", "magic_008b.sa", "magic_008c.sa", "magic_008d.sa")},
    **{n: (fam_column, {"sparks": 22}) for n in ("heal_003.sa", "heal_003a.sa", "heal_003b.sa", "heal_003c.sa",
                                                   "heal_003d.sa")},
    "water_005.sa": (fam_column, {"jet": True, "sparks": 14}),
    "water_005solid.sa": (fam_column, {"jet": True, "sparks": 14}),
    "earth_002.sa": (fam_column, {"ramp": "fire", "rocks": 5}),
    "earth_002a.sa": (fam_column, {"ramp": "fire", "rocks": 8}),
    # rings
    "aura_001.sa": (fam_ring, {"width": 0.14, "fill": 0.08, "runes": True, "loop": True}),
    "nova_001.sa": (fam_ring, {"width": 0.3, "fill": 0.05}),
    "nova_001_frfeet.sa": (fam_ring, {"width": 0.25, "fill": 0.0}),
    "gowaypoint.sa": (fam_ring, {"width": 0.2, "fill": 0.1, "runes": True, "loop": True}),
    "fire_003.sa": (fam_ringburst, {}),
    # explosions / puffs
    "fire_001.sa": (fam_burst, {}),
    "fire_effect_002.sa": (fam_burst, {"ramp": "fire", "blobs": 34, "embers": 60, "burnout": 2.0, "carpet": 70}),
    "fire_002.sa": (fam_burst, {"blobs": 14, "embers": 12}),
    "fire_002blue.sa": (fam_burst, {"ramp": "frost", "blobs": 14, "embers": 12}),
    "special_001_s.sa": (fam_burst, {"kind": "puff", "blobs": 9, "embers": 8}),
    "effect_003.sa": (fam_burst, {"kind": "puff", "blobs": 8, "embers": 6, "ramp": "ash"}),
    "effect_008.sa": (fam_burst, {"kind": "spore", "blobs": 12, "embers": 30}),
    "special_002.sa": (fam_burst, {"kind": "spore", "blobs": 10, "embers": 20}),
    "lightning_cloud_02.sa": (fam_storm, {}),
    # orbs / flares / glints
    "heal_001.sa": (fam_orb, {}),
    "light_003.sa": (fam_orb, {"texture": False, "sparks": 3}),
    "light_003a.sa": (fam_orb, {"texture": False, "ring": True}),
    "light_003b.sa": (fam_orb, {"texture": False, "ring": True}),
    "light_001.sa": (fam_comet, {}),
    "light_004.sa": (fam_sparkle, {"count": 4, "core": 0.6, "star": True}),
    "magic_006.sa": (fam_orb, {"sparks": 4}),
    "effect_006.sa": (fam_sparkle, {"count": 5, "core": 0.3, "star": True}),
    "heal_002.sa": (fam_sparkle, {"count": 12, "core": 0.35}),
    "effect_010.sa": (fam_sparkle, {"ramp": "white", "count": 10, "core": 0.5, "rise": 0.8}),
    # frost / hex runes
    "magic_004.sa": (fam_flake, {}),
    "magic_005.sa": (fam_flake, {"arms": 3}),
    "ice_001.sa": (fam_flake, {"shards": 9}),
    "ice_002.sa": (fam_flake, {"shards": 6}),
    # beams and bolts
    "light_002.sa": (fam_beam, {"ramp": "holy"}),
    "lightning_001.sa": (fam_bolt, {}),
    "lightning_001a.sa": (fam_bolt, {}),
    # weapon swings
    "slash_001.sa": (fam_slash, {"kind": "line", "ramp": "bone"}),
    "slash_002.sa": (fam_slash, {"kind": "arc", "a0": 0.0, "a1": 6.0}),
    "slash_002a.sa": (fam_slash, {"kind": "arc", "a0": -0.6, "a1": 2.6}),
    "slash_002b.sa": (fam_slash, {"kind": "arc", "a0": 3.6, "a1": 0.2, "ramp": "blood"}),
    "slash_002c.sa": (fam_slash, {"kind": "arc", "a0": -0.3, "a1": 3.0, "ramp": "rust"}),
    "slash_002d.sa": (fam_slash, {"kind": "arc", "a0": -1.4, "a1": 3.8}),
    # swirls, sparks, spirits, wind
    "dark_effect_001.sa": (fam_swirl, {}),
    "effect_bluepink_01.sa": (fam_swirl, {"arms": 3, "star": False}),
    "darkness_001.sa": (fam_swirl, {"orbs": 8, "star": False}),
    "effect_004.sa": (fam_rays, {"ramp": "fire"}),
    "water_001.sa": (fam_rays, {"count": 26, "ramp": "bone"}),
    "effect_009.sa": (fam_rays, {"count": 28, "ring": True}),
    "darkness_002.sa": (fam_spirit, {}),
    "angel_001.sa": (fam_angel, {}),
    "angel_001a.sa": (fam_angel, {}),
    "wind_001.sa": (fam_wisps, {}),
    "wind_002.sa": (fam_rays, {"count": 16, "ring": True}),
    **{n: (fam_whirl, {}) for n in ("wind_003.sa", "wind_003a_frfreet.sa")},
    "wind_003a.sa": (fam_whirl, {"ramp": "rust"}),
    "wind_003b.sa": (fam_whirl, {"ramp": "bone"}),
    # moving things
    "effect_002.sa": (fam_comet, {}),
    "effect_002a.sa": (fam_comet, {}),
    "water_004.sa": (fam_comet, {}),
    # gateways
    "lightblue_portal1.sa": (fam_portal, {}),
    "instance_entrance_a.sa": (fam_portal, {"diamond": True}),
    # real alpha
    "quest_ready.sa": ("glyph", {"ch": "!", "ramp": ["#7a4a08", "#c0820e", "#f0b81c", "#ffe066", "#fff6c0"]}),
    "quest_done.sa": ("glyph", {"ch": "?", "ramp": ["#7a4a08", "#c0820e", "#f0b81c", "#ffe066", "#fff6c0"]}),
    "green_quest_ready.sa": ("glyph", {"ch": "!", "ramp": ["#1e5208", "#3a8a10", "#62c41c", "#a4f04a", "#e4ffb0"]}),
    "return_obelisk.sa": ("obelisk", {}),
    **{f"arrow_{d}.sa": ("arrow", {"angle": a}) for d, a in ARROWS.items()},
}


# --- output -----------------------------------------------------------------------------------


def compose(fx, layers):
    """Layers of (field, ramp) -> (index image, palette): per pixel the brightest layer wins."""
    names = []
    best = np.zeros((fx.W, fx.W), np.int16)
    which = np.zeros((fx.W, fx.W), np.int16)
    for F, ramp in layers:
        if ramp not in names:
            names.append(ramp)
        q = fx.quant(F)
        m = q > best
        best[m] = q[m]
        which[m] = names.index(ramp)
    idx = np.where(best > 0, which * K + best, 0).astype(np.uint8)
    pal = [c for n in names for rgb in RAMP_RGB[n] for c in rgb]
    return idx, pal


def save_frame(fx, frame, path):
    """Trims, scales by ratio (nearest), saves; returns the canvas offset."""
    r = fx.r
    if isinstance(frame, np.ndarray) and frame.ndim == 3:  # RGBA sprite
        lit = frame[..., 3] > 0
        img = frame
    elif frame is None:
        lit = np.zeros((fx.W, fx.W), bool)
        img = None
    else:
        idx, pal = compose(fx, frame)
        lit = idx > 0
        img = (idx, pal)
    if not lit.any():
        Image.new("P", (1, 1), 0).save(path)  # black pixel = keyed fully transparent
        return fx.W * r // 2, fx.W * r // 2
    ys, xs = np.nonzero(lit)
    y0, y1, x0, x1 = ys.min(), ys.max() + 1, xs.min(), xs.max() + 1
    if isinstance(img, tuple):
        crop = np.kron(img[0][y0:y1, x0:x1], np.ones((r, r), np.uint8))
        im = Image.frombytes("P", (crop.shape[1], crop.shape[0]), crop.tobytes())
        im.putpalette(img[1])
    else:
        crop = np.kron(img[y0:y1, x0:x1], np.ones((r, r, 1), np.uint8))
        im = Image.fromarray(crop)
    im.save(path, optimize=True)
    return int(x0 * r), int(y0 * r)


def build(name: str, lay: dict):
    fx = Fx(name, lay)
    fam, params = SPEC[name]
    params = dict(params)
    if fam == "glyph":
        frames = sprite_glyph(fx, params["ch"], params["ramp"])
    elif fam == "arrow":
        frames = sprite_arrow(fx, params["angle"])
    elif fam == "obelisk":
        frames = sprite_obelisk(fx)
    else:
        ramp = params.pop("ramp", None) or auto_ramp(lay["hue"], lay["sat"])
        frames = fam(fx, ramp, **params)
    base = "sfx_" + lay["filename"]
    OUT_FRAMES.mkdir(parents=True, exist_ok=True)
    lines = [f"ratio={lay['ratio']}", f"size={lay['size']}", f"filename={base}", f"loopstart={lay['loopstart']}",
             f"loopend={lay['loopend']}", f"delay={lay['delay']}", ""]
    for num, frame in zip(lay["frames"], frames):
        x, y = save_frame(fx, frame, OUT_FRAMES / f"{base}_{num}.png")
        lines.append(f"{num},{x},{y}")
    OUT_SA.mkdir(parents=True, exist_ok=True)
    (OUT_SA / name).write_text("\n".join(lines) + "\n", newline="")
    return fx, frames


# --- particle atlas -----------------------------------------------------------------------------


def particle_atlas():
    """16 white 32x32 sprites (`data/particles.txt` picks cells by index):
    row 0: soft blob, sparkle cluster, 6-ray star, small burst
    row 1: thin X cross, solid star, 8-ray flare, small soft glow
    row 2: smoke clump, spiky starburst, bright flare, hollow ring
    row 3: smoke puff, flat disc, small disc, sleep "Z"
    Alpha is quantized to 5 levels with Bayer dithering for the oldschool look."""
    S = 32
    Y, X = np.mgrid[0:S, 0:S] + 0.5
    c = S / 2
    d = np.hypot(X - c, Y - c)
    th = np.arctan2(Y - c, X - c)
    B = np.tile(BAYER4, (8, 8))
    rng = np.random.default_rng(7)

    def rays(n, width, length, off=0.0):
        a = np.abs(((th - off) * n / TAU + 0.5) % 1 - 0.5) * TAU / n
        return np.clip(1 - a * d / width, 0, 1) * np.clip(1 - d / length, 0, 1)

    def cluster(count, rmin, rmax, spread):
        F = np.zeros((S, S))
        for _ in range(count):
            r = rng.uniform(0, spread)
            a = rng.uniform(0, TAU)
            dot(F, c + r * math.cos(a), c + r * math.sin(a), rng.uniform(rmin, rmax), rng.uniform(0.5, 1.0))
        return F

    def noise_clump(seed, sc):
        n = fbm(X / sc, Y / sc, seed)
        return np.clip(1 - d / 14, 0, 1) ** 0.8 * (0.4 + 1.0 * n)

    cells = [
        np.clip(1 - d / 15, 0, 1) ** 1.3,
        cluster(26, 0.8, 2.0, 12),
        np.maximum(rays(6, 1.0, 15, 0.3), np.exp(-(d / 2.5) ** 2)),
        np.maximum(cluster(18, 0.6, 1.4, 9), rays(8, 0.8, 9) * 0.8),
        rays(4, 0.9, 16, math.pi / 4),
        _star_mask(X, Y, c, 13, 5.5),
        np.maximum(rays(8, 0.9, 15, 0.2) * np.where(np.arange(1) == 0, 1, 1), np.exp(-(d / 2.2) ** 2)),
        np.clip(1 - d / 10, 0, 1) ** 1.6,
        noise_clump(11, 5.0),
        np.maximum(rays(14, 0.8, 14), np.exp(-(d / 3.5) ** 2)),
        np.maximum(rays(4, 1.2, 15) * 0.8, np.clip(1 - d / 7, 0, 1) ** 0.7),
        np.clip(1 - np.abs(d - 12) / 2.5, 0, 1) + (d < 12) * 0.12,
        noise_clump(23, 7.0) * 0.9,
        (d < 11) * 0.6 + np.clip(1 - np.abs(d - 11) / 1.5, 0, 1) * 0.4,
        (d < 6) * 0.75 + np.clip(1 - np.abs(d - 6) / 1.5, 0, 1) * 0.3,
        _z_mask(X, Y),
    ]
    atlas = np.zeros((128, 128, 4), np.uint8)
    atlas[..., :3] = 255
    for k, F in enumerate(cells):
        q = np.clip(np.floor(np.clip(F, 0, 1) * 4 + B), 0, 4) / 4.0
        q[F < 0.04] = 0
        row, col = divmod(k, 4)
        atlas[row * S:(row + 1) * S, col * S:(col + 1) * S, 3] = (q * 255).astype(np.uint8)
    OUT_ATLAS.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(atlas).save(OUT_ATLAS, optimize=True)
    return atlas


def _star_mask(X, Y, c, ro, ri):
    th = np.arctan2(Y - c, X - c) + math.pi / 2
    d = np.hypot(X - c, Y - c)
    a = np.abs((th * 5 / TAU) % 1 - 0.5) * 2  # 0 at the inner notch... 1 at the tip
    edge = ri + (ro - ri) * (1 - a) ** 1.0
    return np.clip((edge - d) / 1.2 + 0.5, 0, 1)


def _z_mask(X, Y):
    F = np.zeros(X.shape)
    for (x0, y0, x1, y1) in ((8, 7, 24, 7), (24, 7, 8, 25), (8, 25, 24, 25)):
        seg(F, x0, y0, x1, y1, 2.6, 1.0)
    return F


# --- previews -----------------------------------------------------------------------------------


def preview(built, out: Path, step_cols=10):
    """Rows = animations, columns = evenly spaced frames, drawn in canvas coordinates."""
    rows = []
    for name, (fx, frames) in built:
        sa = (OUT_SA / name).read_text().splitlines()
        offs = [tuple(map(int, l.split(","))) for l in sa if l.count(",") == 2]
        S = fx.lay["size"]
        sel = list(range(0, fx.n, max(1, fx.n // step_cols)))[:step_cols]
        row = Image.new("RGB", (step_cols * (S + 2), S + 14), (24, 24, 30))
        ImageDraw.Draw(row).text((2, 1), name, fill=(160, 160, 160))
        for j, i in enumerate(sel):
            num, x, y = offs[i]
            cell = Image.new("RGB", (S, S), (0, 0, 0))
            im = Image.open(OUT_FRAMES / f"sfx_{fx.lay['filename']}_{num}.png").convert("RGBA")
            cell.paste(im, (x, y), im)
            row.paste(cell, (j * (S + 2), 13))
        rows.append(row)
    if not rows:
        return
    W = max(r.width for r in rows)
    sheet = Image.new("RGB", (W, sum(r.height for r in rows)), (10, 10, 10))
    y = 0
    for r in rows:
        sheet.paste(r, (0, y))
        y += r.height
    out.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out)


def main(argv):
    layout = json.loads(LAYOUT.read_text())
    if "--atlas" in argv or not argv:
        particle_atlas()
        print("fx_particles.png ->", OUT_ATLAS)
        if argv:
            return
    names = [a for a in argv if a.endswith(".sa")] or sorted(layout)
    built = []
    for name in names:
        if name not in SPEC:
            print("no spec for", name)
            continue
        built.append((name, build(name, layout[name])))
    print(f"{len(built)} flipbooks, {sum(fx.n for _, (fx, _) in built)} frames")
    for k in range(0, len(built), 12):
        preview(built[k:k + 12], PREVIEW / f"spellfx_{k // 12:02d}.png")


if __name__ == "__main__":
    main(sys.argv[1:])
