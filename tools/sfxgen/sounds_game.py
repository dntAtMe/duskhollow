"""Game sounds: melee results, UI, spell kits and the glade creatures' voices.

Same conventions as sounds.py (one function or generated variant per file, fixed seeds,
(samples, kind) return values). Every family of repeated sounds (hits, blocks, hurt grunts)
has variants built from the same recipe with different seeds AND different parameters, so
a fight does not sound like a loop. Weighty and dry: flesh, leather, iron, bone, embers.
"""

import numpy as np

from dsp import (
    SR, bp, brown, exp_decay, fade, formants, hp, lp, modal, osc, pad, pink, place, ramp, resonator,
    reverb, rng, saturate, secs, smooth_random, svf, white,
)
from sounds import bell, body_fall, choir, crackles, metal_clank, room, stone_strike, swish, thump, vox

# ------------------------------------------------------------------ building blocks


def _out(y, r, size="small", wet=0.12):
    return reverb(y, room(r, size), wet=wet)


def flesh(n, r, weight=1.0, f_hi=140, tau=0.045):
    """Body impact: a short pitch-dropping thump (no boom), the slap of flesh and cloth that
    carries the hit on small speakers, and a little cloth rustle."""
    body = thump(n, f_hi, 0.6 * f_hi, tau * weight, r, click=0.0, glide=0.012)
    slap = bp(white(n, r), 220, 1800) * exp_decay(n, 0.02 * weight, 0.0006)
    slap = resonator(slap, r.uniform(320, 520), 1.5) * 1.5 + slap
    cloth = bp(pink(n, r), 900, 4000) * exp_decay(n, 0.05, 0.002)
    return 0.9 * body + 0.9 * slap + 0.12 * cloth


def crunch(n, r, count=7, spread=0.03, lo=900, hi=4000, amp=1.0):
    """Bone or chitin cracking: a burst of tiny resonant snaps within `spread` seconds."""
    y = np.zeros(n)
    for _ in range(count):
        m = secs(r.uniform(0.004, 0.012))
        snap = resonator(white(m, r) * np.exp(-np.arange(m) / (0.0015 * SR)), r.uniform(lo, hi), r.uniform(2, 5))
        place(y, snap, secs(r.uniform(0, spread)), amp * r.uniform(0.4, 1.0))
    return lp(y, 7000)


def blade_ring(n, f0, r, decay=0.07):
    """The short ring of a struck edge (thin steel: high, sparse, fast decaying)."""
    return modal(n, f0, [1.0, 1.52, 2.31, 2.97, 3.84], [decay * k for k in (1.0, 0.7, 0.5, 0.4, 0.3)],
                 [1.0, 0.6, 0.45, 0.3, 0.2], r, detune=0.01)


def wood_knock(n, f0, r, decay=0.05):
    """Hollow wood (shield boards, club, bow limb)."""
    return modal(n, f0, [1.0, 2.31, 3.89, 5.2], [decay, decay * 0.6, decay * 0.4, decay * 0.25],
                 [1.0, 0.55, 0.3, 0.15], r, detune=0.02)


def slice_tail(n, r, f_from=6500, f_to=1400, dur=0.12):
    """Edge drawn through the target: a downward noise sweep."""
    m = secs(dur)
    s = svf(white(m, r), ramp(m, [(0, f_from), (dur, f_to)]), 2.2, "bp") * np.hanning(m * 2)[m:] ** 1.5
    return pad(s, n)


def whoosh(n, r, f_lo=250, f_hi=1500, q=1.6, peak=0.5, colour="pink"):
    """Swing through air; `peak` = where in the sound the swing is fastest (0..1)."""
    src = pink(n, r) if colour == "pink" else white(n, r)
    k = int(n * peak)
    env = np.concatenate([np.sin(np.linspace(0, np.pi / 2, max(k, 1))) ** 3,
                          np.cos(np.linspace(0, np.pi / 2, max(n - k, 1))) ** 2])[:n]
    cut = f_lo + (f_hi - f_lo) * env
    return svf(src, cut, q, "bp") * env


def clicks(n, r, rate, f_lo, f_hi, decay=0.0012, jitter=0.3, amp_env=None, q=(3, 8)):
    """Regular-ish train of resonant clicks (mandibles, chitter, chain links)."""
    y = np.zeros(n)
    t = 0.0
    dur = n / SR
    while t < dur:
        at = secs(t)
        if at >= n:
            break
        a = 1.0 if amp_env is None else amp_env[at]
        if a > 0.01:
            m = secs(0.012)
            c = resonator(white(m, r) * np.exp(-np.arange(m) / (decay * SR)), r.uniform(f_lo, f_hi), r.uniform(*q))
            place(y, c, at, a * r.uniform(0.5, 1.0))
        rr = rate if np.isscalar(rate) else rate[min(at, n - 1)]
        t += (1 + jitter * r.uniform(-1, 1)) / max(rr, 1.0)
    return y


