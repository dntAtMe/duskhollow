"""Item icon models: small voxel objects built from `iconlib` primitives, chosen by heuristics.

`item_icon(stem, info)` -> (prims, view rotation, fit). How a model is picked, in order:
1. `item_template.model` of the items using the icon (weapons / shields: dagger, longsword, maul,
   rod_purple, greatbow_orange, kite shield ...) -> a weapon / shield builder;
2. armour icons `icon_item_<rb|lt|ch|pl>_<glove|head|pants|shoes|torso>_*` -> cloth / leather /
   chain / plate builders of that slot;
3. our data (`by_data`): drink names, weapon models by substring or `weapon_type`, armour slot and
   family from the model / name words, `equip_type` and `armor_type`, junk keywords;
4. keywords of the icon file name and the item name (potion_hp / mp, serum, ring, necklace, belt,
   gem names, crystalball = enchant orbs, scrolls, food dishes, bones, teeth, claws, ores ...);
5. a generic sack of loot.

Quality (2 plain .. 6 purple) picks the accents: trim metal (bronze, silver, steel, gold, gold),
inset gem (none, emerald, sapphire, ruby, amethyst) and cloth colour, so tiers of the same base item
read as upgrades of each other.
"""

from __future__ import annotations

import math
import re

import numpy as np

from iconlib import (
    S3, Sh, box, cap, circle, cone, convex, cyl, ell, ellipse, ext, facet_rock, gem_cut, lathe, poly, rbox,
    rect, rough, seg, sphere, t_chain, t_grain, t_noise, t_stripes, t_wood, taper, torus, union, view,
)
from vox import rot_x, rot_y, rot_z

# quality -> accents
TRIM = {1: "stone", 2: "copper", 3: "metal", 4: "plate", 5: "gold", 6: "gold"}
GEM = {1: None, 2: None, 3: "emerald", 4: "sapphire", 5: "ruby", 6: "amethyst"}
CLOTH = {1: "linen", 2: "linen", 3: "cloth_g", 4: "cloth_b", 5: "redmark", 6: "mage"}
LEATHER = {1: "boot", 2: "leather", 3: "leather", 4: "boot", 5: "red", 6: "boot"}

GEM_WORDS = {"ruby": "ruby", "sapphire": "sapphire", "emerald": "emerald", "topaz": "topaz", "amethyst": "amethyst"}


def gem_of(text, default="ruby"):
    for w, r in GEM_WORDS.items():
        if w in text:
            return r
    for w, r in (("red", "ruby"), ("blue", "sapphire"), ("green", "emerald"), ("yellow", "topaz"), ("purple", "amethyst")):
        if w in text:
            return r
    return default


W = dict(spec=0.35)  # metal shine
DIAG = view(diag=45, tilt=12, yaw=-20)  # long items: bottom-left to top-right


# --- weapons ---------------------------------------------------------------------------------------


def blade(z0, length, w, t, tip, ramp="metal", curve=0.0):
    """Diamond-section blade along +z from z0, tapering to a point over `tip`."""
    z1 = z0 + length
    planes = [((0, 0, -1), -z0)]
    for sx in (1, -1):
        planes.append(((sx * t, w, 0), t * w))
        planes.append(((sx * t, -w, 0), t * w))
        planes.append(((sx * tip, 0, w), w * z1))
    s = convex(planes, ramp, spec=0.6, bounds=((-w, -t, z0), (w, t, z1)))
    if curve:
        f = s.f
        s = S3(lambda P: f(P - np.stack([curve * (P[:, 2] - z0) ** 2, 0 * P[:, 0], 0 * P[:, 0]], -1)), s.lo - [0, 0, 0], s.hi + [curve * length**2, 0, 0], ramp, 0.6)
    return s


def sword(q, length=1.5, w=0.13, guard=0.36, grip="leather", gem=True, broken=False, ricasso=False):
    trim, g = TRIM[max(q, 2)], GEM[q]
    z0 = -0.15
    prims = [
        blade(z0, length * (0.6 if broken else 1.0), w, 0.035, 0.05 if broken else w * 2.2, "metal" if q < 4 else "plate"),
        cap((0, 0, z0 - 0.05), (0, 0, z0 - 0.42), 0.055, grip, tone=t_stripes(70, 0.25)),
        rbox((0, 0, z0 - 0.02), (guard, 0.07, 0.05), 0.03, trim, **W),
        sphere((0, 0, z0 - 0.5), 0.09, trim, **W),
    ]
    if q >= 4:  # flared guard tips
        for sx in (1, -1):
            prims.append(sphere((sx * guard, 0, z0 + 0.02), 0.06, trim, **W))
    if ricasso:
        prims.append(box((0, 0, z0 + 0.15), (w * 0.7, 0.05, 0.15), "leather"))
    if gem and g:
        prims.append(sphere((0, -0.06, z0 - 0.02), 0.055, g, spec=0.9))
        prims.append(sphere((0, 0, z0 - 0.5), 0.06, g, spec=0.9).at((0, -0.04, 0)))
    if q >= 5:  # fuller inlay
        prims.append(box((0, 0, z0 + length * 0.35), (0.018, 0.04, length * 0.28), trim, **W))
    return prims, DIAG, 0.88


def dagger(q, red=False):
    trim, g = TRIM[max(q, 2)], GEM[q]
    prims = [
        blade(0.0, 0.85, 0.11, 0.03, 0.3, "plate" if q >= 4 else "metal", curve=0.12 if red else 0.0),
        cap((0, 0, -0.05), (0, 0, -0.4), 0.06, "redmark" if red else "leather", tone=t_stripes(60, 0.3)),
        rbox((0, 0, -0.02), (0.24, 0.07, 0.045), 0.03, trim, **W),
        sphere((0, 0, -0.46), 0.08, trim, **W),
    ]
    if g:
        prims.append(sphere((0, -0.06, -0.02), 0.05, g, spec=0.9))
    return prims, DIAG, 0.8


def haft(z0, z1, r=0.055, ramp="wood"):
    return cap((0, 0, z0), (0, 0, z1), r, ramp, tone=t_wood)


def axe(q, kind):
    trim, g = TRIM[max(q, 2)], GEM[q]
    metal = "plate" if q >= 4 else "metal"
    if kind == "hand_axe":
        head = poly([(0.05, 0.62), (0.35, 0.78), (0.55, 0.95), (0.6, 0.6), (0.55, 0.25), (0.35, 0.35), (0.05, 0.42)])
        prims = [haft(-0.95, 0.75), ext(head, 0.05, metal, rnd=0.02, spec=0.6), cyl((0, 0, 0.4), (0, 0, 0.66), 0.08, trim, **W)]
    elif kind == "infantry_axe":  # long-hafted crescent "reaper"
        head = poly([(0.0, 0.55), (0.35, 0.65), (0.62, 1.05), (0.72, 0.6), (0.62, 0.1), (0.35, 0.4), (0.0, 0.42)])
        prims = [haft(-1.3, 0.95), ext(head, 0.045, metal, rnd=0.02, spec=0.6), cone((0, 0, 0.9), (0, 0, 1.2), 0.06, 0.01, metal, spec=0.6)]
    else:  # battle axe: double crescent
        half = poly([(0.0, 0.5), (0.3, 0.62), (0.55, 0.98), (0.66, 0.55), (0.55, 0.1), (0.3, 0.45), (0.0, 0.4)])
        prims = [haft(-1.1, 0.85), ext(half.mirror_x(), 0.05, metal, rnd=0.02, spec=0.6), cyl((0, 0, 0.35), (0, 0, 0.72), 0.1, trim, **W)]
    prims.append(cyl((0, 0, -0.75), (0, 0, -0.45), 0.065, "leather", tone=t_stripes(60, 0.3)))
    if g:
        prims.append(sphere((0, -0.08, 0.52), 0.06, g, spec=0.9))
    return prims, view(diag=35, tilt=10, yaw=-15), 0.88


def mace(q, kind):
    trim, g = TRIM[max(q, 2)], GEM[q]
    metal = "plate" if q >= 4 else "metal"
    if kind == "club":
        prims = [cone((0, 0, -0.9), (0, 0, 0.75), 0.06, 0.2, "wood", tone=t_wood)]
        for i, (a, z) in enumerate(((0.3, 0.5), (2.4, 0.35), (4.3, 0.6), (1.2, 0.2))):
            prims.append(cone((0.17 * math.cos(a), 0.17 * math.sin(a), z), (0.27 * math.cos(a), 0.27 * math.sin(a), z + 0.04), 0.035, 0.005, "metal", **W))
    elif kind == "smith_hammer":
        prims = [haft(-0.9, 0.6), rbox((0.05, 0, 0.62), (0.32, 0.12, 0.13), 0.03, metal, spec=0.5)]
    elif kind == "war_hammer":
        prims = [haft(-1.0, 0.75), rbox((0.12, 0, 0.62), (0.22, 0.13, 0.14), 0.03, metal, spec=0.5),
                 cone((-0.08, 0, 0.62), (-0.48, 0, 0.55), 0.09, 0.01, metal, spec=0.5), cone((0, 0, 0.74), (0, 0, 1.0), 0.06, 0.01, metal, spec=0.5)]
    elif kind == "maul":
        prims = [haft(-1.0, 0.6, 0.065), rbox((0, 0, 0.66), (0.42, 0.24, 0.24), 0.05, metal, spec=0.5),
                 rbox((0, 0, 0.66), (0.44, 0.26, 0.06), 0.02, trim, **W)]
    else:  # flanged mace
        prims = [haft(-1.0, 0.55), sphere((0, 0, 0.66), 0.17, metal, spec=0.5)]
        for i in range(6):
            a = i * math.pi / 3
            prims.append(box((0, 0, 0), (0.025, 0.12, 0.2), metal, spec=0.5).at((0.16 * math.cos(a), 0.16 * math.sin(a), 0.66), rot_z(a + math.pi / 2)))
        prims.append(cone((0, 0, 0.8), (0, 0, 1.0), 0.06, 0.01, metal, spec=0.5))
    prims.append(cyl((0, 0, -0.85), (0, 0, -0.5), 0.07, "leather", tone=t_stripes(60, 0.3)))
    if g:
        prims.append(sphere((0, 0, -0.95), 0.07, g, spec=0.9))
    return prims, view(diag=35, tilt=10, yaw=-25), 0.88


