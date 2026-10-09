"""Spell icons: symbolic motifs painted in the palette of the spell's school.

`spell_icon(stem, info, n)` -> RGBA (n x n). Two decisions per spell:

School (palette): a strong element word in the name / icon name wins (poison, fire, frost ...),
otherwise the spell's `school` (1 physical, 2 frost, 3 fire, 4 shadow, 5/6 holy), otherwise
weaker words in name + description (mana -> arcane, curse -> shadow, bless -> holy ...).

Motif: `RULES` is an ordered list of (regex, recipe); the in-game name is tried first, then the icon
file name, then the description, so a spell keeps a fitting icon after a rename. Recipes compose 2D motifs (flames,
shards, bolts, bursts, skulls, hands, wings, runes, waves ...) and small voxel props shared with
the item icons (swords, daggers, shields, bows, arrows, hammers, boots, chains, books, horns).
Unmatched spells fall back on their effect types: heal -> hands with light, damage -> a bolt,
positive aura -> rune ring with an up chevron, negative aura -> a down chevron, else a burst.
"""

from __future__ import annotations

import math
import re

import numpy as np

import icon_items as it
from iconlib import (
    Canvas, arc, box, cap, circle, cone, cyl, ellipse, ext, poly, polyline, rect, seg, sphere, star, t_stripes, t_wood,
    taper, torus, union, view,
)
from vox import rot_x, rot_y, rot_z

FRAME = ((198, 160, 96), (72, 50, 24))  # bronze bevel shared by all spell icons

PALETTES = {
    "fire": ("fire", "fire", "flame"),
    "frost": ("ice", "ice", "snow"),
    "holy": ("holy", "holy", "light"),
    "shadow": ("shadow", "shadow", "amethyst"),
    "nature": ("poison", "poison", "leaf"),
    "physical": ("phys", "metal", "blood"),
    "arcane": ("arcane", "arcane", "snow"),
}


class Pal:
    def __init__(self, school):
        self.school = school
        self.bg, self.main, self.core = PALETTES[school]


SCHOOL_IDS = {1: "physical", 2: "frost", 3: "fire", 4: "shadow", 5: "holy", 6: "holy"}
STRONG = [
    (r"poison|venom|blight|plague|infect|toxic|entangl|tranquility|viper", "nature"),
    (r"fire|flame|burn|ignite|scorch|inferno|hellpain|warmth|explosive", "fire"),
    (r"frost|\bice\b|icebolt|freeze|frozen", "frost"),
]
WEAK = [
    (r"\bveil\b|shadow|dark|curse|fear|madness|satan|grave|harrow|bind spirit|disintegrate|nether|forcecage|mind blast|penance|cursed|brainstorm", "shadow"),
    (r"mana|magic|arcane|teleport|illusion|clairvoy|wisdom|intellect", "arcane"),
    (r"holy|divine|light|bless|heal|radian|prayer|smite|salvation|resurrect|reincarn|redemption|renew|rejuven|devotion|aegis|cleanse", "holy"),
]


def school_of(stem, info):
    name = f"{info.get('name', '')} {stem}".lower()
    desc = (info.get("description") or "").lower()
    for pat, s in STRONG:
        if re.search(pat, name):
            return s
    s = SCHOOL_IDS.get(info.get("school"))
    if s and s != "physical":
        return s
    for text in (name, desc):
        for pat, sch in WEAK:
            if re.search(pat, text):
                return sch
    return "physical"


# --- 2D motifs ---------------------------------------------------------------------------------------


def glowy(cv, sh, ramp, lo=0.45, hi=1.05, bevel=0.22, halo=0.3, radius=0.4, outline=True):
    if halo:
        cv.glow(sh, halo, radius)
    return cv.paint(sh, ramp, "glow", bevel=bevel, lo=lo, hi=hi, outline=outline)


def tongue(x, y0, h, w, lean=0.0, phase=0.0):
    sh = taper(x, y0 + w, x + lean, y0 + h, w, 0.012)
    return sh.warp(lambda X, Y: (X - 0.07 * np.sin((Y - y0) * 8 + phase) * np.clip((Y - y0) / h, 0, 1), Y))


def flames(cv, P, cx=0.0, y0=-0.72, h=1.35, w=0.34, n=3, ramp="fire", core="flame", halo=0.35):
    outer, inner = [], []
    for i in range(n):
        k = (i - (n - 1) / 2) / max((n - 1) / 2, 1)
        hh = h * (1 - 0.35 * abs(k))
        x = cx + k * w * 1.1
        outer.append(tongue(x, y0, hh, w * (1 - 0.25 * abs(k)), lean=k * 0.18, phase=i * 1.7))
        inner.append(tongue(x, y0 + 0.02, hh * 0.58, w * 0.52 * (1 - 0.25 * abs(k)), lean=k * 0.1, phase=i * 1.7 + 0.5))
    o = union(*outer)
    glowy(cv, o, ramp, lo=0.5, hi=1.0, bevel=w * 0.9, halo=halo)
    cv.paint(union(*inner), core, "glow", bevel=w * 0.5, lo=0.35, hi=1.0, outline=False)
    return o


def fireball(cv, P, ramp="fire", core="flame", cx=-0.22, cy=-0.22, r=0.33, ang=math.radians(45)):
    dx, dy = math.cos(ang), math.sin(ang)
    tails = []
    for i, (off, ln, rr) in enumerate(((0, 1.05, r), (0.17, 0.75, r * 0.6), (-0.17, 0.8, r * 0.6))):
        px, py = -dy * off, dx * off
        tails.append(taper(cx + px, cy + py, cx + px + dx * ln, cy + py + dy * ln, rr, 0.02).warp(
            lambda X, Y, i=i: (X + 0.04 * np.sin((X + Y) * 9 + i), Y - 0.04 * np.sin((X - Y) * 9 + i))))
    t = union(*tails)
    glowy(cv, t, ramp, lo=0.45, hi=0.95, bevel=0.25, halo=0.3)
    ball = circle(cx, cy, r)
    cv.paint(ball, core, "glow", bevel=r, lo=0.4, hi=1.05, outline=True)
    cv.paint(circle(cx - r * 0.25, cy + r * 0.25, r * 0.35), core, "flat", lo=1.0, outline=False)


def bolt(cv, P, ramp, core, cx=-0.25, cy=-0.25, r=0.26, ang=math.radians(45), sparks=True):
    """Magic missile: glowing orb with streaks trailing up-right."""
    dx, dy = math.cos(ang), math.sin(ang)
    streaks = union(*[taper(cx - dy * o, cy + dx * o, cx - dy * o + dx * ln, cy + dx * o + dy * ln, rr, 0.01)
                      for o, ln, rr in ((0, 1.1, r * 0.95), (0.2, 0.8, 0.07), (-0.2, 0.85, 0.07), (0.33, 0.5, 0.04), (-0.33, 0.55, 0.04))])
    glowy(cv, streaks, ramp, lo=0.4, hi=0.95, bevel=0.2, halo=0.25)
    glowy(cv, circle(cx, cy, r), core, lo=0.45, hi=1.05, bevel=r, halo=0.4, radius=0.3)
    cv.paint(circle(cx - r * 0.3, cy + r * 0.3, r * 0.3), core, "flat", lo=1.05, outline=False)
    if sparks:
        for sx, sy, s in ((-0.62, 0.15, 0.07), (0.15, -0.6, 0.06), (-0.5, -0.62, 0.05)):
            cv.paint(star(sx, sy, s, s * 0.35, 4), core, "flat", lo=1.0, outline=False)


def burst(cv, P, ramp, core, cx=0.0, cy=0.0, ro=0.78, ri=0.32, n=12, rot=0.0, core_r=0.25, halo=0.35):
    rays = star(cx, cy, ro, ri, n, a0=rot) | star(cx, cy, ro * 0.7, ri * 0.9, n, a0=rot + math.pi / n)
    glowy(cv, rays, ramp, lo=0.45, hi=1.0, bevel=0.3, halo=halo)
    if core_r:
        cv.paint(circle(cx, cy, core_r), core, "glow", bevel=core_r, lo=0.6, hi=1.05, outline=False)
    return rays


def shard(cx, cy, length, w, ang):
    pts = [(0, length), (w, length * 0.25), (0, -length * 0.15), (-w, length * 0.25)]
    return poly(pts).rot(-ang).move(cx, cy)


def ice_shards(cv, P, cx=0.0, cy=-0.55, spec=((0, 1.25, 0.2), (-0.5, 0.85, 0.15), (0.5, 0.9, 0.16), (-0.25, 0.7, 0.12), (0.28, 0.65, 0.12))):
    for a, ln, w in sorted(spec, key=lambda s: -abs(s[0])):
        sh = shard(cx + a * 0.25, cy, ln, w, a)
        cv.glow(sh, 0.15, 0.25)
        cv.paint(sh, "ice", "lit", bevel=w * 0.8, base=0.15)
        cv.paint(shard(cx + a * 0.25 - w * 0.25 * math.cos(a), cy, ln * 0.85, w * 0.35, a), "snow", "flat", lo=0.85, outline=False)


