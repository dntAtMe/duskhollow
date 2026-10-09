"""Creatures and people of Duskhollow (docs/world.md), for the "First Gaze" demo.

Usage (from repo root):  python -I tools/artgen/valefolk.py [model ...]

Same pipeline as creatures.py (SDF voxel models on a skeleton -> 8-direction sheets), written to
`assets/scripts/npc/<model>.txt` + `content/sprites/custom_npc_<model>.png`:

| model | who |
|---|---|
| `glarewolf` | Eye-turned wolf (quadruped rig: trot, lunging bite, flinch, fall on its side) |
| `stooped` | hollowed lightworker, bent double, head craned up at the sky, rusty sickle |
| `hollowed_warden` | Warden Corvin: tarnished plate, torn tabard, halberd; sweep + overhead slam (`cast`) |
| `cairnkeeper` | Ysolde: layered shawls and hood, fire poker with an ember tip, ember pouch |
| `lightworker` | gaunt field worker: straw hat with a face veil, sack, hoe |
| `lowshade_guard` | hooded mantle over brigandine, spear, lantern, a canopy on a back pole |

Palette follows the art rules: dark albedo lit by a crimson key from above and falling into bruised
violet shadow (`lit()`); nothing near white; only fire (lantern, embers) glows.
"""

from __future__ import annotations

import math
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import creatures  # noqa: E402,F401  (registers its ramps first, so ids stay stable)
import sheet  # noqa: E402
import smear  # noqa: E402
import vox  # noqa: E402
from keyframes import keyed, retime  # noqa: E402
from vox import Bone, Model, Prim, rot_x, rot_y, rot_z  # noqa: E402

R = math.radians
P = Prim
SIDES = (("r", -1), ("l", 1))

# --- palette ---------------------------------------------------------------------------------------

SHADOW = np.array([0.50, 0.34, 0.72])  # bruised violet
KEY = np.array([1.0, 0.68, 0.60])  # crimson key light
LEVELS = (0.2, 0.36, 0.54, 0.72, 0.88)


def lit(albedo: str) -> list[str]:
    """Five shades of a material under the Eye: violet-black shadow -> crimson-lit highlight."""
    a = np.array(vox.hex_rgb(albedo), float) / 255
    out = []
    for i, level in enumerate(LEVELS):
        k = i / 4
        c = a * level * (SHADOW * (1 - k) + KEY * k) + np.array([0.03, 0.018, 0.045]) * (1 - k)
        c = np.clip(c, 0, 0.74)
        out.append("#" + "".join(f"{int(round(v * 255)):02x}" for v in c))
    return out


VALE_RAMPS = {
    "v_wfur": lit("#8c5034"),  # rust fur
    "v_wchar": lit("#3c3436"),  # charcoal fur
    "v_wrib": lit("#7e6a5a"),  # ribs under thin hide
    "v_sheen": ["#1c0505", "#300807", "#430c0a", "#57110d", "#6a1610"],  # dull red eye-sheen, no glow
    "v_void": ["#080507", "#0c080a", "#110b0d", "#150e10", "#1a1113"],
    "v_ashen": lit("#86787a"),  # hollowed skin
    "v_weath": lit("#a07258"),  # weathered living skin
    "v_sack": lit("#7c6644"),  # sackcloth
    "v_straw": lit("#8e7450"),
    "v_linen": lit("#7a6a50"),
    "v_rust": lit("#8e4a2c"),
    "v_tarnish": lit("#6e6c68"),  # old plate
    "v_brass": lit("#8e6c3a"),  # tarnished trim
    "v_tabard": lit("#6c2a26"),  # faded oxblood
    "v_cape": lit("#4a2226"),
    "v_shawl": lit("#4a3a50"),
    "v_shawl2": lit("#2c2430"),
    "v_shawlred": lit("#5e2a28"),
    "v_wool": lit("#5e5244"),  # mantle
    "v_oil": lit("#6a5634"),  # oilcloth canopy
    "v_brig": lit("#5c3e2a"),
    "v_wood": lit("#6c4a30"),
    "v_iron": lit("#4c4a4c"),
    "v_bone": lit("#8a8070"),  # grey hair, dull bone
    "v_veil": lit("#4c4038"),
    "v_fire": ["#7a2408", "#9e3a0c", "#bc5212", "#d26a1c", "#de8430"],  # lantern flame glows
    "v_ember": ["#4a1206", "#6a1c08", "#8a2c0c", "#a63e12", "#bc521a"],
}
vox.RAMPS.update(VALE_RAMPS)
vox.RAMP_RGB = {k: np.array([vox.hex_rgb(c) for c in v], dtype=np.uint8) for k, v in vox.RAMPS.items()}
vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])


def speckle(model: Model, ramps: tuple[str, ...], amount: float, depth: float, seed: int):
    """Darkens random voxels of the given ramps (pitted plate, matted fur)."""
    rng = np.random.default_rng(seed)
    ids = [vox.RAMP_IDS[r] for r in ramps]
    for name, (p, n, c) in model.parts.items():
        hit = np.isin(c, ids) & (rng.random(len(c)) < amount)
        tone = np.where(hit, 1.0 - depth, 1.0)
        model.parts[name] = (p, n * tone[:, None], c)
    return model


def strips(x, y0, y1, z_top, length, n, color, seed, half_x=0.012, axis="y"):
    """Ragged hem: `n` hanging strips spread along y (or x), random lengths, at depth `x`."""
    rng = np.random.default_rng(seed)
    out = []
    for i in range(n):
        s = y0 + (y1 - y0) * (i + 0.5) / n
        ln = length * (0.45 + 0.75 * rng.random())
        w = abs(y1 - y0) / n * 0.48
        c = (x, s, z_top - ln / 2) if axis == "y" else (s, x, z_top - ln / 2)
        h = (half_x, w, ln / 2) if axis == "y" else (w, half_x, ln / 2)
        out.append(P("box", (c, h), color))
    return out


def brim(cx, cz, radius, n, skip, color, seed, thick=0.018):
    """Hat brim as a ring of flat discs; `skip` indices leave torn gaps."""
    rng = np.random.default_rng(seed)
    out = []
    for i in range(n):
        if i in skip:
            continue
        a = 2 * math.pi * i / n
        r = radius * (0.85 + 0.2 * rng.random())
        z = cz + (rng.random() - 0.5) * 0.03
        out.append(P("ellipsoid", ((cx + r * math.cos(a), r * math.sin(a), z), (0.1, 0.1, thick)), color))
    out.append(P("ellipsoid", ((cx, 0, cz), (radius * 0.75, radius * 0.75, thick)), color))
    return out


# --- humanoid skeleton (same bones and pivots as the adventurer, + a left-hand item) ----------------

SKELETON = [
    ("pelvis", None, (0, 0, 0.95)),
    ("torso", "pelvis", (0, 0, 1.05)),
    ("head", "torso", (0, 0, 1.55)),
    ("upper_arm_r", "torso", (0, -0.25, 1.46)),
    ("forearm_r", "upper_arm_r", (0, -0.28, 1.2)),
    ("sword", "forearm_r", (0.03, -0.28, 0.93)),
    ("upper_arm_l", "torso", (0, 0.25, 1.46)),
    ("forearm_l", "upper_arm_l", (0, 0.28, 1.2)),
    ("item_l", "forearm_l", (0.03, 0.28, 0.93)),
    ("thigh_r", "pelvis", (0, -0.09, 0.95)),
    ("shin_r", "thigh_r", (0, -0.09, 0.53)),
    ("thigh_l", "pelvis", (0, 0.09, 0.95)),
    ("shin_l", "thigh_l", (0, 0.09, 0.53)),
]
HAND_Z = 0.93