def staff(q, kind):
    trim = TRIM[max(q, 2)]
    head = {"staff_grey": "stone", "staff": "emerald", "greatstaff": "sapphire", "greatstaff_purple": "amethyst", "greatstaff_red": "ruby"}.get(kind, GEM[q] or "topaz")
    prims = [cap((0, 0, -1.0), (0, 0, 1.0), 0.065, "wood", tone=t_wood)]
    if kind == "staff_grey":
        prims.append(rough(sphere((0, 0, 1.1), 0.24, "stone"), 0.04, 9, 3))
    elif kind == "staff":  # curled crook holding a gem
        prims.append(torus((0, 0, 0), 0.26, 0.065, "wood", tone=t_wood).at((0.0, 0, 1.24), rot_x(math.pi / 2)))
        prims.append(sphere((0, 0, 1.24), 0.15, head, spec=0.9, emit=0.75))
    else:  # prongs around a crystal
        prims.append(gem_cut((0, 0, 0), 0.24, head, emit=0.7).at((0, 0, 1.3), rot_x(0.2)))
        for i in range(3):
            a = i * 2 * math.pi / 3
            prims.append(cone((0.07 * math.cos(a), 0.07 * math.sin(a), 0.98), (0.28 * math.cos(a), 0.28 * math.sin(a), 1.48), 0.045, 0.015, trim, **W))
        prims.append(cyl((0, 0, 0.9), (0, 0, 1.04), 0.1, trim, **W))
    prims.append(cyl((0, 0, -0.1), (0, 0, 0.2), 0.072, "leather", tone=t_stripes(60, 0.3)))
    return prims, view(diag=40, tilt=8, yaw=0), 0.9


def wand(q, kind):
    trim = TRIM[max(q, 2)]
    col = {"wand": "topaz", "rod": "emerald", "rod_blue": "sapphire", "rod_purple": "amethyst", "rod_red": "ruby"}.get(kind, "topaz")
    if kind == "wand":
        prims = [cone((0, 0, -0.9), (0, 0, 0.7), 0.06, 0.035, "wood", tone=t_wood), sphere((0, 0, 0.76), 0.07, col, emit=0.8)]
    else:
        prims = [cap((0, 0, -0.9), (0, 0, 0.5), 0.05, trim, **W), cyl((0, 0, -0.75), (0, 0, -0.3), 0.065, "leather", tone=t_stripes(60, 0.3)),
                 sphere((0, 0, 0.72), 0.17, col, spec=0.8, emit=0.7)]
        for i in range(4):
            a = i * math.pi / 2 + 0.4
            prims.append(cone((0.05 * math.cos(a), 0.05 * math.sin(a), 0.45), (0.17 * math.cos(a), 0.17 * math.sin(a), 0.8), 0.03, 0.012, trim, **W))
        prims.append(sphere((0, 0, -0.95), 0.07, trim, **W))
    return prims, view(diag=40, tilt=10), 0.82


def bow(q, kind):
    limb = {"greatbow_orange": "copper", "greatbow": "wood", "longbow": "wood"}.get(kind, "wood")
    h = {"shortbow": 1.0, "longbow": 1.25, "greatbow": 1.35, "greatbow_orange": 1.35}.get(kind, 1.2)
    bend = 0.32 if kind == "shortbow" else 0.26
    prims = []
    steps = 10
    pts = []
    for i in range(steps + 1):
        a = -1 + 2 * i / steps
        recurve = 0.06 * a**4 if kind.startswith("greatbow") else 0
        pts.append((bend * (1 - a * a) - recurve, 0, a * h))
    for i in range(steps):
        r = 0.055 - 0.02 * abs(-1 + 2 * (i + 0.5) / steps)
        prims.append(cap(pts[i], pts[i + 1], r, limb, tone=t_wood))
    prims.append(cap((pts[0][0] - 0.01, 0, -h), (pts[-1][0] - 0.01, 0, h), 0.012, "linen"))
    prims.append(cap((bend + 0.01, 0, -0.14), (bend + 0.01, 0, 0.14), 0.07, "leather", tone=t_stripes(60, 0.3)))
    trim, g = TRIM[max(q, 2)], GEM[q]
    if q >= 4:
        for sz in (1, -1):
            prims.append(cap((pts[1][0], 0, sz * h * 0.8), (pts[0][0], 0, sz * h), 0.04, trim, **W))
    if g:
        prims.append(sphere((bend + 0.06, -0.05, 0.2), 0.05, g, spec=0.9))
    return prims, view(diag=-38, tilt=0, yaw=15), 0.9


def spear(q):
    head = poly([(0, 1.55), (0.2, 1.15), (0.07, 0.9), (-0.07, 0.9), (-0.2, 1.15)])
    return [haft(-0.9, 1.0, 0.05), ext(head, 0.035, "metal", rnd=0.015, spec=0.6), cyl((0, 0, 0.88), (0, 0, 1.0), 0.06, "copper", **W),
            cyl((0, 0, -0.2), (0, 0, 0.15), 0.055, "leather", tone=t_stripes(60, 0.3))], view(diag=40, tilt=8), 0.92


def shield(q, kind):
    trim, g = TRIM[max(q, 2)], GEM[q]
    if kind in ("buckler", "iron_buckler"):
        face = "metal" if kind == "iron_buckler" else "wood"
        prims = [
            lathe(ellipse(0, 0, 0.9, 0.16) & rect(0, 0.5, 2, 0.5), face, tone=t_wood if face == "wood" else None, spec=0.3 if face != "wood" else 0),
            torus((0, 0, 0), 0.88, 0.06, trim, **W),
            sphere((0, 0, 0.12), 0.2, trim, **W),
        ]
        for i in range(8):
            a = i * math.pi / 4
            prims.append(sphere((0.72 * math.cos(a), 0.72 * math.sin(a), 0.06), 0.04, trim, **W))
        if g:
            prims.append(sphere((0, 0, 0.3), 0.08, g, spec=0.9))
        return [p.at((0, 0, 0), rot_x(math.pi / 2)) for p in prims], view(tilt=0, yaw=25) @ rot_x(0.25), 0.84
    kite = poly([(-0.75, 0.85), (0.75, 0.85), (0.72, 0.1), (0.4, -0.6), (0.0, -1.05), (-0.4, -0.6), (-0.72, 0.1)])
    prims = [
        ext(kite, 0.07, "plate" if q >= 4 else "metal", rnd=0.05, spec=0.4),
        ext(kite.shell(0.05), 0.1, trim, rnd=0.03, **W),
        ext(rect(0, 0.05, 0.1, 0.55) | rect(0, 0.3, 0.42, 0.09), 0.09, CLOTH[q] if q >= 3 else "redmark", rnd=0.03),
    ]
    if g:
        prims.append(sphere((0, -0.09, 0.3), 0.09, g, spec=0.9))
    return prims, view(tilt=6, yaw=22), 0.86


WEAPON_MODELS = {
    "dagger": lambda q: dagger(q), "dagger_red": lambda q: dagger(q, red=True),
    "shortsword": lambda q: sword(q, 1.05, 0.14, 0.3), "longsword": lambda q: sword(q, 1.45, 0.12, 0.34),
    "zweihander": lambda q: sword(q, 1.75, 0.12, 0.46, ricasso=True), "greatsword": lambda q: sword(q, 1.75, 0.17, 0.42),
    "club": lambda q: mace(q, "club"), "smith_hammer": lambda q: mace(q, "smith_hammer"), "mace": lambda q: mace(q, "mace"),
    "war_hammer": lambda q: mace(q, "war_hammer"), "maul": lambda q: mace(q, "maul"),
    "hand_axe": lambda q: axe(q, "hand_axe"), "infantry_axe": lambda q: axe(q, "infantry_axe"), "battle_axe": lambda q: axe(q, "battle_axe"),
    "buckler": lambda q: shield(q, "buckler"), "iron_buckler": lambda q: shield(q, "iron_buckler"), "shield": lambda q: shield(q, "kite"),
}
for _m in ("staff", "staff_grey", "greatstaff", "greatstaff_purple", "greatstaff_red"):
    WEAPON_MODELS[_m] = (lambda m: lambda q: staff(q, m))(_m)
for _m in ("wand", "rod", "rod_blue", "rod_purple", "rod_red"):
    WEAPON_MODELS[_m] = (lambda m: lambda q: wand(q, m))(_m)
for _m in ("shortbow", "longbow", "greatbow", "greatbow_orange"):
    WEAPON_MODELS[_m] = (lambda m: lambda q: bow(q, m))(_m)


# --- armour --------------------------------------------------------------------------------------------

FAMILY = {"rb": ("cloth", None), "lt": ("leather", None), "ch": ("chain", t_chain), "pl": ("plate", None)}


def fam_mat(fam, q):
    if fam == "rb":
        return CLOTH[q], t_stripes(40, 0.12)
    if fam == "lt":
        return LEATHER[q], t_noise(0.1, 25)
    if fam == "ch":
        return "chain", t_chain
    return "plate", t_stripes(9, 0.18)