def nova(cv, P):
    ground = ellipse(0, -0.45, 0.85, 0.3)
    cv.paint(ground, "ice", "glow", bevel=0.3, lo=0.3, hi=0.75, outline=False)
    burst(cv, P, "snow", "snow", 0, -0.1, 0.75, 0.2, 8, core_r=0.0, halo=0.4)
    for i in range(7):
        a = -1.2 + 2.4 * i / 6
        sh = shard(math.sin(a) * 0.55, -0.45 + math.cos(a) * 0.08, 0.55 - 0.1 * abs(a), 0.11, a * 0.8)
        cv.paint(sh, "ice", "lit", bevel=0.08, base=0.15)
    cv.paint(circle(0, -0.1, 0.16), "snow", "glow", bevel=0.16, lo=0.7, hi=1.05, outline=False)


def cross(cv, ramp="light", cx=0.0, cy=0.0, s=1.0, halo=0.45):
    sh = (rect(cx, cy, 0.13 * s, 0.6 * s, rr=0.04) | rect(cx, cy + 0.15 * s, 0.42 * s, 0.13 * s, rr=0.04))
    cv.glow(sh, halo, 0.5)
    cv.paint(sh, ramp, "lit", bevel=0.1 * s, base=0.1)
    return sh


def heart(cv, ramp="ruby", cx=0.0, cy=0.0, s=1.0):
    sh = (circle(-0.22, 0.12, 0.27) | circle(0.22, 0.12, 0.27) | poly([(-0.47, 0.05), (0.47, 0.05), (0, -0.55)])).scale(s).move(cx, cy)
    cv.glow(sh, 0.3, 0.35)
    cv.paint(sh, ramp, "lit", bevel=0.18 * s, base=0.1)
    cv.paint(circle(cx - 0.22 * s, cy + 0.2 * s, 0.07 * s), ramp, "flat", lo=1.05, outline=False)
    return sh


def hand_shape(cx=0.0, cy=0.0, s=1.0, a=0.0, mirror=False):
    fingers = [seg(x, 0.1, x, 0.1 + ln, 0.062) for x, ln in ((-0.22, 0.34), (-0.075, 0.44), (0.075, 0.42), (0.22, 0.32))]
    thumb = seg(-0.3, -0.14, -0.5, 0.14, 0.07)
    palm = rect(0, -0.12, 0.29, 0.28, rr=0.12)
    wrist = rect(0, -0.5, 0.2, 0.16, rr=0.05)
    sh = union(palm, thumb, wrist, *fingers)
    if mirror:
        sh = sh.warp(lambda X, Y: (-X, Y))
    return sh.rot(a).scale(s).move(cx, cy)


def hands_light(cv, P, two=True):
    burst(cv, P, P.main, P.core, 0, 0.35, 0.7, 0.25, 10, core_r=0.2, halo=0.5)
    if two:
        cv.paint(hand_shape(-0.3, -0.35, 0.8, -0.5, mirror=False), "skin", "lit", bevel=0.1)
        cv.paint(hand_shape(0.3, -0.35, 0.8, 0.5, mirror=True), "skin", "lit", bevel=0.1)
    else:
        cv.paint(hand_shape(0, -0.25, 1.05), "skin", "lit", bevel=0.1)


def praying(cv, P):
    burst(cv, P, P.main, P.core, 0, 0.4, 0.75, 0.25, 12, core_r=0.15, halo=0.45)
    left = (poly([(-0.02, 0.6), (-0.28, 0.0), (-0.3, -0.55), (-0.02, -0.55)]) | circle(-0.12, 0.05, 0.16))
    right = left.warp(lambda X, Y: (-X, Y))
    cv.paint(left, "skin", "lit", bevel=0.1)
    cv.paint(right, "skin", "lit", bevel=0.1, base=-0.1)
    cv.paint(rect(0, -0.62, 0.32, 0.1, rr=0.03), P.main, "lit", bevel=0.06)


def wings(cv, ramp="snow", cy=0.0, s=1.0):
    parts = []
    for side in (1, -1):
        for i in range(5):
            a = math.radians(20 + i * 17)
            ln = (0.82 - 0.09 * i) * s
            bx, by = side * 0.08 * s, cy + (0.12 - 0.04 * i) * s
            parts.append(taper(bx, by, bx + side * ln * math.sin(a + 0.35), by + ln * math.cos(a + 0.35) - 0.2 * s, 0.1 * s, 0.035 * s))
    sh = union(*parts)
    cv.glow(sh, 0.3, 0.4)
    cv.paint(sh, ramp, "lit", bevel=0.07 * s)
    return sh


def crown(cv, cy=0.0, s=1.0):
    pts = [(-0.6, -0.35), (0.6, -0.35), (0.6, 0.38), (0.32, 0.05), (0.0, 0.5), (-0.32, 0.05), (-0.6, 0.38)]
    sh = poly(pts).scale(s).move(0, cy)
    cv.glow(sh, 0.4, 0.45)
    cv.paint(sh, "gold", "lit", bevel=0.12 * s, base=0.05)
    cv.paint(rect(0, cy - 0.25 * s, 0.6 * s, 0.1 * s), "gold", "lit", bevel=0.06 * s, base=-0.1)
    for x, y in ((-0.6, 0.38), (0.0, 0.5), (0.6, 0.38)):
        cv.paint(circle(x * s, cy + y * s, 0.08 * s), "gold", "lit", bevel=0.06)
    for x, g in ((-0.32, "sapphire"), (0.0, "ruby"), (0.32, "emerald")):
        cv.paint(circle(x * s, cy - 0.05 * s, 0.09 * s), g, "lit", bevel=0.07, base=0.15)


def eye(cv, P, cx=0.0, cy=0.0, s=1.0, iris=None, rays=True):
    alm = (circle(0, -0.42, 0.62) & circle(0, 0.42, 0.62)).scale(s).move(cx, cy)
    if rays:
        burst(cv, P, P.main, P.core, cx, cy, 0.85 * s, 0.4 * s, 12, core_r=0, halo=0.4)
    cv.paint(alm, "bone", "lit", bevel=0.08 * s, base=0.1)
    cv.paint(circle(cx, cy, 0.17 * s), iris or P.core, "glow", bevel=0.17 * s, lo=0.4, hi=1.0)
    cv.paint(circle(cx, cy, 0.07 * s), "void", "flat", lo=0.2, outline=False)
    cv.paint(circle(cx - 0.05 * s, cy + 0.05 * s, 0.03 * s), "snow", "flat", lo=1.05, outline=False)


def skull(cv, cx=0.0, cy=0.0, s=1.0, ramp="bone", eyes=None):
    sk = union(circle(0, 0.12, 0.42), rect(0, -0.08, 0.33, 0.12, rr=0.1), rect(0, -0.28, 0.24, 0.17, rr=0.07)).scale(s).move(cx, cy)
    cv.paint(sk, ramp, "lit", bevel=0.12 * s)
    holes = (circle(-0.16, 0.0, 0.11) | circle(0.16, 0.0, 0.11) | poly([(0, -0.07), (-0.05, -0.17), (0.05, -0.17)])).scale(s).move(cx, cy)
    cv.paint(holes, "void", "flat", lo=0.3, outline=False)
    teeth = union(*[rect(x, -0.33, 0.012, 0.08) for x in (-0.12, -0.04, 0.04, 0.12)]).scale(s).move(cx, cy)
    cv.paint(teeth, "dark", "flat", lo=0.6, outline=False)
    if eyes:
        cv.paint((circle(-0.16, 0.0, 0.05) | circle(0.16, 0.0, 0.05)).scale(s).move(cx, cy), eyes, "flat", lo=1.05, outline=False)
    return sk


def spiral(cv, ramp, core, cx=0.0, cy=0.0, r0=0.05, r1=0.75, turns=1.6, arms=2, th=0.09):
    parts = []
    for k in range(arms):
        pts = []
        for i in range(40):
            t = i / 39
            a = k * 2 * math.pi / arms + turns * 2 * math.pi * t
            r = r0 + (r1 - r0) * t
            pts.append((cx + r * math.cos(a), cy + r * math.sin(a)))
        for i in range(39):
            t = i / 39
            parts.append(seg(*pts[i], *pts[i + 1], th * (0.35 + 0.65 * t)))
    sh = union(*parts)
    glowy(cv, sh, ramp, lo=0.45, hi=1.0, bevel=th, halo=0.35)
    cv.paint(circle(cx, cy, 0.14), core, "glow", bevel=0.14, lo=0.6, hi=1.05, outline=False)
    return sh


def waves(cv, ramp, cx=-0.3, cy=0.0, rs=(0.35, 0.6, 0.85), span=0.75, th=0.055, a=0.0):
    sh = union(*[arc(cx, cy, r, a - span, a + span, th) for r in rs])
    glowy(cv, sh, ramp, lo=0.55, hi=1.0, bevel=th, halo=0.2)
    return sh


def zs(cv, ramp="snow", x=0.15, y=0.25, s=1.0):
    for i, k in enumerate((1.0, 0.75, 0.55)):
        w, h = 0.17 * k * s, 0.15 * k * s
        cx, cy = x + 0.28 * i * s, y + 0.25 * i * s
        sh = polyline([(cx - w, cy + h), (cx + w, cy + h), (cx - w, cy - h), (cx + w, cy - h)], 0.045 * k * s)
        cv.paint(sh, ramp, "lit", bevel=0.04, base=0.2)


