"""Base adventurer: model, animations and sprite-sheet export in the game's sprite-script format.

Usage (from repo root):
    python -I tools/artgen/character.py            # writes custom_assets/...
    python -I tools/artgen/character.py --preview  # preview strips into custom_assets/preview/

Output: `custom_assets/content/custom/custom_adventurer.png` + `custom_assets/scripts/player/custom/adventurer.txt`
using the same `[anim] frames= duration= type= frame=F,D,x,y,w,h,px,py` format as
`scripts/player/male/*.txt` (see docs/formats.md), so the engine renders it like original sprites.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).parent))
import sheet  # noqa: E402
import smear  # noqa: E402
from keyframes import keyed  # noqa: E402
from vox import Bone, Model, Prim, rot_x, rot_y, rot_z  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets"
NAME = "adventurer"

FRAME = 160
FOOT = (80, 112)  # room below the feet for bodies falling toward the camera
# Original player sprites stand ~60 px tall; the 1.8-unit model would be ~70 px.
SCALE = 0.85


def build() -> Model:
    """~1.8 units tall human in a leather jerkin with hood, belt, boots and a short sword.
    Facing +x; left side is +y."""
    B, P = Bone, Prim
    bones = [
        B("pelvis", None, (0, 0, 0.95), [
            P("box", ((0, 0, 0.98), (0.11, 0.17, 0.09)), "cloth"),
            P("box", ((0.0, 0, 1.06), (0.125, 0.185, 0.035)), "leather"),  # belt
            P("box", ((0.12, 0.0, 1.06), (0.02, 0.035, 0.03)), "gold"),  # buckle
        ]),
        B("torso", "pelvis", (0, 0, 1.05), [
            P("ellipsoid", ((0, 0, 1.3), (0.15, 0.22, 0.26)), "leather"),
            P("box", ((0.0, 0, 1.15), (0.14, 0.2, 0.1)), "leather"),
            P("capsule", ((0.13, -0.08, 1.45), (0.14, 0.08, 1.45), 0.035), "red"),  # collar / scarf
            P("ellipsoid", ((0, -0.23, 1.47), (0.09, 0.08, 0.06)), "leather"),  # shoulder pads
            P("ellipsoid", ((0, 0.23, 1.47), (0.09, 0.08, 0.06)), "leather"),
            P("box", ((-0.16, 0, 1.2), (0.025, 0.2, 0.27)), "red"),  # short cape
        ]),
        B("head", "torso", (0, 0, 1.55), [
            P("ellipsoid", ((0.03, 0, 1.67), (0.1, 0.09, 0.12)), "skin"),
            P("ellipsoid", ((-0.05, 0, 1.73), (0.11, 0.118, 0.12)), "red"),  # hood, pulled back off the face
            P("ellipsoid", ((-0.1, 0, 1.6), (0.07, 0.12, 0.12)), "red"),  # hood back
            P("box", ((0.1, 0, 1.6), (0.03, 0.07, 0.035)), "hair"),  # beard
            P("box", ((0.122, 0.035, 1.69), (0.008, 0.014, 0.01)), "hair"),  # eyes
            P("box", ((0.122, -0.035, 1.69), (0.008, 0.014, 0.01)), "hair"),
        ]),
        B("upper_arm_r", "torso", (0, -0.25, 1.46), [P("capsule", ((0, -0.26, 1.44), (0, -0.28, 1.2), 0.066), "cloth")]),
        B("forearm_r", "upper_arm_r", (0, -0.28, 1.2), [
            P("capsule", ((0, -0.28, 1.2), (0.02, -0.28, 0.98), 0.05), "leather"),  # bracer
            P("ellipsoid", ((0.03, -0.28, 0.93), (0.045, 0.04, 0.05)), "skin"),
        ]),
        B("sword", "forearm_r", (0.03, -0.28, 0.93), [
            P("box", ((0.03, -0.28, 0.93), (0.025, 0.07, 0.02)), "gold"),  # crossguard
            P("capsule", ((0.03, -0.28, 0.9), (0.03, -0.28, 0.96), 0.02), "wood"),  # grip
            P("box", ((0.03, -0.28, 0.6), (0.015, 0.03, 0.3)), "metal"),  # blade hangs down in rest pose
        ]),
        B("upper_arm_l", "torso", (0, 0.25, 1.46), [P("capsule", ((0, 0.26, 1.44), (0, 0.28, 1.2), 0.066), "cloth")]),
        B("forearm_l", "upper_arm_l", (0, 0.28, 1.2), [
            P("capsule", ((0, 0.28, 1.2), (0.02, 0.28, 0.98), 0.05), "leather"),
            P("ellipsoid", ((0.03, 0.28, 0.93), (0.045, 0.04, 0.05)), "skin"),
        ]),
        B("thigh_r", "pelvis", (0, -0.09, 0.95), [P("capsule", ((0, -0.09, 0.95), (0, -0.09, 0.53), 0.085), "cloth")]),
        B("shin_r", "thigh_r", (0, -0.09, 0.53), [
            P("capsule", ((0, -0.09, 0.53), (0, -0.09, 0.1), 0.066), "boot"),
            P("box", ((0.07, -0.09, 0.045), (0.11, 0.055, 0.045)), "boot"),
        ]),
        B("thigh_l", "pelvis", (0, 0.09, 0.95), [P("capsule", ((0, 0.09, 0.95), (0, 0.09, 0.53), 0.085), "cloth")]),
        B("shin_l", "thigh_l", (0, 0.09, 0.53), [
            P("capsule", ((0, 0.09, 0.53), (0, 0.09, 0.1), 0.066), "boot"),
            P("box", ((0.07, 0.09, 0.045), (0.11, 0.055, 0.045)), "boot"),
        ]),
    ]
    return Model(bones)


# --- animations -----------------------------------------------------------------------
# Each returns (rotations, root_offset, root_rot) for t in [0, 1).
# rot_y(a): negative a swings a hanging limb forward (+x); positive a tips an upright body forward.

R = math.radians


def guard(rot):
    """Sword held in front, pointing forward-up (the default combat ready pose)."""
    rot.setdefault("upper_arm_r", rot_y(R(-25)))
    rot.setdefault("forearm_r", rot_y(R(-55)))
    rot.setdefault("sword", rot_y(R(-120)))
    rot.setdefault("upper_arm_l", rot_y(R(-10)) @ rot_x(R(-8)))
    rot.setdefault("forearm_l", rot_y(R(-35)))
    return rot


def stance(t):
    breathe = math.sin(t * 2 * math.pi)
    rot = guard({"torso": rot_y(R(2 + 1.5 * breathe)), "head": rot_y(R(-2 - 1.5 * breathe))})
    return rot, (0, 0, -0.008 * (1 + breathe)), None


# --- run cycle --------------------------------------------------------------------------------------
# Feet are planted: while a foot is on the ground it slides back exactly as fast as the client
# moves the unit (player.rs RUN_SPEED cells/s; one model unit renders as SCALE cells), and the
# leg angles come from 2-bone IK on that foot path, so nothing skates. Stance 25% of the cycle per
# foot, the rest is swing (heel kicks up behind, knee drives forward) with short flights between.

RUN_SPEED = 4.0  # cells/s, crates/dusk_client/src/player.rs
RUN_FRAMES, RUN_MS = 10, 660
RUN_STANCE = 0.25
THIGH, SHIN = 0.42, 0.43  # hip 0.95 -> knee 0.53 -> ankle 0.10 in rest pose
ANKLE_Z = 0.10
RUN_HALF = RUN_SPEED / SCALE * RUN_STANCE * RUN_MS / 1000 / 2  # foot x at touchdown (model units)
RUN_LIFT = 0.2
RUN_HIP = -0.095  # pelvis drop at touchdown/toe-off (keeps the planted leg within reach)


def leg_ik(fx: float, fz: float):
    """(thigh, shin) rot_y angles (radians) putting the ankle at (fx, fz) from the hip, knee forward."""
    d = min(math.hypot(fx, fz), (THIGH + SHIN) * 0.999)
    phi = math.atan2(-fx, -fz)
    alpha = math.acos(max(-1.0, min(1.0, (THIGH**2 + d * d - SHIN**2) / (2 * THIGH * d))))
    knee = math.acos(max(-1.0, min(1.0, (THIGH**2 + SHIN**2 - d * d) / (2 * THIGH * SHIN))))
    return phi - alpha, math.pi - knee


def foot_path(p: float):
    """Ankle (x, lift) relative to the hip's ground point at cycle phase p (0 = touchdown)."""
    p %= 1.0
    if p < RUN_STANCE:
        return RUN_HALF - 2 * RUN_HALF * p / RUN_STANCE, 0.0
    q = (p - RUN_STANCE) / (1 - RUN_STANCE)
    ease = q * q * (3 - 2 * q)
    return -RUN_HALF + 2 * RUN_HALF * ease, RUN_LIFT * math.sin(math.pi * q**0.75)