def humanoid(prims: dict[str, list[Prim]], voxel: float = 0.02) -> Model:
    return Model([Bone(n, parent, pivot, prims.get(n, [])) for n, parent, pivot in SKELETON], voxel=voxel)


def limbs(arm, fore, hand, thigh, shin, foot, r_arm=0.06, r_fore=0.05, r_thigh=0.08, r_shin=0.065):
    """Plain arms and legs in the given ramps (None = leave that part out)."""
    out = {}
    for s, y in SIDES:
        if arm:
            out[f"upper_arm_{s}"] = [P("capsule", ((0, y * 0.26, 1.44), (0, y * 0.28, 1.2), r_arm), arm)]
        out[f"forearm_{s}"] = []
        if fore:
            out[f"forearm_{s}"].append(P("capsule", ((0, y * 0.28, 1.2), (0.02, y * 0.28, 0.98), r_fore), fore))
        if hand:
            out[f"forearm_{s}"].append(P("ellipsoid", ((0.03, y * 0.28, HAND_Z), (0.045, 0.04, 0.05)), hand))
        if thigh:
            out[f"thigh_{s}"] = [P("capsule", ((0, y * 0.09, 0.95), (0, y * 0.09, 0.53), r_thigh), thigh)]
        out[f"shin_{s}"] = []
        if shin:
            out[f"shin_{s}"].append(P("capsule", ((0, y * 0.09, 0.53), (0, y * 0.09, 0.1), r_shin), shin))
        if foot:
            out[f"shin_{s}"].append(P("box", ((0.07, y * 0.09, 0.045), (0.11, 0.055, 0.045)), foot))
    return out


def merge(*dicts):
    out: dict[str, list[Prim]] = {}
    for d in dicts:
        for k, v in d.items():
            out.setdefault(k, []).extend(v)
    return out


def pole(down: float, up: float, r: float, color: str, y: float = -0.28):
    """A shaft through the hand, along +z in rest pose (`down` below the grip, `up` above)."""
    return P("capsule", ((0.03, y, HAND_Z - down), (0.03, y, HAND_Z + up), r), color)


# --- pose helpers -----------------------------------------------------------------------------------
# rot_y(a): negative a swings a hanging limb forward (+x), positive a tips an upright part forward.
# For items pointing up from the hand the summed rot_y angle `phi` is 0 = up, +90 = forward.


def legs_pose(rot, knee=0.0, stride=0.0, lift=0.0, phase=0.0):
    a = math.sin(phase)
    b = math.sin(phase + math.pi / 2)
    rot["thigh_r"] = rot_y(R(-knee - stride * a))
    rot["shin_r"] = rot_y(R(2 * knee + lift * max(0.0, b)))
    rot["thigh_l"] = rot_y(R(-knee + stride * a))
    rot["shin_l"] = rot_y(R(2 * knee + lift * max(0.0, -b)))
    return -0.85 * (1 - math.cos(R(knee)))


def body(rot, lean=0.0, twist=0.0, roll=0.0, head=0.0, head_roll=0.0, head_turn=0.0):
    rot["torso"] = rot_z(R(twist)) @ rot_y(R(lean)) @ rot_x(R(roll))
    rot["head"] = rot_z(R(head_turn)) @ rot_y(R(head)) @ rot_x(R(head_roll))


def arm(rot, side, upper, fore, spread=0.0, item=None, phi=None, lean=0.0):
    """Arm angles (rot_y, relative); with `phi` the held item is turned to that world angle."""
    s = "r" if side < 0 else "l"
    rot[f"upper_arm_{s}"] = rot_y(R(upper)) @ rot_x(R(spread * side))
    rot[f"forearm_{s}"] = rot_y(R(fore))
    if phi is not None:
        rot[item or ("sword" if side < 0 else "item_l")] = rot_y(R(phi - lean - upper - fore))


def fall(t, forward=True, delay=0.0):
    """Death fall around the feet: (progress, root rotation, z lift keeping the body above ground)."""
    k = min(max((t - delay) / (1 - delay) * 1.2, 0.0), 1.0)
    a = R(86) * k**1.6
    return k, rot_y(a if forward else -a), 0.1 * k


# --- glarewolf -------------------------------------------------------------------------------------


def glarewolf() -> Model:
    B = Bone
    rng = np.random.default_rng(7)
    rib_c, rib_r = np.array([0.16, 0, 0.62]), np.array([0.3, 0.135, 0.19])

    def on_rib(x, side, z):
        q = 1 - ((x - rib_c[0]) / rib_r[0]) ** 2 - ((z - rib_c[2]) / rib_r[2]) ** 2
        return (x, side * (rib_r[1] * math.sqrt(max(q, 0.0)) + 0.002), z)

    ribs = []
    for side in (1, -1):
        for x in (0.02, 0.1, 0.18, 0.26):
            pts = [on_rib(x + dz * 0.25, side, 0.62 + dz) for dz in (0.12, 0.04, -0.05, -0.12)]
            ribs += [P("capsule", (a, b, 0.017), "v_wrib") for a, b in zip(pts, pts[1:])]
    hackles = [
        P("ellipsoid", ((x, (rng.random() - 0.5) * 0.08, z + rng.random() * 0.04), (0.05, 0.035, 0.06 + 0.04 * rng.random())), "v_wchar")
        for x, z in [(0.44, 0.86), (0.36, 0.84), (0.28, 0.82), (0.2, 0.81), (0.1, 0.8), (0.0, 0.77), (-0.12, 0.76), (-0.26, 0.76), (-0.4, 0.78)]
    ]
    belly = [
        P("ellipsoid", ((x, (rng.random() - 0.5) * 0.12, 0.45 + rng.random() * 0.03), (0.04, 0.035, 0.06)), "v_wfur")
        for x in (0.28, 0.2, 0.12, 0.04)
    ]
    bones = [
        B("body", None, (0, 0, 0.6), [
            P("ellipsoid", (tuple(rib_c), tuple(rib_r)), "v_wfur"),
            P("ellipsoid", ((-0.17, 0, 0.63), (0.24, 0.1, 0.12)), "v_wfur"),  # tucked waist
            P("ellipsoid", ((-0.4, 0, 0.64), (0.16, 0.12, 0.14)), "v_wfur"),  # haunches
            P("capsule", ((0.3, 0, 0.78), (-0.45, 0, 0.74), 0.05), "v_wchar"),  # dark saddle
            *ribs, *hackles, *belly,
        ]),
        B("neck", "body", (0.38, 0, 0.7), [
            P("capsule", ((0.38, 0, 0.72), (0.62, 0, 0.6), 0.1), "v_wfur"),
            P("ellipsoid", ((0.44, 0, 0.7), (0.14, 0.16, 0.16)), "v_wchar"),  # ruff
            *[P("ellipsoid", ((0.44 + 0.1 * math.cos(a), 0.15 * math.sin(a), 0.58 + 0.03 * math.cos(3 * a)), (0.05, 0.04, 0.07)), "v_wchar")
              for a in np.linspace(0.5, 2 * math.pi - 0.5, 6)],
        ]),
        B("head", "neck", (0.62, 0, 0.6), [
            P("ellipsoid", ((0.7, 0, 0.61), (0.14, 0.105, 0.11)), "v_wfur"),
            P("capsule", ((0.75, 0, 0.57), (0.94, 0, 0.52), 0.05), "v_wchar"),  # snout
            P("box", ((0.97, 0, 0.53), (0.02, 0.025, 0.02)), "v_void"),  # nose
            P("capsule", ((0.66, 0.06, 0.67), (0.59, 0.09, 0.83), 0.032), "v_wchar"),  # ears
            P("capsule", ((0.66, -0.06, 0.67), (0.59, -0.09, 0.83), 0.032), "v_wchar"),
            P("box", ((0.8, 0.065, 0.625), (0.016, 0.014, 0.011)), "v_sheen"),
            P("box", ((0.8, -0.065, 0.625), (0.016, 0.014, 0.011)), "v_sheen"),
        ]),
        B("jaw", "head", (0.74, 0, 0.53), [
            P("capsule", ((0.74, 0, 0.52), (0.91, 0, 0.48), 0.03), "v_wchar"),
            P("box", ((0.88, 0, 0.51), (0.02, 0.03, 0.012)), "v_wrib"),  # teeth
        ]),
        B("tail", "body", (-0.52, 0, 0.7), [
            P("capsule", ((-0.52, 0, 0.7), (-0.76, 0, 0.44), 0.05), "v_wfur"),
            P("ellipsoid", ((-0.8, 0, 0.38), (0.06, 0.05, 0.1)), "v_wchar"),
        ]),
    ]
    for s, y in SIDES:
        bones += [
            B(f"fu_{s}", "body", (0.28, y * 0.1, 0.56), [P("capsule", ((0.28, y * 0.1, 0.6), (0.3, y * 0.1, 0.3), 0.055), "v_wfur")]),
            B(f"fd_{s}", f"fu_{s}", (0.3, y * 0.1, 0.3), [
                P("capsule", ((0.3, y * 0.1, 0.3), (0.28, y * 0.1, 0.05), 0.033), "v_wchar"),
                P("ellipsoid", ((0.31, y * 0.1, 0.03), (0.06, 0.04, 0.03)), "v_wchar"),
            ]),
            B(f"hu_{s}", "body", (-0.42, y * 0.1, 0.6), [P("capsule", ((-0.42, y * 0.1, 0.64), (-0.3, y * 0.1, 0.36), 0.07), "v_wfur")]),
            B(f"hd_{s}", f"hu_{s}", (-0.3, y * 0.1, 0.36), [
                P("capsule", ((-0.3, y * 0.1, 0.36), (-0.45, y * 0.1, 0.16), 0.038), "v_wchar"),
                P("capsule", ((-0.45, y * 0.1, 0.16), (-0.41, y * 0.1, 0.04), 0.032), "v_wchar"),
                P("ellipsoid", ((-0.38, y * 0.1, 0.03), (0.06, 0.04, 0.03)), "v_wchar"),
            ]),
        ]
    return speckle(Model(bones, voxel=0.018), ("v_wfur", "v_wchar"), 0.25, 0.25, 3)