def moon(cv, ramp="light", cx=-0.15, cy=0.05, r=0.48):
    sh = circle(cx, cy, r) - circle(cx + r * 0.45, cy + r * 0.3, r * 0.85)
    cv.glow(sh, 0.3, 0.4)
    cv.paint(sh, ramp, "lit", bevel=0.15, base=0.05)


def stars(cv, pts, ramp="topaz", s=0.13):
    for x, y in pts:
        cv.paint(star(x, y, s, s * 0.45, 5), ramp, "lit", bevel=0.05, base=0.2)


def drop(cv, ramp, cx, cy, r, halo=0.2):
    sh = taper(cx, cy, cx, cy + 1.9 * r, r, 0.01)
    if halo:
        cv.glow(sh, halo, 0.3)
    cv.paint(sh, ramp, "lit", bevel=r * 0.7, base=0.1)
    cv.paint(circle(cx - r * 0.35, cy + r * 0.2, r * 0.22), "snow", "flat", lo=1.05, outline=False)
    return sh


def figure(cx=0.0, cy=0.0, s=1.0):
    return union(circle(0, 0.45, 0.16), taper(0, 0.22, 0, -0.55, 0.17, 0.27), seg(-0.18, 0.18, -0.38, -0.2, 0.07), seg(0.18, 0.18, 0.38, -0.2, 0.07)).scale(s).move(cx, cy)


def hood(cv, P, eyes="emerald"):
    sh = circle(0, 0.15, 0.5) | rect(0, -0.4, 0.6, 0.45, rr=0.2) | poly([(-0.2, 0.55), (0.0, 0.85), (0.2, 0.55)])
    cv.paint(sh, "cloth", "lit", bevel=0.18)
    cv.paint(ellipse(0, 0.02, 0.27, 0.33), "void", "flat", lo=0.25, outline=False)
    cv.glow(ellipse(0, 0.05, 0.3, 0.12), 0.1, 0.2)
    for x in (-0.11, 0.11):
        cv.paint(ellipse(x, 0.05, 0.07, 0.035), eyes, "flat", lo=1.05, outline=False)


def rune_ring(cv, P, ramp=None, core=None, r=0.62, cx=0.0, cy=0.0):
    ramp, core = ramp or P.main, core or P.core
    ring = circle(cx, cy, r).shell(0.07)
    glowy(cv, ring, ramp, lo=0.5, hi=0.95, bevel=0.07, halo=0.3)
    marks = []
    for i in range(8):
        a = i * math.pi / 4 + 0.2
        x, y = cx + r * math.cos(a), cy + r * math.sin(a)
        marks.append(rect(x, y, 0.025, 0.05, a=a) | rect(x, y + 0.02, 0.05, 0.015, a=a))
    cv.paint(union(*marks), core, "flat", lo=1.0, outline=False)
    return ring


def chevron(cv, up=True, ramp="emerald", cx=0.0, cy=0.0, s=1.0, n=2):
    for i in range(n):
        y = cy + (i * 0.22 - 0.1) * s * (1 if up else -1)
        pts = [(-0.3, -0.08), (0, 0.18), (0.3, -0.08), (0.3, -0.24), (0, 0.02), (-0.3, -0.24)]
        if not up:
            pts = [(x, -yy) for x, yy in pts]
        cv.paint(poly(pts).scale(s).move(cx, y), ramp, "lit", bevel=0.06 * s, base=0.15)


def lightning(cv, ramp="light", pts=((0.3, 0.9), (-0.12, 0.25), (0.14, 0.18), (-0.25, -0.55)), th=0.08):
    sh = union(*[taper(*pts[i], *pts[i + 1], th * (1 - 0.25 * i), th * (1 - 0.25 * (i + 1)) + 0.01) for i in range(len(pts) - 1)])
    glowy(cv, sh, ramp, lo=0.65, hi=1.05, bevel=th, halo=0.45, radius=0.35)
    return sh


def dome(cv, P):
    cv.paint(ellipse(0, -0.55, 0.8, 0.2), P.main, "glow", bevel=0.2, lo=0.35, hi=0.7, outline=False)
    shell = (circle(0, -0.5, 0.75).shell(0.05)) & rect(0, 0.2, 1, 0.7)
    cv.glow(circle(0, -0.5, 0.75), 0.18, 0.1)
    glowy(cv, shell, P.core, lo=0.6, hi=1.05, bevel=0.05, halo=0.3)
    cv.paint(arc(0, -0.5, 0.6, math.radians(110), math.radians(150), 0.035), "snow", "flat", lo=1.05, outline=False)


def slashes(cv, ramp, n=3, ang=-0.7, gap=0.28, length=1.0):
    parts = []
    for i in range(n):
        o = (i - (n - 1) / 2) * gap
        c = circle(0, 0, 0.75 * length) - circle(0.12, -0.12, 0.75 * length)
        parts.append(c.rot(ang).move(o * math.cos(ang + math.pi / 2) + 0.05, o * math.sin(ang + math.pi / 2)))
    sh = union(*parts)
    glowy(cv, sh, ramp, lo=0.55, hi=1.05, bevel=0.06, halo=0.3)
    return sh


def fangs(cv, ramp="bone", y=0.2):
    gum = ellipse(0, y + 0.42, 0.7, 0.22)
    cv.paint(gum, "flesh", "lit", bevel=0.1)
    for x in (-0.3, 0.3):
        f = taper(x, y + 0.35, x * 0.8, y - 0.45, 0.14, 0.012).warp(lambda X, Y, x=x: (X - 0.08 * math.copysign(1, x) * (Y - y - 0.35) ** 2, Y))
        cv.paint(f, ramp, "lit", bevel=0.08, base=0.05)
    for x in (-0.1, 0.1):
        cv.paint(taper(x, y + 0.3, x, y + 0.05, 0.06, 0.02), ramp, "lit", bevel=0.04)


def leaf(cv, ramp="leaf", cx=0.0, cy=0.0, s=1.0, a=0.6):
    sh = (circle(-0.26, 0, 0.5) & circle(0.26, 0, 0.5)).rot(a).scale(s).move(cx, cy)
    cv.paint(sh, ramp, "lit", bevel=0.1 * s, base=0.05)
    cv.paint(seg(-0.38, 0, 0.38, 0, 0.018).rot(a + math.pi / 2).scale(s).move(cx, cy), ramp, "flat", lo=0.2, outline=False)


def ankh(cv, ramp="gold", cy=0.0, s=1.0):
    sh = union(ellipse(0, 0.4, 0.2, 0.26).shell(0.07), rect(0, 0.06, 0.42, 0.07, rr=0.02), poly([(-0.08, 0.06), (0.08, 0.06), (0.15, -0.75), (-0.15, -0.75)])).scale(s).move(0, cy)
    cv.glow(sh, 0.45, 0.45)
    cv.paint(sh, ramp, "lit", bevel=0.08 * s, base=0.1)


def speedlines(cv, ramp, y0=-0.6, y1=0.6, x0=-0.85, n=6, ln=0.9, seed=0):
    rng = np.random.default_rng(seed)
    parts = []
    for i in range(n):
        y = y0 + (y1 - y0) * i / max(n - 1, 1)
        l = ln * rng.uniform(0.5, 1.0)
        x = x0 + rng.uniform(0, 0.25)
        parts.append(taper(x, y, x + l, y, 0.01, 0.035))
    cv.paint(union(*parts), ramp, "flat", lo=0.95, outline=False)


def target(cv, ramp="blood", cx=0.0, cy=0.0, s=1.0):
    sh = union(circle(cx, cy, 0.6 * s).shell(0.05 * s), circle(cx, cy, 0.32 * s).shell(0.045 * s), circle(cx, cy, 0.08 * s),
               rect(cx, cy + 0.62 * s, 0.035 * s, 0.2 * s), rect(cx, cy - 0.62 * s, 0.035 * s, 0.2 * s),
               rect(cx + 0.62 * s, cy, 0.2 * s, 0.035 * s), rect(cx - 0.62 * s, cy, 0.2 * s, 0.035 * s))
    cv.glow(sh, 0.2, 0.3)
    cv.paint(sh, ramp, "lit", bevel=0.04, base=0.2)


def vines(cv, ramp="leaf"):
    for k, (x0, ph) in enumerate(((-0.55, 0.0), (0.45, 2.0), (-0.05, 4.0))):
        pts = [(x0 + 0.18 * math.sin(t * 5 + ph), -0.85 + t * 1.4) for t in np.linspace(0, 1, 14)]
        cv.paint(polyline(pts, 0.05), ramp, "lit", bevel=0.04)
        for j in (4, 8, 11):
            x, y = pts[j]
            leaf(cv, ramp, x + 0.08, y, 0.25, a=0.8 if j % 2 else -0.8)


def ghost(cv, ramp="snow"):
    body = circle(0, 0.2, 0.38) | rect(0, -0.2, 0.38, 0.4)
    body = body - union(*[circle(x, -0.62, 0.11) for x in (-0.26, 0.0, 0.26)])
    body = body.warp(lambda X, Y: (X - 0.08 * np.sin(Y * 4), Y))
    glowy(cv, body, ramp, lo=0.45, hi=0.95, bevel=0.3, halo=0.35)
    cv.paint(ellipse(-0.13, 0.25, 0.07, 0.1) | ellipse(0.13, 0.25, 0.07, 0.1) | ellipse(0.0, 0.0, 0.08, 0.1), "void", "flat", lo=0.3, outline=False)