def torso(fam, q):
    mat, tone = fam_mat(fam, q)
    trim, g = TRIM[max(q, 2)], GEM[q]
    body = poly([(-0.72, 0.72), (-0.25, 0.86), (0.25, 0.86), (0.72, 0.72), (0.6, 0.25), (0.48, -0.25), (0.55, -0.85),
                 (-0.55, -0.85), (-0.48, -0.25), (-0.6, 0.25)]) - ellipse(0, 0.9, 0.24, 0.18)
    prims = [ext(body, 0.3, mat, rnd=0.26, tone=tone, spec=0.4 if fam == "pl" else 0.05)]
    if fam == "rb":  # robe: sleeves + V-neck trim + sash
        for sx in (1, -1):
            prims.append(cone((sx * 0.55, 0, 0.6), (sx * 0.9, 0, -0.2), 0.2, 0.17, mat, tone=tone))
        prims.append(ext(poly([(-0.26, 0.82), (0, 0.3), (0.26, 0.82), (0.16, 0.84), (0, 0.48), (-0.16, 0.84)]), 0.33, trim, rnd=0.05, **W).at((0, -0.02, 0)))
        prims.append(box((0, -0.27, -0.3), (0.5, 0.06, 0.07), trim if q >= 3 else "leather"))
    elif fam == "lt":  # jerkin: shoulder pads, laces, belt
        for sx in (1, -1):
            prims.append(ell((sx * 0.6, -0.02, 0.62), (0.24, 0.3, 0.17), mat, tone=tone))
        for z in (0.55, 0.35, 0.15):
            prims.append(cap((-0.08, -0.29, z), (0.08, -0.29, z - 0.08), 0.02, "linen"))
        prims.append(box((0, -0.24, -0.35), (0.52, 0.08, 0.08), "boot"))
        prims.append(rbox((0, -0.31, -0.35), (0.09, 0.03, 0.09), 0.02, trim, **W))
    elif fam == "ch":  # chainmail: short sleeves, leather collar and belt
        for sx in (1, -1):
            prims.append(cone((sx * 0.55, 0, 0.62), (sx * 0.85, 0, 0.15), 0.21, 0.18, mat, tone=tone))
        prims.append(torus((0, 0, 0), 0.26, 0.06, "leather").at((0, 0, 0.82)))
        prims.append(box((0, -0.24, -0.4), (0.52, 0.08, 0.08), "leather"))
        prims.append(rbox((0, -0.31, -0.4), (0.09, 0.03, 0.09), 0.02, trim, **W))
    else:  # breastplate: pauldrons, ridge, trim
        for sx in (1, -1):
            prims.append(ell((sx * 0.62, 0, 0.62), (0.3, 0.34, 0.24), "plate", spec=0.5))
            prims.append(ell((sx * 0.64, -0.02, 0.6), (0.31, 0.33, 0.07), trim, **W))
        prims.append(cap((0, -0.3, 0.7), (0, -0.3, -0.6), 0.04, "plate", spec=0.5))
        prims.append(ext(body.shell(0.035) & rect(0, 0, 0.7, 0.75), 0.31, trim, rnd=0.02, **W))
    if g and fam != "rb":
        prims.append(gem_cut((0, 0, 0), 0.09, g).at((0, -0.33, 0.45), rot_x(-math.pi / 2)))
    elif g:
        prims.append(sphere((0, -0.3, -0.3), 0.08, g, spec=0.9))
    return prims, view(tilt=8, yaw=18), 0.86


def pants(fam, q):
    mat, tone = fam_mat(fam, q)
    trim = TRIM[max(q, 2)]
    legs = union(taper(-0.25, 0.4, -0.3, -0.85, 0.27, 0.19), taper(0.25, 0.4, 0.3, -0.85, 0.27, 0.19), rect(0, 0.62, 0.5, 0.26, rr=0.08))
    prims = [ext(legs, 0.22, mat, rnd=0.18, tone=tone, spec=0.4 if fam == "pl" else 0.05)]
    prims.append(box((0, -0.0, 0.78), (0.52, 0.24, 0.08), "leather" if fam != "pl" else trim, **(W if fam == "pl" else {})))
    if fam == "pl":
        for sx in (1, -1):
            prims.append(ell((sx * 0.28, -0.18, -0.25), (0.17, 0.12, 0.15), trim, **W))
    if fam == "lt":
        for sx in (1, -1):
            prims.append(cap((sx * 0.28, -0.2, -0.45), (sx * 0.3, -0.18, -0.8), 0.025, "boot"))
    if fam == "rb" and q >= 3:
        for sx in (1, -1):
            prims.append(box((sx * 0.3, 0, -0.8), (0.2, 0.2, 0.05), trim))
    return prims, view(tilt=8, yaw=15), 0.86


def boots(fam, q):
    mat, tone = fam_mat(fam, q)
    trim = TRIM[max(q, 2)]
    if fam == "rb":
        prof = union(rect(0.1, -0.62, 0.55, 0.16, rr=0.14), taper(-0.25, -0.4, -0.2, -0.05, 0.22, 0.2))
    else:
        prof = union(rect(0.12, -0.6, 0.6, 0.2, rr=0.16), rect(-0.22, -0.05, 0.27, 0.62, rr=0.1))
    prims = []
    for k, dy in ((0, 0.0), (1, 0.5)):
        o = (0.22 * k, dy, 0.1 * k)
        prims.append(ext(prof, 0.2, mat, rnd=0.15, tone=tone, spec=0.4 if fam == "pl" else 0.05).at(o))
        prims.append(box((0.12, 0, -0.8), (0.62, 0.21, 0.05), "boot").at(o))
        if fam != "rb":
            prims.append(box((-0.22, 0, 0.52), (0.3, 0.23, 0.07), trim if q >= 3 else "leather", **W).at(o))
        if fam == "pl":
            for z in (-0.35, -0.1, 0.15):
                prims.append(box((-0.22, 0, z), (0.29, 0.215, 0.02), trim, **W).at(o))
    return prims, view(tilt=10, yaw=-25), 0.86


def gloves(fam, q):
    mat, tone = fam_mat(fam, q)
    trim, g = TRIM[max(q, 2)], GEM[q]
    sp = 0.4 if fam == "pl" else 0.05
    prims = [rbox((0, 0, 0.05), (0.32, 0.14, 0.3), 0.12, mat, tone=tone, spec=sp)]
    for i, x in enumerate((-0.24, -0.08, 0.08, 0.24)):
        top = 0.78 - 0.08 * abs(i - 1.5) ** 1.5
        prims.append(cap((x, 0, 0.3), (x * 1.1, -0.03, top), 0.08, mat, tone=tone, spec=sp))
    prims.append(cap((-0.3, 0, -0.05), (-0.58, -0.05, 0.35), 0.09, mat, tone=tone, spec=sp))
    cuff = cone((0, 0, -0.25), (0, 0, -0.75), 0.36, 0.42 if fam in ("pl", "ch") else 0.36, trim if fam == "pl" else ("leather" if fam != "rb" else mat))
    prims.append(cuff.opts(spec=0.4 if fam == "pl" else 0, tone=tone if fam == "rb" else None))
    if fam == "pl":
        for i, x in enumerate((-0.24, -0.08, 0.08, 0.24)):
            prims.append(sphere((x, -0.07, 0.42), 0.075, trim, **W))
    if g:
        prims.append(sphere((0, -0.14, 0.05), 0.08, g, spec=0.9))
    return [p.at((0, 0, 0), np.diag([1, 0.85, 1])) for p in prims], view(tilt=5, yaw=10, diag=-12), 0.86


def helm(fam, q):
    mat, tone = fam_mat(fam, q)
    trim, g = TRIM[max(q, 2)], GEM[q]
    if fam == "pl":
        prims = [
            lathe(circle(0, 0.05, 0.62) | rect(0, -0.3, 0.62, 0.35), "plate", spec=0.5),
            box((0, -0.6, -0.08), (0.36, 0.06, 0.05), "dark"),  # eye slit
            box((0, -0.6, -0.36), (0.035, 0.06, 0.2), "dark"),
            cap((0, -0.66, 0.6), (0, -0.62, -0.2), 0.05, trim, **W),  # ridge / nose guard
            torus((0, 0, -0.62), 0.6, 0.06, trim, **W),
        ]
        if q >= 4:
            prims.append(ell((0, 0.05, 0.75), (0.06, 0.4, 0.2), CLOTH[q]))
    elif fam == "ch":
        prims = [
            lathe(circle(0, 0.1, 0.58) | rect(0, -0.45, 0.62, 0.45, rr=0.1), "chain", tone=t_chain),
            ell((0, -0.5, -0.12), (0.3, 0.12, 0.34), "dark"),  # face opening
            torus((0, 0, -0.9), 0.6, 0.06, "leather"),
        ]
    elif fam == "lt":
        prims = [
            lathe(circle(0, -0.1, 0.6) & rect(0, 0.3, 1, 0.4), mat, tone=tone),
            torus((0, 0, -0.06), 0.62, 0.08, trim if q >= 3 else "boot", **(W if q >= 3 else {})),
            ell((0.45, -0.25, -0.22), (0.18, 0.08, 0.26), mat, tone=tone),
            ell((-0.45, -0.25, -0.22), (0.18, 0.08, 0.26), mat, tone=tone),
        ]
    else:  # cloth hat: pointed with a brim
        prims = [
            lathe(taper(0, -0.3, 0.0, 0.95, 0.5, 0.02), mat, tone=tone),
            lathe(ellipse(0, -0.35, 0.95, 0.07), mat, tone=tone),
            torus((0, 0, -0.2), 0.48, 0.06, trim if q >= 3 else "leather"),
        ]
    if g:
        prims.append(sphere((0, -0.6, 0.3 if fam != "rb" else -0.2), 0.08, g, spec=0.9))
    return prims, view(tilt=12, yaw=20), 0.84