def wolf_legs(rot, phase, amp, lift):
    """Trot: front-left moves with hind-right."""
    for s, off in (("l", 0.0), ("r", math.pi)):
        for leg, ph in (("f", off), ("h", off + math.pi)):
            p = phase + ph
            rot[f"{leg}u_{s}"] = rot_y(R(-amp * math.sin(p)))
            rot[f"{leg}d_{s}"] = rot_y(R(lift * max(0.0, math.cos(p)) + (6 if leg == "h" else 0)))
    return rot


def wolf_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = wolf_legs({}, 0, 0, 0)
        rot["body"] = rot_y(R(2 + 1.5 * b))
        rot["neck"] = rot_y(R(14 + 3 * b)) @ rot_z(R(6 * b))
        rot["head"] = rot_y(R(-6))
        rot["jaw"] = rot_y(R(6 + 4 * (b + 1)))  # panting
        rot["tail"] = rot_z(R(8 * b))
        return rot, (0, 0, -0.01 * (1 + b)), None

    def run(t):
        p = t * 2 * math.pi
        rot = wolf_legs({}, p, 28, 55)
        rot["body"] = rot_y(R(4 + 3 * math.sin(2 * p)))
        rot["neck"] = rot_y(R(18 - 6 * math.sin(2 * p)))
        rot["jaw"] = rot_y(R(14))
        rot["tail"] = rot_y(R(-15)) @ rot_z(R(10 * math.sin(p)))
        return rot, (0, 0, 0.03 * abs(math.sin(2 * p)) - 0.02), None

    base = {"body": (0, 2, 0), "neck": (0, 14, 0), "head": (0, -6, 0), "jaw": (0, 8, 0), "tail": (0, 0, 0),
            **{f"{leg}_{s}": (0, 0, 0) for leg in ("fu", "fd", "hu", "hd") for s in "lr"}, "root": (0, 0, 0)}
    # Snapping bite: haunches drop and the head pulls back low with the lips peeled (hold), then
    # the whole body springs forward and the jaws slam shut on frame 2, a savage head shake, back off.
    bite = keyed(7, base, [
        (0, {"body": (0, 6, 0), "neck": (0, 30, 0), "head": (0, -14, 0), "jaw": (0, 22, 0), "tail": (0, -20, 0),
             "hu_l": (0, 20, 0), "hu_r": (0, 22, 0), "hd_l": (0, -16, 0), "hd_r": (0, -18, 0),
             "fu_l": (0, 8, 0), "fu_r": (0, 8, 0), "root": (-0.08, 0, -0.04)}, "lin"),
        (1, {"neck": (0, 36, 0), "jaw": (0, 42, 0), "root": (-0.1, 0, -0.05), "body": (0, 8, 0)}, "out"),
        (2, {"body": (0, -6, 0), "neck": (0, -18, 0), "head": (0, 12, 0), "jaw": (0, 2, 0), "tail": (0, -30, 0),
             "fu_l": (0, -40, 0), "fu_r": (0, -22, 0), "fd_l": (0, 20, 0), "hu_l": (0, 28, 0), "hu_r": (0, 30, 0),
             "hd_l": (0, 0, 0), "hd_r": (0, 0, 0), "root": (0.26, 0, 0.03)}, "in3"),
        (3, {"neck": (12, -14, 18), "head": (10, 10, 0), "root": (0.24, 0, 0.0)}, "out"),
        (4, {"neck": (-10, -10, -16), "head": (-8, 8, 0)}, "io"),
        (6, {**base, "neck": (0, 18, 0)}, "io"),
    ])

    # Pounce: a low crouch with the hindquarters wound up, a leap with the forepaws reaching and
    # the jaws wide, landing on the target with paws and teeth, then a hop back.
    pounce = keyed(8, base, [
        (0, {"body": (0, 4, 0), "neck": (0, 26, 0), "head": (0, -10, 0), "jaw": (0, 14, 0), "tail": (0, 10, 0),
             "fu_l": (0, 22, 0), "fu_r": (0, 18, 0), "fd_l": (0, -30, 0), "fd_r": (0, -28, 0),
             "hu_l": (0, 30, 0), "hu_r": (0, 30, 0), "hd_l": (0, -30, 0), "hd_r": (0, -30, 0),
             "root": (-0.06, 0, -0.12)}, "lin"),
        (1, {"body": (0, 6, 0), "tail": (0, 20, 0), "root": (-0.09, 0, -0.15)}, "out"),
        (2, {"body": (0, -18, 0), "neck": (0, -6, 0), "head": (0, 0, 0), "jaw": (0, 46, 0), "tail": (0, -25, 0),
             "fu_l": (0, -75, 0), "fu_r": (0, -65, 0), "fd_l": (0, 30, 0), "fd_r": (0, 26, 0),
             "hu_l": (0, 45, 0), "hu_r": (0, 50, 0), "hd_l": (0, 10, 0), "hd_r": (0, 10, 0),
             "root": (0.22, 0, 0.2)}, "in"),
        (3, {"body": (0, 12, 0), "neck": (0, 4, 0), "head": (0, 16, 0), "jaw": (0, 2, 0),
             "fu_l": (0, -40, 0), "fu_r": (0, -30, 0), "fd_l": (0, 10, 0), "fd_r": (0, 10, 0),
             "hu_l": (0, 10, 0), "hu_r": (0, 12, 0), "hd_l": (0, -10, 0), "hd_r": (0, -10, 0),
             "root": (0.4, 0, -0.04)}, "in"),
        (4, {"body": (0, 10, 0), "neck": (8, 6, 14), "root": (0.38, 0, -0.06)}, "out"),
        (5, {"neck": (-6, 8, -10)}, "io"),
        (7, {**base, "neck": (0, 18, 0)}, "io"),
    ])

    def hit(t):
        k = math.sin(t * math.pi)
        rot = wolf_legs({}, 0, 0, 0)
        rot["body"] = rot_y(R(-8 * k))
        rot["neck"] = rot_y(R(30 * k))
        rot["jaw"] = rot_y(R(25 * k))
        rot["tail"] = rot_y(R(25 * k))
        return rot, (-0.08 * k, 0, 0), None

    def die(t):
        k = min(t * 1.25, 1.0)
        rot = {}
        for s in "lr":
            rot[f"fu_{s}"] = rot_y(R(-25 * k))
            rot[f"hu_{s}"] = rot_y(R(25 * k))
            rot[f"fd_{s}"] = rot_y(R(15 * k))
        rot["neck"] = rot_y(R(14 - 40 * k))
        rot["jaw"] = rot_y(R(25 * k))
        rot["tail"] = rot_z(R(-20 * k))
        # Rolls onto its side.
        return rot, (0, 0, 0.12 * math.sin(k * math.pi) + 0.15 * k), rot_x(R(88) * k**1.4)

    return [
        ("stance", stance, 4, 1000, "back_forth"),
        ("run", run, 8, 560, "looped"),
        ("swing", bite, 7, 560, "play_once"),
        ("swing2", pounce, 8, 720, "play_once"),
        ("cast", bite, 7, 560, "play_once"),
        ("shoot", bite, 7, 560, "play_once"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 6, 900, "play_once"),
    ]


