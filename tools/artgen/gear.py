"""Paper-doll gear for the custom character: a naked base body plus one sprite layer per item model.

Usage (from repo root):  python -I tools/artgen/gear.py [model ...]

Every layer is rendered with the adventurer's skeleton and animations (character.py) and written
under the original item model name (`item_template.model`), e.g. `scripts/player/custom/leather_chest.txt`,
so equipping any item with that model shows our art (client: `--art custom`).

Layers are depth-tested against the body: a gear pixel is kept only where the gear is in front of
the body, so an arm in front of a cuirass stays visible and a sword held behind the back is hidden.
Gear-vs-gear overlap is left to the client's draw order (legs, feet, chest, hands, head, shield, weapon).
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import character  # noqa: E402
import sheet  # noqa: E402
import vox  # noqa: E402
from vox import Bone, Model, Prim, compose  # noqa: E402

vox.RAMPS.update({
    "linen": ["#2b241b", "#4a3f30", "#6e5f48", "#968266", "#bba686"],
    "chain": ["#141517", "#2b2e33", "#4a4f56", "#727981", "#a1a8b0"],
    "plate": ["#1d2024", "#3e444b", "#67707a", "#9aa4ae", "#d4dbe1"],
    "mage": ["#160f22", "#2a1b40", "#422c63", "#5f4289", "#8364b2"],
    "redmark": ["#2a0606", "#4f0c0c", "#7a1612", "#a5241a", "#cf3a26"],
    "stone": ["#1c1c1f", "#34343a", "#4f4f55", "#6e6d70", "#918e8c"],
    "mage_red": ["#220a0a", "#3f1414", "#5e1f1c", "#832d27", "#ad4436"],
    "mage_teal": ["#0a1c1d", "#123234", "#1b4a4c", "#286769", "#3c8c8c"],
    "orange": ["#2e1404", "#5a2a08", "#8a440e", "#b9641a", "#e68a30"],
    "azure": ["#06162e", "#0d2a56", "#174684", "#2a68b4", "#56a0e6"],
    "violet": ["#1a0626", "#331049", "#521c74", "#7630a3", "#a45bd4"],
    "crimson": ["#2a0408", "#540a12", "#84141d", "#b52530", "#e24a4f"],
})
vox.RAMP_RGB = {k: np.array([vox.hex_rgb(c) for c in v], dtype=np.uint8) for k, v in vox.RAMPS.items()}
vox.RAMP_IDS = {k: i for i, k in enumerate(vox.RAMPS)}
vox.RAMP_TABLE = np.stack([vox.RAMP_RGB[k] for k in vox.RAMPS])

P = Prim
SKELETON = [(b.name, b.parent, b.pivot) for b in character.build().bones.values()]
DEPTH_EPS = 0.015  # gear within this of the body surface still counts as "in front"


def model(prims_by_bone: dict[str, list[Prim]], voxel: float = 0.02, pattern: str | None = None) -> Model:
    m = Model([Bone(n, parent, pivot, prims_by_bone.get(n, [])) for n, parent, pivot in SKELETON], voxel=voxel)
    if pattern:
        for name, (p, n, c) in m.parts.items():
            if pattern == "chain":  # interlocking rings: alternating bright/dark 2-voxel checks
                k = (np.floor(p[:, 0] / 0.025) + np.floor(p[:, 1] / 0.025) + np.floor(p[:, 2] / 0.025)) % 2
                tone = 0.75 + 0.4 * k
            elif pattern == "plate":  # broad polished plates with dark seams
                seam = (np.abs((p[:, 2] * 8) % 1 - 0.5) < 0.06).astype(float)
                tone = 1.05 - 0.45 * seam
            else:
                tone = np.ones(len(p))
            m.parts[name] = (p, n * tone[:, None], c)
    return m


# --- base body ----------------------------------------------------------------------------------


def body() -> Model:
    return model({
        "pelvis": [P("box", ((0, 0, 0.98), (0.11, 0.165, 0.085)), "cloth")],  # smallclothes
        "torso": [
            P("ellipsoid", ((0, 0, 1.3), (0.14, 0.205, 0.25)), "skin"),
            P("box", ((0, 0, 1.15), (0.13, 0.185, 0.1)), "skin"),
        ],
        "head": [
            P("ellipsoid", ((0.03, 0, 1.67), (0.1, 0.09, 0.12)), "skin"),
            P("ellipsoid", ((-0.02, 0, 1.72), (0.1, 0.095, 0.09)), "hair"),
            P("box", ((0.1, 0, 1.6), (0.03, 0.07, 0.035)), "hair"),
            P("box", ((0.122, 0.035, 1.69), (0.008, 0.014, 0.01)), "hair"),
            P("box", ((0.122, -0.035, 1.69), (0.008, 0.014, 0.01)), "hair"),
        ],
        **{
            f"upper_arm_{s}": [P("capsule", ((0, y * 0.26, 1.44), (0, y * 0.28, 1.2), 0.056), "skin")]
            for s, y in (("r", -1), ("l", 1))
        },
        **{
            f"forearm_{s}": [
                P("capsule", ((0, y * 0.28, 1.2), (0.02, y * 0.28, 0.98), 0.047), "skin"),
                P("ellipsoid", ((0.03, y * 0.28, 0.93), (0.045, 0.04, 0.05)), "skin"),
            ]
            for s, y in (("r", -1), ("l", 1))
        },
        **{f"thigh_{s}": [P("capsule", ((0, y * 0.09, 0.95), (0, y * 0.09, 0.53), 0.075), "skin")] for s, y in (("r", -1), ("l", 1))},
        **{
            f"shin_{s}": [
                P("capsule", ((0, y * 0.09, 0.53), (0, y * 0.09, 0.1), 0.06), "skin"),
                P("box", ((0.06, y * 0.09, 0.04), (0.1, 0.05, 0.04)), "skin"),
            ]
            for s, y in (("r", -1), ("l", 1))
        },
    })


# --- armour pieces ---------------------------------------------------------------------------------

SIDES = (("r", -1), ("l", 1))


def chest(mat, trim, pads=False, pattern=None, hem=0.0):
    prims = {
        "torso": [
            P("ellipsoid", ((0, 0, 1.3), (0.158, 0.222, 0.27)), mat),
            P("box", ((0, 0, 1.14), (0.15, 0.205, 0.115)), mat),
            P("capsule", ((0.13, -0.09, 1.46), (0.14, 0.09, 1.46), 0.035), trim),  # collar
        ],
        "pelvis": [
            P("box", ((0, 0, 1.0 - hem / 2), (0.13, 0.19, 0.075 + hem / 2)), mat),  # tunic hem / skirt
            P("box", ((0, 0, 1.06), (0.135, 0.195, 0.03)), "leather"),  # belt
            P("box", ((0.135, 0, 1.06), (0.015, 0.035, 0.028)), "gold"),
        ],
        **{f"upper_arm_{s}": [P("capsule", ((0, y * 0.26, 1.44), (0, y * 0.28, 1.22), 0.07), mat)] for s, y in SIDES},
    }
    if pads:
        for s, y in SIDES:
            prims["torso"].append(P("ellipsoid", ((0, y * 0.24, 1.48), (0.1, 0.09, 0.07)), trim))
    return model(prims, pattern=pattern)


def legs(mat, pattern=None, to_ankle=True):
    end = 0.14 if to_ankle else 0.36
    return model({
        **{f"thigh_{s}": [P("capsule", ((0, y * 0.09, 0.96), (0, y * 0.09, 0.53), 0.088), mat)] for s, y in SIDES},
        **{f"shin_{s}": [P("capsule", ((0, y * 0.09, 0.53), (0, y * 0.09, end), 0.072), mat)] for s, y in SIDES},
        "pelvis": [P("box", ((0, 0, 0.97), (0.118, 0.178, 0.095)), mat)],
    }, pattern=pattern)


def feet(mat, high=0.32, pattern=None, sandal=False):
    shin = []
    if not sandal:
        shin.append(lambda y: P("capsule", ((0, y * 0.09, high), (0, y * 0.09, 0.09), 0.072), mat))
    return model({
        f"shin_{s}": [f(y) for f in shin] + [P("box", ((0.065, y * 0.09, 0.045 if not sandal else 0.02), (0.112, 0.062, 0.05 if not sandal else 0.022)), mat)]
        for s, y in SIDES
    }, pattern=pattern)


def hands(mat, cuff=1.08, pattern=None):
    return model({
        f"forearm_{s}": [
            P("capsule", ((0.01, y * 0.28, cuff), (0.02, y * 0.28, 0.98), 0.06), mat),
            P("ellipsoid", ((0.03, y * 0.28, 0.93), (0.055, 0.05, 0.06)), mat),
        ]
        for s, y in SIDES
    }, pattern=pattern)


def head(kind, mat, pattern=None):
    if kind == "hood":
        prims = [
            P("ellipsoid", ((-0.04, 0, 1.73), (0.12, 0.122, 0.125)), mat),
            P("ellipsoid", ((-0.09, 0, 1.6), (0.08, 0.125, 0.13)), mat),
        ]
    elif kind == "coif":
        prims = [
            P("ellipsoid", ((-0.01, 0, 1.7), (0.122, 0.112, 0.14)), mat),
            P("capsule", ((-0.02, -0.11, 1.52), (-0.02, 0.11, 1.52), 0.06), mat),
        ]
    else:  # helm
        prims = [
            P("ellipsoid", ((0.0, 0, 1.72), (0.13, 0.12, 0.13)), mat),
            P("box", ((0.0, 0, 1.64), (0.13, 0.12, 0.05)), mat),
            P("box", ((0.135, 0, 1.64), (0.012, 0.018, 0.07)), mat),  # nose guard
        ]
    return model({"head": prims}, pattern=pattern)


# --- weapons and shields (in the right hand / on the left forearm) ------------------------------------

HAND_R = (0.03, -0.28, 0.93)


def blade_weapon(length, width, guard=0.07, grip="wood", metal="metal", pommel="gold"):
    x, y, z = HAND_R
    return model({"sword": [
        P("box", ((x, y, z), (0.025, guard, 0.02)), pommel),
        P("capsule", ((x, y, z - 0.03), (x, y, z + 0.05), 0.02), grip),
        P("ellipsoid", ((x, y, z + 0.07), (0.025, 0.025, 0.025)), pommel),
        P("box", ((x, y, z - 0.03 - length / 2), (0.014, width, length / 2)), metal),
    ]}, voxel=0.014)


def blunt_weapon(length, head):
    x, y, z = HAND_R
    prims = [P("capsule", ((x, y, z + 0.06), (x, y, z - length), 0.024), "wood")]
    if head == "club":
        prims.append(P("capsule", ((x, y, z - length * 0.55), (x, y, z - length), 0.05), "wood"))
    elif head == "mace":
        prims.append(P("ellipsoid", ((x, y, z - length), (0.07, 0.07, 0.08)), "metal"))
    elif head == "hammer":
        prims.append(P("box", ((x, y, z - length), (0.06, 0.12, 0.06)), "metal"))
    elif head == "maul":
        prims.append(P("box", ((x, y, z - length), (0.1, 0.17, 0.1)), "metal"))
    elif head == "smith":
        prims.append(P("box", ((x, y, z - length), (0.04, 0.08, 0.04)), "metal"))
    return model({"sword": prims}, voxel=0.014)


def axe(length, blade):
    x, y, z = HAND_R
    return model({"sword": [
        P("capsule", ((x, y, z + 0.06), (x, y, z - length), 0.024), "wood"),
        P("box", ((x + blade * 0.6, y, z - length + 0.06), (blade * 0.6, 0.012, blade * 0.55)), "metal"),
    ]}, voxel=0.014)


def staff(head_color, length=0.75, crystal=False):
    """Quarterstaff through the fist; `crystal` adds a forked head cradling a gem (great staves)."""
    x, y, z = HAND_R
    prims = [
        P("capsule", ((x, y, z + length), (x, y, z - length), 0.026), "wood"),
        P("ellipsoid", ((x, y, z - length - 0.05), (0.06, 0.06, 0.06 if not crystal else 0.09)), head_color),
    ]
    if crystal:
        for side in (-1, 1):
            prims.append(P("capsule", ((x, y, z - length + 0.04), (x + 0.07 * side, y, z - length - 0.14), 0.016), "wood"))
    return model({"sword": prims}, voxel=0.014)


def rod(head_color, length=0.45):
    """Short metal sceptre with an orb (rods and wands)."""
    x, y, z = HAND_R
    return model({"sword": [
        P("capsule", ((x, y, z + 0.06), (x, y, z - length), 0.018), "metal"),
        P("ellipsoid", ((x, y, z - length - 0.04), (0.045, 0.045, 0.045)), head_color),
        P("box", ((x, y, z - length + 0.01), (0.03, 0.03, 0.012)), "gold"),
    ]}, voxel=0.012)


def bow(height, wood="wood"):
    x, y, z = HAND_R
    prims = []
    steps = 8
    for i in range(steps):  # limb arc bulging forward (+x)
        a0, a1 = -1 + 2 * i / steps, -1 + 2 * (i + 1) / steps
        p0 = (x + 0.12 * (1 - a0 * a0), y, z + a0 * height / 2)
        p1 = (x + 0.12 * (1 - a1 * a1), y, z + a1 * height / 2)
        prims.append(P("capsule", (p0, p1, 0.018 + 0.006 * (height > 1.0)), wood))
    prims.append(P("capsule", ((x - 0.02, y, z + height / 2), (x - 0.02, y, z - height / 2), 0.006), "linen"))
    return model({"sword": prims}, voxel=0.012)


def shield(kind):
    y = 0.37
    if kind in ("buckler", "iron_buckler"):
        face = "wood" if kind == "buckler" else "metal"
        prims = [P("ellipsoid", ((0.03, y, 1.03), (0.15, 0.025, 0.15)), face), P("ellipsoid", ((0.03, y + 0.02, 1.03), (0.05, 0.02, 0.05)), "gold" if face == "metal" else "metal")]
    else:
        prims = [
            P("box", ((0.03, y, 1.0), (0.17, 0.02, 0.24)), "wood"),
            P("box", ((0.03, y + 0.005, 1.0), (0.185, 0.015, 0.255)), "metal"),
            P("box", ((0.03, y + 0.025, 1.02), (0.05, 0.01, 0.12)), "redmark"),
        ]
    return model({"forearm_l": prims}, voxel=0.016)


MAGE_PIECES = {
    "mage_vest": lambda m: chest(m, "gold"),
    "mage_skirt": lambda m: legs(m, to_ankle=True),
    "mage_boots": lambda m: feet(m),
    "mage_sleeves": lambda m: hands(m, cuff=1.12),
    "mage_hood": lambda m: head("hood", m),
}

# model name (item_template.model) -> builder
GEAR = {
    # starter cloth
    "cloth_shirt": lambda: chest("linen", "linen"),
    "cloth_pants": lambda: legs("linen"),
    "cloth_sandals": lambda: feet("leather", sandal=True),
    "cloth_gloves": lambda: hands("linen", cuff=1.02),
    # leather
    "leather_chest": lambda: chest("leather", "red", pads=True),
    "leather_pants": lambda: legs("boot"),
    "leather_boots": lambda: feet("boot"),
    "leather_gloves": lambda: hands("leather"),
    "leather_hood": lambda: head("hood", "red"),
    # chain
    "chain_cuirass": lambda: chest("chain", "leather", pads=True, pattern="chain", hem=0.12),
    "chain_greaves": lambda: legs("chain", pattern="chain"),
    "chain_boots": lambda: feet("chain", pattern="chain"),
    "chain_gloves": lambda: hands("chain", pattern="chain"),
    "chain_coif": lambda: head("coif", "chain", pattern="chain"),
    # plate
    "plate_cuirass": lambda: chest("plate", "gold", pads=True, pattern="plate"),
    "plate_greaves": lambda: legs("plate", pattern="plate"),
    "plate_boots": lambda: feet("plate", high=0.38, pattern="plate"),
    "plate_gauntlets": lambda: hands("plate", cuff=1.12, pattern="plate"),
    "plate_helm": lambda: head("helm", "plate", pattern="plate"),
    # mage
    "mage_vest": lambda: chest("mage", "gold"),
    "mage_skirt": lambda: legs("mage", to_ankle=True),
    "mage_boots": lambda: feet("mage"),
    "mage_sleeves": lambda: hands("mage", cuff=1.12),
    "mage_hood": lambda: head("hood", "mage"),
    # mage colour variants (alt1 crimson, alt2 teal)
    **{
        f"{piece}_{alt}": (lambda piece=piece, ramp=ramp: MAGE_PIECES[piece](ramp))
        for alt, ramp in (("alt1", "mage_red"), ("alt2", "mage_teal"))
        for piece in ("mage_vest", "mage_skirt", "mage_boots", "mage_sleeves", "mage_hood")
    },
    # weapons
    "dagger": lambda: blade_weapon(0.28, 0.022, guard=0.05),
    "dagger_red": lambda: blade_weapon(0.28, 0.022, guard=0.05, grip="redmark"),
    "shortsword": lambda: blade_weapon(0.5, 0.028),
    "longsword": lambda: blade_weapon(0.72, 0.03),
    "greatsword": lambda: blade_weapon(0.95, 0.04, guard=0.11),
    "club": lambda: blunt_weapon(0.55, "club"),
    "mace": lambda: blunt_weapon(0.55, "mace"),
    "war_hammer": lambda: blunt_weapon(0.65, "hammer"),
    "hand_axe": lambda: axe(0.5, 0.11),
    "battle_axe": lambda: axe(0.75, 0.16),
    "staff": lambda: staff("wood"),
    "staff_grey": lambda: staff("stone"),
    "greatstaff": lambda: staff("azure", length=0.85, crystal=True),
    "greatstaff_purple": lambda: staff("violet", length=0.85, crystal=True),
    "greatstaff_red": lambda: staff("crimson", length=0.85, crystal=True),
    "rod": lambda: rod("stone"),
    "rod_blue": lambda: rod("azure"),
    "rod_purple": lambda: rod("violet"),
    "rod_red": lambda: rod("crimson"),
    "wand": lambda: rod("gold", length=0.3),
    "zweihander": lambda: blade_weapon(1.05, 0.045, guard=0.13),
    "maul": lambda: blunt_weapon(0.8, "maul"),
    "smith_hammer": lambda: blunt_weapon(0.4, "smith"),
    "infantry_axe": lambda: axe(0.9, 0.14),
    "greatbow": lambda: bow(1.15),
    "greatbow_orange": lambda: bow(1.15, wood="orange"),
    "iron_buckler": lambda: shield("iron_buckler"),
    "shortbow": lambda: bow(0.7),
    "longbow": lambda: bow(0.95),
    # shields
    "buckler": lambda: shield("buckler"),
    "shield": lambda: shield("kite"),
}


def render_layers(body_model: Model, layers: dict[str, Model]):
    """{layer: {anim: [[rgba per dir] per frame]}}, layer "custom_body" included."""
    out = {name: {} for name in ["custom_body", *layers]}
    for anim, fn, frames, _, kind in character.ANIMS:
        for name in out:
            out[name][anim] = []
        for f in range(frames):
            t = f / frames if kind == "looped" else f / max(frames - 1, 1)
            rot, off, root_rot = fn(t * 0.999)
            body_world = body_model.pose(rot, off, root_rot)
            per = {name: [] for name in out}
            for d in range(8):
                facing = sheet.DIR_TO_ORIENTATION[d]
                args = (facing, character.FRAME, character.FOOT, character.SCALE)
                bz, bs, br = body_model.raster(body_world, *args)
                per["custom_body"].append(compose(bs, br, br >= 0))
                for name, m in layers.items():
                    gz, gs, gr = m.raster(m.pose(rot, off, root_rot), *args)
                    visible = (gr >= 0) & ((br < 0) | (gz >= bz - DEPTH_EPS))
                    per[name].append(compose(gs, gr, visible))
            for name in out:
                out[name][anim].append(per[name])
    return out


def main(only: list[str]):
    names = only or list(GEAR)
    layers = {n: GEAR[n]() for n in names}
    print(f"rendering body + {len(layers)} gear layers ...")
    renders = render_layers(body(), layers)
    for name, r in renders.items():
        if only and name == "custom_body" and "custom_body" not in only:
            continue
        sheet.export(
            r,
            character.ANIMS,
            character.FOOT,
            f"custom_gear_{name}.png",
            sheet.OUT / "scripts" / "player" / "custom" / f"{name}.txt",
            sheet_width=512,
        )
    sheet.preview(renders["custom_body"], character.ANIMS, character.FRAME, "gear_body")


if __name__ == "__main__":
    main(sys.argv[1:])