def cloud(cv, ramp, cy=0.25):
    sh = union(circle(-0.4, cy, 0.3), circle(0, cy + 0.15, 0.38), circle(0.4, cy, 0.3), rect(0, cy - 0.15, 0.6, 0.18, rr=0.15))
    cv.paint(sh, ramp, "lit", bevel=0.2)
    return sh


def sun(cv, P, cx=0.0, cy=0.0, s=1.0):
    burst(cv, P, P.main, P.core, cx, cy, 0.85 * s, 0.38 * s, 12, core_r=0, halo=0.5)
    cv.paint(circle(cx, cy, 0.34 * s), P.core, "glow", bevel=0.34 * s, lo=0.65, hi=1.05)


def lockpick(cv):
    plate = rect(0, 0, 0.55, 0.65, rr=0.1)
    cv.paint(plate, "copper", "lit", bevel=0.12)
    cv.paint(circle(0, 0.12, 0.14) | poly([(-0.08, 0.05), (0.08, 0.05), (0.14, -0.35), (-0.14, -0.35)]), "void", "flat", lo=0.2, outline=False)
    for x, y in ((-0.42, 0.52), (0.42, 0.52), (-0.42, -0.52), (0.42, -0.52)):
        cv.paint(circle(x, y, 0.05), "gold", "lit", bevel=0.04)
    cv.paint(polyline([(0.85, -0.8), (0.05, 0.0), (0.0, 0.12)], 0.035), "plate", "lit", bevel=0.03, base=0.2)


# --- voxel props -----------------------------------------------------------------------------------------


def arrows(n=1, spread=0.0, parallel=0.0, head="metal", fletch="redmark", length=1.6):
    prims = []
    for i in range(n):
        k = i - (n - 1) / 2
        R = rot_y(k * spread)
        o = (k * parallel, 0, -k * parallel * 0.3)
        p = [cap((0, 0, -length / 2), (0, 0, length / 2 - 0.15), 0.03, "wood", tone=t_wood),
             ext(poly([(0, length / 2 + 0.18), (0.12, length / 2 - 0.12), (0, length / 2 - 0.05), (-0.12, length / 2 - 0.12)]), 0.025, head, rnd=0.01, spec=0.6)]
        for a in (0, math.pi / 2):
            p.append(ext(poly([(0.0, -length / 2 + 0.05), (0.13, -length / 2 + 0.0), (0.13, -length / 2 + 0.28), (0.0, -length / 2 + 0.38)]).mirror_x(), 0.012, fletch).at((0, 0, 0), rot_z(a)))
        prims += [q.at(o, R) for q in p]
    return prims


def prop(cv, built, fit=0.8, off=(0.0, 0.0), rot=None, shadow=True):
    prims, r, _ = built
    return cv.obj(prims, rot if rot is not None else r, fit=fit, off=off, shadow=shadow)


def horn():
    prims = []
    n = 8
    for i in range(n):
        t0, t1 = i / n, (i + 1) / n
        p0 = (-0.7 + 1.3 * t0, 0, 0.4 * (t0 - 0.5) ** 2 * 3 - 0.2)
        p1 = (-0.7 + 1.3 * t1, 0, 0.4 * (t1 - 0.5) ** 2 * 3 - 0.2)
        prims.append(cone(p0, p1, 0.06 + 0.28 * t0 ** 2, 0.06 + 0.28 * t1 ** 2, "bone"))
    prims.append(cyl((0.58, 0, 0.1), (0.62, 0, 0.1), 0.36, "gold", spec=0.4))
    prims.append(torus((0, 0, 0), 0.12, 0.03, "gold", spec=0.4).at((-0.2, 0, -0.12), rot_y(math.pi / 2)))
    return prims, view(tilt=5, yaw=0, diag=-15), 0.7


def chain_links(n=5, ramp="metal", emit=None):
    prims = []
    for i in range(n):
        R = rot_x(math.pi / 2) if i % 2 else rot_y(math.pi / 2) @ rot_x(math.pi / 2)
        prims.append(torus((0, 0, 0), 0.2, 0.06, ramp, spec=0.5, emit=emit).at((0, 0, -0.9 + 0.33 * i), R))
    return prims


def ice_block():
    return [box((0, 0, 0), (0.6, 0.6, 0.7), "ice", spec=0.8, tone=lambda P: 1 + 0.2 * np.sin((P[:, 0] + P[:, 2]) * 9)),
            box((0.0, 0.0, 0.0), (0.62, 0.05, 0.72), "snow", emit=0.85).at((0, 0, 0), rot_z(math.pi / 4) @ rot_y(0.4)).opts(spec=0.5)]


def book_open(page="paper", glow=None):
    prims = []
    for side in (1, -1):
        prims.append(box((side * 0.45, 0, 0), (0.43, 0.6, 0.06), page, tone=t_stripes(60, 0.15, 1)).at((0, 0, 0), rot_y(side * 0.18)))
        prims.append(box((side * 0.47, 0, -0.08), (0.47, 0.64, 0.03), "redmark").at((0, 0, 0), rot_y(side * 0.18)))
    return prims, view(tilt=55, yaw=0), 0.84


# --- recipes ---------------------------------------------------------------------------------------------


def r_sword(glow=False, ramp=None, q=5, diag=45):
    def f(cv, P):
        if glow:
            cv.glow(seg(-0.6, -0.6, 0.6, 0.6, 0.15), 0.45, 0.5)
        prop(cv, it.sword(q, 1.5, 0.13, 0.36), 0.86, rot=view(diag=diag, tilt=12, yaw=-20))
    return f


def r_swords_crossed(cv, P):
    a = it.sword(4, 1.4, 0.12, 0.34)[0]
    b = it.sword(3, 1.4, 0.12, 0.34)[0]
    prims = [p.at((0.0, 0.1, 0), rot_y(math.radians(40))) for p in a] + [p.at((0.0, -0.1, 0), rot_y(math.radians(-40))) for p in b]
    cv.glow(circle(0, 0.1, 0.3), 0.3, 0.4)
    cv.obj(prims, view(tilt=8, yaw=0), fit=0.86)


def r_bow(cv, P):
    prims, rot, _ = it.bow(3, "longbow")
    prims += [p.at((0.1, -0.05, 0), rot_y(math.pi / 2)) for p in arrows(1, length=1.5)]
    cv.obj(prims, rot, fit=0.88)


def r_arrows(n=1, spread=0.0, parallel=0.0, fletch="redmark", head="metal", diag=45, fit=0.86, off=(0, 0)):
    def f(cv, P):
        cv.obj(arrows(n, spread, parallel, head, fletch), view(diag=diag, tilt=5, yaw=10), fit=fit, off=off)
    return f


def combo(*fs):
    def f(cv, P):
        for g in fs:
            g(cv, P)
    return f


def r_shield(kind="kite", q=4, glow=False, emblem_ramp=None):
    def f(cv, P):
        if glow:
            burst(cv, P, P.main, P.core, 0, 0.05, 0.9, 0.45, 14, core_r=0, halo=0.5)
        prop(cv, it.shield(q, kind), 0.78)
    return f


def r_hammer(cv, P):
    cv.glow(circle(0.25, 0.3, 0.3), 0.5, 0.5)
    prop(cv, it.mace(5, "war_hammer"), 0.84)


def r_dagger(cv, P):
    prop(cv, it.dagger(4), 0.8, rot=view(diag=50, tilt=10, yaw=-20))


def r_boot(cv, P):
    prims = it.boots("lt", 3)[0][:3]
    speedlines(cv, P.core if P.school != "physical" else "linen", -0.55, 0.55, -0.9, 6, 0.8)
    cv.obj(prims, view(tilt=10, yaw=-25), fit=0.66, off=(0.18, 0.0))


def r_bolt(ramp=None, core=None):
    def f(cv, P):
        bolt(cv, P, ramp or P.main, core or P.core)
    return f


def r_flames(**kw):
    return lambda cv, P: flames(cv, P, **kw)


def r_burst(cv, P):
    burst(cv, P, P.main, P.core)


def r_fire_wall(cv, P):
    flames(cv, P, -0.42, -0.75, 0.95, 0.22, 2)
    flames(cv, P, 0.42, -0.75, 1.0, 0.22, 2)
    flames(cv, P, 0.0, -0.8, 1.45, 0.3, 3)


def r_fire_ward(cv, P):
    cv.paint(circle(0, 0, 0.72).shell(0.07), "gold", "lit", bevel=0.06)
    flames(cv, P, 0, -0.55, 1.05, 0.26, 3, halo=0.3)


def r_charge(cv, P):
    speedlines(cv, "linen", -0.55, 0.45, -0.95, 6, 0.9)
    cv.obj(it.shield(3, "kite")[0], view(tilt=4, yaw=60), fit=0.66, off=(0.25, 0.0))


def r_stun(cv, P):
    stars(cv, [(-0.45, 0.55), (0.0, 0.7), (0.45, 0.55)], "topaz", 0.14)


def r_heal(cv, P):
    hands_light(cv, P)
    cross(cv, "light", 0, 0.35, 0.55, halo=0.2)


def r_buff(cv, P):
    rune_ring(cv, P)
    chevron(cv, True, "emerald", 0, 0, 1.0)


def r_debuff(cv, P):
    rune_ring(cv, P)
    chevron(cv, False, "blood", 0, 0, 1.0)


