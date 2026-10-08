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


def run(t):
    a = math.sin(t * 2 * math.pi)
    b = math.sin(t * 2 * math.pi + math.pi / 2)
    rot = guard({
        "torso": rot_y(R(-10)),
        "thigh_r": rot_y(R(-38 * a)),
        "shin_r": rot_y(R(35 + 30 * max(0.0, b))),
        "thigh_l": rot_y(R(38 * a)),
        "shin_l": rot_y(R(35 + 30 * max(0.0, -b))),
        "upper_arm_l": rot_y(R(30 * a)),
        "upper_arm_r": rot_y(R(-25 - 18 * a)),
    })
    return rot, (0, 0, -0.04 + 0.035 * abs(math.cos(t * 2 * math.pi))), None


def swing(t):
    # wind up (0..0.35) then slash across (0.35..0.75), recover
    if t < 0.35:
        k = t / 0.35
        sh, el, tw = -25 - 110 * k, -55 + 25 * k, 25 * k
    elif t < 0.75:
        k = (t - 0.35) / 0.4
        sh, el, tw = -135 + 165 * k, -30 - 20 * k, 25 - 55 * k
    else:
        k = (t - 0.75) / 0.25
        sh, el, tw = 30 - 55 * k, -50 - 5 * k, -30 + 30 * k
    rot = guard({
        "torso": rot_z(R(tw)) @ rot_y(R(-8)),
        "upper_arm_r": rot_y(R(sh)),
        "forearm_r": rot_y(R(el)),
        "sword": rot_y(R(-110)),
        "thigh_l": rot_y(R(-20)),
        "thigh_r": rot_y(R(12)),
    })
    return rot, (0.03 * math.sin(t * math.pi), 0, -0.02), None


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


def cast(t):
    k = math.sin(t * math.pi)
    rot = {
        "upper_arm_r": rot_y(R(-80 - 50 * k)),
        "forearm_r": rot_y(R(-10)),
        "sword": rot_y(R(-150)),
        "upper_arm_l": rot_y(R(-80 - 50 * k)),
        "forearm_l": rot_y(R(-10)),
        "torso": rot_y(R(-5 * k)),
        "head": rot_y(R(-10 * k)),
    }
    return rot, (0, 0, 0.02 * k), None


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
    ("run", run, 8, 640, "looped"),
    ("swing", swing, 6, 600, "play_once"),
    ("hit", hit, 2, 300, "play_once"),
    ("block", block, 2, 300, "play_once"),
    ("cast", cast, 4, 600, "play_once"),
    ("shoot", cast, 4, 600, "play_once"),
    ("die", die, 6, 900, "play_once"),
]


def main(preview_only: bool = False):
    model = build()
    print(f"adventurer: {sum(len(p[0]) for p in model.parts.values())} voxels")
    renders = sheet.render_all(model, ANIMS, FRAME, FOOT, SCALE)
    if not preview_only:
        script = OUT / "scripts" / "player" / "custom" / f"{NAME}.txt"
        sheet.export(renders, ANIMS, FOOT, f"custom_{NAME}.png", script)
    sheet.preview(renders, ANIMS, FRAME, NAME)


if __name__ == "__main__":
    main("--preview" in sys.argv)