ARMOUR = {"torso": torso, "pants": pants, "shoes": boots, "glove": gloves, "head": helm}


# --- jewellery, belts --------------------------------------------------------------------------------


def ring(q, gem=None):
    trim, g = TRIM[max(q, 2)], gem or GEM[q] or "topaz"
    prims = [torus((0, 0, 0), 0.55, 0.13, trim, **W).at((0, 0, 0), rot_x(math.pi / 2)),
             cyl((0, 0, 0.55), (0, 0, 0.75), 0.22, trim, **W),
             gem_cut((0, 0, 0.78), 0.2, g)]
    for i in range(4):
        a = i * math.pi / 2 + math.pi / 4
        prims.append(cap((0.17 * math.cos(a), 0.17 * math.sin(a), 0.68), (0.19 * math.cos(a), 0.19 * math.sin(a), 0.86), 0.035, trim, **W))
    return prims, view(tilt=25, yaw=30), 0.82


def necklace(q, kind="pendant"):
    trim, g = TRIM[max(q, 2)], GEM[q] or "topaz"
    prims = []
    n = 16
    for i in range(n + 1):
        a = math.pi * (0.1 + 0.8 * i / n)
        x, z = 0.75 * math.cos(a), 0.55 - 0.95 * math.sin(a) * (1 if kind != "choker" else 0.6)
        prims.append(torus((0, 0, 0), 0.06, 0.022, trim, **W).at((x, 0, z), rot_x(math.pi / 2) if i % 2 else rot_y(math.pi / 2) @ rot_x(math.pi / 2)))
    bottom = 0.55 - 0.95 * (1 if kind != "choker" else 0.6)
    prims.append(ext(circle(0, 0, 0.26) | poly([(-0.2, -0.1), (0.2, -0.1), (0, -0.55)]), 0.05, trim, rnd=0.03, **W).at((0, 0, bottom - 0.2)))
    prims.append(gem_cut((0, 0, 0), 0.17, g).at((0, -0.07, bottom - 0.2), rot_x(-math.pi / 2)))
    return prims, view(tilt=5, yaw=10), 0.86


def belt(q, sash=False):
    trim, g = TRIM[max(q, 2)], GEM[q]
    mat = CLOTH[q] if sash else LEATHER[q]
    band = S3(lambda P: np.maximum(np.abs(np.hypot(P[:, 0], P[:, 1] / 0.55) - 0.75) - 0.06, np.abs(P[:, 2]) - 0.16),
              (-0.85, -0.5, -0.17), (0.85, 0.5, 0.17), mat, tone=t_stripes(30, 0.12) if sash else t_noise(0.1, 25))
    prims = [band]
    if sash:
        prims += [ell((0.25, -0.43, 0), (0.15, 0.08, 0.14), mat), cone((0.25, -0.45, -0.05), (0.38, -0.5, -0.6), 0.09, 0.12, mat),
                  cone((0.25, -0.45, -0.05), (0.15, -0.5, -0.55), 0.09, 0.11, mat)]
    else:
        prims += [ext(rect(0, 0, 0.17, 0.2, rr=0.04).shell(0.035), 0.03, trim, **W).at((0, -0.43, 0)),
                  cap((0, -0.46, 0), (0.12, -0.46, 0), 0.02, trim, **W),
                  box((-0.3, -0.42, -0.25), (0.06, 0.04, 0.2), mat)]
        for x in (0.3, 0.45):
            prims.append(cyl((x, -0.44, 0), (x, -0.47, 0), 0.025, trim, **W))
    if g:
        prims.append(sphere((0, -0.5, 0.0) if sash else (-0.55, -0.32, 0), 0.07, g, spec=0.9))
    return prims, view(tilt=40, yaw=0), 0.86


# --- consumables ---------------------------------------------------------------------------------------


def flask(liquid, size=3, tall=False, square=False):
    """Glass flask with coloured liquid (the glass front is left out so the liquid shows)."""
    k = 0.8 + 0.06 * size
    if tall:
        body = rect(0, -0.25, 0.3 * k, 0.6 * k, rr=0.25 * k)
        neck = rect(0, 0.5 * k, 0.12, 0.25, rr=0.03)
        fill = 0.15 * k
    elif square:
        body = rect(0, -0.3, 0.5 * k, 0.5 * k, rr=0.12)
        neck = rect(0, 0.35 * k, 0.13, 0.25, rr=0.03)
        fill = 0.0
    else:
        body = circle(0, -0.3, 0.55 * k)
        neck = rect(0, 0.35 * k, 0.13, 0.28, rr=0.03)
        fill = -0.1 * k
    top = 0.35 * k + 0.28
    glass = (body | neck) - rect(0, fill - 1, 2, 1)
    liquid_shape = body & rect(0, fill - 1, 2, 1)
    prims = [
        ext(liquid_shape, 0.4 * k, liquid, rnd=0.1, spec=0.9) if square else lathe(liquid_shape, liquid, spec=0.9),
        ext(glass, 0.4 * k, "glass", rnd=0.1, spec=0.8) if square else lathe(glass, "glass", spec=0.8),
        lathe(rect(0, top + 0.02, 0.17, 0.05, rr=0.02), "glass", spec=0.8),
        lathe(rect(0, top + 0.12, 0.11, 0.1, rr=0.03), "wood" if size < 4 else "gold", tone=t_wood if size < 4 else None, spec=0 if size < 4 else 0.4),
    ]
    if size >= 3:  # wax seal / string around the neck
        prims.append(torus((0, 0, top - 0.06), 0.15, 0.035, "redmark" if size < 5 else "gold", spec=0.3))
    if size >= 5:
        prims.append(torus((0, 0, -0.3 if not tall else -0.25), (0.55 if not tall else 0.3) * k + 0.01, 0.04, "gold", spec=0.5))
    # glint on the liquid
    prims.append(ell((-0.22 * k, -0.5, -0.12 if not tall else 0.1), (0.05, 0.04, 0.12), "glass", emit=1.0))
    return prims, view(tilt=8, yaw=10), 0.84


def mug():
    return [lathe(rect(0, 0, 0.42, 0.5, rr=0.05) - rect(0, 0.5, 0.34, 0.1), "wood", tone=t_grain(30, 0.15, 0)),
            torus((0, 0, 0.32), 0.43, 0.04, "copper", **W), torus((0, 0, -0.32), 0.43, 0.04, "copper", **W),
            rough(ell((0, 0, 0.5), (0.4, 0.4, 0.14), "paper"), 0.03, 12, 2),
            torus((0, 0, 0), 0.25, 0.06, "wood").at((0.48, 0, 0), rot_x(math.pi / 2))], view(tilt=12, yaw=20), 0.8


def scroll(seal="redmark", ribbon="redmark", open_=False):
    prims = [cyl((-0.7, 0, 0.5), (0.7, 0, 0.5), 0.17, "paper", tone=t_grain(60, 0.08, 0)),
             cyl((-0.7, 0, -0.5), (0.7, 0, -0.5), 0.17, "paper", tone=t_grain(60, 0.08, 0)),
             box((0, 0.0, 0), (0.62, 0.03, 0.5), "paper", tone=lambda P: 1 - 0.18 * ((np.abs(P[:, 2] * 7 % 1 - 0.5) < 0.1) & (np.abs(P[:, 0]) < 0.45)))]
    for sx in (1, -1):
        for z in (0.5, -0.5):
            prims.append(cyl((sx * 0.7, 0, z), (sx * 0.8, 0, z), 0.09, "wood", tone=t_wood))
    prims.append(cyl((0, -0.04, -0.15), (0, -0.09, -0.15), 0.15, seal, spec=0.3))
    prims.append(box((0, -0.03, -0.45), (0.06, 0.02, 0.28), ribbon))
    return prims, view(tilt=-5, yaw=8, diag=-18), 0.88


def rolled_scroll(seal="sapphire"):
    return [cyl((-0.85, 0, 0), (0.85, 0, 0), 0.32, "paper", tone=t_grain(60, 0.08, 0)),
            cyl((-0.86, 0, 0), (-0.8, 0, 0), 0.2, "paper", tone=lambda P: 0.7 + 0 * P[:, 0]),
            cyl((-0.12, 0, 0), (0.12, 0, 0), 0.34, seal),
            sphere((0, -0.33, 0), 0.13, "redmark", spec=0.3)], view(diag=-30, tilt=15, yaw=10), 0.88


def tome(cover="redmark", trim="gold", gem=None, emblem=None):
    prims = [rbox((0, 0, 0), (0.6, 0.18, 0.8), 0.04, cover, tone=t_noise(0.08, 25)),
             box((0.04, 0, 0), (0.58, 0.14, 0.76), "paper", tone=t_stripes(90, 0.2, 1)),
             rbox((-0.02, -0.2, 0), (0.6, 0.03, 0.8), 0.03, cover, tone=t_noise(0.08, 25)),
             rbox((-0.58, 0, 0), (0.06, 0.2, 0.8), 0.04, cover)]
    for sx in (1, -1):
        for sz in (1, -1):
            prims.append(box((sx * 0.5, -0.2, sz * 0.7), (0.12, 0.04, 0.12), trim, **W))
    for z in (0.5, -0.5):
        prims.append(box((-0.6, 0, z), (0.07, 0.21, 0.04), trim, **W))
    if emblem == "skull":
        prims += [ell((0, -0.24, 0.1), (0.22, 0.05, 0.2), "bone"), box((0, -0.24, -0.1), (0.13, 0.05, 0.08), "bone"),
                  sphere((-0.08, -0.29, 0.08), 0.05, "dark"), sphere((0.08, -0.29, 0.08), 0.05, "dark")]
    else:
        prims.append(ext(rect(0, 0, 0.26, 0.34, rr=0.06).shell(0.03), 0.04, trim, **W).at((0, -0.22, 0.05)))
        if gem:
            prims.append(gem_cut((0, 0, 0), 0.13, gem).at((0, -0.24, 0.05), rot_x(-math.pi / 2)))
    return prims, view(tilt=5, yaw=25), 0.84