def r_sound(ramp=None, skull_=False):
    def f(cv, P):
        waves(cv, ramp or P.core, -0.2, 0.0)
        if skull_:
            skull(cv, -0.35, -0.05, 0.75)
        else:
            cv.obj(horn()[0], horn()[1], fit=0.6, off=(-0.3, -0.1))
    return f


def r_slam(cv, P):
    cv.paint(ellipse(0, -0.55, 0.8, 0.22).shell(0.04) | ellipse(0, -0.55, 0.5, 0.13).shell(0.04), P.core if P.school != "physical" else "topaz", "glow", bevel=0.04, lo=0.7, hi=1.0)
    burst(cv, P, "topaz", "light", 0, -0.5, 0.55, 0.2, 8, core_r=0.0, halo=0.3)
    cv.obj(it.mace(3, "maul")[0], view(diag=-30, tilt=10, yaw=-20), fit=0.66, off=(0.12, 0.25))


def r_wound(ramp="blood", drop_ramp="blood"):
    def f(cv, P):
        slashes(cv, ramp)
        drop(cv, drop_ramp, 0.42, -0.62, 0.12)
    return f


def r_bite(drop_ramp=None):
    def f(cv, P):
        fangs(cv)
        if drop_ramp:
            drop(cv, drop_ramp, 0.0, -0.62, 0.12)
    return f


def r_eye_gouge(cv, P):
    eye(cv, P, 0, 0.0, 1.15, iris="sapphire", rays=False)
    slashes(cv, "blood", 2, -0.9, 0.3, 0.8)


def r_hamstring(cv, P):
    cv.obj(it.boots("lt", 3)[0][:3], view(tilt=10, yaw=-25), fit=0.7)
    slashes(cv, "blood", 2, -0.6, 0.25, 0.8)


def r_sleep(cv, P):
    moon(cv, "light", -0.2, -0.1, 0.45)
    zs(cv, "snow", 0.2, 0.2, 0.9)


def r_cage(cv, P):
    cv.paint(figure(0, -0.1, 0.9), "void", "flat", lo=0.6)
    bars = union(*[rect(x, -0.05, 0.04, 0.75) for x in (-0.5, -0.25, 0.0, 0.25, 0.5)], rect(0, 0.72, 0.62, 0.06), rect(0, -0.78, 0.62, 0.06))
    cv.paint(bars, "metal", "lit", bevel=0.05, base=0.1)


def r_chain(ramp="metal", emit=None, ice=False):
    def f(cv, P):
        if ice:
            ice_shards(cv, P, 0, -0.75, ((0, 0.8, 0.12), (-0.6, 0.6, 0.1), (0.6, 0.6, 0.1)))
        cv.obj(chain_links(6, ramp, emit), view(diag=40, tilt=10, yaw=20), fit=0.88)
    return f


def r_ice_block(cv, P):
    cv.obj(ice_block(), view(tilt=25, yaw=35), fit=0.74)
    cv.paint(figure(0, -0.08, 0.55), "ice", "flat", lo=0.2, outline=False)


def r_portal(cv, P):
    cv.paint(ellipse(0, 0, 0.55, 0.8).shell(0.08), P.main, "lit", bevel=0.07, base=0.15)
    spiral(cv, P.main, P.core, 0, 0, 0.05, 0.5, 1.4, 3, 0.07)


def r_antimagic(cv, P):
    rune_ring(cv, P)
    sh = rect(0, 0, 0.75, 0.08, a=-0.8, rr=0.04)
    cv.paint(sh, "blood", "lit", bevel=0.06, base=0.15)


def r_mind(cv, P):
    burst(cv, P, P.main, P.core, 0.15, 0.25, 0.7, 0.3, 10, core_r=0.12)
    head = union(circle(-0.1, 0.1, 0.38), rect(-0.05, -0.35, 0.22, 0.3, rr=0.1), poly([(0.22, 0.0), (0.38, -0.12), (0.2, -0.18)]))
    cv.paint(head, "void", "lit", bevel=0.1, base=0.1)


def r_plague(cv, P):
    cloud(cv, "poison", 0.35)
    skull(cv, 0, -0.25, 0.85, eyes="leaf")
    for x, y in ((-0.6, -0.5), (0.6, -0.4), (0.5, 0.75)):
        cv.paint(circle(x, y, 0.07), "leaf", "lit", bevel=0.05)


def r_scream(cv, P):
    waves(cv, P.core, 0, 0.0, (0.55, 0.75, 0.92), span=math.pi, th=0.04)
    skull(cv, 0, 0.0, 0.95, eyes="amethyst")


def r_poison_skull(cv, P):
    skull(cv, 0, 0.12, 0.85, ramp="bone", eyes="amethyst")
    drop(cv, "amethyst", 0.48, -0.65, 0.13)
    drop(cv, "poison", -0.48, -0.6, 0.1)


def r_wings(cv, P):
    wings(cv, "snow", 0.0, 1.0)
    cv.paint(circle(0, 0.05, 0.12), P.core, "glow", bevel=0.12, lo=0.7, hi=1.05)


def r_heart(cv, P):
    burst(cv, P, P.main, P.core, 0, 0, 0.85, 0.45, 12, core_r=0, halo=0.35)
    heart(cv, "ruby", 0, -0.02, 1.15)


def r_crown(cv, P):
    burst(cv, P, P.main, P.core, 0, 0.1, 0.85, 0.45, 12, core_r=0, halo=0.35)
    crown(cv, -0.05, 1.0)


def r_shield_cross(cv, P):
    burst(cv, P, P.main, P.core, 0, 0.05, 0.9, 0.45, 14, core_r=0, halo=0.4)
    cv.obj(it.shield(4, "kite")[0], view(tilt=6, yaw=10), fit=0.78)


def r_eye(cv, P):
    eye(cv, P, 0, 0, 1.2)


def r_cleanse(cv, P):
    burst(cv, P, P.main, P.core, 0, 0, 0.8, 0.35, 8, core_r=0, halo=0.35)
    drop(cv, "sapphire", 0, -0.35, 0.3)
    stars(cv, [(-0.5, 0.5), (0.55, 0.3)], "light", 0.12)


def r_shield_aura(cv, P):
    cv.paint(ellipse(0, -0.5, 0.8, 0.25).shell(0.05), P.core, "glow", bevel=0.05, lo=0.7, hi=1.05)
    cv.obj(it.shield(4, "kite")[0], view(tilt=6, yaw=15), fit=0.72, off=(0, 0.08))


def r_dispel(cv, P):
    cv.paint(circle(0, 0, 0.6).shell(0.06) - rect(0.4, 0.4, 0.15, 0.6, a=-0.7), P.main, "lit", bevel=0.05, base=0.15)
    burst(cv, P, P.core, P.core, 0, 0, 0.55, 0.12, 8, core_r=0.12, halo=0.35)
    stars(cv, [(0.55, 0.55), (-0.6, -0.5), (0.6, -0.45)], "light", 0.1)


def r_lightning(core="light"):
    def f(cv, P):
        cloud(cv, "stone", 0.55)
        lightning(cv, core)
        cv.paint(ellipse(-0.25, -0.62, 0.45, 0.12).shell(0.035), core, "flat", lo=1.0, outline=False)
    return f


def r_spark(cv, P):
    burst(cv, P, P.main, P.core, 0, 0, 0.9, 0.12, 4, core_r=0.0, halo=0.5)
    burst(cv, P, P.core, P.core, 0, 0, 0.5, 0.1, 4, rot=math.pi / 4, core_r=0.15, halo=0.1)


def r_sun(cv, P):
    sun(cv, P, 0, 0, 1.0)


def r_book(gem="sapphire", glow=None):
    def f(cv, P):
        burst(cv, P, glow or P.main, P.core, 0, 0.35, 0.65, 0.3, 10, core_r=0.15, halo=0.4)
        prims, rot, _ = book_open()
        cv.obj(prims, rot, fit=0.8, off=(0, -0.28))
    return f


def r_palm(cv, P):
    burst(cv, P, P.main, P.core, 0, 0.1, 0.9, 0.4, 12, core_r=0.3, halo=0.5)
    cv.paint(hand_shape(0, -0.1, 1.1), "skin", "lit", bevel=0.1, base=0.1)


def r_inner_fire(cv, P):
    flames(cv, P, 0, -0.7, 1.4, 0.32, 3, P.main, P.core)
    heart(cv, "ruby", 0, -0.2, 0.6)


def r_ankh(cv, P):
    ankh(cv, "gold", 0.0, 1.0)


def r_leaf_heal(cv, P):
    cv.paint(arc(0, 0, 0.62, 0.3, 2 * math.pi - 0.6, 0.06), "light", "lit", bevel=0.05, base=0.15)
    cv.paint(poly([(0.5, 0.05), (0.75, 0.3), (0.36, 0.42)]), "light", "lit", bevel=0.05, base=0.15)
    leaf(cv, "leaf", 0, 0, 0.95)


def r_remove_curse(cv, P):
    skull(cv, 0, 0, 1.0, "bone")
    cv.paint(rect(0, 0.0, 0.8, 0.07, a=0.9, rr=0.03), "light", "glow", bevel=0.07, lo=0.8, hi=1.05)
    stars(cv, [(-0.55, 0.55), (0.6, -0.5)], "light", 0.11)


