"""Monsters in the same pre-rendered style: goblins (humanoid rig) and many-legged critters.

Usage (from repo root):  python -I tools/artgen/creatures.py

Each creature replaces an original NPC model when the client runs with `--art custom`:
it is written to `custom_assets/scripts/npc/custom/<model>.txt` (+ sheet `custom_npc_<model>.png`),
and the client prefers that over `scripts/npc/<model>.txt`.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import character  # noqa: E402
import sheet  # noqa: E402
import smear  # noqa: E402
import vox  # noqa: E402
from vox import Bone, Model, Prim, rot_x, rot_y, rot_z  # noqa: E402

R = math.radians
FRAME = 112
FOOT = (56, 80)

vox.RAMPS.update({
    "gskin": ["#14200c", "#24391a", "#38552a", "#4f733b", "#6c9450"],
    "rag": ["#1f160e", "#352617", "#4e3a23", "#6a5232", "#866b44"],
    "chitin": ["#0e0c0d", "#1c181a", "#2c2629", "#40373a", "#584b4d"],
    "redmark": ["#2a0606", "#4f0c0c", "#7a1612", "#a5241a", "#cf3a26"],
    "antshell": ["#1a0f08", "#2e1b0e", "#472b16", "#643f20", "#83572d"],
    "eye": ["#3a0505", "#7a0b0b", "#b81a12", "#e8401f", "#ff8a4a"],
})
vox.RAMP_RGB = {k: __import__("numpy").array([vox.hex_rgb(c) for c in v], dtype="uint8") for k, v in vox.RAMPS.items()}
vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
vox.RAMP_TABLE = __import__("numpy").stack([vox.RAMP_RGB[k] for k in vox.RAMPS])


# --- goblins -----------------------------------------------------------------------------------
# Same bone names and pivots as the adventurer, so character.py's animations drive it; the
# body is squat with a big head and ears, and rendered at about 55% scale.


def goblin(weapon: str = "dagger", cloth: str = "rag") -> Model:
    B, P = Bone, Prim
    blade = (
        [P("box", ((0.03, -0.28, 0.72), (0.012, 0.025, 0.2)), "metal")]
        if weapon == "dagger"
        else [P("capsule", ((0.03, -0.28, 1.25), (0.03, -0.28, 0.2), 0.02), "wood"), P("box", ((0.03, -0.28, 0.12), (0.015, 0.035, 0.09)), "metal")]
    )
    bones = [
        B("pelvis", None, (0, 0, 0.95), [
            P("box", ((0, 0, 0.97), (0.12, 0.18, 0.1)), cloth),  # loincloth
            P("box", ((0.12, 0, 0.86), (0.015, 0.09, 0.1)), cloth),
        ]),
        B("torso", "pelvis", (0, 0, 1.05), [
            P("ellipsoid", ((0.02, 0, 1.27), (0.16, 0.21, 0.24)), "gskin"),
            P("capsule", ((0.0, -0.17, 1.38), (0.0, 0.17, 1.38), 0.05), cloth),  # shoulder strap
        ]),
        B("head", "torso", (0, 0, 1.5), [
            P("ellipsoid", ((0.05, 0, 1.66), (0.15, 0.14, 0.15)), "gskin"),
            P("ellipsoid", ((0.18, 0, 1.62), (0.05, 0.04, 0.05)), "gskin"),  # nose
            P("capsule", ((0.0, 0.12, 1.7), (-0.05, 0.33, 1.82), 0.035), "gskin"),  # ears
            P("capsule", ((0.0, -0.12, 1.7), (-0.05, -0.33, 1.82), 0.035), "gskin"),
            P("box", ((0.17, 0.06, 1.7), (0.012, 0.02, 0.015)), "eye"),
            P("box", ((0.17, -0.06, 1.7), (0.012, 0.02, 0.015)), "eye"),
            P("box", ((0.15, 0, 1.56), (0.02, 0.07, 0.012)), "hair"),  # mouth
        ]),
        B("upper_arm_r", "torso", (0, -0.25, 1.42), [P("capsule", ((0, -0.26, 1.4), (0, -0.28, 1.17), 0.06), "gskin")]),
        B("forearm_r", "upper_arm_r", (0, -0.28, 1.17), [
            P("capsule", ((0, -0.28, 1.17), (0.02, -0.28, 0.97), 0.055), "gskin"),
            P("ellipsoid", ((0.03, -0.28, 0.93), (0.055, 0.05, 0.055)), "gskin"),
        ]),
        B("sword", "forearm_r", (0.03, -0.28, 0.93), [P("capsule", ((0.03, -0.28, 0.88), (0.03, -0.28, 0.97), 0.022), "wood"), *blade]),
        B("upper_arm_l", "torso", (0, 0.25, 1.42), [P("capsule", ((0, 0.26, 1.4), (0, 0.28, 1.17), 0.06), "gskin")]),
        B("forearm_l", "upper_arm_l", (0, 0.28, 1.17), [
            P("capsule", ((0, 0.28, 1.17), (0.02, 0.28, 0.97), 0.055), "gskin"),
            P("ellipsoid", ((0.03, 0.28, 0.93), (0.055, 0.05, 0.055)), "gskin"),
        ]),
        B("thigh_r", "pelvis", (0, -0.1, 0.95), [P("capsule", ((0, -0.1, 0.95), (0, -0.1, 0.55), 0.08), "gskin")]),
        B("shin_r", "thigh_r", (0, -0.1, 0.55), [
            P("capsule", ((0, -0.1, 0.55), (0, -0.1, 0.1), 0.065), "gskin"),
            P("box", ((0.08, -0.1, 0.045), (0.12, 0.06, 0.045)), "gskin"),
        ]),
        B("thigh_l", "pelvis", (0, 0.1, 0.95), [P("capsule", ((0, 0.1, 0.95), (0, 0.1, 0.55), 0.08), "gskin")]),
        B("shin_l", "thigh_l", (0, 0.1, 0.55), [
            P("capsule", ((0, 0.1, 0.55), (0, 0.1, 0.1), 0.065), "gskin"),
            P("box", ((0.08, 0.1, 0.045), (0.12, 0.06, 0.045)), "gskin"),
        ]),
    ]
    return Model(bones)


def hunched(fn):
    """Goblins lean forward and crouch a little in every pose."""

    def wrapped(t):
        rot, off, root_rot = fn(t)
        rot = dict(rot)
        rot["torso"] = rot.get("torso", rot_y(0)) @ rot_y(R(18))
        rot["head"] = rot.get("head", rot_y(0)) @ rot_y(R(-14))
        for leg in ("thigh_r", "thigh_l"):
            rot[leg] = rot.get(leg, rot_y(0)) @ rot_y(R(-12))
        for shin in ("shin_r", "shin_l"):
            rot[shin] = rot.get(shin, rot_y(0)) @ rot_y(R(20))
        return rot, (off[0], off[1], off[2] - 0.04), root_rot

    return wrapped


GOBLIN_ANIMS = [(n, hunched(f), fr, ms, k) for n, f, fr, ms, k in character.ANIMS]


# --- many-legged critters ---------------------------------------------------------------------------


def critter(legs_per_side: int, body, abdomen, head, leg_len, colors, fangs: str, extra=()) -> Model:
    """Generic arthropod: body (root) + abdomen + head (with fangs/mandibles bone) and
    `legs_per_side` two-segment legs per side. Facing +x."""
    B, P = Bone, Prim
    shell, mark = colors
    bh = body[2]
    bones = [
        B("body", None, (0, 0, bh), [P("ellipsoid", ((0, 0, bh), body[3]), shell)]),
        B("abdomen", "body", (-body[3][0] * 0.8, 0, bh), [
            P("ellipsoid", ((abdomen[0], 0, abdomen[1]), abdomen[2]), shell),
            *([P("ellipsoid", ((abdomen[0] - 0.02, 0, abdomen[1] + abdomen[2][2] * 0.8), (abdomen[2][0] * 0.35, abdomen[2][1] * 0.25, 0.03)), mark)] if mark else []),
        ]),
        B("head", "body", (body[3][0] * 0.8, 0, bh), [
            P("ellipsoid", ((head[0], 0, head[1]), head[2]), shell),
            P("box", ((head[0] + head[2][0] * 0.85, 0.04, head[1] + 0.03), (0.012, 0.018, 0.015)), "eye"),
            P("box", ((head[0] + head[2][0] * 0.85, -0.04, head[1] + 0.03), (0.012, 0.018, 0.015)), "eye"),
            *extra,
        ]),
    ]
    tip = head[0] + head[2][0]
    if fangs == "mandibles":
        for side in (1, -1):
            bones.append(B(f"fang_{'l' if side > 0 else 'r'}", "head", (tip - 0.02, side * 0.05, head[1] - 0.02), [
                P("capsule", ((tip - 0.02, side * 0.05, head[1] - 0.02), (tip + 0.16, side * 0.02, head[1] - 0.04), 0.022), shell),
            ]))
    else:
        for side in (1, -1):
            bones.append(B(f"fang_{'l' if side > 0 else 'r'}", "head", (tip, side * 0.03, head[1] - 0.03), [
                P("capsule", ((tip, side * 0.03, head[1] - 0.03), (tip + 0.03, side * 0.03, head[1] - 0.12), 0.018), "redmark"),
            ]))
    span = body[3][0] * 1.6
    for i in range(legs_per_side):
        x = span / 2 - span * i / max(legs_per_side - 1, 1)
        splay = 0.35 * (x / span) * 2  # front legs reach forward, back legs backward
        for side in (1, -1):
            s = "l" if side > 0 else "r"
            hip = (x, side * body[3][1] * 0.7, bh)
            # High knees, feet planted wide: reads as a crouching arthropod from every angle.
            knee = (x + splay * leg_len * 0.5, side * (body[3][1] + leg_len * 0.45), bh + leg_len * 0.6)
            foot = (x + splay * leg_len * 1.25, side * (body[3][1] + leg_len * 1.15), 0.0)
            bones.append(B(f"leg_{s}{i}", "body", hip, [P("capsule", (hip, knee, 0.036), shell)]))
            bones.append(B(f"shin_{s}{i}", f"leg_{s}{i}", knee, [P("capsule", (knee, foot, 0.026), shell)]))
    m = Model(bones, voxel=0.018)
    m.legs = legs_per_side
    return m


def leg_pose(legs: int, phase: float, swing: float, lift: float, extra=None):
    """Alternating gait: even legs on one side move with odd legs on the other."""
    rot = dict(extra or {})
    for i in range(legs):
        for side, s in ((1, "l"), (-1, "r")):
            group = (i + (0 if side > 0 else 1)) % 2
            p = phase + group * math.pi
            # forward/back around z, lift around the leg's outward axis (x for +-y legs)
            rot[f"leg_{s}{i}"] = rot_z(R(swing * math.sin(p)) * side) @ rot_x(R(lift * max(0.0, math.cos(p))) * side)
    return rot


def critter_anims(legs: int):
    def stance(t):
        b = math.sin(t * 2 * math.pi)
        return leg_pose(legs, t * 2 * math.pi, 3, 2, {"abdomen": rot_y(R(3 * b))}), (0, 0, 0.01 * b), None

    def run(t):
        p = t * 2 * math.pi
        return leg_pose(legs, p, 22, 25, {"abdomen": rot_y(R(4 * math.sin(2 * p)))}), (0, 0, 0.015 * abs(math.sin(p))), None

    def bite(t):
        # rear up with the fangs spread (held), stab down and forward on frame 2, ease back
        u = t * 5
        keys = [(0, -0.5), (1, -0.75), (2, 1.0), (3, 0.85), (5, 0.0)]
        k = next(a + (b - a) * (u - f0) / (f1 - f0) for (f0, a), (f1, b) in zip(keys, keys[1:]) if u <= f1)
        r, s = max(0.0, -k), max(0.0, k)
        spread = min(1.0, r * 1.6) if u < 2 else 0.0
        rot = leg_pose(legs, 0, 0, 0, {
            "body": rot_y(R(-22 * r + 12 * s)),
            "head": rot_y(R(-12 * r + 12 * s)),
            "fang_l": rot_z(R(-35 * spread)),
            "fang_r": rot_z(R(35 * spread)),
        })
        lift = r + 0.5 * s
        rot["leg_l0"] = rot_x(R(45 * lift)) @ rot_y(R(-35 * lift))
        rot["leg_r0"] = rot_x(R(-45 * lift)) @ rot_y(R(-35 * lift))
        return rot, (-0.06 * r + 0.15 * s, 0, 0.05 * r), None

    def hit(t):
        k = math.sin(t * math.pi)
        return leg_pose(legs, 0, 6 * k, 10 * k, {"body": rot_y(R(10 * k))}), (-0.06 * k, 0, 0), None

    def die(t):
        k = min(t * 1.3, 1.0)
        rot = {}
        for i in range(legs):
            for side, s in ((1, "l"), (-1, "r")):
                rot[f"leg_{s}{i}"] = rot_x(R(-50 * k) * side)  # legs fold in
                rot[f"shin_{s}{i}"] = rot_x(R(-70 * k) * side)
        # Flip over on its back.
        return rot, (0, 0, 0.25 * math.sin(k * math.pi) + 0.12 * k), rot_x(R(175) * k**1.5)

    return [
        ("stance", stance, 4, 900, "back_forth"),
        ("run", run, 8, 560, "looped"),
        ("swing", bite, 6, 540, "play_once"),
        ("cast", bite, 6, 540, "play_once"),
        ("shoot", bite, 6, 540, "play_once"),
        ("hit", hit, 2, 300, "play_once"),
        ("block", hit, 2, 300, "play_once"),
        ("die", die, 6, 900, "play_once"),
    ]


def spider() -> Model:
    return critter(
        4,
        body=(0, 0, 0.32, (0.15, 0.13, 0.1)),
        abdomen=(-0.2, 0.42, (0.3, 0.25, 0.22)),
        head=(0.08, 0.33, (0.08, 0.08, 0.07)),
        leg_len=0.55,
        colors=("chitin", "redmark"),
        fangs="fangs",
    )


def antling() -> Model:
    return critter(
        3,
        body=(0, 0, 0.24, (0.12, 0.09, 0.08)),
        abdomen=(-0.14, 0.27, (0.17, 0.12, 0.11)),
        head=(0.06, 0.27, (0.09, 0.09, 0.08)),
        leg_len=0.38,
        colors=("antshell", None),
        fangs="mandibles",
    )


CRITTER_HITS = {"swing": 2, "cast": 2, "shoot": 2}

# model name (npc_models.name) -> (builder, anims, scale)
CREATURES = {
    "goblin": (lambda: goblin("dagger", "rag"), GOBLIN_ANIMS, 0.5),
    "goblin_charger": (lambda: goblin("spear", "red"), GOBLIN_ANIMS, 0.5),
    "spider": (spider, None, 0.8),
    "antlion_small": (antling, None, 0.75),
}


def main():
    for name, (build, anims, scale) in CREATURES.items():
        model = build()
        anims = anims or critter_anims(model.legs)
        print(f"{name}: {sum(len(p[0]) for p in model.parts.values())} voxels")
        renders = sheet.render_all(model, anims, FRAME, FOOT, scale)
        hits = character.HITS if anims is GOBLIN_ANIMS else CRITTER_HITS
        script_dir = sheet.OUT / "scripts" / "npc" / "custom"
        sheet.export(renders, anims, FOOT, f"custom_npc_{name}.png", script_dir / f"{name}.txt", hits=hits)
        sheet.preview(renders, anims, FRAME, f"npc_{name}")
        if anims is GOBLIN_ANIMS:
            base, tip = smear.blade_from_part(model)
            trail = smear.render_layer(model, anims, FRAME, FOOT, scale, base, tip, hits)
            rows = smear.anims_for(anims, trail)
            sheet.export(trail, rows, FOOT, f"custom_npc_{name}_smear.png", script_dir / f"{name}_smear.txt", 512)


if __name__ == "__main__":
    main()