def run_hip(t: float) -> float:
    """Pelvis height offset: lowest mid-stance, highest in flight (two bobs per cycle)."""
    return RUN_HIP - 0.025 * math.cos(4 * math.pi * (t - RUN_STANCE / 2))


def run(t):
    a = math.cos(t * 2 * math.pi)
    dz = run_hip(t)
    rot = {}
    for side, phase in (("r", 0.0), ("l", 0.5)):
        x, lift = foot_path(t + phase)
        hip_z = 0.95 + dz
        thigh, shin = leg_ik(x, ANKLE_Z + lift - hip_z)
        rot[f"thigh_{side}"] = rot_y(thigh)
        rot[f"shin_{side}"] = rot_y(shin)
    # Right foot forward at t=0 -> left arm forward, shoulders counter-rotated.
    rot.update(guard({
        "torso": rot_z(R(-9 * a)) @ rot_y(R(10)),
        "head": rot_z(R(8 * a)) @ rot_y(R(-8)),
        "upper_arm_l": rot_y(R(-32 * a)) @ rot_x(R(-8)),
        "forearm_l": rot_y(R(-50 + 15 * a)),
        "upper_arm_r": rot_y(R(-25 + 14 * a)),
    }))
    return rot, (0, 0, dz), None


# --- attacks (keyframed, see keyframes.py) ----------------------------------------------------
# Every attack: a short coiled anticipation (frames 0-1, the second a hold), the strike snapped
# into one frame (`HITS`: the frame where the blow lands, exported as `hit=` ms so the client can
# line damage up with it), a held follow-through, then an eased recovery into the guard.