def hiss(n, r, lo=2500, hi=9000, env=None):
    y = bp(white(n, r), lo, hi, 2)
    return y if env is None else y * env


def chain(n, r, links=8, spread=0.25, f=(2200, 5200)):
    """Loose iron chain rattling: many small metallic ticks."""
    y = np.zeros(n)
    for _ in range(links):
        m = secs(0.06)
        tick = metal_clank(m, r.uniform(*f), r, decay=0.02, amp_noise=0.3)
        place(y, tick, secs(r.uniform(0, spread)), r.uniform(0.3, 1.0))
    return y


def fire_body(n, r, cut_pts, env_pts, crackle_rate=40):
    """Gas and flame: low-passed noise roar with a moving cutoff, plus ember crackle."""
    roar = svf(pink(n, r), ramp(n, cut_pts), 0.9, "lp") * ramp(n, env_pts)
    cr = crackles(n, r, crackle_rate, density_env=ramp(n, env_pts) / max(v for _, v in env_pts), amp=0.8)
    return roar + 0.6 * cr


# ------------------------------------------------------------------ melee results


def _hit_npc(seed, f_hi, crunchy, claw):
    def make():
        r = rng(seed)
        n = secs(0.45)
        y = flesh(n, r, weight=1.0, f_hi=f_hi)
        if crunchy:
            y += 0.35 * crunch(n, r, count=5, lo=700, hi=2600)
        if claw:  # rake of claws or teeth: a short bright scratch after the blow
            sc = svf(white(secs(0.09), r), ramp(secs(0.09), [(0, 3500), (0.09, 1800)]), 3.0, "bp")
            place(y, 0.3 * sc * np.hanning(secs(0.09)), secs(0.006))
        return _out(saturate(y * 1.2, 1.8), r, wet=0.1), "sfx"
    return make


def _hit_blade(seed, ring_f, sweep_from, weight):
    def make():
        r = rng(seed)
        n = secs(0.5)
        y = flesh(n, r, weight=weight, f_hi=150, tau=0.05)
        y += 0.5 * slice_tail(n, r, sweep_from, 1300, dur=r.uniform(0.09, 0.14))
        y += 0.12 * blade_ring(n, ring_f, r, decay=0.06)
        y += 0.25 * hp(white(n, r), 2500) * exp_decay(n, 0.006, 0.0003)  # the edge biting
        return _out(saturate(y * 1.2, 1.6), r, wet=0.1), "sfx"
    return make


def _hit_blunt(seed, f_hi, knock_f, bones):
    def make():
        r = rng(seed)
        n = secs(0.55)
        y = flesh(n, r, weight=1.35, f_hi=f_hi, tau=0.08)
        y += 0.35 * wood_knock(n, knock_f, r, decay=0.05)
        y += bones * crunch(n, r, count=8, spread=0.04, lo=600, hi=2400)
        return _out(saturate(y * 1.4, 2.2), r, wet=0.12), "sfx"
    return make


def miss():
    r = rng(810)
    n = secs(0.32)
    y = whoosh(n, r, 220, 1300, q=1.4, peak=0.45)
    return _out(y, r, wet=0.08), "sfx"


def dodge():
    r = rng(811)
    n = secs(0.4)
    y = whoosh(n, r, 300, 2200, q=1.8, peak=0.35)
    flap = bp(pink(n, r), 500, 2500) * ramp(n, [(0, 0), (0.08, 1.0), (0.2, 0.3), (0.4, 0)])
    flap *= 0.6 + 0.4 * np.sign(np.sin(2 * np.pi * 22 * np.arange(n) / SR))  # cloth flutter
    scuff = np.zeros(n)
    place(scuff, bp(white(secs(0.1), r), 800, 5000) * exp_decay(secs(0.1), 0.03, 0.004), secs(0.2))
    y = y + 0.35 * lp(flap, 3000) + 0.4 * scuff
    return _out(y, r, wet=0.08), "sfx"


def parry():
    r = rng(812)
    n = secs(0.7)
    clang = metal_clank(n, 930, r, decay=0.3, amp_noise=0.5)
    clang += 0.5 * blade_ring(n, 2650, r, decay=0.18)
    m = secs(0.16)  # the blades grinding off each other
    grind = bp(white(m, r), 2500, 8000) * (0.5 + 0.5 * np.abs(np.sin(2 * np.pi * 70 * np.arange(m) / SR)))
    grind *= ramp(m, [(0, 0), (0.01, 1.0), (0.16, 0)])
    y = clang + 0.0
    place(y, 0.35 * grind, secs(0.01))
    y += 0.4 * thump(n, 180, 90, 0.03, r, click=0.0)
    return _out(y, r, "medium", 0.15), "sfx"