def letter():
    env = poly([(-0.85, 0.5), (0.85, 0.5), (0.85, -0.5), (-0.85, -0.5)])
    flap = poly([(-0.85, 0.5), (0.85, 0.5), (0, -0.1)])
    return [ext(env, 0.03, "paper"), ext(flap, 0.02, "paper", tone=lambda P: 0.85 + 0 * P[:, 0]).at((0, -0.035, 0)),
            cyl((0, -0.05, -0.08), (0, -0.1, -0.08), 0.16, "redmark", spec=0.3)], view(tilt=-10, yaw=10, diag=-10), 0.86


# --- gems & orbs ----------------------------------------------------------------------------------------


def gem_icon(col, cut):
    if cut == "rough":
        return [facet_rock((0, 0, 0), 0.6, col, n=12, seed=hash(col) % 97, spec=0.5)], view(tilt=20, yaw=15), 0.66
    if cut == "crystal":
        planes = []
        for i in range(6):
            a = i * math.pi / 3
            planes.append(((math.cos(a), math.sin(a), 0), 0.32))
            planes.append(((math.cos(a), math.sin(a), 0.45), 0.32 + 0.45 * 0.7))
            planes.append(((math.cos(a), math.sin(a), -0.45), 0.32 + 0.45 * 0.7))
        return [convex(planes, col, spec=0.9).at((0, 0, 0), rot_y(0.5))], view(tilt=10, yaw=20), 0.78
    if cut == "refined":  # oval cabochon with a table
        planes = [((0, 0, 1), 0.28)]
        for i in range(10):
            a = i * math.pi / 5
            planes.append(((math.cos(a) * 0.65, math.sin(a), 0.3), 0.5))
            planes.append(((math.cos(a) * 0.65, math.sin(a), -0.5), 0.45))
        return [convex(planes, col, spec=0.9)], view(tilt=50, yaw=20), 0.74
    if cut == "draconic":  # big brilliant in golden claws
        prims = [gem_cut((0, 0, 0), 0.55, col, n=10)]
        for i in range(4):
            a = i * math.pi / 2 + math.pi / 4
            c, s = math.cos(a), math.sin(a)
            prims.append(cone((0.5 * c, 0.5 * s, -0.4), (0.62 * c, 0.62 * s, 0.05), 0.08, 0.05, "gold", **W))
            prims.append(cone((0.62 * c, 0.62 * s, 0.05), (0.45 * c, 0.45 * s, 0.22), 0.05, 0.01, "gold", **W))
        prims.append(lathe(rect(0, -0.5, 0.45, 0.08, rr=0.03), "gold", **W))
        return prims, view(tilt=35, yaw=10), 0.84
    return [gem_cut((0, 0, 0), 0.6, col, n=8, facet=0.5)], view(tilt=18, yaw=10), 0.78  # flawless brilliant


def orb(col, tier):
    """Enchanting orb on a stand; tier 0..5 adds a fancier stand, rays and glow."""
    stand = {0: "wood", 1: "wood", 2: "metal", 3: "plate", 4: "gold", 5: "gold"}[tier]
    prims = [sphere((0, 0, 0.18), 0.55, col, spec=0.9, tone=lambda P: 0.85 + 0.25 * np.sin(P[:, 0] * 9 + P[:, 2] * 6 + np.sin(P[:, 1] * 8) * 2)),
             ell((-0.2, -0.45, 0.4), (0.08, 0.05, 0.12), "glass", emit=1.0),
             lathe(poly([(0, -0.78), (0.5, -0.78), (0.45, -0.66), (0.22, -0.55), (0.32, -0.3), (0, -0.3)]), stand,
                   tone=t_wood if stand == "wood" else None, spec=0 if stand == "wood" else 0.5)]
    if tier >= 3:
        for i in range(3):
            a = i * 2 * math.pi / 3 + 0.5
            prims.append(cone((0.3 * math.cos(a), 0.3 * math.sin(a), -0.45), (0.5 * math.cos(a), 0.5 * math.sin(a), 0.2), 0.05, 0.02, stand, **W))
    if tier >= 5:
        prims.append(torus((0, 0, 0), 0.72, 0.035, "gold", emit=0.9).at((0, 0, 0.18), rot_x(1.2)))
    return prims, view(tilt=12, yaw=10), 0.8 + 0.02 * tier


# --- food ------------------------------------------------------------------------------------------------


def plate_dish():
    return lathe(ellipse(0, -0.55, 0.95, 0.07), "glass", spec=0.4)


def food(kind):
    if kind == "pie":
        prims = [lathe(poly([(0, -0.3), (0.75, -0.3), (0.85, 0.05), (0, 0.12)]), "crust", tone=lambda P: 1 + 0.15 * np.sin(np.arctan2(P[:, 1], P[:, 0]) * 14)),
                 lathe(ellipse(0, 0.08, 0.72, 0.12), "crust", tone=lambda P: 0.85 + 0.3 * ((np.abs(P[:, 0] * 6 % 1 - 0.5) < 0.12) | (np.abs(P[:, 1] * 6 % 1 - 0.5) < 0.12)))]
        return prims, view(tilt=40, yaw=10), 0.86
    if kind == "turkey":
        prims = [plate_dish(), rough(ell((0, 0, -0.15), (0.62, 0.45, 0.38), "meat"), 0.02, 10, 4)]
        for sx in (1, -1):
            prims += [ell((sx * 0.45, -0.25, -0.1), (0.18, 0.18, 0.22), "meat"), cap((sx * 0.5, -0.3, 0.05), (sx * 0.72, -0.45, 0.3), 0.05, "bone"),
                      sphere((sx * 0.74, -0.47, 0.33), 0.07, "bone")]
        return prims, view(tilt=25, yaw=15), 0.88
    if kind == "pork":
        prims = [plate_dish(), rough(ell((0, 0, -0.18), (0.72, 0.42, 0.34), "meat"), 0.025, 8, 5),
                 ell((0.62, -0.05, -0.1), (0.2, 0.24, 0.2), "meat"), sphere((0.78, -0.12, -0.06), 0.09, "ruby", spec=0.6),
                 sphere((-0.6, -0.4, -0.4), 0.12, "leaf"), sphere((-0.4, -0.5, -0.42), 0.1, "leaf"), sphere((0.4, -0.5, -0.42), 0.09, "ruby", spec=0.5)]
        return prims, view(tilt=22, yaw=15), 0.88
    if kind == "sandwich":
        prims = [rbox((0, 0, -0.35), (0.7, 0.5, 0.12), 0.08, "crust"), rough(box((0, 0, -0.2), (0.74, 0.54, 0.04), "leaf"), 0.03, 14, 1),
                 rbox((0, 0, -0.1), (0.72, 0.52, 0.06), 0.03, "flesh"), rbox((0, 0, 0.0), (0.7, 0.5, 0.05), 0.02, "cheese"),
                 rbox((0, 0, 0.18), (0.7, 0.5, 0.13), 0.1, "crust")]
        return [p.at((0, 0, 0), rot_z(0.3)) for p in prims], view(tilt=30, yaw=10), 0.88
    # ration: loaf + cheese wedge
    wedge = poly([(-0.5, -0.3), (0.5, -0.3), (-0.5, 0.25)])
    prims = [rough(ell((-0.15, 0.1, 0.0), (0.6, 0.35, 0.32), "crust"), 0.02, 9, 7),
             ext(wedge, 0.22, "cheese", rnd=0.03, tone=lambda P: 1 - 0.3 * (np.sin(P[:, 0] * 30) * np.sin(P[:, 2] * 30) > 0.8)).at((0.3, -0.3, -0.15))]
    for x in (-0.4, -0.15, 0.1):
        prims.append(cap((x, -0.2, 0.28), (x + 0.1, -0.25, 0.2), 0.025, "fur"))
    return prims, view(tilt=20, yaw=20), 0.86


# --- materials & junk -----------------------------------------------------------------------------------


def curved_spike(ramp, length=1.6, r=0.2, bend=0.5, n=8, broken=False):
    prims = []
    for i in range(n):
        t0, t1 = i / n, (i + 1) / n
        p0 = (bend * t0 * t0 - 0.3, 0, -length / 2 + length * t0)
        p1 = (bend * t1 * t1 - 0.3, 0, -length / 2 + length * t1)
        r0 = r * (1 - t0) + 0.015
        r1 = r * (1 - t1) + 0.015
        if broken and i == n - 1:
            break
        prims.append(cone(p0, p1, r0, r1, ramp))
    return prims


def bone(kind=1):
    prims = [cap((0, 0, -0.6), (0, 0, 0.6), 0.12, "bone", tone=t_noise(0.06, 20))]
    for sz in (1, -1):
        if kind == 2:  # femur: ball + knobs
            prims += [sphere((0.1, 0, sz * 0.72), 0.17, "bone"), sphere((-0.1, 0, sz * 0.7), 0.15, "bone")]
        else:
            prims += [sphere((0.09, 0, sz * 0.68), 0.15, "bone"), sphere((-0.09, 0, sz * 0.68), 0.15, "bone")]
    if kind == 1:
        prims.append(ell((0.04, -0.12, 0.1), (0.05, 0.03, 0.12), "blood"))
    return prims, view(diag=45, tilt=10, yaw=0), 0.86