GUARD = {
    "torso": (0, 2, 0),
    "head": (0, -2, 0),
    "upper_arm_r": (0, -25, 0),
    "forearm_r": (0, -55, 0),
    "sword": (0, -120, 0),
    "upper_arm_l": (-8, -10, 0),
    "forearm_l": (0, -35, 0),
    "thigh_r": (0, 0, 0),
    "shin_r": (0, 0, 0),
    "thigh_l": (0, 0, 0),
    "shin_l": (0, 0, 0),
    "root": (0, 0, 0),
}
RECOVER = dict(GUARD)

# Horizontal forehand slash, right to left: coil the torso right with the blade cocked over the
# right shoulder, whip across at chest height on a step-in, finish wrapped around to the left.
swing = keyed(8, GUARD, [
    (0, {"torso": (0, -6, -30), "head": (0, 0, 22), "upper_arm_r": (-80, 0, -45), "forearm_r": (0, -70, 0),
         "sword": (0, 30, 0), "upper_arm_l": (10, -45, 25), "forearm_l": (0, -40, 0),
         "thigh_l": (0, -18, 0), "shin_l": (0, 14, 0), "thigh_r": (0, 10, 0), "root": (-0.02, 0, -0.02)}, "lin"),
    (1, {"torso": (0, -8, -45), "head": (0, 0, 32), "upper_arm_r": (-85, 0, -70), "forearm_r": (0, -85, 0),
         "sword": (0, 45, 0), "root": (-0.03, 0, -0.03)}, "out"),
    (2, {"torso": (0, 8, 10), "head": (0, 0, -6), "upper_arm_r": (-80, 0, 75), "forearm_r": (0, -15, 0),
         "sword": (0, -10, 0), "upper_arm_l": (0, -20, -10), "thigh_l": (0, -32, 0), "shin_l": (0, 28, 0),
         "thigh_r": (0, 18, 0), "root": (0.1, 0, -0.06)}, "in"),
    (3, {"torso": (0, 10, 45), "head": (0, 0, -30), "upper_arm_r": (-75, 0, 140), "forearm_r": (0, -25, 0),
         "sword": (0, -30, 0), "upper_arm_l": (10, 10, -20), "root": (0.12, 0, -0.06)}, "out"),
    (4, {"torso": (0, 9, 50), "head": (0, 0, -32), "upper_arm_r": (-65, 0, 150), "sword": (0, -40, 0)}, "out"),
    (5, {"torso": (0, 6, 30), "head": (0, 0, -20), "upper_arm_r": (-40, -20, 90), "forearm_r": (0, -45, 0),
         "sword": (0, -80, 0)}, "io"),
    (7, RECOVER, "io"),
])