def r_sword_ring(cv, P):
    rune_ring(cv, P, r=0.7)
    prop(cv, it.sword(5, 1.3, 0.12, 0.34), 0.78, rot=view(diag=0, tilt=8, yaw=-20))


def r_flaming_sword(cv, P):
    flames(cv, P, 0, -0.8, 1.55, 0.32, 3, "fire" if P.school == "fire" else P.main, P.core)
    prop(cv, it.sword(5, 1.4, 0.12, 0.34), 0.84, rot=view(diag=0, tilt=8, yaw=-20), shadow=False)


def r_smite(cv, P):
    burst(cv, P, P.main, P.core, 0, -0.15, 0.85, 0.3, 10, core_r=0.0)
    flames(cv, P, 0, -0.6, 1.0, 0.3, 3, P.main, P.core, halo=0.2)
    cv.paint(rect(0, 0.65, 0.08, 0.3) | rect(0, 0.72, 0.22, 0.07), P.core, "lit", bevel=0.05, base=0.2)


def r_hood(cv, P):
    hood(cv, P, "emerald")


def r_afterimage(cv, P):
    for i, (x, lo) in enumerate(((-0.4, 0.3), (-0.15, 0.5), (0.15, 0.8))):
        cv.paint(figure(x, -0.05, 0.95), P.core if P.school != "physical" else "linen", "flat", lo=lo, outline=i == 2)
    speedlines(cv, "snow", -0.6, 0.5, -0.9, 5, 0.5)


def r_ghost(cv, P):
    ghost(cv, "snow")
    cv.obj(chain_links(4, "metal"), view(diag=80, tilt=10, yaw=20), fit=0.9, off=(0, -0.45), shadow=False)


def r_target_arrow(cv, P):
    target(cv, "blood", -0.15, 0.15, 0.9)
    cv.obj(arrows(1), view(diag=-135, tilt=5, yaw=10), fit=0.75, off=(0.2, -0.25))


def r_mark(cv, P):
    target(cv, "blood", 0, 0, 1.15)


def r_taunt(cv, P):
    for a in range(4):
        ang = a * math.pi / 2 + math.pi / 4
        x, y = 0.55 * math.cos(ang), 0.55 * math.sin(ang)
        tri = poly([(0.0, 0.18), (0.22, -0.12), (-0.22, -0.12)]).rot(ang + math.pi / 2).move(x, y)
        cv.paint(tri, "blood", "lit", bevel=0.06, base=0.15)
    cv.glow(circle(0, 0, 0.3), 0.4, 0.4)
    cv.paint(rect(0, 0.12, 0.08, 0.28, rr=0.04) | circle(0, -0.3, 0.09), "blood", "lit", bevel=0.05, base=0.25)


def r_berserk(cv, P):
    cv.glow(circle(0, 0, 0.6), 0.45, 0.5)
    prop(cv, it.axe(5, "battle_axe"), 0.86)


def r_grave_shard(cv, P):
    cv.paint(ellipse(0, -0.6, 0.6, 0.15), "void", "flat", lo=0.6, outline=False)
    for a, ln, w in ((0.0, 1.2, 0.2), (-0.55, 0.8, 0.15), (0.5, 0.85, 0.15)):
        cv.paint(shard(a * 0.4, -0.62, ln, w, a), "bone", "lit", bevel=w * 0.7)
    cv.glow(circle(0, -0.1, 0.3), 0.3, 0.4)


def r_lockpick(cv, P):
    lockpick(cv)


def r_grab(cv, P):
    stars(cv, [(0.45, 0.55), (-0.5, 0.4), (0.6, -0.1)], "topaz", 0.12)
    cv.obj(it.pouch("leather", None)[0], view(tilt=10, yaw=15), fit=0.55, off=(0.0, -0.35))
    cv.paint(hand_shape(0.05, 0.3, 0.75, math.pi), "skin", "lit", bevel=0.08)


def r_sting(cv, P):
    drop(cv, P.core if P.school != "physical" else "sapphire", 0.45, -0.65, 0.13)
    cv.obj(arrows(1, head=P.main if P.school != "physical" else "metal", fletch="emerald"), view(diag=45, tilt=5, yaw=10), fit=0.8)


def r_sap(cv, P):
    r_stun(cv, P)
    prop(cv, it.dagger(3), 0.62, rot=view(diag=110, tilt=10, yaw=-20), off=(0.0, -0.2))
    speedlines(cv, "linen", -0.6, -0.1, -0.95, 3, 0.4)


def r_cheap_shot(cv, P):
    r_stun(cv, P)
    cv.paint(hand_shape(0, -0.25, 0.9, 0.3), "skin", "lit", bevel=0.1)


def r_impact_sword(cv, P):
    burst(cv, P, "topaz", "light", 0.25, 0.25, 0.6, 0.2, 9, core_r=0.0, halo=0.3)
    prop(cv, it.sword(4, 1.5, 0.14, 0.36), 0.86)


def r_dagger_slash(cv, P):
    slashes(cv, "blood", 3, -0.6, 0.26, 0.85)
    prop(cv, it.dagger(4), 0.72, rot=view(diag=45, tilt=10, yaw=-20), off=(-0.1, -0.1))


def r_holy_sword(cv, P):
    r_sword(glow=True)(cv, P)


def r_shadow_sword(cv, P):
    cv.glow(seg(-0.6, -0.6, 0.6, 0.6, 0.15), 0.5, 0.45)
    prop(cv, it.sword(6, 1.5, 0.13, 0.36), 0.86)


def r_up_sword(cv, P):
    chevron(cv, True, "emerald", 0.45, -0.45, 0.8)
    prop(cv, it.sword(4, 1.4, 0.13, 0.36), 0.84)


def r_shield_down(cv, P):
    cv.obj(it.shield(4, "kite")[0], view(tilt=6, yaw=15), fit=0.74)
    chevron(cv, False, "emerald", 0.4, -0.45, 0.75)


def r_fire_bolt(cv, P):
    fireball(cv, P, r=0.24)


def r_hellpain(cv, P):
    flames(cv, P, 0, -0.75, 1.45, 0.34, 3, "blood", "fire")
    skull(cv, 0, -0.15, 0.6, eyes="topaz")


def r_ignite(cv, P):
    burst(cv, P, "fire", "flame", 0, -0.05, 0.85, 0.3, 11, core_r=0.0, halo=0.4)
    flames(cv, P, 0, -0.55, 0.95, 0.24, 3, halo=0.0)


def r_scorch(cv, P):
    cv.paint(ellipse(0, -0.6, 0.85, 0.2), "fire", "glow", bevel=0.2, lo=0.3, hi=0.8, outline=False)
    flames(cv, P, -0.4, -0.7, 0.9, 0.2, 2, halo=0.15)
    flames(cv, P, 0.35, -0.7, 1.1, 0.24, 2, halo=0.15)


def r_explosive(cv, P):
    burst(cv, P, "fire", "flame", 0.3, 0.3, 0.6, 0.22, 10, core_r=0.15, halo=0.4)
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.7, off=(-0.18, -0.18))


def r_flame_arrow(cv, P):
    fireball(cv, P, cx=0.3, cy=0.3, r=0.16, ang=math.radians(225))
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.82, shadow=False)


def r_entangle(cv, P):
    vines(cv, "leaf")
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.82, shadow=False)


def r_stun_arrow(cv, P):
    r_stun(cv, P)
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.78, off=(0, -0.12))


def r_sleep_arrow(cv, P):
    zs(cv, "snow", 0.05, 0.25, 0.8)
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.75, off=(-0.15, -0.15))


def r_kidney(cv, P):
    burst(cv, P, "blood", "ruby", 0.25, 0.25, 0.55, 0.2, 9, core_r=0.0, halo=0.3)
    cv.obj(arrows(1), view(diag=45, tilt=5, yaw=10), fit=0.8)


def r_flurry(cv, P):
    speedlines(cv, P.core if P.school != "physical" else "linen", -0.7, 0.3, -0.95, 5, 0.7)
    cv.obj(arrows(3, spread=0.35), view(diag=60, tilt=5, yaw=10), fit=0.86)


def r_multishot(cv, P):
    cv.obj(arrows(3, parallel=0.32), view(diag=45, tilt=5, yaw=10), fit=0.88)


def r_ice_shard(cv, P):
    ice_shards(cv, P)


def r_poison_bolt(cv, P):
    bolt(cv, P, "poison", "leaf")


# --- Duskhollow skills (assets/data/spells.txt) ---------------------------------------------------


def r_cairnbreaker(cv, P):
    """Battle axe driven down into cracked ground."""
    cv.paint(ellipse(0, -0.62, 0.85, 0.2), "stone", "lit", bevel=0.1, base=0.1)
    cracks = union(*[taper(0, -0.62, x, y, 0.05, 0.01) for x, y in ((-0.75, -0.7), (0.7, -0.55), (-0.35, -0.85), (0.4, -0.85))])
    cv.paint(cracks, "fire", "glow", bevel=0.03, lo=0.6, hi=1.0, outline=False)
    burst(cv, P, "topaz", "flame", 0, -0.55, 0.5, 0.18, 9, core_r=0.0, halo=0.25)
    prop(cv, it.axe(4, "battle_axe"), 0.8, rot=view(diag=200, tilt=10, yaw=-15), off=(0.05, 0.12))