def wolf_portrait(t):
    """Head raised and snarling, shifted so the head sits over the origin (portraits.py)."""
    rot, off, root_rot = wolf_anims()[0][1](t)
    rot["neck"] = rot_y(R(-14))
    rot["head"] = rot_y(R(4))
    rot["jaw"] = rot_y(R(24))
    turn = R(-45)  # show the profile
    hx, hy = 0.72 * math.cos(turn), 0.72 * math.sin(turn)
    return rot, (off[0] - hx, off[1] - hy, off[2]), rot_z(turn)


# --- the Stooped (hollowed lightworker) --------------------------------------------------------------


def sickle(y=-0.28):
    top = HAND_Z + 0.03
    end = HAND_Z - 0.17
    c = np.array([0.03 + 0.15, y, end])
    pts = [c + 0.15 * np.array([math.cos(a), 0, math.sin(a)]) for a in np.radians(np.linspace(180, 395, 9))]
    blade = [P("capsule", (tuple(a), tuple(b), 0.02 - 0.0015 * i), "v_rust") for i, (a, b) in enumerate(zip(pts, pts[1:]))]
    return [P("capsule", ((0.03, y, top), (0.03, y, end), 0.021), "v_wood"), *blade]


def stooped() -> Model:
    prims = merge(
        limbs("v_ashen", "v_ashen", "v_ashen", "v_ashen", "v_ashen", "v_sack", r_arm=0.038, r_fore=0.032, r_thigh=0.05, r_shin=0.04),
        {
            "pelvis": [
                P("box", ((0, 0, 0.92), (0.1, 0.15, 0.1)), "v_sack"),
                *strips(0.1, -0.14, 0.14, 0.86, 0.26, 6, "v_sack", 1),
                *strips(-0.1, -0.14, 0.14, 0.86, 0.3, 6, "v_sack", 2),
                *strips(0.15, -0.08, 0.08, 0.86, 0.22, 3, "v_sack", 3, axis="x"),
                *strips(-0.15, -0.08, 0.08, 0.86, 0.22, 3, "v_sack", 4, axis="x"),
            ],
            "torso": [
                P("ellipsoid", ((0, 0, 1.28), (0.11, 0.165, 0.24)), "v_sack"),
                P("ellipsoid", ((0, 0.19, 1.45), (0.05, 0.05, 0.045)), "v_ashen"),  # bony shoulders
                P("ellipsoid", ((0, -0.19, 1.45), (0.05, 0.05, 0.045)), "v_ashen"),
                P("capsule", ((0, 0, 1.44), (0.02, 0, 1.56), 0.035), "v_ashen"),  # neck
                P("capsule", ((0.08, -0.15, 1.12), (0.09, 0.15, 1.1), 0.02), "v_brig"),  # rope belt
                *strips(0.1, -0.12, 0.1, 1.2, 0.16, 4, "v_sack", 5),
            ],
            "head": [
                P("ellipsoid", ((0.03, 0, 1.66), (0.085, 0.075, 0.105)), "v_ashen"),
                P("box", ((0.105, 0.032, 1.68), (0.012, 0.017, 0.016)), "v_void"),  # empty eyes
                P("box", ((0.105, -0.032, 1.68), (0.012, 0.017, 0.016)), "v_void"),
                P("box", ((0.095, 0, 1.585), (0.014, 0.026, 0.014)), "v_void"),  # slack mouth
                P("ellipsoid", ((0.0, 0, 1.79), (0.1, 0.1, 0.07)), "v_straw"),
                *brim(0.0, 1.755, 0.21, 14, {3, 4, 10}, "v_straw", 9),
            ],
        },
    )
    for s, y in SIDES:
        prims[f"upper_arm_{s}"].append(P("ellipsoid", ((0, y * 0.25, 1.4), (0.06, 0.06, 0.07)), "v_sack"))  # rag sleeves
    prims["sword"] = sickle()
    return humanoid(prims)


ST_LEAN, ST_HEAD, ST_KNEE = 70.0, -98.0, 22.0


def stooped_base(rot, lean=ST_LEAN, sway=0.0):
    body(rot, lean=lean, roll=sway, head=ST_HEAD + (ST_LEAN - lean), head_roll=-8)
    arm(rot, -1, -lean - 12, -15)
    arm(rot, 1, -lean - 4, -10, spread=4)
    rot.setdefault("sword", rot_y(R(-20)))


def stooped_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = {}
        stooped_base(rot, sway=2 * b)
        drop = legs_pose(rot, ST_KNEE)
        rot["head"] = rot["head"] @ rot_z(R(4 * b))
        return rot, (0, 0, drop - 0.01 * b), None

    def run(t):
        p = t * 2 * math.pi
        rot = {}
        stooped_base(rot, sway=5 * math.sin(p))
        drop = legs_pose(rot, ST_KNEE, stride=16, lift=10, phase=p)
        arm(rot, -1, -ST_LEAN - 12 + 8 * math.sin(p), -15)
        arm(rot, 1, -ST_LEAN - 4 - 8 * math.sin(p), -10, spread=4)
        return rot, (0, 0, drop + 0.012 * abs(math.cos(p))), None

    def swing(t):
        if t < 0.45:
            k = t / 0.45
            up, lean = -ST_LEAN - 12 - 150 * k, ST_LEAN - 18 * k
        elif t < 0.75:
            k = (t - 0.45) / 0.3
            up, lean = -ST_LEAN - 162 + 190 * k, ST_LEAN - 18 + 30 * k
        else:
            k = (t - 0.75) / 0.25
            up, lean = -ST_LEAN + 28 - 40 * k, ST_LEAN + 12 - 12 * k
        rot = {}
        stooped_base(rot, lean=lean)
        arm(rot, -1, up, -30)
        rot["torso"] = rot_z(R(-12 * math.sin(t * math.pi))) @ rot["torso"]
        drop = legs_pose(rot, ST_KNEE + 6)
        rot["thigh_l"] = rot_y(R(-ST_KNEE - 22))
        return rot, (0.06 * math.sin(t * math.pi), 0, drop), None

    def hit(t):
        k = math.sin(t * math.pi)
        rot = {}
        stooped_base(rot, lean=ST_LEAN - 16 * k)
        rot["head"] = rot["head"] @ rot_x(R(14 * k))
        drop = legs_pose(rot, ST_KNEE)
        return rot, (-0.06 * k, 0, drop), None

    def die(t):
        k, rr, lift = fall(t, forward=True, delay=0.25)
        buckle = min(t / 0.3, 1.0)
        rot = {}
        stooped_base(rot, lean=ST_LEAN * (1 - k) + 5 * k)
        rot["head"] = rot_y(R(ST_HEAD * (1 - k) + 10 * k))
        arm(rot, -1, -100 * k - (1 - k) * (ST_LEAN + 12), -10)
        arm(rot, 1, -110 * k - (1 - k) * (ST_LEAN + 4), -10, spread=20 * k)
        drop = legs_pose(rot, ST_KNEE + 30 * buckle * (1 - k))
        return rot, (0, 0, drop * (1 - k) + lift), rr

    # hold the sickle cocked high, rip it down and across in one frame
    hook = retime(swing, 7, [(0, 0.25), (1, 0.45), (2, 0.68), (3, 0.78), (6, 1.0)])

    return [
        ("stance", stance, 4, 1400, "back_forth"),
        ("run", run, 8, 1100, "looped"),
        ("swing", hook, 7, 700, "play_once"),
        ("cast", hook, 7, 700, "play_once"),
        ("shoot", hook, 7, 700, "play_once"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 7, 1100, "play_once"),
    ]