# Overhead chop: both hands rise high behind the head, the back arches, then the blade is driven
# straight down in front on a lunging step, the torso folding over it.
swing2 = keyed(8, GUARD, [
    (0, {"torso": (0, -10, -10), "head": (0, -6, 0), "upper_arm_r": (-10, -150, 0), "forearm_r": (0, -50, 0),
         "sword": (0, -40, 0), "upper_arm_l": (0, -140, 10), "forearm_l": (0, -60, 0),
         "thigh_l": (0, -10, 0), "root": (-0.02, 0, 0.01)}, "lin"),
    (1, {"torso": (0, -16, -12), "head": (0, -10, 0), "upper_arm_r": (-10, -175, 0), "forearm_r": (0, -80, 0),
         "sword": (0, -60, 0), "upper_arm_l": (0, -165, 5), "forearm_l": (0, -80, 0), "root": (-0.03, 0, 0.02)}, "out"),
    (2, {"torso": (0, 30, 0), "head": (0, -15, 0), "upper_arm_r": (-10, -70, 0), "forearm_r": (0, -10, 0),
         "sword": (0, 15, 0), "upper_arm_l": (0, -70, -5), "forearm_l": (0, -20, 0),
         "thigh_l": (0, -38, 0), "shin_l": (0, 34, 0), "thigh_r": (0, 20, 0), "shin_r": (0, 10, 0),
         "root": (0.12, 0, -0.09)}, "in3"),
    (3, {"torso": (0, 36, 0), "upper_arm_r": (-10, -45, 0), "forearm_r": (0, -10, 0), "sword": (0, 20, 0),
         "upper_arm_l": (0, -50, -5), "root": (0.13, 0, -0.1)}, "out"),
    (4, {"torso": (0, 34, 0)}, "out"),
    (7, RECOVER, "io"),
])