def _block(seed, f0, rim):
    def make():
        r = rng(seed)
        n = secs(0.5)
        y = wood_knock(n, f0, r, decay=0.06) + 0.9 * thump(n, 130, 60, 0.06, r, click=0.0)
        y += rim * metal_clank(n, r.uniform(500, 700), r, decay=0.12, amp_noise=0.2)
        y += 0.3 * bp(white(n, r), 400, 3000) * exp_decay(n, 0.012, 0.0005)
        return _out(saturate(y * 1.2, 1.7), r, wet=0.12), "sfx"
    return make


def _hurt(seed, f0, vowels, dur, fry):
    def make():
        r = rng(seed)
        y = vox(dur, [(0, f0 * 1.15), (0.06, f0 * 1.25), (dur, f0 * 0.8)],
                [(0, vowels[0]), (dur, vowels[1])], r, breath=0.6, fry=fry, jitter=0.02, open_q=0.45,
                env_pts=[(0, 0), (0.02, 1.0), (dur * 0.45, 0.45), (dur, 0)])
        n = secs(dur)
        y += 0.3 * lp(white(n, r), 900) * exp_decay(n, 0.03, 0.002)  # breath forced out
        return _out(lp(y, 4500), r, wet=0.08), "sfx"
    return make


# ------------------------------------------------------------------ alerts + ui


def level_up():
    r = rng(830)
    dur = 2.6
    n = secs(dur)
    y = np.zeros(n)
    place(y, 0.9 * bell(secs(2.6), 98, r, muted=0.55), 0)
    place(y, 0.5 * bell(secs(2.4), 146.8, r, muted=0.6), secs(0.22))
    place(y, 0.35 * bell(secs(2.0), 196, r, muted=0.65), secs(0.44))
    swell = choir(n, 196, [1, 1.5, 2.0, 3.0], r, voices=3, vowel="o",
                  env=ramp(n, [(0, 0), (0.3, 0.1), (1.1, 0.8), (dur, 0)]))
    embers = crackles(n, r, 35, density_env=ramp(n, [(0, 0), (0.4, 1.0), (dur, 0)]), amp=0.7)
    y = y + 0.3 * swell + 0.35 * embers + 0.6 * thump(n, 70, 38, 0.4, r, click=0.0, glide=0.1)
    return reverb(lp(y, 6000), room(r, "large"), wet=0.35), "sfx"


def ui_click():
    r = rng(840)
    n = secs(0.09)
    y = wood_knock(n, 1450, r, decay=0.012) + 0.35 * lp(white(n, r), 3500) * exp_decay(n, 0.002, 0.0002)
    return lp(y, 6000), "sfx"


def ui_target():
    r = rng(841)
    n = secs(0.26)
    y = 0.7 * thump(n, 260, 170, 0.04, r, click=0.0, glide=0.01)
    y += 0.25 * modal(n, 880, [1.0, 2.76, 5.4], [0.07, 0.04, 0.02], [1.0, 0.4, 0.2], r)
    y += 0.2 * bp(pink(n, r), 600, 3000) * exp_decay(n, 0.015, 0.001)
    return _out(lp(y, 5000), r, wet=0.12), "sfx"


def ui_window_open():
    r = rng(842)
    dur = 0.3
    n = secs(dur)
    y = svf(pink(n, r), ramp(n, [(0, 500), (0.18, 2600), (dur, 1400)]), 1.1, "bp")
    y *= ramp(n, [(0, 0), (0.06, 0.7), (0.16, 0.4), (dur, 0)])
    y += 0.5 * thump(n, 200, 120, 0.03, r, click=0.0, glide=0.01)
    return _out(y, r, wet=0.1), "sfx"


def ui_window_close():
    r = rng(843)
    dur = 0.28
    n = secs(dur)
    y = svf(pink(n, r), ramp(n, [(0, 2200), (0.16, 600), (dur, 400)]), 1.1, "bp")
    y *= ramp(n, [(0, 0), (0.03, 0.5), (0.12, 0.3), (dur, 0)])
    th = np.zeros(n)
    place(th, thump(secs(0.15), 170, 90, 0.03, r, click=0.0, glide=0.01), secs(0.1))
    y += 0.7 * th
    return _out(y, r, wet=0.1), "sfx"


def item_use():
    r = rng(844)
    dur = 0.9
    n = secs(dur)
    y = np.zeros(n)
    m = secs(0.05)  # cork
    pop = osc(ramp(m, [(0, 700), (0.05, 260)])) * exp_decay(m, 0.012, 0.0005)
    place(y, pop + 0.4 * bp(white(m, r), 500, 3000) * exp_decay(m, 0.004, 0.0002), 0)
    for k in range(4):  # three or four glugs: rising bubbles in a narrow neck
        at = 0.18 + 0.13 * k + r.uniform(-0.02, 0.02)
        g = secs(0.09)
        tt = np.arange(g) / SR
        f = r.uniform(260, 360) * (1 + 1.6 * tt / 0.09)
        bub = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-tt / 0.03) * np.sin(np.pi * tt / 0.09)
        place(y, bub, secs(at), 0.8 - 0.1 * k)
    y += 0.15 * lp(pink(n, r), 800) * ramp(n, [(0, 0), (0.18, 0.6), (0.7, 0.4), (dur, 0)])
    return _out(y, r, wet=0.12), "sfx"