# --- Hollowed Warden Corvin -----------------------------------------------------------------------------


def halberd(y=-0.28):
    top = HAND_Z + 1.2
    return [
        pole(1.1, 1.2, 0.026, "v_wood", y),
        P("box", ((0.03, y, HAND_Z - 1.08), (0.03, 0.03, 0.03)), "v_iron"),  # butt cap
        P("box", ((0.03, y, top - 0.1), (0.028, 0.028, 0.16)), "v_iron"),  # langets
        P("box", ((0.15, y, top - 0.12), (0.11, 0.013, 0.13)), "v_rust"),  # axe blade
        P("box", ((0.27, y, top - 0.12), (0.025, 0.014, 0.17)), "v_rust"),  # blade edge flares
        P("capsule", ((-0.0, y, top - 0.12), (-0.17, y, top - 0.06), 0.022), "v_rust"),  # back spike
        P("capsule", ((0.03, y, top), (0.03, y, top + 0.3), 0.024), "v_iron"),  # top spike
    ]


def hollowed_warden() -> Model:
    prims = merge(
        limbs("v_tarnish", "v_tarnish", "v_iron", "v_tarnish", "v_tarnish", "v_iron", r_arm=0.078, r_fore=0.066, r_thigh=0.095, r_shin=0.078),
        {
            "pelvis": [
                P("box", ((0, 0, 0.94), (0.14, 0.2, 0.13)), "v_tarnish"),  # faulds
                P("box", ((0, 0, 1.06), (0.15, 0.21, 0.03)), "v_brass"),
                P("box", ((0.16, 0, 0.74), (0.018, 0.13, 0.26)), "v_tabard"),
                P("box", ((-0.16, 0, 0.74), (0.018, 0.13, 0.26)), "v_tabard"),
                *strips(0.16, -0.13, 0.13, 0.5, 0.2, 5, "v_tabard", 11),
                *strips(-0.16, -0.13, 0.13, 0.5, 0.22, 5, "v_tabard", 12),
            ],
            "torso": [
                P("ellipsoid", ((0.01, 0, 1.3), (0.17, 0.25, 0.28)), "v_tarnish"),
                P("box", ((0.165, 0, 1.18), (0.018, 0.13, 0.2)), "v_tabard"),
                P("box", ((0.178, 0, 1.24), (0.008, 0.035, 0.08)), "v_brass"),  # faded sigil
                P("ellipsoid", ((0, 0, 1.52), (0.12, 0.15, 0.07)), "v_tarnish"),  # gorget
                P("ellipsoid", ((0, 0, 1.49), (0.125, 0.155, 0.03)), "v_brass"),
                P("box", ((-0.2, 0, 1.05), (0.02, 0.22, 0.4)), "v_cape"),  # torn cape
                *strips(-0.2, -0.22, 0.22, 0.66, 0.34, 7, "v_cape", 13),
            ],
            "head": [
                P("ellipsoid", ((0.02, 0, 1.7), (0.13, 0.12, 0.15)), "v_tarnish"),
                P("box", ((0.1, 0, 1.66), (0.045, 0.1, 0.09)), "v_tarnish"),  # faceplate
                P("box", ((0.148, 0, 1.71), (0.008, 0.08, 0.012)), "v_void"),  # visor slit
                P("box", ((0.155, 0.04, 1.71), (0.006, 0.012, 0.008)), "v_sheen"),
                P("box", ((0.155, -0.04, 1.71), (0.006, 0.012, 0.008)), "v_sheen"),
                P("box", ((0.0, 0, 1.86), (0.11, 0.016, 0.04)), "v_brass"),  # crest
                P("box", ((0.148, 0, 1.62), (0.006, 0.05, 0.03)), "v_void"),  # breaths
            ],
        },
    )
    for s, y in SIDES:
        prims[f"upper_arm_{s}"] += [
            P("ellipsoid", ((0, y * 0.27, 1.48), (0.14, 0.13, 0.1)), "v_tarnish"),  # pauldrons
            P("ellipsoid", ((0, y * 0.285, 1.42), (0.13, 0.12, 0.045)), "v_brass"),
        ]
        prims[f"forearm_{s}"].append(P("ellipsoid", ((0, y * 0.28, 1.2), (0.07, 0.07, 0.06)), "v_brass"))  # couters
        prims[f"shin_{s}"].append(P("ellipsoid", ((0.05, y * 0.09, 0.54), (0.07, 0.07, 0.07)), "v_brass"))  # poleyns
    prims["sword"] = halberd()
    return speckle(humanoid(prims), ("v_tarnish", "v_brass"), 0.14, 0.32, 21)


def warden_base(rot, lean=0.0, head_up=-26.0, twitch=0.0):
    """Too straight, head craned up and canted, right shoulder dropped: still at its post."""
    body(rot, lean=lean, roll=-5, twist=6, head=head_up - lean, head_roll=16 + twitch, head_turn=-10)


def warden_hold(rot, lean=0.0):
    arm(rot, -1, -12, -70, phi=0, lean=lean)
    arm(rot, 1, -8, -20, spread=6)