# Thrust: chamber the blade at the hip with the elbow drawn back, then lunge, driving the point
# straight at the target with the off hand flung back for balance.
swing3 = keyed(8, GUARD, [
    (0, {"torso": (0, -4, -35), "head": (0, 0, 25), "upper_arm_r": (-20, 10, 0), "forearm_r": (0, -95, 0),
         "sword": (0, -5, 0), "upper_arm_l": (0, -60, 20), "forearm_l": (0, -30, 0),
         "thigh_r": (0, 12, 0), "thigh_l": (0, -10, 0), "root": (-0.04, 0, -0.03)}, "lin"),
    (1, {"torso": (0, -6, -40), "head": (0, 0, 30), "upper_arm_r": (-20, 20, 0), "root": (-0.05, 0, -0.04)}, "out"),
    (2, {"torso": (0, 14, 20), "head": (0, 0, -15), "upper_arm_r": (0, -95, 15), "forearm_r": (0, -5, 0),
         "sword": (0, -15, 0), "upper_arm_l": (-20, 10, -10), "forearm_l": (0, -20, 0),
         "thigh_l": (0, -45, 0), "shin_l": (0, 40, 0), "thigh_r": (0, 25, 0), "root": (0.18, 0, -0.1)}, "in3"),
    (3, {"torso": (0, 16, 25), "head": (0, 0, -18), "root": (0.2, 0, -0.11)}, "out"),
    (4, {}, "lin"),
    (7, RECOVER, "io"),
])


def hit(t):
    k = math.sin(t * math.pi)
    rot = guard({"torso": rot_y(R(14 * k)), "head": rot_y(R(10 * k)), "upper_arm_l": rot_y(R(25 * k))})
    return rot, (-0.05 * k, 0, 0), None


def block(t):
    k = math.sin(min(t * 2, 1) * math.pi / 2)
    rot = guard({
        "upper_arm_l": rot_y(R(-70 * k)) @ rot_x(R(-20 * k)),
        "forearm_l": rot_y(R(-60 * k)),
        "upper_arm_r": rot_y(R(-60 * k)),
        "forearm_r": rot_y(R(-70)),
        "torso": rot_y(R(-6 * k)),
    })
    return rot, (-0.02 * k, 0, -0.03 * k), None


# Gather the power low at the chest on bent knees, hold, then fling both hands up and out.
cast = keyed(6, GUARD, [
    (0, {"upper_arm_r": (15, -40, 0), "forearm_r": (0, -100, 0), "sword": (0, -150, 0),
         "upper_arm_l": (-15, -40, 0), "forearm_l": (0, -100, 0), "torso": (0, 6, 0), "root": (0, 0, -0.02)}, "lin"),
    (1, {"upper_arm_r": (25, -30, 0), "forearm_r": (0, -120, 0), "upper_arm_l": (-25, -30, 0),
         "forearm_l": (0, -120, 0), "torso": (0, 14, 0), "head": (0, -4, 0), "thigh_r": (0, -15, 0),
         "shin_r": (0, 28, 0), "thigh_l": (0, -15, 0), "shin_l": (0, 28, 0), "root": (0, 0, -0.06)}, "out"),
    (2, {"torso": (0, 16, 0), "root": (0, 0, -0.065)}, "lin"),
    (3, {"upper_arm_r": (-35, -105, 0), "forearm_r": (0, -15, 0), "sword": (0, -100, 0),
         "upper_arm_l": (35, -105, 0), "forearm_l": (0, -15, 0), "torso": (0, -10, 0), "head": (0, -16, 0),
         "thigh_r": (0, 0, 0), "shin_r": (0, 0, 0), "thigh_l": (0, -12, 0), "shin_l": (0, 8, 0),
         "root": (0.04, 0, 0.02)}, "in3"),
    (4, {"upper_arm_r": (-38, -110, 0), "upper_arm_l": (38, -110, 0), "torso": (0, -12, 0)}, "out"),
    (5, RECOVER, "io"),
])

# Bow (held in the weapon hand) raised at arm's length, the off hand draws the string to the
# cheek and holds, the release snaps the hand back and the bow arm kicks up.
_BOW = {"upper_arm_r": (-8, -88, -6), "forearm_r": (0, -4, 0), "sword": (0, 92, 0), "torso": (0, 0, -30),
        "head": (0, 0, 28), "thigh_l": (0, -14, 0), "thigh_r": (0, 8, 0)}