def skull():
    return [ell((0, 0, 0.12), (0.5, 0.55, 0.5), "bone", tone=t_noise(0.06, 18)),
            rbox((0, -0.18, -0.38), (0.3, 0.3, 0.18), 0.1, "bone"),
            ell((-0.19, -0.45, 0.02), (0.13, 0.08, 0.12), "dark"), ell((0.19, -0.45, 0.02), (0.13, 0.08, 0.12), "dark"),
            ell((0, -0.5, -0.17), (0.05, 0.05, 0.07), "dark"),
            box((0, -0.47, -0.42), (0.2, 0.02, 0.02), "dark"),
            cap((0.3, -0.38, 0.4), (0.12, -0.45, 0.25), 0.015, "dark")], view(tilt=8, yaw=25), 0.78


def coins(n, metal="gold", seed=0):
    rng = np.random.default_rng(seed)
    prims = []
    for i in range(n):
        if i < n // 2 + 1 and n > 3:  # stack
            x, y, z = 0.2 - 0.25 * (i % 2) * 0.1, 0.1, -0.5 + 0.1 * i
            a = (0.0, 0.0)
        else:
            x, y, z = rng.uniform(-0.6, 0.6), rng.uniform(-0.4, 0.2), rng.uniform(-0.55, -0.35)
            a = rng.uniform(-0.5, 0.5, 2)
        R = rot_x(a[0]) @ rot_y(a[1]) if n > 1 else rot_x(1.1) @ rot_y(0.3)
        prims.append(cyl((0, 0, -0.04), (0, 0, 0.04), 0.3, metal, spec=0.5,
                         tone=lambda P: 1 - 0.25 * ((np.hypot(P[:, 0], P[:, 1]) > 0.22) & (np.hypot(P[:, 0], P[:, 1]) < 0.25))).at((x, y, z), R))
    return prims, view(tilt=35, yaw=10), 0.66 + 0.03 * min(n, 7)


def hide(ramp, seed=0, spots=None):
    rng = np.random.default_rng(seed)
    ph = rng.uniform(0, 6.28, 3)

    def f(X, Y):
        a = np.arctan2(Y, X)
        r = 0.75 + 0.1 * np.sin(3 * a + ph[0]) + 0.07 * np.sin(5 * a + ph[1]) + 0.05 * np.sin(8 * a + ph[2])
        return np.hypot(X, Y * 1.15) - r

    tone = t_noise(0.12, 14, seed)
    if spots:
        tone = lambda P: (1 - 0.35 * (np.sin(P[:, 0] * 11) * np.sin(P[:, 2] * 11 + 1) > 0.6))
    return [ext(Sh(f), 0.06, ramp, rnd=0.04, tone=tone).at((0, 0, 0), rot_x(-0.2))], view(tilt=50, yaw=10), 0.86


def feather(ramp, broken=False):
    vane = taper(0, -0.7, 0, 0.85, 0.08, 0.02).warp(lambda X, Y: (X - 0.08 * np.sin(Y * 2), Y)) | \
        (ellipse(0, 0.1, 0.32, 0.85).warp(lambda X, Y: (X - 0.08 * np.sin(Y * 2), Y)) - rect(0.3, -0.75, 0.3, 0.12, a=0.4))
    if broken:
        vane = vane - rect(0.3, 0.3, 0.25, 0.04, a=-0.4) - rect(-0.3, -0.1, 0.25, 0.03, a=0.4)
    return [ext(vane, 0.03, ramp, tone=lambda P: 1 + 0.15 * np.sin(P[:, 2] * 40 + np.abs(P[:, 0]) * 30)),
            cap((0, -0.05, -1.0), (0, -0.05, 0.8), 0.025, "bone").at((0, 0, 0))], view(diag=40, tilt=0, yaw=10), 0.9


def fur_tuft(ramp):
    prims = []
    rng = np.random.default_rng(5)
    for i in range(9):
        a = -0.8 + 1.6 * i / 8
        prims.append(cone((0, 0, -0.5), (0.9 * math.sin(a), rng.uniform(-0.1, 0.1), -0.5 + 1.0 * math.cos(a) + rng.uniform(-0.1, 0.1)), 0.14, 0.02, ramp))
    return prims, view(tilt=10), 0.82


def key(ramp="metal", glow=None):
    shape = (circle(0, 0.5, 0.32) - circle(0, 0.5, 0.17)) | rect(0, -0.25, 0.07, 0.55) | rect(0.17, -0.7, 0.13, 0.08) | rect(0.15, -0.48, 0.1, 0.06)
    prims = [ext(shape, 0.06, ramp, rnd=0.03, spec=0.5)]
    if glow:
        prims.append(gem_cut((0, 0, 0), 0.13, glow, emit=0.8).at((0, 0, 0.5), rot_x(-math.pi / 2)))
    return prims, view(diag=35, tilt=5, yaw=20), 0.86


def flower():
    prims = [cap((0, 0, -0.9), (0.1, 0, 0.1), 0.04, "leaf"), ell((0.25, 0, -0.45), (0.22, 0.04, 0.09), "leaf").at((0, 0, 0), rot_y(-0.4))]
    for i in range(6):
        a = i * math.pi / 3
        prims.append(ell((0.1 + 0.25 * math.cos(a), 0.0, 0.3 + 0.25 * math.sin(a)), (0.2, 0.08, 0.2), "pink"))
    prims.append(sphere((0.1, -0.08, 0.3), 0.12, "topaz"))
    return prims, view(tilt=5, yaw=10), 0.82


def chest():
    return [rbox((0, 0, -0.25), (0.75, 0.45, 0.35), 0.04, "wood", tone=t_grain(25, 0.15, 0)),
            S3(lambda P: np.maximum(np.hypot(P[:, 1], P[:, 2] - 0.1) - 0.45, np.abs(P[:, 0]) - 0.75), (-0.75, -0.45, 0.1), (0.75, 0.45, 0.55), "redmark"),
            box((0.0, 0, 0.1), (0.78, 0.48, 0.04), "gold", **W),
            box((0.45, 0, 0.05), (0.06, 0.48, 0.4), "gold", **W), box((-0.45, 0, 0.05), (0.06, 0.48, 0.4), "gold", **W),
            rbox((0, -0.47, 0.05), (0.1, 0.03, 0.12), 0.02, "gold", **W),
            gem_cut((0, 0, 0), 0.1, "ruby").at((0, -0.5, 0.35), rot_x(-math.pi / 2))], view(tilt=18, yaw=25), 0.86


def pouch(ramp="leather", sparkle="amethyst"):
    prims = [rough(ell((0, 0, -0.2), (0.55, 0.5, 0.5), ramp), 0.03, 7, 3), cone((0, 0, 0.25), (0, 0, 0.55), 0.18, 0.25, ramp),
             torus((0, 0, 0.32), 0.19, 0.04, "linen")]
    if sparkle:
        for x, z in ((-0.4, 0.6), (0.35, 0.75), (0.05, 0.85), (0.5, 0.35)):
            prims.append(sphere((x, -0.3, z), 0.06, sparkle, emit=1.0))
    return prims, view(tilt=10, yaw=15), 0.82


def junk_metal(seed, n=3, ramp="metal"):
    prims = []
    rng = np.random.default_rng(seed)
    for i in range(n):
        R = rot_x(rng.uniform(-1, 1)) @ rot_z(rng.uniform(-1, 1)) @ rot_y(rng.uniform(-1, 1))
        prims.append(facet_rock((0, 0, 0), 0.32, ramp, n=9, seed=seed * 10 + i, squash=(1, 1, 0.4), spec=0.3,
                                tone=t_noise(0.25, 12, seed + i)).at((rng.uniform(-0.45, 0.45), rng.uniform(-0.2, 0.2), rng.uniform(-0.35, 0.35)), R))
    return prims, view(tilt=20, yaw=10), 0.82


def mineral(base, gem, seed):
    prims = [rough(facet_rock((0, 0, -0.1), 0.6, base, n=11, seed=seed), 0.03, 8, seed)]
    if gem:
        rng = np.random.default_rng(seed)
        for i in range(3):
            a = rng.uniform(-1.2, 1.2)
            prims.append(convex([((math.cos(k * 1.047), math.sin(k * 1.047), 0), 0.09) for k in range(6)] + [((0, 0, 1), 0.45), ((0, 0, -1), 0.1)],
                                gem, spec=0.9).at((0.25 * math.sin(a), -0.25, 0.1), rot_y(a * 0.8)))
    return prims, view(tilt=15, yaw=20), 0.78


def insect_leg(arm=False):
    pts = [(-0.7, 0, -0.75), (-0.3, 0, 0.2), (0.35, 0, 0.55), (0.75, 0, -0.1)] if not arm else [(-0.75, 0, -0.6), (-0.1, 0, 0.1), (0.6, 0, 0.6)]
    prims = []
    for i in range(len(pts) - 1):
        prims.append(cone(pts[i], pts[i + 1], 0.13 - 0.03 * i, 0.1 - 0.03 * i, "chitin", spec=0.4))
        prims.append(sphere(pts[i + 1], 0.12 - 0.03 * i, "chitin", spec=0.4))
    if arm:
        prims += curved_spike("chitin", 0.6, 0.08, 0.3, 5)
    return prims, view(tilt=5, yaw=15), 0.86


def animal_leg():
    return [cone((-0.2, 0, 0.75), (0.0, 0, 0.0), 0.32, 0.18, "fur", tone=t_noise(0.2, 25)), cone((0.0, 0, 0.0), (0.05, 0, -0.6), 0.14, 0.12, "fur", tone=t_noise(0.2, 25)),
            lathe(poly([(0, -1.0), (0.22, -1.0), (0.17, -0.65), (0, -0.65)]), "dark").at((0.08, 0, 0.1)),
            ell((-0.2, 0, 0.78), (0.3, 0.3, 0.1), "meat")], view(diag=-25, tilt=10, yaw=20), 0.86