# ------------------------------------------------------------------ spell kits


def spell_heavy_slash():
    r = rng(850)
    n = secs(0.8)
    y = 0.7 * whoosh(secs(0.8), r, 160, 900, q=1.3, peak=0.18)
    imp = flesh(n, r, weight=1.5, f_hi=120, tau=0.09)
    imp += 0.5 * slice_tail(n, r, 5500, 900, dur=0.18) + 0.12 * blade_ring(n, 2100, r, decay=0.1)
    place(y, imp, secs(0.1))
    return _out(saturate(y * 1.3, 2.0), r, wet=0.14), "sfx"


def spell_bleed_hit():
    r = rng(851)
    n = secs(0.7)
    y = flesh(n, r, weight=1.0, f_hi=150, tau=0.05) + 0.5 * slice_tail(n, r, 7000, 1800, dur=0.1)
    # wet tear: modulated low noise with squelchy resonances
    m = secs(0.35)
    tear = formants(white(m, r), [380, 900, 1700], [80, 150, 260], [1.0, 0.6, 0.3])
    tear *= (0.5 + 0.5 * smooth_random(m, r, 40, 0, 1)) * ramp(m, [(0, 0), (0.02, 1.0), (0.35, 0)])
    place(y, 0.6 * tear, secs(0.03))
    place(y, 0.15 * hiss(secs(0.25), r, 2000, 6000, np.hanning(secs(0.25))), secs(0.06))  # spray
    return _out(y, r, wet=0.1), "sfx"


def spell_skull_crack():
    r = rng(852)
    n = secs(0.7)
    y = flesh(n, r, weight=1.3, f_hi=170, tau=0.06)
    y += 0.9 * crunch(n, r, count=12, spread=0.035, lo=900, hi=3500)
    y += 0.3 * wood_knock(n, 520, r, decay=0.04)  # the hollow of the head
    ring = osc(ramp(n, [(0, 3150), (0.7, 3120)])) * ramp(n, [(0, 0), (0.05, 0.05), (0.7, 0)])  # ears ring
    return _out(saturate(y * 1.3, 2.0) + ring, r, wet=0.14), "sfx"


def spell_dash():
    r = rng(853)
    dur = 0.7
    n = secs(dur)
    rush = whoosh(n, r, 150, 1700, q=1.0, peak=0.4)
    low = lp(brown(n, r), 300) * np.hanning(n) ** 2
    steps = np.zeros(n)
    for k, at in enumerate([0.02, 0.17, 0.31]):
        place(steps, thump(secs(0.12), 150, 70, 0.025, r, click=0.0), secs(at), 0.7 - 0.15 * k)
        place(steps, bp(white(secs(0.08), r), 700, 4000) * exp_decay(secs(0.08), 0.02, 0.003), secs(at), 0.25)
    y = rush + 0.5 * low + steps
    return _out(y, r, wet=0.12), "sfx"


def spell_blade_hit():
    return _hit_blade(854, 2900, 7200, 1.1)()


def spell_ground_slam():
    r = rng(855)
    dur = 1.7
    n = secs(dur)
    boom = thump(n, 90, 32, 0.45, r, click=0.0, glide=0.06)
    rumble = lp(brown(n, r), 140) * ramp(n, [(0, 0), (0.02, 1.0), (0.6, 0.5), (dur, 0)])
    crack = stone_strike(n, 110, r, bright=0.3)
    debris = np.zeros(n)
    for _ in range(22):  # earth and stones falling back
        at = r.uniform(0.08, 0.9)
        place(debris, stone_strike(secs(0.12), r.uniform(500, 1800), r, 0.4) * exp_decay(secs(0.12), 0.02),
              secs(at), r.uniform(0.1, 0.4) * (1 - at))
    y = 1.4 * boom + 0.9 * rumble + 0.5 * crack + 0.6 * debris
    return reverb(saturate(y * 1.2, 2.0), room(r, "medium"), wet=0.25), "sfx"