def r_open_vein(cv, P):
    """Hooked cut with a run of blood."""
    slashes(cv, "blood", 2, -0.75, 0.3, 0.95)
    prop(cv, it.dagger(3, red=True), 0.6, rot=view(diag=135, tilt=10, yaw=-20), off=(-0.25, 0.25), shadow=False)
    drop(cv, "blood", 0.35, -0.55, 0.13)
    drop(cv, "blood", 0.6, -0.15, 0.09, halo=0.0)


def r_skullcrack(cv, P):
    """A mace head meeting a brow, stars knocked loose."""
    skull(cv, -0.12, -0.22, 0.85, "bone")
    burst(cv, P, "topaz", "light", 0.2, 0.2, 0.45, 0.14, 8, core_r=0.0, halo=0.25)
    prop(cv, it.mace(3, "maul"), 0.6, rot=view(diag=-30, tilt=10, yaw=-20), off=(0.3, 0.3), shadow=False)
    stars(cv, [(-0.6, 0.45), (-0.25, 0.68)], "topaz", 0.11)


def r_run_down(cv, P):
    """A sword thrust forward, trailing dust."""
    speedlines(cv, "linen", -0.55, 0.35, -0.95, 6, 0.9, seed=3)
    cv.paint(ellipse(-0.35, -0.7, 0.55, 0.12), "phys", "flat", lo=0.6, outline=False)
    prop(cv, it.sword(4, 1.5, 0.13, 0.36), 0.86, rot=view(diag=45, tilt=12, yaw=-20), off=(0.12, 0.05))


def r_clear_the_row(cv, P):
    """A full circle of steel around a reaping blade."""
    ring = arc(0, -0.05, 0.68, 0.3, 2 * math.pi - 0.3, 0.07)
    glowy(cv, ring, "linen", lo=0.5, hi=1.0, bevel=0.05, halo=0.25)
    cv.paint(poly([(0.66, 0.35), (0.9, 0.05), (0.55, 0.1)]), "linen", "lit", bevel=0.04, base=0.2)
    cv.paint(ellipse(0, -0.68, 0.7, 0.13), "redmark", "flat", lo=0.5, outline=False)
    prop(cv, it.axe(3, "infantry_axe"), 0.78, rot=view(diag=30, tilt=10, yaw=-15), shadow=False)


def r_flung_blade(cv, P):
    """A spinning knife in flight."""
    speedlines(cv, "linen", -0.65, 0.05, -0.95, 5, 0.75, seed=5)
    cv.paint(arc(0.25, 0.25, 0.45, 2.4, 4.3, 0.04), "linen", "flat", lo=0.8, outline=False)
    prop(cv, it.dagger(3), 0.7, rot=view(diag=60, tilt=10, yaw=-20), off=(0.18, 0.12))


def r_ember_bolt(cv, P):
    """A coal of cairn fire, tumbling with sparks."""
    fireball(cv, P, "fire", "flame", cx=-0.2, cy=-0.2, r=0.28)
    cv.paint(circle(-0.27, -0.27, 0.11), "fire", "lit", bevel=0.08, base=0.1)
    stars(cv, [(0.5, -0.45), (-0.55, 0.4), (0.15, -0.65)], "flame", 0.08)


def r_drag_hook(cv, P):
    """Barbed iron hook on a run of chain."""
    cv.obj(chain_links(5, "metal"), view(diag=-50, tilt=10, yaw=20), fit=0.62, off=(0.3, 0.32), shadow=False)
    hook = it.curved_spike("plate", length=1.2, r=0.12, bend=0.9)
    cv.obj(hook, view(diag=200, tilt=10, yaw=0), fit=0.66, off=(-0.25, -0.22))
    cv.paint(poly([(-0.62, -0.2), (-0.42, -0.02), (-0.5, -0.28)]), "plate", "lit", bevel=0.03, base=0.2)


def r_kept_ember(cv, P):
    """Cupped hands around a small fire."""
    cv.glow(circle(0, 0.1, 0.55), 0.5, 0.5)
    flames(cv, P, 0, -0.15, 0.85, 0.2, 3, "fire", "flame", halo=0.3)
    cv.paint(hand_shape(-0.32, -0.42, 0.72, -0.9), "skin", "lit", bevel=0.08)
    cv.paint(hand_shape(0.32, -0.42, 0.72, 0.9, mirror=True), "skin", "lit", bevel=0.08)


def r_draw_the_veil(cv, P):
    """A hooded head bowed under the Eye."""
    eye(cv, P, 0, 0.58, 0.8, iris="ruby", rays=False)
    sh = (circle(0, 0.0, 0.42) | rect(0, -0.5, 0.56, 0.36, rr=0.18) | poly([(-0.2, 0.3), (0.0, 0.52), (0.2, 0.3)])).move(0, -0.18)
    cv.paint(sh, "phys", "lit", bevel=0.16, base=0.1)
    cv.paint(ellipse(0, -0.22, 0.24, 0.3), "void", "flat", lo=0.25, outline=False)
    # The veil, pulled low over the eyes.
    veil = poly([(-0.34, -0.02), (0.34, -0.02), (0.3, -0.36), (0.12, -0.3), (0.0, -0.4), (-0.12, -0.3), (-0.3, -0.36)])
    cv.paint(veil, "redmark", "lit", bevel=0.06, base=0.15)


def r_potion(liquid):
    def f(cv, P):
        cv.glow(circle(0, -0.1, 0.55), 0.35, 0.45)
        prims, rot, _ = it.flask(liquid, 3)
        cv.obj(prims, rot, fit=0.8)
    return f


# Class kit skills 50011-50017 (Stream A).


def r_hurled_brand(cv, P):
    """A burning brand from the cairn, tumbling through the air."""
    speedlines(cv, "fire", -0.75, 0.05, -0.95, 5, 0.7, seed=7)
    cv.paint(taper(-0.55, -0.6, 0.2, 0.15, 0.1, 0.075), "fur", "lit", bevel=0.06, base=0.05)
    cv.paint(taper(0.05, 0.0, 0.2, 0.15, 0.09, 0.08), "fire", "glow", bevel=0.04, lo=0.5, hi=1.0, outline=False)
    flames(cv, P, cx=0.3, y0=0.05, h=0.75, w=0.2, n=3, ramp="fire", core="flame", halo=0.4)


def r_scatter_the_coals(cv, P):
    """A fistful of live coals bursting on the ground."""
    cv.paint(ellipse(0, -0.55, 0.85, 0.22), "stone", "lit", bevel=0.08, base=0.05)
    burst(cv, P, "fire", "flame", 0, -0.35, 0.72, 0.22, 11, core_r=0.12, halo=0.4)
    for x, y, r in ((-0.55, -0.5, 0.12), (0.5, -0.55, 0.1), (-0.15, -0.7, 0.09), (0.25, -0.25, 0.11), (-0.35, -0.1, 0.08)):
        cv.paint(circle(x, y, r), "fire", "lit", bevel=0.08, base=0.15)
    stars(cv, [(0.55, 0.35), (-0.6, 0.45), (0.05, 0.6)], "flame", 0.09)


def r_blinding_flare(cv, P):
    """A flash of fire in front of a wide eye."""
    burst(cv, P, "fire", "flame", 0.0, 0.0, 0.92, 0.3, 14, core_r=0.0, halo=0.5)
    eye(cv, P, 0, -0.05, 0.85, iris="ruby", rays=False)
    stars(cv, [(0.55, 0.5), (-0.55, 0.5), (0.0, 0.72)], "flame", 0.12)


def r_between_the_ribs(cv, P):
    """A knife slipped between bare ribs."""
    for i, y in enumerate((0.45, 0.15, -0.15, -0.45)):
        cv.paint(arc(-0.2, y + 0.55, 0.62, 3.6, 5.2 - 0.05 * i, 0.07), "bone", "lit", bevel=0.05)
    cv.paint(rect(-0.62, 0.0, 0.07, 0.62, rr=0.03), "bone", "lit", bevel=0.05)
    prop(cv, it.dagger(3, red=True), 0.66, rot=view(diag=135, tilt=10, yaw=-20), off=(0.18, 0.05), shadow=False)
    drop(cv, "blood", 0.45, -0.55, 0.11)


def r_ember_prayer(cv, P):
    """Hands pressed together over a small cairn flame."""
    praying(cv, P)
    flames(cv, P, cx=0, y0=0.45, h=0.45, w=0.12, n=2, ramp="fire", core="flame", halo=0.3)


def r_ash_ward(cv, P):
    """A buckler smeared with grey cairn ash, warm at the edges."""
    cv.glow(circle(0, 0, 0.8), 0.3, 0.5)
    prop(cv, it.shield(3, "iron_buckler"), 0.82)
    cv.paint(poly([(-0.36, 0.2), (0.3, 0.3), (0.34, 0.16), (-0.32, 0.06)]), "stone", "flat", lo=0.6, outline=False)
    cv.paint(poly([(-0.26, -0.1), (0.24, -0.02), (0.26, -0.14), (-0.22, -0.22)]), "stone", "flat", lo=0.5, outline=False)
    stars(cv, [(0.62, -0.55), (-0.62, -0.5)], "flame", 0.09)


# Auto attacks, potions and NPC spells (Stream A; the icons Stream D may redo).