def carapace():
    return [S3(lambda P: np.maximum(np.linalg.norm(P / [0.8, 0.6, 0.55], axis=1) - 1, -P[:, 2] - 0.05) * 0.5, (-0.8, -0.6, -0.1), (0.8, 0.6, 0.6), "chitin", spec=0.5,
               tone=lambda P: 1 - 0.3 * (np.abs(P[:, 0] * 3 % 1 - 0.5) < 0.06))], view(tilt=35, yaw=20), 0.86


def scale_item():
    sh = circle(0, 0.15, 0.6) | poly([(-0.55, 0.0), (0.55, 0.0), (0, -0.85)])
    return [ext(sh, 0.06, "chitin", rnd=0.05, spec=0.6, tone=lambda P: 1 + 0.15 * np.sin(np.hypot(P[:, 0], P[:, 2] - 0.8) * 25)).at((0, 0, 0), rot_x(-0.3))], view(tilt=20, yaw=10), 0.8


def brain():
    return [rough(ell((0, 0, 0), (0.7, 0.55, 0.5), "flesh"), 0.05, 14, 3), cap((0, -0.5, 0.4), (0, -0.55, -0.3), 0.02, "flesh").opts(tone=lambda P: 0.6 + 0 * P[:, 0])], view(tilt=20, yaw=20), 0.82


def broken_ring():
    return [torus((0, 0, 0), 0.5, 0.11, "gold", **W).at((0, 0, 0), rot_x(1.3)),
            facet_rock((0.45, -0.3, 0.3), 0.18, "ruby", n=8, seed=4, spec=0.8),
            facet_rock((-0.5, -0.2, -0.45), 0.12, "gold", n=7, seed=5, spec=0.5)], view(tilt=10, yaw=10), 0.8


def lamp():
    return [lathe(ellipse(0, 0, 0.6, 0.28) | rect(0, 0.3, 0.15, 0.12), "copper", spec=0.4, tone=t_noise(0.2, 20, 3)),
            cone((0.5, 0, 0.0), (0.9, 0, 0.15), 0.15, 0.06, "copper", spec=0.4),
            torus((0, 0, 0), 0.22, 0.05, "copper", spec=0.4).at((-0.6, 0, 0.05), rot_x(math.pi / 2)),
            box((0.1, -0.5, 0.05), (0.18, 0.05, 0.02), "dark").at((0, 0, 0))], view(tilt=20, yaw=20), 0.84


def fossil():
    def tone(P):
        a = np.arctan2(P[:, 2], P[:, 0])
        r = np.hypot(P[:, 0], P[:, 2])
        return 0.8 + 0.4 * (np.sin(np.log(r + 0.05) * 9 - a) > 0.4)

    return [rough(cyl((0, -0.15, 0), (0, 0.15, 0), 0.75, "stone", tone=tone), 0.04, 6, 1)], view(tilt=5, yaw=15), 0.82


def hoof():
    return [lathe(poly([(0, -0.6), (0.6, -0.6), (0.42, 0.25), (0, 0.25)]), "dark", tone=t_stripes(30, 0.2)), cone((0, 0, 0.2), (0, 0, 0.75), 0.42, 0.35, "fur", tone=t_noise(0.2, 25))], view(tilt=15, yaw=20), 0.78


def eyepatch():
    return [ext(ellipse(0, 0, 0.45, 0.32), 0.05, "boot", rnd=0.04, tone=t_noise(0.2, 20)),
            S3(lambda P: np.hypot(np.hypot(P[:, 0] / 1.0, P[:, 1] / 0.4 + 0.2) - 0.85, P[:, 2] - 0.15) - 0.03, (-0.9, -0.6, 0.0), (0.9, 0.3, 0.3), "leather")], view(tilt=-10, yaw=0), 0.84


def gift():
    return [rbox((0, 0, -0.2), (0.6, 0.6, 0.5), 0.03, "cloth_b"), box((0, 0, -0.2), (0.1, 0.62, 0.52), "gold", **W), box((0, 0, -0.2), (0.62, 0.1, 0.52), "gold", **W),
            ell((-0.18, 0, 0.42), (0.2, 0.08, 0.14), "gold", **W).at((0, 0, 0), rot_y(0.4)), ell((0.18, 0, 0.42), (0.2, 0.08, 0.14), "gold", **W).at((0, 0, 0), rot_y(-0.4))], view(tilt=20, yaw=30), 0.82


def extractor():
    return [rough(facet_rock((0, 0, -0.35), 0.45, "stone", n=10, seed=3), 0.02, 8, 1), sphere((0, 0, 0.3), 0.35, "teal", emit=0.75, spec=0.8)] + \
        [cone((0.4 * math.cos(a), 0.4 * math.sin(a), -0.2), (0.3 * math.cos(a), 0.3 * math.sin(a), 0.6), 0.07, 0.02, "gold", **W) for a in (0.5, 2.6, 4.7)], view(tilt=15, yaw=10), 0.82


def shatter():
    prims, _, _ = mace(4, "smith_hammer")
    prims = [p.at((0.1, 0, 0.2), rot_y(0.6)) for p in prims]
    for i, (x, z, g) in enumerate(((-0.6, -0.7, "ruby"), (-0.25, -0.85, "sapphire"), (0.15, -0.8, "emerald"))):
        prims.append(facet_rock((x, -0.2, z), 0.15, g, n=7, seed=i, spec=0.9))
    return prims, view(tilt=10, yaw=15), 0.86


def time_crystal():
    planes = []
    for i in range(6):
        a = i * math.pi / 3
        planes.append(((math.cos(a), math.sin(a), 0.6), 0.5))
    top = convex(planes + [((0, 0, -1), 0.0)], "teal", spec=0.9)
    bot = convex([((math.cos(i * 1.047), math.sin(i * 1.047), -0.6), 0.5) for i in range(6)] + [((0, 0, 1), 0.0)], "teal", spec=0.9)
    return [top.at((0, 0, 0.02)), bot.at((0, 0, -0.02)), torus((0, 0, 0), 0.3, 0.05, "gold", **W)], view(tilt=15, yaw=15), 0.8


def essence():
    prims = []
    for i, (x, z, h, a) in enumerate(((0, -0.2, 0.9, 0), (-0.3, -0.3, 0.55, -0.5), (0.3, -0.35, 0.6, 0.5), (0.12, -0.4, 0.4, 0.9))):
        prims.append(convex([((math.cos(k * 1.047), math.sin(k * 1.047), 0), 0.12) for k in range(6)] + [((0, 0, 1), h), ((0, 0, -1), 0.0)]
                            + [((math.cos(k * 1.047), math.sin(k * 1.047), 0.4), 0.12 + 0.4 * (h - 0.15))for k in range(6)],
                            "amethyst", spec=0.9, emit=0.7).at((x, 0, z), rot_y(a)))
    prims.append(rough(ell((0, 0, -0.45), (0.55, 0.35, 0.15), "stone"), 0.03, 9, 2))
    return prims, view(tilt=12, yaw=15), 0.84


def generic_sack():
    return pouch("linen", None)


# --- classification -----------------------------------------------------------------------------------

ARMOUR_RE = re.compile(r"icon_item_(rb|lt|ch|pl)_(glove|head|pants|shoes|torso)_")
POTION_RE = re.compile(r"potion_(hp|mp)0(\d)_(\d)")


# --- our data ----------------------------------------------------------------------------------------

EQUIP_NAMES = {"head": 1, "neck": 2, "chest": 3, "belt": 4, "legs": 5, "feet": 6, "hands": 7, "ring": 8,
               "weapon": 9, "shield": 10, "offhand": 10, "ranged": 11}
WEAPON_NAMES = {"axe": 1, "bow": 2, "mace": 3, "sword": 4, "staff": 5, "dagger": 6, "wand": 7}
WEAPON_TYPE_MODEL = {1: "hand_axe", 2: "shortbow", 3: "mace", 4: "longsword", 5: "staff", 6: "dagger", 7: "wand"}
EQUIP_SLOT = {1: "head", 3: "torso", 5: "pants", 6: "shoes", 7: "glove"}
SLOT_WORDS = [
    ("torso", r"shirt|chest|cuirass|vest|robe|tunic|jerkin|hauberk|coat|brigandine|mail\b"),
    ("pants", r"pants|greaves|skirt|leggings|trousers|breeches|legguards"),
    ("shoes", r"sandals|boots|shoes|sabatons|treads"),
    ("glove", r"gloves|gauntlets|sleeves|wraps|bracers|mitts"),
    ("head", r"hood|coif|helm|\bcap\b|\bhat\b|cowl|circlet"),
]
FAMILY_WORDS = [("rb", r"cloth|mage|robe|linen|silk"), ("lt", r"leather|hide"), ("ch", r"chain|mail|ring"),
                ("pl", r"plate|iron|steel")]


def _enum(v, names: dict) -> int:
    v = str(v or "").strip().lower()
    if v.lstrip("-").isdigit():
        return int(v)
    return names.get(v, 0)


def armour_family(armor_type, text: str) -> str:
    """rb / lt / ch / pl from `armor_type` (1 and 12-15 cloth, 2-4 leather, 5-8 chain, 9-11 plate)."""
    t = str(armor_type or "").strip().lower()
    if t.isdigit() and int(t) > 0:
        n = int(t)
        return "rb" if n <= 1 or n >= 12 else "lt" if n <= 4 else "ch" if n <= 8 else "pl"
    for fam, pat in FAMILY_WORDS:
        if re.search(pat, f"{t} {text}"):
            return fam
    return "rb"