def spell_knife_throw():
    r = rng(856)
    dur = 0.5
    n = secs(dur)
    tt = np.arange(n) / SR
    spin = 0.4 + 0.6 * np.abs(np.sin(2 * np.pi * 14 * tt))  # the blade turning end over end
    y = svf(white(n, r), ramp(n, [(0, 2500), (dur, 1600)]), 2.5, "bp") * spin
    y *= ramp(n, [(0, 0), (0.03, 1.0), (0.35, 0.6), (dur, 0)])
    rel = np.zeros(n)
    place(rel, swish(secs(0.12), r, 800, 3500), 0)
    return _out(0.7 * y + 0.5 * rel, r, wet=0.1), "sfx"


def spell_fire_cast():
    r = rng(857)
    dur = 1.2
    n = secs(dur)
    y = fire_body(n, r, [(0, 200), (0.8, 1800), (dur, 900)], [(0, 0), (0.7, 1.0), (0.95, 0.7), (dur, 0)],
                  crackle_rate=50)
    y += 0.3 * lp(brown(n, r), 120) * ramp(n, [(0, 0), (0.8, 1.0), (dur, 0)])
    return _out(y, r, wet=0.15), "sfx"


def spell_ember_whoosh():
    r = rng(858)
    dur = 0.8
    n = secs(dur)
    y = fire_body(n, r, [(0, 900), (0.15, 2400), (dur, 700)], [(0, 0), (0.06, 1.0), (0.4, 0.6), (dur, 0)],
                  crackle_rate=30)
    y += 0.6 * whoosh(n, r, 200, 1200, q=1.2, peak=0.2)
    return _out(y, r, wet=0.12), "sfx"


def spell_ember_burst():
    r = rng(859)
    dur = 1.4
    n = secs(dur)
    boom = thump(n, 110, 40, 0.18, r, click=0.0, glide=0.03)
    burst = svf(pink(n, r), ramp(n, [(0, 3500), (0.15, 1200), (dur, 300)]), 0.8, "lp")
    burst *= ramp(n, [(0, 0), (0.008, 1.0), (0.25, 0.4), (dur, 0)])
    cr = crackles(n, r, 120, density_env=ramp(n, [(0, 1.0), (0.4, 0.6), (dur, 0)]), amp=1.0)
    y = 1.0 * boom + 0.9 * burst + 0.7 * cr
    return reverb(saturate(y * 1.3, 2.0), room(r, "medium"), wet=0.2), "sfx"


def spell_hook_throw():
    r = rng(860)
    dur = 0.65
    n = secs(dur)
    y = 0.6 * whoosh(n, r, 300, 1500, q=1.4, peak=0.25)
    y += 0.5 * lp(chain(n, r, links=14, spread=0.45, f=(1600, 3800)), 6000)
    return _out(y, r, wet=0.1), "sfx"


def spell_hook_hit():
    r = rng(861)
    n = secs(0.6)
    y = flesh(n, r, weight=1.0, f_hi=150, tau=0.05)
    y += 0.2 * lp(metal_clank(n, 900, r, decay=0.06, amp_noise=0.2), 4000)
    tug = np.zeros(n)
    place(tug, lp(chain(secs(0.3), r, links=6, spread=0.1, f=(1500, 3200)), 5000), secs(0.12))
    y += 0.3 * tug
    return _out(saturate(y * 1.2, 1.7), r, wet=0.1), "sfx"


def spell_bow_draw():
    r = rng(862)
    dur = 0.7
    n = secs(dur)
    # creak: stick-slip pulses through wood resonances, rate rising as the limbs bend
    rate = ramp(n, [(0, 40), (0.5, 120), (dur, 90)])
    ph = np.cumsum(rate) / SR
    pulses = (np.diff(np.floor(ph), prepend=0) > 0).astype(float)
    creak = formants(pulses, [420, 1150, 2600], [50, 90, 220], [1.0, 0.6, 0.25])
    creak *= ramp(n, [(0, 0), (0.1, 0.7), (0.55, 1.0), (dur, 0)])
    string = bp(white(n, r), 2000, 6000) * ramp(n, [(0, 0), (0.15, 0.3), (0.6, 0.15), (dur, 0)])
    leather = bp(pink(n, r), 300, 1500) * ramp(n, [(0, 0), (0.05, 0.5), (0.3, 0.1), (dur, 0)])
    return _out(creak + 0.08 * string + 0.4 * leather, r, wet=0.1), "sfx"


def spell_arrow_release():
    r = rng(863)
    dur = 0.5
    n = secs(dur)
    tt = np.arange(n) / SR
    twang = (osc(ramp(n, [(0, 150), (0.05, 118), (dur, 112)])) + 0.5 * osc(ramp(n, [(0, 300), (dur, 225)])))
    twang *= np.exp(-tt / 0.09) * (1 + 0.3 * np.sin(2 * np.pi * 9 * tt))
    snap = lp(white(n, r), 4000) * exp_decay(n, 0.004, 0.0002)
    fly = whoosh(n, r, 900, 3500, q=2.5, peak=0.15, colour="white") * 0.8
    return _out(0.9 * twang + 0.5 * snap + 0.5 * fly, r, wet=0.1), "sfx"