shoot = keyed(7, GUARD, [
    (0, {**_BOW, "upper_arm_l": (10, -80, 10), "forearm_l": (0, -40, 0), "root": (-0.01, 0, -0.01)}, "lin"),
    (1, {"upper_arm_l": (40, -75, -50), "forearm_l": (0, -120, 0), "torso": (0, -4, -38), "root": (-0.03, 0, -0.02)}, "out"),
    (2, {"upper_arm_l": (45, -72, -60), "forearm_l": (0, -125, 0)}, "lin"),
    (3, {"upper_arm_l": (55, -50, -95), "forearm_l": (0, -60, 0), "upper_arm_r": (-8, -102, -6),
         "torso": (0, -8, -34), "root": (-0.05, 0, -0.02)}, "in3"),
    (4, {"upper_arm_l": (50, -40, -100), "upper_arm_r": (-8, -96, -6)}, "out"),
    (6, {**RECOVER, "sword": (0, 240, 0)}, "io"),
])


def die(t):
    k = min(t * 1.25, 1.0)
    fall = R(88) * (k**1.6)
    rot = guard({
        "torso": rot_y(R(-15 * k)),
        "head": rot_y(R(20 * k)),
        "thigh_r": rot_y(R(-25 * k)),
        "shin_r": rot_y(R(40 * k)),
        "upper_arm_l": rot_x(R(-55 * k)) @ rot_y(R(-20 * k)),
        "upper_arm_r": rot_x(R(55 * k)) @ rot_y(R(-30 * k)),
    })
    # Fall backwards around the heels.
    return rot, (-0.05 * k, 0, 0.12 * math.sin(k * math.pi) * 0.3), rot_y(-fall)


# name, function, frames, duration ms, play type (same vocabulary as the original scripts)
ANIMS = [
    ("stance", stance, 4, 800, "back_forth"),
    ("run", run, RUN_FRAMES, RUN_MS, "looped"),
    ("swing", swing, 8, 640, "play_once"),
    ("swing2", swing2, 8, 680, "play_once"),
    ("swing3", swing3, 8, 600, "play_once"),
    ("hit", hit, 2, 300, "play_once"),
    ("block", block, 2, 300, "play_once"),
    ("cast", cast, 6, 600, "play_once"),
    ("shoot", shoot, 7, 700, "play_once"),
    ("die", die, 6, 900, "play_once"),
]

# Frame of each attack where the blow lands / the spell leaves the hands.
HITS = {"swing": 2, "swing2": 2, "swing3": 2, "cast": 3, "shoot": 3}


def main(preview_only: bool = False):
    model = build()
    print(f"adventurer: {sum(len(p[0]) for p in model.parts.values())} voxels")
    renders = sheet.render_all(model, ANIMS, FRAME, FOOT, SCALE)
    base, tip = smear.blade_from_part(model)
    trail = smear.render_layer(model, ANIMS, FRAME, FOOT, SCALE, base, tip, HITS)
    trail_anims = smear.anims_for(ANIMS, trail)
    if not preview_only:
        script = OUT / "scripts" / "player" / "custom" / f"{NAME}.txt"
        sheet.export(renders, ANIMS, FOOT, f"custom_{NAME}.png", script, hits=HITS)
        script = OUT / "scripts" / "player" / "custom" / f"{NAME}_smear.txt"
        sheet.export(trail, trail_anims, FOOT, f"custom_{NAME}_smear.png", script, sheet_width=512)
    sheet.preview(renders, ANIMS, FRAME, NAME)
    sheet.preview(trail, trail_anims, FRAME, f"{NAME}_smear")


if __name__ == "__main__":
    main("--preview" in sys.argv)