def r_ember_draught(cv, P):
    """A flask of ember draught, warm in the hand."""
    cv.glow(circle(0, -0.1, 0.6), 0.4, 0.5)
    prop(cv, it.flask("fire", 3), 0.8)


def r_lamp_tonic(cv, P):
    """A tall flask of amber lamp oil."""
    cv.glow(circle(0, -0.1, 0.5), 0.3, 0.5)
    prop(cv, it.flask("topaz", 3, tall=True), 0.8)


def r_set_your_feet(cv, P):
    """Boots planted on cracked ground under a lowered guard."""
    cv.paint(ellipse(0, -0.62, 0.85, 0.2), "stone", "lit", bevel=0.1, base=0.1)
    cracks = union(*[taper(0, -0.62, x, y, 0.04, 0.01) for x, y in ((-0.8, -0.66), (0.78, -0.58), (-0.4, -0.86))])
    cv.paint(cracks, "dark", "flat", lo=0.4, outline=False)
    prims = it.boots("pl", 3)[0][:3]
    cv.obj(prims, view(tilt=10, yaw=-25), fit=0.6, off=(0.0, -0.18))
    chevron(cv, up=False, ramp="plate", cy=0.55, s=0.55, n=2)


DUSK_RULES = [
    (r"^attack$", r_sword()),
    (r"^shoot$", r_bow),
    (r"draught|potion|elixir", r_potion("ruby")),
    (r"tonic", r_potion("sapphire")),
    (r"gate slam|\bslam\b", r_slam),
    (r"^cairnbreaker$", r_cairnbreaker),
    (r"^open vein$", r_open_vein),
    (r"^skullcrack$", r_skullcrack),
    (r"^run down$", r_run_down),
    (r"^clear the row$", r_clear_the_row),
    (r"^flung blade$", r_flung_blade),
    (r"^ember bolt$", r_ember_bolt),
    (r"^drag-hook$", r_drag_hook),
    (r"^kept ember$", r_kept_ember),
    (r"^draw the veil$", r_draw_the_veil),
    (r"^hurled brand$", r_hurled_brand),
    (r"^scatter the coals$", r_scatter_the_coals),
    (r"^blinding flare$", r_blinding_flare),
    (r"^between the ribs$", r_between_the_ribs),
    (r"^ember prayer$", r_ember_prayer),
    (r"^ash ward$", r_ash_ward),
    (r"^set your feet$", r_set_your_feet),
    (r"^attack$", r_sword(q=2)),
    (r"^shoot$", r_bow),
    (r"^ember draught$", r_ember_draught),
    (r"^lamp tonic$", r_lamp_tonic),
    (r"^gate slam$", r_slam),
    (r"^mandible nip$", r_bite("chitin")),
]

RULES = DUSK_RULES + [
    (r"pick lock|pincer", r_lockpick),
    (r"\binteract\b|^loot$|get_item", r_grab),
    (r"sleep arrow", r_sleep_arrow),
    (r"stunning shot", r_stun_arrow),
    (r"entangling", r_entangle),
    (r"explosive arrow", r_explosive),
    (r"flame arrow", r_flame_arrow),
    (r"multi-shot", r_multishot),
    (r"arrow flurry", r_flurry),
    (r"aimed shot", r_target_arrow),
    (r"kidney shot", r_kidney),
    (r"ranged attack|auto shot", r_bow),
    (r"melee swing|auto attack", r_sword()),
    (r"mark target|misdirection", r_mark),
    (r"\bsting\b", r_sting),
    (r"\bsleep\b|restmode|\bnap\b", r_sleep),
    (r"charge", r_charge),
    (r"tactical retreat|sprint", r_boot),
    (r"evasion|determination", r_afterimage),
    (r"vanish|stealth", r_hood),
    (r"blindside|\bsap\b", r_sap),
    (r"sabotage|cheap shot", r_cheap_shot),
    (r"eye gouge", r_eye_gouge),
    (r"hamstring", r_hamstring),
    (r"mighty blow|heroic strike", r_impact_sword),
    (r"sinister strike", r_dagger_slash),
    (r"retribution aura|vengeance aura", r_sword_ring),
    (r"retribution|overpower|counter attack", r_swords_crossed),
    (r"harrowing|hallowed", r_shadow_sword),
    (r"divine strike|crusader", r_holy_sword),
    (r"shield block", r_shield("kite", 3)),
    (r"aegis|shield wall", r_shield("kite", 5, glow=True)),
    (r"blessed shield", r_shield("kite", 4, glow=True)),
    (r"mortal wound", r_wound()),
    (r"infected wound", r_wound("leaf", "poison")),
    (r"\brend\b|whirlwind", r_wound("blood", "blood")),
    (r"venomous bite", r_bite("poison")),
    (r"bite|tigerfang", r_bite()),
    (r"cursed poison|elementalwraith", r_poison_skull),
    (r"poison bolt|tranquility", r_poison_bolt),
    (r"thunder clap|war stomp|sunder", r_slam),
    (r"duke's fury|intimidating", r_sound("blood", skull_=True)),
    (r"demoralizing", r_sound("blood")),
    (r"bellowing roar|brainstorm", r_sound("amethyst", skull_=True)),
    (r"shout|roar", r_sound("topaz")),
    (r"taunt", r_taunt),
    (r"recklessness", r_berserk),
    (r"grave shard|revenge", r_grave_shard),
    (r"immobile|redemption", r_chain()),
    (r"netherverse|salvation aura", lambda cv, P: dome(cv, P)),
    (r"forcecage", r_cage),
    (r"fireball volley|inferno", r_fire_wall),
    (r"fireball", lambda cv, P: fireball(cv, P)),
    (r"fire bolt|holy vengeance", r_fire_bolt),
    (r"ignite|fire blast", r_ignite),
    (r"warmth|fire ward", r_fire_ward),
    (r"scorch", r_scorch),
    (r"hellpain", r_hellpain),
    (r"ice blast|frost nova", nova),
    (r"chains of ice|frost ward", r_chain("ice", ice=True)),
    (r"ice shard|frostbolt", r_ice_shard),
    (r"frost bolt|icebolt", r_bolt("ice", "snow")),
    (r"deep freeze|frozen armor", r_ice_block),
    (r"antimagic|counter spell", r_antimagic),
    (r"dark resolve|fear ward", r_shield("kite", 6, glow=True)),
    (r"illusion gate", r_portal),
    (r"teleport", lambda cv, P: spiral(cv, P.main, P.core)),
    (r"penance|mana burn", r_flames(ramp="sapphire", core="snow")),
    (r"mind blast", r_mind),
    (r"blight|plague", r_plague),
    (r"bind spirit|polymorph", r_ghost),
    (r"satanic madness|psychic scream", r_scream),
    (r"disintegrate|shadowbolt", r_bolt("shadow", "amethyst")),
    (r"amplif", r_buff),
    (r"dampen", r_debuff),
    (r"wings|freedom", r_wings),
    (r"blessing of health", r_heart),
    (r"champion|kings", r_crown),
    (r"blessing of defense|blessing of protection", r_shield_cross),
    (r"boon of protection|iron-clad", r_shield("buckler", 5, glow=True)),
    (r"clairvoyance", r_eye),
    (r"cleanse", r_cleanse),
    (r"desperate prayer", praying),
    (r"fortification aura|devotion aura", r_shield_aura),
    (r"devotion", r_up_sword),
    (r"dispel", r_dispel),
    (r"divine protection|divine shield", lambda cv, P: dome(cv, P)),
    (r"divine storm bolt|smiteblue", r_lightning("snow")),
    (r"holy wrath|divine storm", r_lightning("light")),
    (r"spark of light|flash heal", r_spark),
    (r"radiance|flash of light|holy light", r_sun),
    (r"vim of wisdom|fortify intellect", r_book()),
    (r"wisdom of lazarus|lumiel", r_book(glow="sapphire")),
    (r"hammer of might|hammer of justice", r_hammer),
    (r"holy bolt", r_bolt("holy", "light")),
    (r"inner strength|inner fire", r_inner_fire),
    (r"touch of salvation|lay on hands", r_palm),
    (r"discipline|pain suppression", r_shield_down),
    (r"reincarnation|resurrection", r_ankh),
    (r"remove curse", r_remove_curse),
    (r"rejuvenation|renew", r_leaf_heal),
    (r"righteous", r_flaming_sword),
    (r"smite", r_smite),
    (r"heal", r_heal),
]


def pick_recipe(stem, info):
    texts = [(info.get("name") or "").lower(), stem.lower(), (info.get("description") or "").lower()]
    for text in texts:
        if not text:
            continue
        for pat, fn in RULES:
            if re.search(pat, text):
                return fn, pat
    effects = info.get("effects") or []
    if 6 in effects or 27 in effects:
        return r_heal, "effect:heal"
    if 1 in effects:
        return r_bolt(), "effect:damage"
    if 3 in effects:
        return (r_buff, "effect:buff") if info.get("positive") else (r_debuff, "effect:debuff")
    return r_burst, "fallback"


def spell_icon(stem: str, info: dict, n: int) -> np.ndarray:
    P = Pal(school_of(stem, info))
    fn, _ = pick_recipe(stem, info)
    cv = Canvas(n, P.bg, lo=0.06, hi=0.5)
    fn(cv, P)
    return cv.finish(*FRAME)