def spell_arrow_hit():
    r = rng(864)
    n = secs(0.4)
    y = flesh(n, r, weight=0.8, f_hi=170, tau=0.04)
    y += 0.4 * wood_knock(n, 640, r, decay=0.03)  # shaft shudder
    tt = np.arange(n) / SR
    y += 0.12 * osc(ramp(n, [(0, 95), (0.4, 88)])) * np.exp(-tt / 0.08) * np.sin(2 * np.pi * 21 * tt)
    return _out(y, r, wet=0.1), "sfx"


def spell_ember_heal():
    r = rng(865)
    dur = 1.7
    n = secs(dur)
    warm = fire_body(n, r, [(0, 250), (0.6, 700), (dur, 400)], [(0, 0), (0.5, 0.7), (1.1, 0.5), (dur, 0)],
                     crackle_rate=25)
    tone = np.zeros(n)
    for f, a in [(130.8, 1.0), (196.0, 0.6), (261.6, 0.25), (131.4, 0.5)]:
        tone += a * osc(f * (1 + 0.002 * smooth_random(n, r, 0.6, -1, 1)), phase=r.uniform(0, 6))
    tone = lp(tone, 900) * ramp(n, [(0, 0), (0.5, 0.8), (1.2, 0.5), (dur, 0)])
    y = 0.8 * warm + 0.35 * tone
    return reverb(y, room(r, "medium"), wet=0.25), "sfx"


def spell_veil():
    r = rng(866)
    dur = 1.6
    n = secs(dur)
    shroud = svf(pink(n, r), ramp(n, [(0, 3000), (0.5, 900), (dur, 300)]), 1.3, "bp")
    shroud *= ramp(n, [(0, 0), (0.15, 1.0), (0.8, 0.5), (dur, 0)])
    shimmer = choir(n, 233, [1, 1.414, 2.03, 2.71], r, voices=3, vowel="u",
                    env=ramp(n, [(0, 0), (0.25, 0.6), (1.0, 0.3), (dur, 0)]))
    low = osc(ramp(n, [(0, 62), (dur, 55)])) * ramp(n, [(0, 0), (0.3, 0.6), (dur, 0)])
    y = 0.7 * shroud + 0.3 * shimmer + 0.35 * low
    return reverb(y, room(r, "large"), wet=0.35), "sfx"


def spell_warden_sweep():
    r = rng(867)
    dur = 1.0
    n = secs(dur)
    y = whoosh(n, r, 110, 700, q=1.1, peak=0.3) + 0.4 * lp(brown(n, r), 200) * np.hanning(n) ** 2
    plate = np.zeros(n)
    place(plate, metal_clank(secs(0.6), 290, r, decay=0.25), 0)
    place(plate, 0.6 * metal_clank(secs(0.5), 410, r, decay=0.2), secs(0.3))
    y += 0.4 * plate
    return _out(saturate(y * 1.2, 1.6), r, "medium", 0.2), "sfx"


def spell_warden_roar():
    r = rng(868)
    dur = 1.8
    n = secs(dur)
    g = vox(dur, [(0, 55), (0.7, 72), (1.4, 66), (dur, 50)], [(0, "u"), (0.6, "a"), (1.3, "o"), (dur, "u")], r,
            breath=0.5, fry=0.25, jitter=0.03, scale=0.75, open_q=0.5,
            env_pts=[(0, 0), (0.35, 0.9), (1.2, 1.0), (dur, 0)])
    from dsp import comb
    g = saturate(comb(g, 0.0031, 0.6) * 1.3, 1.8)
    sub = osc(ramp(n, [(0, 45), (dur, 38)])) * ramp(n, [(0, 0), (0.6, 0.6), (1.3, 0.6), (dur, 0)])
    y = g + 0.4 * sub + 0.3 * metal_clank(n, 300, r, decay=0.3)
    return reverb(y, room(r, "medium"), wet=0.25), "sfx"


# ------------------------------------------------------------------ glade creatures


# goblins (also the crazed charger, through the model-name prefix): small, wiry, half mad


def npc_goblin_aggro():
    r = rng(901)
    dur = 1.1
    y = vox(dur, [(0, 330), (0.15, 520), (0.35, 470), (0.5, 600), (0.75, 540), (dur, 380)],
            [(0, "e"), (0.3, "a"), (0.55, "e"), (dur, "a")], r, breath=0.4, fry=0.2, jitter=0.04,
            shimmer=0.3, scale=1.25, open_q=0.4,
            env_pts=[(0, 0), (0.04, 1.0), (0.28, 0.4), (0.4, 1.0), (0.7, 0.5), (0.8, 0.8), (dur, 0)])
    return _out(saturate(y * 1.3, 1.8), r, wet=0.12), "sfx"