def warden_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = {}
        warden_base(rot, lean=1 + b, twitch=9 if t > 0.7 else 0)
        warden_hold(rot, 1 + b)
        drop = legs_pose(rot, 4)
        return rot, (0, 0, drop - 0.008 * b), None

    def run(t):
        p = t * 2 * math.pi
        rot = {}
        warden_base(rot, lean=4)
        arm(rot, -1, -12 + 6 * math.sin(p), -70, phi=10, lean=4)
        arm(rot, 1, -8 - 18 * math.sin(p), -20, spread=6)
        drop = legs_pose(rot, 6, stride=26, lift=30, phase=p)
        rot["torso"] = rot_z(R(5 * math.sin(p))) @ rot["torso"]
        return rot, (0, 0, drop + 0.03 * abs(math.cos(p)) - 0.02), None

    def sweep(t):
        # wind up over the right shoulder, cleave down-across, recover
        if t < 0.4:
            k = math.sin(t / 0.4 * math.pi / 2)
            up, fore, phi, lean, tw = -12 - 150 * k, -70 + 40 * k, -40 * k, -8 * k, 30 * k
        elif t < 0.7:
            k = (t - 0.4) / 0.3
            up, fore, phi, lean, tw = -162 + 120 * k, -30 - 10 * k, -40 + 160 * k, -8 + 36 * k, 30 - 60 * k
        else:
            k = (t - 0.7) / 0.3
            up, fore, phi, lean, tw = -42 + 30 * k, -40 - 30 * k, 120 - 120 * k, 28 - 28 * k, -30 + 30 * k
        rot = {}
        warden_base(rot, lean=lean)
        rot["torso"] = rot_z(R(tw)) @ rot["torso"]
        arm(rot, -1, up, fore, phi=phi, lean=lean)
        arm(rot, 1, up * 0.8, fore, spread=-14)
        drop = legs_pose(rot, 10)
        rot["thigh_l"] = rot_y(R(-34))
        rot["shin_l"] = rot_y(R(30))
        return rot, (0.1 * math.sin(t * math.pi), 0, drop), None

    def slam(t):
        # both hands raise the halberd straight overhead, then drive it into the ground
        if t < 0.45:
            k = math.sin(t / 0.45 * math.pi / 2)
            up, fore, phi, lean, knee, head = -12 - 160 * k, -70 + 60 * k, -15 * k, -10 * k, 4, -26 - 14 * k
        elif t < 0.65:
            k = (t - 0.45) / 0.2
            up, fore, phi, lean, knee, head = -172 + 120 * k, -10 - 25 * k, -15 + 115 * k, -10 + 45 * k, 4 + 26 * k, -40 + 30 * k
        else:
            k = (t - 0.65) / 0.35
            up, fore, phi, lean, knee, head = -52, -35, 100, 35 - 8 * k, 30 - 6 * k, -10
            # wrench the blade free and come back up to the post
            r = max(0.0, (t - 0.8) / 0.2)
            r = r * r * (3 - 2 * r)
            up, fore, phi, lean, knee, head = (
                a + (b - a) * r for a, b in zip((up, fore, phi, lean, knee, head), (-12, -70, 0, 0, 4, -26))
            )
        rot = {}
        warden_base(rot, lean=lean, head_up=head)
        arm(rot, -1, up, fore, phi=phi, lean=lean)
        arm(rot, 1, up, fore, spread=-18)
        drop = legs_pose(rot, knee)
        rot["thigh_l"] = rot_y(R(-knee - 20))
        back = min(max((t - 0.8) / 0.2, 0.0), 1.0)
        return rot, (0.08 * min(t / 0.65, 1.0) * (1 - back), 0, drop), None

    def hit(t):
        k = math.sin(t * math.pi)
        rot = {}
        warden_base(rot, lean=-8 * k, head_up=-26 - 20 * k, twitch=-12 * k)
        warden_hold(rot, -8 * k)
        drop = legs_pose(rot, 4)
        return rot, (-0.04 * k, 0, drop), None

    def die(t):
        kneel = min(t / 0.4, 1.0)
        k, rr, lift = fall(t, forward=True, delay=0.4)
        rot = {}
        warden_base(rot, lean=12 * kneel - 10 * k, head_up=-26 - 30 * kneel + 40 * k)
        arm(rot, -1, -12 - 60 * kneel - 40 * k, -70 + 50 * kneel, phi=60 * kneel - 70 * k, lean=12 * kneel)
        arm(rot, 1, -8 - 90 * k, -20, spread=10 + 20 * k)
        for s in "rl":
            rot[f"thigh_{s}"] = rot_y(R(-10 * kneel))
            rot[f"shin_{s}"] = rot_y(R(95 * kneel))
        return rot, (0, 0, -0.44 * kneel * (1 - k) + lift), rr

    # Corvin telegraphs: a slow climb to the wind-up, a held beat, then the blow lands in one frame.
    sweep9 = retime(sweep, 9, [(0, 0.15), (1, 0.32), (2, 0.4), (3, 0.62), (4, 0.7), (5, 0.76), (8, 1.0)])
    slam9 = retime(slam, 9, [(0, 0.2), (1, 0.38), (2, 0.45), (3, 0.65), (4, 0.75), (8, 1.0)])

    return [
        ("stance", stance, 4, 1600, "back_forth"),
        ("run", run, 8, 900, "looped"),
        ("swing", sweep9, 9, 900, "play_once"),
        ("swing2", slam9, 9, 900, "play_once"),
        ("cast", slam9, 9, 900, "play_once"),
        ("shoot", slam9, 9, 900, "play_once"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 8, 1400, "play_once"),
    ]


# --- Ysolde, cairnkeeper ------------------------------------------------------------------------------


def cairnkeeper() -> Model:
    prims = merge(
        limbs("v_shawl", "v_shawl", "v_weath", None, None, "v_brig", r_arm=0.06, r_fore=0.05),
        {
            "pelvis": [
                P("ellipsoid", ((0, 0, 0.55), (0.21, 0.25, 0.56)), "v_shawl"),  # long skirt
                P("ellipsoid", ((0, 0, 0.18), (0.24, 0.27, 0.16)), "v_shawl2"),
                P("capsule", ((0.12, -0.17, 1.03), (0.12, 0.17, 1.03), 0.02), "v_sack"),  # rope belt
                P("ellipsoid", ((0.06, 0.21, 0.9), (0.055, 0.045, 0.065)), "v_brig"),  # ember pouch
                P("box", ((0.06, 0.21, 0.96), (0.03, 0.03, 0.012)), "v_ember"),
                P("capsule", ((0.06, 0.21, 0.96), (0.1, 0.18, 1.03), 0.01), "v_sack"),
            ],
            "torso": [
                P("ellipsoid", ((0, 0, 1.27), (0.13, 0.19, 0.24)), "v_shawl"),
                P("ellipsoid", ((0, 0, 1.35), (0.17, 0.25, 0.16)), "v_shawlred"),  # inner shawl
                *strips(0.15, -0.2, 0.2, 1.24, 0.14, 7, "v_shawlred", 31),
                P("ellipsoid", ((-0.02, 0, 1.42), (0.16, 0.26, 0.11)), "v_shawl2"),  # outer shawl
                *strips(-0.13, -0.22, 0.22, 1.36, 0.22, 7, "v_shawl2", 32),
            ],
            "head": [
                P("ellipsoid", ((0.04, 0, 1.63), (0.085, 0.078, 0.1)), "v_weath"),
                P("ellipsoid", ((0.125, 0, 1.62), (0.025, 0.02, 0.03)), "v_weath"),  # nose
                P("box", ((0.112, 0.03, 1.665), (0.008, 0.014, 0.006)), "v_void"),
                P("box", ((0.112, -0.03, 1.665), (0.008, 0.014, 0.006)), "v_void"),
                P("box", ((0.09, 0.06, 1.71), (0.02, 0.02, 0.05)), "v_bone"),  # grey wisps
                P("box", ((0.09, -0.06, 1.71), (0.02, 0.02, 0.05)), "v_bone"),
                P("ellipsoid", ((-0.045, 0, 1.7), (0.13, 0.125, 0.15)), "v_shawl2"),  # hood
                P("ellipsoid", ((-0.08, 0, 1.55), (0.1, 0.16, 0.12)), "v_shawl2"),
            ],
            "sword": [
                pole(0.86, 0.42, 0.016, "v_iron"),
                P("ellipsoid", ((0.03, -0.28, HAND_Z + 0.45), (0.03, 0.015, 0.035)), "v_iron"),  # grip loop
                P("capsule", ((0.03, -0.28, HAND_Z - 0.86), (0.1, -0.28, HAND_Z - 0.8), 0.014), "v_iron"),  # hook
                P("ellipsoid", ((0.03, -0.28, HAND_Z - 0.84), (0.026, 0.026, 0.035)), "v_ember"),
            ],
        },
    )
    return humanoid(prims)


YS_LEAN = 16.0


def keeper_base(rot, lean=YS_LEAN, poke=0.0):
    body(rot, lean=lean, head=6 - 6)
    arm(rot, -1, -22, -50, phi=8 + poke, lean=lean)
    arm(rot, 1, -lean - 30, -70, spread=-10)  # hand over the pouch