def by_data(info: dict, q: int, text: str):
    """(prims, rot, fit) from our item data, or None."""
    equip = _enum(info.get("equip_type"), EQUIP_NAMES)
    wtype = _enum(info.get("weapon_type"), WEAPON_NAMES)
    model = (info.get("model") or "").lower()
    if not equip and re.search(r"draught|potion|tonic|elixir|philter|brew|vial|flask", text):
        liquid = "sapphire" if re.search(r"mana|lamp|tonic|blue|clear", text) else "ruby"
        return flask(liquid, 3, tall="tonic" in text)
    if model:
        for m in sorted(WEAPON_MODELS, key=len, reverse=True):
            if m in model:
                return WEAPON_MODELS[m](q)
    if equip in (9, 11) and wtype in WEAPON_TYPE_MODEL:
        return WEAPON_MODELS[WEAPON_TYPE_MODEL[wtype]](q)
    if equip == 10:
        return shield(q, "buckler" if "buckler" in text else "kite")
    if equip == 2:
        return necklace(q, "choker" if "choker" in text else "pendant")
    if equip == 4:
        return belt(q, sash="sash" in text)
    if equip == 8:
        return ring(q)
    slot = next((s for s, pat in SLOT_WORDS if re.search(pat, f"{model} {text}")), None) or EQUIP_SLOT.get(equip)
    if slot and (equip in EQUIP_SLOT or not equip):
        return ARMOUR[slot](armour_family(info.get("armor_type"), f"{model} {text}"), q)
    if equip == 11:
        return bow(q, "shortbow")
    if equip == 9:
        return sword(q)
    if equip:
        return None
    seed = sum(map(ord, text)) % 50
    if re.search(r"fang|tooth|claw|talon|mandible|pincer|horn", text):
        return curved_spike("bone", 1.6, 0.22, 0.6), view(diag=20, tilt=5), 0.82
    if re.search(r"skull", text):
        return skull()
    if re.search(r"bone|rib", text):
        return bone(1 + seed % 3)
    if re.search(r"pelt|hide|fur|mane", text):
        return hide("fur", seed=seed % 5)
    if re.search(r"rag|cloth|scrap|wrap|tatter", text):
        return hide("leather", seed=seed % 5)
    if re.search(r"carapace|shell|chitin|plate of", text):
        return carapace()
    if re.search(r"\bleg\b|limb", text):
        return insect_leg()
    if re.search(r"scale", text):
        return scale_item()
    if re.search(r"\beye\b|brain|gland|heart|sac\b|ichor", text):
        return brain()
    if re.search(r"feather|plume", text):
        return feather("linen")
    if re.search(r"ring|band|trinket|charm|locket", text):
        return broken_ring()
    if re.search(r"lamp|lantern|wick", text):
        return lamp()
    if re.search(r"key\b", text):
        return key("copper")
    if re.search(r"letter|note|writ|page", text):
        return letter()
    if re.search(r"coin|copper|silver", text):
        return coins(3, "copper", seed)
    if re.search(r"herb|flower|moss|root|petal|mushroom|cap\b", text):
        return flower()
    if re.search(r"meat|bread|ration|jerky|haunch", text):
        return food("ration")
    if re.search(r"ember|cinder|ash|coal|soot", text):
        return mineral("stone", "topaz", seed)
    if re.search(r"stone|rock|shard|pebble|ore|slag", text):
        return mineral("stone", None, seed)
    if re.search(r"nail|buckle|rivet|chain|hook|blade|iron|scrap", text):
        return junk_metal(seed)
    return None


def item_icon(stem: str, info: dict):
    """(prims, rotation, fit) for an item icon; `info` = {quality, model, name, equip_type}."""
    q = info.get("quality") or 2
    model = (info.get("model") or "").lower()
    name = (info.get("name") or "").lower()
    s = stem.lower()
    text = f"{s} {name}"
    if model in WEAPON_MODELS:
        return WEAPON_MODELS[model](q)
    m = ARMOUR_RE.search(s)
    if m:
        return ARMOUR[m.group(2)](m.group(1), q)
    m = POTION_RE.search(s)
    if m:
        liquid = "ruby" if m.group(1) == "hp" else "sapphire"
        return flask(liquid, int(m.group(3)), tall=m.group(2) == "3")
    r = by_data(info, q, f"{s.replace('_', ' ')} {name}")
    if r:
        return r
    if "flask" in s or "elixir" in name:
        return flask("topaz", 4, square=True)
    if "blood" in s:
        return flask("blood", 2, tall=True)
    if "drink" in s:
        return mug()
    if "_ring_" in s:
        return ring(q)
    if "necklace" in s:
        return necklace(q, "choker" if "choker" in name else "pendant")
    if "belt" in s:
        return belt(q, sash="sash" in name)
    if "crystalball" in s:
        col = {"b": "sapphire", "c": "ruby", "e": "emerald"}[re.search(r"crystalball01([bce])", s).group(1)]
        tier = {"": 0, "_r": 1, "_l": 2, "_u": 3, "_up": 4, "_gal": 5}[re.search(r"crystalball01[bce](_\w+)?$", s).group(1) or ""]
        return orb(col, tier)
    if "draconic" in s:
        return gem_icon(gem_of(text), "draconic")
    if "flawless" in s:
        return gem_icon(gem_of(text), "flawless")
    if "polish" in s:
        return gem_icon(gem_of(text), "refined")
    if "enchantstone" in s:
        return gem_icon(gem_of(text), "rough")
    if "crystal02" in s:
        return gem_icon(gem_of(text), "crystal")
    if "time_crystal" in s:
        return time_crystal()
    if "skillbook" in s:
        return tome({"01": "cloth_b", "02": "cloth_g", "r01": "redmark", "r02": "mage"}.get(s.split("_")[-1], "redmark"), "gold", gem=GEM.get(q) or "sapphire")
    if "scroll12" in s:
        return tome("chitin", "copper", emblem="skull")
    if "scroll02" in s:
        return rolled_scroll("sapphire")
    if "scroll_" in s:
        seal = {"mag": "sapphire", "phy": "amethyst", "atk": "ruby", "casting": "topaz", "run": "emerald"}
        k = next((v for kk, v in seal.items() if f"_{kk}_" in s), "redmark")
        return scroll(k, k)
    if "letter" in s:
        return letter()
    if "dish01" in s:
        return food("pie")
    if "dish14" in s:
        return food("turkey")
    if "dish15" in s:
        return food("pork")
    if "dish21" in s:
        return food("sandwich")
    if "food" in s or "ration" in name:
        return food("ration")
    if "skull" in s:
        return skull()
    if "bone0" in s:
        return bone(int(s[-1]) if s[-1].isdigit() else 1)
    if "magicsword09" in s:
        return sword(1, 1.3, 0.12, 0.3, gem=False, broken=True)
    if any(k in s for k in ("claw", "nipper", "horn03", "thorn", "tooth", "magicsword01")):
        ramp = "bone"
        big = "claw02" in s or "nipper" in s
        return curved_spike(ramp, 1.6, 0.26 if big else 0.22, 0.7 if "claw" in s or "nipper" in s or "horn03" in s else 0.35, broken="chipped" in name), view(diag=20, tilt=5), 0.82
    if "horn01" in s:
        return curved_spike("bone", 1.8, 0.3, 0.9, 9, broken=True), view(diag=30, tilt=10, yaw=10), 0.84
    if "coin01" in s:
        return coins(1, "copper", 1)
    if "gold_big" in s:
        return coins(9, "gold", 2)
    if "gold_mid" in s:
        return coins(5, "gold", 3)
    if "kina" in s:
        return coins(3, "gold", 4)
    if "fossil" in s:
        return fossil()
    if "feather" in s:
        return feather("sapphire" if "azure" in name or "event" in s else "linen", broken="broken" in s)
    if "hair" in s:
        return fur_tuft("fur")
    if "hoof" in s:
        return hoof()
    if "junk_bug" in s or "brain" in name:
        return brain()
    if "humanoid" in s or "jewelry" in name:
        return broken_ring()
    if "lamp" in s:
        return lamp()
    if "leather0" in s:
        return hide({"01": "red", "03": "chitin", "04": "fur"}.get(s[-2:], "leather"), seed=int(s[-1]), spots=s.endswith("04"))
    if "leg02" in s:
        return animal_leg()
    if "leg04" in s or "leg05" in s:
        return insect_leg(arm=s.endswith("05"))
    if "leg06" in s:
        return carapace()
    if "scale" in s:
        return scale_item()
    if any(k in s for k in ("meteor", "metal01", "junk_soul", "magicstone", "slate")):
        seed = sum(map(ord, s)) % 50
        return junk_metal(seed, 1 if "chunk" in name or "piece" in name else 3, "metal" if "slate" not in s else "stone")
    if "elementalstone" in s:
        return extractor()
    if "stone0" in s:
        seed = int(s[-1])
        gem = {"1": "topaz", "3": None, "4": "teal", "6": "sapphire"}.get(s[-1])
        return mineral("stone", gem, seed)
    if "demiplane_key" in s:
        return key("plate", "amethyst")
    if "key0" in s:
        return key("copper" if s.endswith("2") else "stone")
    if "arcane_essence" in s:
        return essence()
    if "eyepatch" in s:
        return eyepatch()
    if "loveflower" in s or "flower" in name:
        return flower()
    if "arrow02" in s or "spear" in name:
        return spear(q)
    if "treasure" in s:
        return chest()
    if "elementalstone" in s:
        return extractor()
    if "od03" in s or "shatter" in name:
        return shatter()
    if "magicdust" in s:
        return pouch("mage", "topaz")
    if "reward" in s:
        return gift()
    return generic_sack()