def npc_goblin_attack():
    r = rng(902)
    dur = 0.45
    n = secs(dur)
    y = vox(dur, [(0, 300), (0.1, 360), (dur, 240)], [(0, "a"), (dur, "@")], r, breath=0.6, fry=0.25,
            scale=1.2, env_pts=[(0, 0), (0.02, 1.0), (0.15, 0.4), (0.3, 0), (dur, 0)])
    y += 0.4 * swish(n, r, 700, 3000)
    return _out(y, r), "sfx"


def npc_goblin_hit():
    r = rng(903)
    dur = 0.4
    y = vox(dur, [(0, 560), (0.06, 680), (dur, 380)], [(0, "i"), (dur, "@")], r, breath=0.4, fry=0.15,
            scale=1.25, env_pts=[(0, 0), (0.015, 1.0), (0.2, 0.4), (dur, 0)])
    return _out(y, r), "sfx"


def npc_goblin_death():
    r = rng(904)
    dur = 1.5
    n = secs(dur)
    y = vox(dur, [(0, 520), (0.25, 600), (0.9, 260), (dur, 180)], [(0, "a"), (0.4, "@"), (dur, "u")], r,
            breath=0.6, fry=0.4, jitter=0.05, shimmer=0.4, scale=1.2,
            env_pts=[(0, 0), (0.03, 1.0), (0.5, 0.6), (1.0, 0.15), (dur, 0)])
    gurgle = lp(white(n, r), 600) * (0.5 + 0.5 * np.sin(2 * np.pi * 13 * np.arange(n) / SR))
    y += 0.2 * gurgle * ramp(n, [(0, 0), (0.5, 0.0), (0.7, 0.8), (1.2, 0), (dur, 0)])
    fall = np.zeros(n)
    place(fall, body_fall(secs(0.45), r, 0.5), secs(0.8))
    return _out(y + 0.7 * fall, r), "sfx"


# spiders: no voice, only air forced through spiracles, fangs and many legs


def npc_spider_aggro():
    r = rng(911)
    dur = 1.0
    n = secs(dur)
    env = ramp(n, [(0, 0), (0.08, 1.0), (0.6, 0.8), (dur, 0)])
    y = formants(white(n, r), [1900, 3300, 5200], [600, 800, 1100], [1.0, 0.6, 0.3]) * env
    y = lp(y, 7000)
    rate = ramp(n, [(0, 25), (dur, 40)])
    y += 0.8 * clicks(n, r, rate, 1800, 4200, amp_env=env)
    return _out(y, r), "sfx"


def npc_spider_attack():
    r = rng(912)
    dur = 0.4
    n = secs(dur)
    h = hiss(n, r, 2500, 8000, ramp(n, [(0, 0), (0.02, 1.0), (0.12, 0.3), (dur, 0)]))
    fang = clicks(secs(0.06), r, 45, 2500, 5000)
    y = 0.8 * h + pad(1.2 * fang, n) + 0.35 * swish(n, r, 900, 3500)
    return _out(y, r), "sfx"


def npc_spider_hit():
    r = rng(913)
    dur = 0.4
    n = secs(dur)
    squelch = formants(white(n, r), [350, 800], [90, 160], [1.0, 0.5]) * exp_decay(n, 0.04, 0.002)
    y = 0.8 * squelch + 0.5 * crunch(n, r, count=5, lo=1500, hi=5000)
    y += 0.5 * clicks(n, r, 55, 2000, 4500, amp_env=ramp(n, [(0, 1.0), (dur, 0)]))
    return _out(y, r), "sfx"


def npc_spider_death():
    r = rng(914)
    dur = 1.6
    n = secs(dur)
    h = svf(white(n, r), ramp(n, [(0, 4500), (dur, 1800)]), 2.0, "bp") * ramp(n, [(0, 0), (0.05, 1.0), (1.2, 0.2), (dur, 0)])
    legs = clicks(n, r, ramp(n, [(0, 50), (dur, 6)]), 1200, 3500, amp_env=ramp(n, [(0, 1.0), (dur, 0.1)]))
    y = 0.7 * h + 0.7 * legs + 0.6 * crunch(n, r, count=7, lo=800, hi=3000)
    fall = np.zeros(n)
    place(fall, thump(secs(0.3), 160, 80, 0.04, r, click=0.0), secs(0.35))
    return _out(y + 0.5 * fall, r), "sfx"


# antlings (small antlions): mandibles and a buzzing stridulation


def _stridulate(n, r, rate_pts, f_lo=1200, f_hi=3200):
    rate = ramp(n, rate_pts)
    saw = osc(rate, shape="saw")
    return bp(saw + 0.3 * white(n, r), f_lo, f_hi)