def keeper_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = {}
        keeper_base(rot, lean=YS_LEAN + 1.5 * b)
        drop = legs_pose(rot, 6)
        return rot, (0, 0, drop - 0.006 * b), None

    def run(t):
        p = t * 2 * math.pi
        rot = {}
        keeper_base(rot, poke=10 * math.sin(p))
        drop = legs_pose(rot, 8, stride=16, lift=14, phase=p)
        return rot, (0, 0, drop + 0.012 * abs(math.cos(p))), None

    def hit(t):
        k = math.sin(t * math.pi)
        rot = {}
        keeper_base(rot, lean=YS_LEAN - 12 * k)
        drop = legs_pose(rot, 6)
        return rot, (-0.04 * k, 0, drop), None

    def die(t):
        k, rr, lift = fall(t, forward=False, delay=0.1)
        rot = {}
        keeper_base(rot, lean=YS_LEAN * (1 - k) - 10 * k)
        drop = legs_pose(rot, 6 + 20 * k)
        return rot, (0, 0, drop * (1 - k) + lift), rr

    return [
        ("stance", stance, 4, 1600, "back_forth"),
        ("run", run, 8, 1000, "looped"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 6, 1000, "play_once"),
    ]


# --- lightworker --------------------------------------------------------------------------------------------


def lightworker() -> Model:
    prims = merge(
        limbs("v_linen", "v_weath", "v_weath", "v_sack", "v_weath", "v_brig", r_arm=0.05, r_fore=0.036, r_thigh=0.062, r_shin=0.042),
        {
            "pelvis": [
                P("box", ((0, 0, 0.94), (0.11, 0.165, 0.1)), "v_sack"),
                *strips(0.11, -0.15, 0.15, 0.86, 0.12, 5, "v_linen", 41),
            ],
            "torso": [
                P("ellipsoid", ((0, 0, 1.28), (0.12, 0.175, 0.24)), "v_linen"),
                P("ellipsoid", ((-0.21, 0, 1.2), (0.12, 0.17, 0.21)), "v_sack"),  # sack on the back
                P("box", ((-0.21, 0, 1.4), (0.05, 0.08, 0.04)), "v_sack"),
                P("capsule", ((0.1, -0.13, 1.4), (-0.14, -0.14, 1.44), 0.022), "v_brig"),
                P("capsule", ((0.1, 0.13, 1.4), (-0.14, 0.14, 1.44), 0.022), "v_brig"),
                P("capsule", ((0, 0, 1.44), (0.02, 0, 1.56), 0.035), "v_weath"),
            ],
            "head": [
                P("ellipsoid", ((0.03, 0, 1.66), (0.085, 0.078, 0.105)), "v_weath"),
                P("ellipsoid", ((0.0, 0, 1.79), (0.1, 0.1, 0.07)), "v_straw"),
                *brim(0.0, 1.755, 0.23, 16, {6}, "v_straw", 43),
                P("box", ((0.17, 0, 1.64), (0.01, 0.13, 0.11)), "v_veil"),  # face veil from the brim
                *strips(0.17, -0.13, 0.13, 1.54, 0.08, 5, "v_veil", 44, half_x=0.009),
            ],
            "sword": [
                pole(1.05, 0.3, 0.02, "v_wood"),
                P("box", ((0.09, -0.28, HAND_Z - 1.04), (0.07, 0.065, 0.012)), "v_iron"),  # hoe blade
            ],
        },
    )
    return humanoid(prims)


LW_LEAN = 14.0


def worker_base(rot, lean=LW_LEAN, phi=4.0):
    body(rot, lean=lean, head=10)
    arm(rot, -1, -26, -42, phi=phi, lean=lean)
    arm(rot, 1, -lean - 40, -50, spread=-16)  # left hand on the shaft too


def worker_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = {}
        worker_base(rot, lean=LW_LEAN + 2 * b)
        rot["head"] = rot["head"] @ rot_z(R(5 * b))
        drop = legs_pose(rot, 6)
        return rot, (0, 0, drop - 0.006 * b), None

    def run(t):
        p = t * 2 * math.pi
        rot = {}
        body(rot, lean=LW_LEAN + 4, head=12)
        arm(rot, -1, -20 - 10 * math.sin(p), -50, phi=40, lean=LW_LEAN + 4)  # hoe over the shoulder
        arm(rot, 1, -LW_LEAN + 18 * math.sin(p), -20)
        drop = legs_pose(rot, 8, stride=24, lift=24, phase=p)
        return rot, (0, 0, drop + 0.02 * abs(math.cos(p))), None

    def hit(t):
        k = math.sin(t * math.pi)
        rot = {}
        worker_base(rot, lean=LW_LEAN - 12 * k)
        drop = legs_pose(rot, 6)
        return rot, (-0.04 * k, 0, drop), None

    def die(t):
        k, rr, lift = fall(t, forward=False, delay=0.1)
        rot = {}
        worker_base(rot, lean=LW_LEAN * (1 - k) - 10 * k, phi=4 - 60 * k)
        drop = legs_pose(rot, 6 + 20 * k)
        return rot, (0, 0, drop * (1 - k) + lift), rr

    return [
        ("stance", stance, 4, 1500, "back_forth"),
        ("run", run, 8, 760, "looped"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 6, 1000, "play_once"),
    ]


# --- Lowshade guard --------------------------------------------------------------------------------------------


def lowshade_guard() -> Model:
    rivets = [P("box", ((0.135, y, z), (0.012, 0.012, 0.012)), "v_brass") for y in (-0.1, -0.03, 0.04, 0.11) for z in (1.12, 1.24)]
    prims = merge(
        limbs("v_wool", "v_brig", "v_weath", "v_wool", "v_brig", "v_brig", r_arm=0.065, r_fore=0.055),
        {
            "pelvis": [
                P("box", ((0, 0, 0.95), (0.13, 0.19, 0.11)), "v_brig"),
                *strips(0.13, -0.18, 0.18, 0.88, 0.14, 5, "v_brig", 51),
            ],
            "torso": [
                P("ellipsoid", ((0, 0, 1.28), (0.15, 0.21, 0.26)), "v_brig"),
                *rivets,
                P("ellipsoid", ((-0.02, 0, 1.4), (0.2, 0.28, 0.16)), "v_wool"),  # mantle over the shoulders
                P("box", ((-0.17, 0, 1.12), (0.03, 0.25, 0.3)), "v_wool"),
                *strips(-0.17, -0.25, 0.25, 0.84, 0.18, 7, "v_wool", 52),
                *strips(0.16, -0.26, 0.26, 1.3, 0.12, 8, "v_wool", 53),
                # portable cover: an oilcloth canopy on a pole strapped to the back
                P("capsule", ((-0.21, 0.12, 0.98), (-0.21, 0.12, 2.12), 0.016), "v_wood"),
                P("box", ((0.0, 0.02, 2.12), (0.25, 0.26, 0.014)), "v_oil"),
                P("box", ((-0.21, 0.12, 2.1), (0.02, 0.02, 0.02)), "v_iron"),
                *strips(0.25, -0.26, 0.26, 2.11, 0.1, 6, "v_oil", 54, half_x=0.008),
            ],
            "head": [
                P("ellipsoid", ((0.04, 0, 1.65), (0.085, 0.08, 0.1)), "v_weath"),
                P("box", ((0.112, 0.03, 1.675), (0.008, 0.014, 0.008)), "v_void"),
                P("box", ((0.112, -0.03, 1.675), (0.008, 0.014, 0.008)), "v_void"),
                P("box", ((0.11, 0, 1.58), (0.025, 0.06, 0.03)), "v_hair_dk"),
                P("ellipsoid", ((-0.015, 0, 1.69), (0.13, 0.125, 0.14)), "v_wool"),  # deep hood
                P("box", ((0.11, 0, 1.79), (0.07, 0.115, 0.016)), "v_wool"),  # stiff peak
            ],
            "sword": [
                pole(1.0, 1.05, 0.018, "v_wood"),
                P("ellipsoid", ((0.03, -0.28, HAND_Z + 1.16), (0.014, 0.035, 0.12)), "v_iron"),
            ],
            "item_l": [
                P("capsule", ((0.03, 0.28, HAND_Z), (0.03, 0.28, HAND_Z - 0.08), 0.01), "v_iron"),
                P("box", ((0.03, 0.28, HAND_Z - 0.09), (0.055, 0.055, 0.014)), "v_iron"),
                P("box", ((0.03, 0.28, HAND_Z - 0.25), (0.055, 0.055, 0.014)), "v_iron"),
                P("ellipsoid", ((0.03, 0.28, HAND_Z - 0.17), (0.042, 0.042, 0.07)), "v_fire"),
                *[P("capsule", ((0.03 + dx, 0.28 + dy, HAND_Z - 0.09), (0.03 + dx, 0.28 + dy, HAND_Z - 0.25), 0.009), "v_iron")
                  for dx, dy in ((0.05, 0.05), (0.05, -0.05), (-0.05, 0.05), (-0.05, -0.05))],
            ],
        },
    )
    return humanoid(prims)