def npc_antlion_small_aggro():
    r = rng(921)
    dur = 1.0
    n = secs(dur)
    env = ramp(n, [(0, 0), (0.1, 1.0), (0.7, 0.8), (dur, 0)])
    y = 0.6 * _stridulate(n, r, [(0, 140), (0.5, 190), (dur, 150)]) * env
    y += clicks(n, r, 14, 1500, 3000, amp_env=env, q=(4, 10))
    return _out(y, r), "sfx"


def npc_antlion_small_attack():
    r = rng(922)
    dur = 0.35
    n = secs(dur)
    y = np.zeros(n)
    for at in (0.0, 0.07):
        place(y, crunch(secs(0.05), r, count=3, spread=0.006, lo=1800, hi=3800), secs(at))
    y += 0.3 * swish(n, r, 900, 3500)
    return _out(y, r), "sfx"


def npc_antlion_small_hit():
    r = rng(923)
    dur = 0.4
    n = secs(dur)
    y = crunch(n, r, count=9, spread=0.03, lo=1600, hi=5000)
    y += 0.5 * _stridulate(n, r, [(0, 260), (dur, 180)], 1500, 4000) * ramp(n, [(0, 0), (0.02, 1.0), (dur, 0)])
    y += 0.5 * thump(n, 200, 110, 0.03, r, click=0.0)
    return _out(y, r), "sfx"


def npc_antlion_small_death():
    r = rng(924)
    dur = 1.4
    n = secs(dur)
    y = crunch(n, r, count=12, spread=0.06, lo=1200, hi=4500)
    y += 0.6 * _stridulate(n, r, [(0, 200), (dur, 25)]) * ramp(n, [(0, 0), (0.05, 1.0), (1.1, 0.2), (dur, 0)])
    y += 0.5 * clicks(n, r, ramp(n, [(0, 30), (dur, 4)]), 1500, 3200, amp_env=ramp(n, [(0, 0.8), (dur, 0)]))
    return _out(y, r), "sfx"


# ------------------------------------------------------------------ registry


def _variants():
    out = {}
    for i, (f_hi, crunchy, claw) in enumerate([(135, False, True), (115, True, False), (155, False, False),
                                               (105, True, True)], 1):
        out[f"hit_npc_{i}"] = _hit_npc(800 + i, f_hi, crunchy, claw)
    for i, (ring, sweep, w) in enumerate([(2700, 6800, 1.0), (3300, 7600, 0.9), (2300, 6000, 1.15),
                                          (3050, 5200, 1.05)], 1):
        out[f"hit_blade_{i}"] = _hit_blade(810 + i, ring, sweep, w)
    for i, (f_hi, knock, bones) in enumerate([(120, 330, 0.25), (105, 280, 0.45), (135, 390, 0.15),
                                              (95, 250, 0.6)], 1):
        out[f"hit_blunt_{i}"] = _hit_blunt(820 + i, f_hi, knock, bones)
    for i, (f0, rim) in enumerate([(210, 0.35), (185, 0.15), (240, 0.5)], 1):
        out[f"block_{i}"] = _block(830 + i, f0, rim)
    for i, (f0, vowels, dur, fry) in enumerate([(125, ("@", "u"), 0.42, 0.2), (138, ("a", "@"), 0.36, 0.3),
                                                (116, ("m", "u"), 0.5, 0.15)], 1):
        out[f"player_hurt_{i}"] = _hurt(840 + i, f0, vowels, dur, fry)
    return out


PREFIXES = ("spell_", "ui_", "npc_")
SOUNDS = {name: fn for name, fn in globals().items()
          if callable(fn) and not name.startswith("_") and (
              name.startswith(PREFIXES) or name in ("miss", "dodge", "parry", "level_up", "item_use"))}
SOUNDS.update(_variants())

TARGETS = {
    "ui_click": -29.0, "ui_": -27.0, "item_use": -21.0, "level_up": -19.0,
    "miss": -22.0, "dodge": -22.0, "player_hurt_": -21.0,
    "spell_bow_draw": -22.0, "spell_fire_cast": -21.0, "spell_knife_throw": -21.0, "spell_hook_throw": -21.0,
    "npc_spider_": -20.0, "npc_antlion_small_": -20.0,
}

# Specific names before their prefixes (the first match wins).
MAX_LEN = {
    "hit_npc_": 0.5, "hit_blade_": 0.5, "hit_blunt_": 0.55, "block_": 0.55, "player_hurt_": 0.6,
    "miss": 0.4, "dodge": 0.45, "parry": 0.8, "ui_": 0.35, "item_use": 1.0, "level_up": 3.0,
    "spell_ground_slam": 1.9, "spell_ember_burst": 1.7, "spell_ember_heal": 1.9, "spell_veil": 1.9,
    "spell_warden_roar": 2.0, "spell_warden_sweep": 1.3, "spell_": 1.5,
    "npc_goblin_": 1.6, "npc_spider_": 1.6, "npc_antlion_small_": 1.5,
}