GD_LEAN = 3.0


def guard_base(rot, lean=GD_LEAN, spear=0.0, lantern=None):
    body(rot, lean=lean, head=4)
    arm(rot, -1, -14, -62, phi=spear, lean=lean)
    upper = -22 if lantern is None else lantern
    arm(rot, 1, upper, -28, phi=0, item="item_l", lean=lean)


def guard_anims():
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        rot = {}
        guard_base(rot, lean=GD_LEAN + b)
        rot["head"] = rot["head"] @ rot_z(R(10 * b))  # watching the fields
        drop = legs_pose(rot, 3)
        return rot, (0, 0, drop - 0.006 * b), None

    def run(t):
        p = t * 2 * math.pi
        rot = {}
        guard_base(rot, lean=GD_LEAN + 4, spear=8, lantern=-22 - 14 * math.sin(p))
        drop = legs_pose(rot, 6, stride=30, lift=34, phase=p)
        return rot, (0, 0, drop + 0.03 * abs(math.cos(p)) - 0.02), None

    def thrust(t):
        if t < 0.35:
            k = t / 0.35
            up, fore, phi, off = -14 - 40 * k, -62 + 2 * k, 70 * k, -0.04 * k
        elif t < 0.6:
            k = (t - 0.35) / 0.25
            up, fore, phi, off = -54 - 40 * k, -60 + 55 * k, 70 + 20 * k, -0.04 + 0.16 * k
        else:
            k = (t - 0.6) / 0.4
            up, fore, phi, off = -94 + 80 * k, -5 - 57 * k, 90 - 90 * k, 0.12 - 0.12 * k
        rot = {}
        guard_base(rot, lean=GD_LEAN + 10 * math.sin(t * math.pi))
        arm(rot, -1, up, fore, phi=phi, lean=GD_LEAN + 10 * math.sin(t * math.pi))
        drop = legs_pose(rot, 8)
        rot["thigh_l"] = rot_y(R(-30))
        return rot, (off, 0, drop), None

    def hit(t):
        k = math.sin(t * math.pi)
        rot = {}
        guard_base(rot, lean=GD_LEAN - 12 * k)
        drop = legs_pose(rot, 3)
        return rot, (-0.05 * k, 0, drop), None

    def die(t):
        k, rr, lift = fall(t, forward=False, delay=0.1)
        rot = {}
        guard_base(rot, lean=GD_LEAN - 12 * k, spear=-50 * k)
        drop = legs_pose(rot, 3 + 25 * k)
        return rot, (0, 0, drop * (1 - k) + lift), rr

    jab = retime(thrust, 6, [(0, 0.2), (1, 0.35), (2, 0.58), (3, 0.65), (5, 1.0)])

    return [
        ("stance", stance, 4, 1800, "back_forth"),
        ("run", run, 8, 680, "looped"),
        ("swing", jab, 6, 600, "play_once"),
        ("cast", jab, 6, 600, "play_once"),
        ("shoot", jab, 6, 600, "play_once"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 6, 1000, "play_once"),
    ]


vox.RAMPS["v_hair_dk"] = lit("#3a2c24")
vox.RAMP_RGB["v_hair_dk"] = np.array([vox.hex_rgb(c) for c in vox.RAMPS["v_hair_dk"]], dtype=np.uint8)
vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])

# The player renders at 0.85 in a 160 px frame; people match that, Corvin stands ~1.3x taller.
HUMAN = 0.85
# model -> (builder, anims, render scale, frame size, foot)
# Attack frame where the blow lands (`hit=` in the script, see sheet.export).
HITS = {
    "glarewolf": {"swing": 2, "swing2": 3, "cast": 2, "shoot": 2},
    "stooped": {"swing": 2, "cast": 2, "shoot": 2},
    "hollowed_warden": {"swing": 3, "swing2": 3, "cast": 3, "shoot": 3},
    "lowshade_guard": {"swing": 2, "cast": 2, "shoot": 2},
}
# Weapon smear layers: model -> (bone, base, tip) in rest pose (None = lowest voxel of the bone).
SMEARS = {
    "stooped": ("sword", None),
    "hollowed_warden": ("sword", ((0.03, -0.28, HAND_Z + 0.55), (0.27, -0.28, HAND_Z + 1.08))),
}

FOLK = {
    "glarewolf": (glarewolf, wolf_anims, 1.0, 128, (64, 92)),
    "stooped": (stooped, stooped_anims, HUMAN, 160, (80, 112)),
    "hollowed_warden": (hollowed_warden, warden_anims, HUMAN * 1.3, 256, (128, 196)),
    "cairnkeeper": (cairnkeeper, keeper_anims, HUMAN, 160, (80, 112)),
    "lightworker": (lightworker, worker_anims, HUMAN, 160, (80, 112)),
    "lowshade_guard": (lowshade_guard, guard_anims, HUMAN, 176, (88, 132)),
}


def job(name: str) -> str:
    """Renders and exports one model (+ its smear layer); runs in a worker process."""
    build, anims_fn, scale, frame, foot = FOLK[name]
    model = build()
    anims = anims_fn()
    renders = sheet.render_all(model, anims, frame, foot, scale)
    hits = HITS.get(name)
    script_dir = sheet.OUT / "scripts" / "npc"
    sheet.export(renders, anims, foot, f"custom_npc_{name}.png", script_dir / f"{name}.txt", hits=hits)
    sheet.preview(renders, anims, frame, f"npc_{name}")
    if name in SMEARS:
        bone, blade = SMEARS[name]
        base, tip = blade or smear.blade_from_part(model, bone, base_frac=0.4)
        attacks = [a for a in ("swing", "swing2") if any(r[0] == a for r in anims)]
        trail = smear.render_layer(model, anims, frame, foot, scale, base, tip, hits, bone, attacks)
        rows = smear.anims_for(anims, trail)
        sheet.export(trail, rows, foot, f"custom_npc_{name}_smear.png", script_dir / f"{name}_smear.txt", 512)
        sheet.preview(trail, rows, frame, f"npc_{name}_smear")
    return f"{name}: {sum(len(p[0]) for p in model.parts.values())} voxels"


def main(only: list[str]):
    names = [n for n in FOLK if not only or n in only]
    with ProcessPoolExecutor(max_workers=6) as pool:
        for line in pool.map(job, names):
            print(line, flush=True)


if __name__ == "__main__":
    main(sys.argv[1:])
