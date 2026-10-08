"""Sound designs for Duskhollow: one function per file name.

Each design returns (samples, kind) where kind is "sfx" (one-shot, gets a tail trim)
or "loop" (seamless; built periodic). Loudness targets are applied by sfxgen.py.
Mood: dread, weariness, a crimson sky that watches. Low drones, cloth, stone, embers.
"""

import numpy as np

from dsp import (
    SR, adsr, bp, brown, comb, exp_decay, fade, formants, formants_tv, glottal, hp, lp, modal, osc,
    pad, periodic_filter, periodic_lfo, pink, place, place_wrap, ramp, resonator, reverb,
    reverb_circular, reverb_ir, rng, saturate, secs, smooth_random, svf, white,
)

VOWELS = {
    "a": (800, 1150, 2900),
    "e": (420, 1900, 2550),
    "i": (300, 2250, 3000),
    "o": (460, 820, 2800),
    "u": (330, 720, 2500),
    "@": (520, 1450, 2500),
    "m": (260, 1050, 2300),
}

# ------------------------------------------------------------------ building blocks


def choir(n, base, ratios, r, voices=3, vowel="a", vib=4.5, spread=0.006, env=None):
    """Inharmonic choir-like shimmer: detuned additive partials through vowel formants."""
    y = np.zeros(n)
    for k, ra in enumerate(ratios):
        amp = 1.0 / (1 + 0.6 * k)
        for v in range(voices):
            det = 1 + spread * r.standard_normal()
            drift = 1 + 0.004 * smooth_random(n, r, 0.4, -1, 1)
            vibrato = 1 + 0.003 * np.sin(2 * np.pi * (vib + r.uniform(-0.6, 0.6)) * np.arange(n) / SR
                                         + r.uniform(0, 6.28))
            f = base * ra * det * drift * vibrato
            if f.max() > SR * 0.45:
                continue
            a = amp * (0.6 + 0.4 * smooth_random(n, r, 0.7, 0, 1))
            y += a * osc(f, phase=r.uniform(0, 6.28))
    f1, f2, f3 = VOWELS[vowel]
    y = 0.35 * y + formants(y, [f1, f2, f3], [90, 120, 180], [1.0, 0.6, 0.35])
    if env is not None:
        y *= env
    return y


def vox(dur, f0_pts, vowel_pts, r, breath=0.15, jitter=0.012, shimmer=0.08, env_pts=None,
        q=(5.0, 7.0, 9.0), scale=1.0, open_q=0.6, fry=0.0):
    """Non-verbal voice: glottal source + breath through time-varying formants.

    f0_pts: [(t, Hz)], vowel_pts: [(t, vowel)], env_pts: [(t, amp)]; scale shifts formants
    (larger creature = lower)."""
    n = secs(dur)
    f0 = ramp(n, f0_pts)
    if fry > 0:  # creaky irregularity
        f0 = f0 * (1 + fry * smooth_random(n, r, 25, -1, 1))
    src = glottal(f0, r, jitter, shimmer, open_q)
    src = src / (np.std(src) + 1e-9)
    src += breath * hp(white(n, r), 400) * 1.5
    tracks = []
    for i in range(3):
        tracks.append(ramp(n, [(t, VOWELS[v][i] * scale) for t, v in vowel_pts]))
    y = formants_tv(src, tracks, list(q), [1.0, 0.55, 0.3])
    env = ramp(n, env_pts) if env_pts else adsr(n, 0.03, 0.1, 0, 0.2, 0.8)
    return y * env


def whisper_voice(dur, r, scale=1.0, rate=7.0, density=0.8):
    """Unintelligible whisper: unvoiced noise through a random vowel/fricative sequence."""
    n = secs(dur)
    out = np.zeros(n)
    t = r.uniform(0.0, 0.15)
    while t < dur - 0.1:
        seg = r.uniform(0.6, 1.4) / rate
        m = secs(seg)
        if r.random() < density:
            nz = white(m, r)
            if r.random() < 0.3:  # fricative s / sh
                lo = r.choice([2500, 4000])
                s = bp(nz, lo, lo * 2.2) * 0.5
            else:
                v = VOWELS[r.choice(list("aeiou@"))]
                s = formants(nz, [f * scale for f in v], [120, 160, 220], [1, 0.7, 0.45])
                s += 0.15 * bp(nz, 3000, 7000)
            w = np.hanning(m) ** 0.7
            out = place(out, s * w * r.uniform(0.4, 1.0), secs(t))
        t += seg * r.uniform(0.7, 1.0)
    # phrase-level swell; whispers carry no chest resonance
    return hp(out, 280, 2) * (0.5 + 0.5 * smooth_random(n, r, 1.5, 0.2, 1.0))


def thump(n, f_start, f_end, tau, r, click=0.3, glide=0.05):
    """Pitch-dropping low thump with a soft noise click."""
    t = np.arange(n) / SR
    f = f_end + (f_start - f_end) * np.exp(-t / glide)
    y = osc(f) * exp_decay(n, tau, 0.002)
    c = lp(white(n, r), 1800) * exp_decay(n, 0.008, 0.0005)
    return fade(y + click * c, 0.0, min(0.05, n / SR / 4))


def crackles(n, r, rate, lo=900, hi=6000, decay=0.004, density_env=None, amp=1.0):
    """Sparse ember pops: filtered impulses with tiny ringing decays."""
    y = np.zeros(n)
    count = int(rate * n / SR)
    for _ in range(count):
        at = r.integers(0, n)
        if density_env is not None and r.random() > density_env[at]:
            continue
        m = secs(r.uniform(0.004, 0.03))
        f = r.uniform(lo, hi)
        pop = white(m, r) * np.exp(-np.arange(m) / (decay * SR * r.uniform(0.5, 2)))
        pop = resonator(pop, f, r.uniform(2, 8))
        y = place(y, pop, at, amp * r.uniform(0.2, 1.0) ** 2)
    return y


def crackles_loop(n, r, rate, lo=900, hi=6000, decay=0.004, amp=1.0):
    y = np.zeros(n)
    for _ in range(int(rate * n / SR)):
        m = secs(r.uniform(0.004, 0.03))
        f = r.uniform(lo, hi)
        pop = white(m, r) * np.exp(-np.arange(m) / (decay * SR * r.uniform(0.5, 2)))
        pop = resonator(pop, f, r.uniform(2, 8))
        y = place_wrap(y, pop, r.integers(0, n), amp * r.uniform(0.15, 1.0) ** 2)
    return y


def bell(n, f0, r, muted=0.5, minor=True):
    """Church-bell partials (hum, prime, tierce, quint, nominal...), damped by `muted`."""
    ratios = [0.5, 1.0, 1.19 if minor else 1.26, 1.5, 2.0, 2.52, 2.66, 3.01, 4.07]
    decays = [5.0, 3.2, 2.4, 1.8, 1.5, 0.9, 0.8, 0.6, 0.35]
    amps = [0.8, 1.0, 0.7, 0.4, 0.55, 0.25, 0.2, 0.15, 0.1]
    decays = [d * (1 - 0.6 * muted) for d in decays]
    amps = [a * (1 - muted * min(1, k / 5)) for k, a in enumerate(amps)]
    y = modal(n, f0, ratios, decays, amps, r, detune=0.002)
    # beating doublets for a struck-metal shimmer
    y += 0.3 * modal(n, f0 * 1.003, ratios[:4], decays[:4], amps[:4], r)
    strike = lp(white(n, r), 3000) * exp_decay(n, 0.01, 0.0005)
    return fade(y + 0.2 * strike, 0.0, min(0.3, n / SR / 4))


def stone_strike(n, f0, r, bright=0.5):
    """Struck stone: dense inharmonic modes, fast decays, gritty attack."""
    ratios = [1.0, 1.73, 2.41, 2.98, 3.88, 4.95, 6.1]
    decays = [0.35, 0.22, 0.16, 0.12, 0.08, 0.06, 0.04]
    amps = [1.0, 0.7, 0.5 * bright + 0.2, 0.4, 0.3 * bright, 0.25, 0.15]
    y = modal(n, f0, ratios, decays, amps, r, detune=0.01)
    grit = bp(white(n, r), 800, 5000) * exp_decay(n, 0.012, 0.0005)
    return fade(y + 0.35 * grit, 0.0, min(0.1, n / SR / 4))


def metal_clank(n, f0, r, decay=0.25, amp_noise=0.4):
    """Armour plate: inharmonic metallic modes + scrape noise."""
    ratios = [1.0, 1.47, 2.09, 2.56, 3.39, 4.17, 5.43, 6.8]
    decays = [decay * k for k in [1.0, 0.8, 0.7, 0.55, 0.45, 0.35, 0.3, 0.2]]
    amps = [0.7, 1.0, 0.8, 0.6, 0.5, 0.4, 0.3, 0.2]
    y = modal(n, f0, ratios, decays, amps, r, detune=0.02)
    y += amp_noise * bp(white(n, r), 2000, 9000) * exp_decay(n, 0.01, 0.0003)
    return fade(y, 0.0, min(0.1, n / SR / 4))


def body_fall(n, r, weight=1.0):
    y = thump(n, 110 * (1.3 - 0.3 * weight), 45, 0.12 * weight + 0.05, r, click=0.6)
    y += 0.6 * lp(white(n, r), 600) * exp_decay(n, 0.06 * weight)
    return saturate(y, 1.5)


def swish(n, r, f_lo=500, f_hi=3500):
    """Blade/limb swing: band-passed noise sweep with a bell-shaped envelope."""
    env = np.hanning(n) ** 2
    cut = f_lo + (f_hi - f_lo) * np.sin(np.linspace(0, np.pi, n))
    return svf(white(n, r), cut, 2.0, "bp") * env


def room(r, size="small"):
    if size == "small":
        return reverb_ir(0.5, r, 0.12, 0.05)
    if size == "medium":
        return reverb_ir(1.4, r, 0.4, 0.15)
    if size == "cave":
        return reverb_ir(2.5, r, 0.7, 0.25, predelay=0.02)
    return reverb_ir(5.0, r, 1.4, 0.5, predelay=0.03)  # vast


# ------------------------------------------------------------------ gaze


def gaze_open():
    r = rng(101)
    dur = 7.0
    n = secs(dur)
    t = np.arange(n) / SR
    # pressure swell: brown noise, low-pass opening upward as the lid lifts
    swell_env = ramp(n, [(0, 0), (0.4, 0.05), (3.4, 1.0), (3.65, 0.5), (5.5, 0.2), (dur, 0)]) ** 1.5
    cut = ramp(n, [(0, 70), (3.4, 900), (3.7, 300), (dur, 120)])
    swell = svf(brown(n, r), cut, 1.2, "lp") * swell_env
    # suction: a breath drawn in by the sky
    inhale = svf(pink(n, r), ramp(n, [(0, 400), (3.4, 2600), (3.6, 600)]), 3.0, "bp")
    inhale *= ramp(n, [(0, 0), (1.0, 0.0), (3.3, 0.35), (3.5, 0.0)])
    # sub boom at the moment it opens
    boom = np.zeros(n)
    bt = 3.45
    b = thump(secs(3.0), 70, 31, 1.2, r, click=0.15, glide=0.25)
    b = saturate(b * 1.4, 1.8)
    place(boom, b, secs(bt))
    # inharmonic choir shimmer, swelling after the boom
    ch_env = ramp(n, [(0, 0), (2.4, 0), (3.6, 0.9), (5.0, 0.6), (dur, 0)])
    sh = choir(n, 92.5, [1, 2.03, 2.97, 4.12, 5.41, 6.83, 8.37], r, voices=3, vowel="o", env=ch_env)
    hi = choir(n, 370, [1, 1.414, 1.97, 2.71], r, voices=4, vowel="i", spread=0.01, env=ch_env ** 2)
    y = 0.9 * swell + 0.5 * inhale + 1.2 * boom + 0.55 * sh + 0.18 * hi
    y = reverb(y, room(r, "vast"), wet=0.45)
    return y[:secs(dur + 2.5)], "sfx"


def gaze_close():
    r = rng(102)
    dur = 4.0
    n = secs(dur)
    # long exhale: noise band sweeping down, settling
    exhale = svf(pink(n, r), ramp(n, [(0, 1800), (1.5, 700), (dur, 220)]), 2.0, "bp")
    exhale *= ramp(n, [(0, 0), (0.25, 1.0), (1.6, 0.6), (dur, 0)])
    # the pressure releasing: low drone gliding down and fading
    f = ramp(n, [(0, 62), (dur, 41)])
    drone = osc(f) + 0.4 * osc(f * 2.01) + 0.2 * osc(f * 3.03)
    drone = saturate(drone * 0.8, 1.5) * ramp(n, [(0, 0), (0.3, 0.7), (dur, 0)]) ** 1.3
    sh = choir(n, 185, [1, 2.03, 2.97, 4.12], r, voices=2, vowel="u",
               env=ramp(n, [(0, 0.5), (1.5, 0.15), (3, 0)]))
    y = 0.8 * exhale + 0.7 * drone + 0.3 * sh
    y = reverb(y, room(r, "vast"), wet=0.35)
    return y, "sfx"


def gaze_spot_enter():
    r = rng(103)
    dur = 2.2
    n = secs(dur)
    pressure = thump(n, 60, 38, 0.6, r, click=0.0, glide=0.2)
    wh = whisper_voice(dur, r, scale=1.1, rate=9)
    wh = hp(wh, 600) * ramp(n, [(0, 0), (0.25, 1.0), (1.4, 0.3), (dur, 0)])
    tone = (osc(ramp(n, [(0, 1480), (dur, 1520)])) + 0.6 * osc(ramp(n, [(0, 1567), (dur, 1540)])))
    tone *= ramp(n, [(0, 0), (0.15, 0.25), (1.8, 0.1), (dur, 0)])
    sting = hp(white(n, r), 3000) * ramp(n, [(0, 0), (0.12, 0.5), (0.5, 0)])
    y = 0.8 * pressure + 0.55 * wh + 0.12 * tone + 0.08 * sting
    y = reverb(y, room(r, "large"), wet=0.4)
    return y, "sfx"


# ------------------------------------------------------------------ strain


def strain_heartbeat():
    r = rng(201)
    dur = 0.9
    n = secs(dur)
    y = np.zeros(n)
    lub = thump(secs(0.35), 75, 42, 0.07, r, click=0.25, glide=0.02)
    dub = thump(secs(0.35), 90, 50, 0.055, r, click=0.2, glide=0.015)
    place(y, lub, 0)
    place(y, 0.75 * dub, secs(0.26))
    # body resonance and a little drive so it reads on small speakers
    y = saturate(y * 1.6, 2.2)
    y = lp(y, 900)
    y = reverb(y, room(r, "small"), wet=0.15, tail=False)
    return fade(y, 0.0, 0.08), "sfx"


def strain_breath():
    r = rng(202)
    dur = 2.4
    n = secs(dur)
    nz = pink(n, r)
    # inhale: tight, high, with a catch
    inh = formants(nz, [900, 1700, 3100], [300, 400, 600], [0.7, 1.0, 0.6]) + 0.3 * hp(nz, 4000)
    inh *= ramp(n, [(0, 0), (0.1, 0.4), (0.75, 0.9), (0.85, 0.0), (dur, 0)])
    # exhale: lower, shaky (tremor), with a bit of strained voicing
    tremor = 1 + 0.35 * np.sin(2 * np.pi * 7.5 * np.arange(n) / SR) * smooth_random(n, r, 4, 0.3, 1)
    exh = formants(nz, [600, 1200, 2500], [250, 350, 500], [1.0, 0.7, 0.4])
    exh *= ramp(n, [(0, 0), (1.0, 0.0), (1.12, 1.0), (1.9, 0.5), (dur, 0)]) * tremor
    voiced = vox(dur, [(0, 118), (dur, 96)], [(0, "@"), (dur, "u")], r, breath=0.6, fry=0.08,
                 env_pts=[(0, 0), (1.0, 0), (1.15, 0.25), (1.6, 0.05), (dur, 0)], open_q=0.4)
    y = 0.9 * inh + exh + 0.6 * voiced
    y = reverb(y, room(r, "small"), wet=0.2)
    return y, "sfx"


def strain_whisper():
    r = rng(203)
    dur = 3.4
    n = secs(dur)
    y = np.zeros(n)
    for k, sc in enumerate([0.9, 1.0, 1.15, 0.8, 1.25]):
        v = whisper_voice(dur - 0.3, r, scale=sc, rate=r.uniform(6, 9), density=0.75)
        place(y, v * r.uniform(0.5, 1.0), secs(r.uniform(0, 0.3)))
    y *= ramp(n, [(0, 0), (0.4, 1), (2.6, 0.8), (dur, 0)])
    y = reverb(y, room(r, "medium"), wet=0.35)
    return y, "sfx"


def strain_overwhelm():
    r = rng(204)
    dur = 4.5
    n = secs(dur)
    t = np.arange(n) / SR
    ring = osc(ramp(n, [(0, 6100), (dur, 6180)])) + 0.5 * osc(7930.0, n) + 0.3 * osc(5230.0, n)
    ring *= ramp(n, [(0, 0), (0.08, 1.0), (2.8, 0.7), (dur, 0)]) * (1 + 0.15 * np.sin(2 * np.pi * 3.1 * t))
    crush = svf(brown(n, r), ramp(n, [(0, 400), (0.5, 120), (dur, 60)]), 1.5, "lp")
    crush = saturate(crush * 2.5, 3.0) * ramp(n, [(0, 0), (0.05, 1.0), (1.0, 0.7), (dur, 0)])
    sub = osc(ramp(n, [(0, 48), (dur, 34)])) * ramp(n, [(0, 0), (0.1, 1.0), (dur, 0)])
    # muffled, everything else pushed away: a dull heartbeat under it
    hb = np.zeros(n)
    beat = lp(thump(secs(0.3), 70, 40, 0.07, r, click=0.1), 300)
    for bt in np.arange(0.2, dur - 0.4, 0.62):
        place(hb, beat, secs(bt))
        place(hb, 0.7 * beat, secs(bt + 0.22))
    y = 0.12 * ring + 0.8 * crush + 0.6 * sub + 0.9 * hb
    y = reverb(y, room(r, "medium"), wet=0.25)
    return y, "sfx"


# ------------------------------------------------------------------ cairn


def cairn_kindle():
    r = rng(301)
    dur = 2.8
    n = secs(dur)
    # whoomph: low noise rush opening, a body of air catching
    env = ramp(n, [(0, 0), (0.08, 1.0), (0.35, 0.6), (1.5, 0.15), (dur, 0)])
    rush = svf(pink(n, r), ramp(n, [(0, 150), (0.12, 2400), (0.6, 700), (dur, 400)]), 1.0, "lp") * env
    body = thump(n, 55, 80, 0.25, r, click=0.0, glide=0.08) * 0.6
    dens = ramp(n, [(0, 0), (0.1, 1), (1.0, 0.6), (dur, 0.15)])
    cr = crackles(n, r, 70, density_env=dens, amp=1.2)
    y = rush + body + 0.8 * cr
    y = saturate(y, 1.3)
    y = reverb(y, room(r, "medium"), wet=0.25)
    return y, "sfx"


def cairn_rest():
    r = rng(302)
    dur = 3.6
    n = secs(dur)
    env = ramp(n, [(0, 0), (0.8, 1.0), (2.2, 0.7), (dur, 0)])
    # warm low dyad (no bright third), slowly beating
    drone = np.zeros(n)
    for f, a in [(98, 1.0), (98.6, 0.7), (147, 0.55), (196.4, 0.3), (293.5, 0.12)]:
        drone += a * osc(f * (1 + 0.002 * smooth_random(n, r, 0.5, -1, 1)), phase=r.uniform(0, 6))
    drone = lp(drone, 700) * env
    sigh = svf(pink(n, r), ramp(n, [(0, 900), (dur, 300)]), 1.5, "bp")
    sigh *= ramp(n, [(0, 0), (0.3, 0.5), (1.8, 0.2), (dur, 0)])
    cr = crackles(n, r, 18, amp=0.5, density_env=ramp(n, [(0, 0.3), (dur, 1)]))
    y = 0.7 * drone + 0.45 * sigh + 0.35 * cr
    y = reverb(y, room(r, "medium"), wet=0.3)
    return y, "sfx"


def loop_cairn_fire():
    r = rng(303)
    dur = 6.0
    n = secs(dur)
    flicker = 0.7 + 0.15 * periodic_lfo(n, r, [3, 7, 13, 29], [1, 0.7, 0.5, 0.4])
    roar = periodic_filter(brown(n, r), lambda x: lp(x, 380, 2)) * flicker
    hiss = periodic_filter(white(n, r), lambda x: bp(x, 2500, 7000)) * (0.6 + 0.3 * periodic_lfo(n, r, [11, 23]))
    cr = crackles_loop(n, r, 22, amp=1.0)
    pops = crackles_loop(n, r, 3, lo=300, hi=1200, decay=0.012, amp=1.5)
    y = 0.6 * roar + 0.05 * hiss + 0.7 * cr + 0.6 * pops
    y = reverb_circular(y, room(r, "small"), wet=0.2)
    return y, "loop"


# ------------------------------------------------------------------ quests + ui


def quest_accept():
    r = rng(401)
    dur = 2.2
    n = secs(dur)
    y = stone_strike(n, 196, r, bright=0.4)
    y += 0.4 * bell(n, 98, r, muted=0.85)
    y = lp(y, 3500)
    y = reverb(y, room(r, "large"), wet=0.35)
    return y, "sfx"


def quest_progress():
    r = rng(402)
    dur = 1.4
    n = secs(dur)
    y = stone_strike(n, 294, r, bright=0.3)
    y = lp(y, 3000)
    y = reverb(y, room(r, "medium"), wet=0.3)
    return y, "sfx"


def quest_complete():
    r = rng(403)
    dur = 4.5
    n = secs(dur)
    y = bell(n, 146.8, r, muted=0.6, minor=True)
    y2 = np.zeros(n)
    place(y2, stone_strike(secs(1.5), 73.4, r, bright=0.2), secs(0.02))
    place(y2, 0.5 * bell(secs(3.5), 110, r, muted=0.75, minor=True), secs(0.55))
    y = lp(y + 0.8 * y2, 4000)
    y = reverb(y, room(r, "large"), wet=0.4)
    return y, "sfx"


def dialogue_open():
    r = rng(404)
    dur = 0.7
    n = secs(dur)
    # cloth: soft broadband swish
    cloth = svf(pink(n, r), ramp(n, [(0, 600), (0.25, 2200), (dur, 900)]), 0.9, "bp")
    cloth *= ramp(n, [(0, 0), (0.08, 0.8), (0.3, 0.4), (dur, 0)])
    # paper: crinkle grains
    grains = np.zeros(n)
    for _ in range(45):
        at = int(r.beta(1.5, 3) * n * 0.8)
        m = secs(r.uniform(0.002, 0.012))
        g = bp(white(m, r), r.uniform(1500, 4000), 9000) * np.hanning(m)
        place(grains, g, at, r.uniform(0.1, 0.6))
    y = cloth + 0.5 * grains
    y = reverb(y, room(r, "small"), wet=0.15)
    return y, "sfx"


def title_sting():
    r = rng(405)
    dur = 7.0
    n = secs(dur)
    drone_env = ramp(n, [(0, 0), (1.5, 0.8), (4.5, 0.6), (dur, 0)])
    f = 55 * (1 + 0.003 * smooth_random(n, r, 0.3, -1, 1))
    drone = osc(f) + 0.5 * osc(f * 1.5) + 0.35 * osc(f * 2.002) + 0.2 * osc(f * 2.99)
    drone = lp(saturate(drone * 0.6, 1.4), 600) * drone_env
    toll = np.zeros(n)
    place(toll, bell(secs(5.5), 73.4, r, muted=0.3), secs(1.2))
    sh = choir(n, 110, [1, 2.03, 2.97, 4.12, 5.41], r, voices=3, vowel="o",
               env=ramp(n, [(0, 0), (1.2, 0), (2.5, 0.6), (dur, 0)]))
    wind = svf(pink(n, r), 400 + 300 * smooth_random(n, r, 0.5), 1.5, "bp") * drone_env * 0.4
    y = drone + 0.9 * toll + 0.35 * sh + wind
    y = reverb(y, room(r, "vast"), wet=0.4)
    return y, "sfx"


def end_sting():
    r = rng(406)
    dur = 9.0
    n = secs(dur)
    env = ramp(n, [(0, 0), (1.0, 0.7), (6.0, 0.5), (dur, 0)])
    f = 41.2
    drone = osc(f) + 0.6 * osc(f * 1.5 + 0.15) + 0.4 * osc(f * 2.003) + 0.15 * osc(f * 4.01)
    drone = lp(saturate(drone * 0.7, 1.5), 500) * env
    tolls = np.zeros(n)
    place(tolls, bell(secs(6), 61.7, r, muted=0.35), secs(0.3))
    place(tolls, 0.7 * bell(secs(5), 61.7, r, muted=0.5), secs(3.8))
    wind = svf(pink(n, r), 300 + 400 * smooth_random(n, r, 0.4), 1.2, "bp") * env * 0.35
    y = drone + tolls + wind
    y = reverb(y, room(r, "vast"), wet=0.45)
    return y, "sfx"


def hit_heavy():
    r = rng(407)
    dur = 0.8
    n = secs(dur)
    body = thump(n, 120, 48, 0.12, r, click=0.0, glide=0.025)
    crunch = bp(white(n, r), 300, 2200) * exp_decay(n, 0.035, 0.0005)
    flesh = lp(white(n, r), 700) * exp_decay(n, 0.07, 0.001)
    snap = hp(white(n, r), 3000) * exp_decay(n, 0.004, 0.0002)
    y = 1.3 * body + 0.8 * crunch + 0.6 * flesh + 0.3 * snap
    y = saturate(y * 1.5, 2.5)
    y = reverb(y, room(r, "small"), wet=0.15)
    return y, "sfx"


# ------------------------------------------------------------------ ambience loops


def amb_open_sky():
    r = rng(501)
    dur = 10.0
    n = secs(dur)
    gust = 0.55 + 0.4 * periodic_lfo(n, r, [1, 2, 3, 5], [1, 0.6, 0.4, 0.3]) / 1.6
    cut = 380 + 520 * (gust - 0.25)
    base = pink(n, r)
    wind = periodic_filter(base, lambda x: svf(x, np.tile(cut, 3), 1.8, "bp"))
    low = periodic_filter(brown(n, r), lambda x: lp(x, 160))
    # higher whistle through grass/stone edges
    whistle_cut = 1300 + 500 * periodic_lfo(n, r, [2, 3], [1, 0.5]) / 1.5
    whistle = periodic_filter(white(n, r), lambda x: svf(x, np.tile(whistle_cut, 3), 12.0, "bp"))
    # the faint high, uneasy tone: two near-dissonant partials (integer cycles over the loop)
    t = np.arange(n) / SR
    fq = lambda f: round(f * dur) / dur
    tone_env = 0.6 + 0.4 * periodic_lfo(n, r, [1, 2], [1, 0.5]) / 1.5
    tone = (np.sin(2 * np.pi * fq(1174.7) * t) + 0.7 * np.sin(2 * np.pi * fq(1244.5) * t)
            + 0.3 * np.sin(2 * np.pi * fq(1661.2) * t)) * tone_env
    y = 0.9 * wind * gust + 0.45 * low * gust + 0.12 * whistle * gust + 0.012 * tone
    y = reverb_circular(y, room(r, "large"), wet=0.25)
    return y, "loop"


def amb_shelter():
    r = rng(502)
    dur = 10.0
    n = secs(dur)
    # muffled wind beyond the rock
    muff = periodic_filter(brown(n, r), lambda x: lp(x, 220, 2)) * (0.8 + 0.2 * periodic_lfo(n, r, [1, 3]))
    room_tone = periodic_filter(pink(n, r), lambda x: bp(x, 150, 600)) * 0.25
    # drips: rising little plinks into a pool
    drips = np.zeros(n)
    drip_times = np.sort(r.uniform(0, dur, 9))
    for dt in drip_times:
        m = secs(0.25)
        f = r.uniform(900, 1600)
        tt = np.arange(m) / SR
        plink = np.sin(2 * np.pi * np.cumsum(f * (1 + 0.6 * (1 - np.exp(-tt / 0.02)))) / SR)
        plink *= np.exp(-tt / r.uniform(0.02, 0.05))
        place_wrap(drips, plink, secs(dt), r.uniform(0.3, 1.0))
    # creaks: stick-slip pulse trains through wood resonances
    creaks = np.zeros(n)
    for ct in r.uniform(0, dur, 3):
        cd = r.uniform(0.35, 0.8)
        m = secs(cd)
        rate = ramp(m, [(0, r.uniform(25, 40)), (cd * 0.5, r.uniform(60, 110)), (cd, r.uniform(20, 40))])
        ph = np.cumsum(rate) / SR
        pulses = (np.diff(np.floor(ph), prepend=0) > 0).astype(float)
        c = formants(pulses, [r.uniform(250, 400), r.uniform(700, 1100), 2200], [40, 80, 200], [1, 0.6, 0.2])
        c *= np.hanning(m)
        place_wrap(creaks, c, secs(ct), r.uniform(0.4, 0.8))
    y = muff + room_tone + 0.12 * drips + 0.6 * creaks
    y = reverb_circular(y, room(r, "cave"), wet=0.4)
    return y, "loop"


def amb_eye_open():
    r = rng(503)
    dur = 10.0
    n = secs(dur)
    t = np.arange(n) / SR
    fq = lambda f: round(f * dur) / dur
    # slow pulse: 5 throbs per loop (every 2 s)
    pulse = 0.55 + 0.45 * np.sin(2 * np.pi * 5 * t / dur - np.pi / 2) ** 4
    drone = np.zeros(n)
    for f, a in [(36.7, 1.0), (37.0, 0.8), (55.0, 0.6), (73.6, 0.35), (77.8, 0.25), (110.1, 0.15)]:
        drone += a * np.sin(2 * np.pi * fq(f) * t + r.uniform(0, 6))
    drone = saturate(drone * 0.7, 2.0)  # growl harmonics
    drone = periodic_filter(drone, lambda x: lp(x, 420, 2))
    # distant inharmonic choir under it, periodic partials
    shimmer = np.zeros(n)
    for ra, a in [(1, 1.0), (2.03, 0.6), (2.97, 0.4), (4.12, 0.3), (5.41, 0.2)]:
        f = fq(146.8 * ra)
        shimmer += a * np.sin(2 * np.pi * f * t + r.uniform(0, 6)) * (0.6 + 0.4 * periodic_lfo(n, r, [1, 2]) / 2)
    shimmer = periodic_filter(shimmer, lambda x: formants(x, list(VOWELS["o"]), [90, 120, 180]))
    air = periodic_filter(pink(n, r), lambda x: bp(x, 2000, 6000)) * (0.5 + 0.5 * pulse)
    pressure = periodic_filter(brown(n, r), lambda x: lp(x, 90))
    y = 0.8 * drone * pulse + 0.25 * shimmer + 0.04 * air + 0.5 * pressure * pulse
    y = reverb_circular(y, room(r, "large"), wet=0.3)
    return y, "loop"


# ------------------------------------------------------------------ npc voices


def _finish(y, r, size="small", wet=0.18):
    return reverb(y, room(r, size), wet=wet)


# glarewolf: lean, eye-turned wolf

def npc_glarewolf_aggro():
    r = rng(601)
    dur = 1.3
    n = secs(dur)
    growl = vox(dur, [(0, 85), (0.6, 95), (dur, 80)], [(0, "u"), (0.5, "o"), (dur, "@")], r,
                breath=0.5, fry=0.25, jitter=0.04, shimmer=0.3, scale=0.85, open_q=0.35,
                env_pts=[(0, 0), (0.1, 0.8), (0.9, 1.0), (1.0, 0.0), (dur, 0)])
    tt = np.arange(n) / SR
    growl *= 0.55 + 0.45 * np.abs(np.sin(np.pi * 26 * tt + 2 * smooth_random(n, r, 3, -1, 1)))
    snarl = bp(white(n, r), 1500, 5000) * ramp(n, [(0, 0), (0.85, 0.3), (0.95, 0.9), (1.15, 0)])
    bark = vox(dur, [(0, 380), (0.95, 380), (1.05, 520), (dur, 300)], [(0, "a"), (dur, "@")], r,
               breath=0.3, scale=0.9, env_pts=[(0, 0), (0.95, 0), (1.0, 1.0), (1.2, 0.0), (dur, 0)])
    y = growl + 0.25 * snarl + 0.9 * bark
    return _finish(saturate(y, 1.5), r), "sfx"


def npc_glarewolf_attack():
    r = rng(602)
    dur = 0.5
    n = secs(dur)
    snap = vox(dur, [(0, 420), (0.12, 330), (dur, 240)], [(0, "a"), (0.2, "@")], r, breath=0.5,
               scale=0.9, fry=0.1, env_pts=[(0, 0), (0.015, 1.0), (0.15, 0.4), (0.3, 0), (dur, 0)])
    jaw = lp(white(n, r), 2500) * exp_decay(n, 0.01, 0.0003)
    y = snap + 0.5 * jaw + 0.6 * swish(n, r, 800, 3000)
    return _finish(saturate(y, 1.4), r), "sfx"


def npc_glarewolf_hit():
    r = rng(603)
    dur = 0.45
    y = vox(dur, [(0, 780), (0.08, 900), (dur, 520)], [(0, "i"), (dur, "@")], r, breath=0.3,
            scale=0.95, env_pts=[(0, 0), (0.02, 1.0), (0.25, 0.5), (dur, 0)])
    return _finish(y, r), "sfx"


def npc_glarewolf_death():
    r = rng(604)
    dur = 1.6
    n = secs(dur)
    whimper = vox(dur, [(0, 640), (0.3, 560), (1.0, 300), (dur, 240)], [(0, "i"), (0.5, "u"), (dur, "u")],
                  r, breath=0.5, fry=0.1, scale=0.95,
                  env_pts=[(0, 0), (0.03, 1.0), (0.5, 0.6), (1.1, 0.15), (dur, 0)])
    fall = np.zeros(n)
    place(fall, body_fall(secs(0.5), r, 0.6), secs(0.55))
    y = whimper + 0.8 * fall
    return _finish(y, r), "sfx"


# the Stooped: hollowed lightworkers with sickles

def npc_stooped_aggro():
    r = rng(611)
    dur = 1.8
    y = vox(dur, [(0, 92), (0.8, 118), (dur, 88)], [(0, "u"), (0.9, "o"), (dur, "@")], r,
            breath=0.45, fry=0.2, jitter=0.03, scale=1.0, open_q=0.5,
            env_pts=[(0, 0), (0.3, 0.8), (1.2, 1.0), (dur, 0)])
    y = comb(y, 0.004, 0.35)  # hollow, throat-closed
    return _finish(y, r, "medium", 0.25), "sfx"


def npc_stooped_attack():
    r = rng(612)
    dur = 0.6
    n = secs(dur)
    grunt = vox(dur, [(0, 140), (dur, 100)], [(0, "@"), (dur, "u")], r, breath=0.5, fry=0.2,
                env_pts=[(0, 0), (0.03, 1.0), (0.18, 0.3), (0.3, 0), (dur, 0)])
    sickle = np.zeros(n)
    place(sickle, swish(secs(0.28), r, 900, 4500), secs(0.1))
    y = grunt + 0.4 * sickle
    return _finish(y, r), "sfx"


def npc_stooped_hit():
    r = rng(613)
    dur = 0.5
    y = vox(dur, [(0, 170), (0.1, 190), (dur, 120)], [(0, "a"), (dur, "@")], r, breath=0.8,
            fry=0.25, env_pts=[(0, 0), (0.02, 1.0), (0.2, 0.3), (dur, 0)], open_q=0.4)
    return _finish(y, r), "sfx"


def npc_stooped_death():
    r = rng(614)
    dur = 2.2
    n = secs(dur)
    rattle = vox(dur, [(0, 120), (1.2, 80), (dur, 60)], [(0, "a"), (1.0, "@"), (dur, "u")], r,
                 breath=0.9, fry=0.45, jitter=0.05, shimmer=0.4, open_q=0.3,
                 env_pts=[(0, 0), (0.05, 1.0), (1.0, 0.5), (1.8, 0.1), (dur, 0)])
    fall = np.zeros(n)
    place(fall, body_fall(secs(0.5), r, 0.7), secs(0.9))
    tinkle = np.zeros(n)
    place(tinkle, metal_clank(secs(0.5), 820, r, decay=0.12), secs(1.0))  # dropped sickle
    y = rattle + fall + 0.25 * tinkle
    return _finish(y, r), "sfx"


# Hollowed Warden: plate armour with something hollow inside

def _groan(dur, f0_pts, r, env_pts):
    y = vox(dur, f0_pts, [(0, "o"), (dur * 0.6, "u"), (dur, "@")], r, breath=0.35, fry=0.18,
            jitter=0.02, scale=0.78, open_q=0.5, env_pts=env_pts)
    y = comb(y, 0.0031, 0.55)  # helmet resonance
    return saturate(y * 1.2, 1.6)


def npc_hollowed_warden_aggro():
    r = rng(621)
    dur = 2.0
    n = secs(dur)
    g = _groan(dur, [(0, 62), (0.9, 74), (dur, 58)], r, [(0, 0), (0.3, 0.9), (1.4, 1.0), (dur, 0)])
    cl = np.zeros(n)
    place(cl, metal_clank(secs(0.6), 310, r, 0.3), secs(0.05))
    place(cl, 0.6 * metal_clank(secs(0.5), 440, r, 0.2), secs(0.22))
    y = g + 0.5 * cl
    return _finish(y, r, "medium", 0.25), "sfx"


def npc_hollowed_warden_attack():
    r = rng(622)
    dur = 0.9
    n = secs(dur)
    sw = np.zeros(n)
    place(sw, swish(secs(0.4), r, 300, 1800), 0)
    g = _groan(dur, [(0, 70), (dur, 60)], r, [(0, 0), (0.1, 0.0), (0.2, 0.8), (0.5, 0.2), (dur, 0)])
    cl = np.zeros(n)
    place(cl, metal_clank(secs(0.5), 260, r, 0.2), secs(0.02))
    y = 1.0 * sw + 0.7 * g + 0.35 * cl
    return _finish(y, r, "small", 0.2), "sfx"


def npc_hollowed_warden_hit():
    r = rng(623)
    dur = 0.9
    n = secs(dur)
    clang = metal_clank(n, 380, r, 0.35, amp_noise=0.6)
    g = _groan(dur, [(0, 80), (dur, 66)], r, [(0, 0), (0.05, 0.0), (0.12, 0.6), (0.6, 0), (dur, 0)])
    y = 0.9 * clang + 0.6 * g
    return _finish(y, r, "small", 0.2), "sfx"


def npc_hollowed_warden_death():
    r = rng(624)
    dur = 3.4
    n = secs(dur)
    g = _groan(2.2, [(0, 70), (1.0, 55), (2.2, 40)], r, [(0, 0), (0.2, 1.0), (1.4, 0.5), (2.2, 0)])
    y = pad(g, n)
    # collapse of plate: knees, then the cascade, then settling pieces
    t = 1.1
    for k in range(9):
        f = r.uniform(240, 700)
        a = r.uniform(0.5, 1.0) * (1.0 if k < 4 else 0.5)
        place(y, a * metal_clank(secs(0.6), f, r, r.uniform(0.15, 0.35)), secs(t))
        t += r.uniform(0.05, 0.22) * (1 + k * 0.15)
    place(y, 1.2 * body_fall(secs(0.7), r, 1.2), secs(1.25))
    place(y, 0.8 * body_fall(secs(0.6), r, 1.0), secs(1.55))
    place(y, 0.4 * metal_clank(secs(0.8), 520, r, 0.5), secs(t + 0.2))  # a helmet rolling to rest
    return _finish(y, r, "medium", 0.25), "sfx"


# greets (non-verbal)

def npc_cairnkeeper_greet():
    r = rng(631)
    dur = 1.6
    # an old woman's closed-mouth hum, two notes falling
    y = vox(dur, [(0, 205), (0.55, 200), (0.65, 178), (dur, 172)], [(0, "m"), (dur, "m")], r,
            breath=0.25, jitter=0.02, shimmer=0.12, scale=1.15, q=(4.0, 3.0, 3.0),
            env_pts=[(0, 0), (0.12, 0.8), (0.55, 0.7), (0.65, 0.9), (1.3, 0.5), (dur, 0)])
    y = lp(y, 2200)
    return _finish(y, r), "sfx"


def npc_lightworker_greet():
    r = rng(632)
    dur = 1.5
    n = secs(dur)
    # a weary sigh: a little voice at the top, then breath
    voiced = vox(dur, [(0, 150), (0.35, 120), (dur, 95)], [(0, "a"), (0.4, "@"), (dur, "u")], r,
                 breath=0.9, fry=0.12, open_q=0.45,
                 env_pts=[(0, 0), (0.06, 0.7), (0.35, 0.25), (0.6, 0.0), (dur, 0)])
    breath = formants(pink(n, r), [600, 1250, 2500], [300, 400, 500])
    breath *= ramp(n, [(0, 0), (0.1, 0.6), (0.7, 0.5), (dur, 0)])
    y = voiced + 0.8 * breath
    return _finish(y, r), "sfx"


def npc_lowshade_guard_greet():
    r = rng(633)
    dur = 1.2
    n = secs(dur)
    y = np.zeros(n)
    # throat clearing: two short rough coughs, chesty
    for at, a in [(0.0, 1.0), (0.32, 0.7)]:
        c = vox(0.3, [(0, 135), (0.3, 105)], [(0, "a"), (0.3, "@")], r, breath=1.2, fry=0.35,
                open_q=0.35, scale=0.9, env_pts=[(0, 0), (0.01, 1.0), (0.12, 0.35), (0.3, 0)])
        burst = bp(white(secs(0.3), r), 300, 2500) * exp_decay(secs(0.3), 0.05, 0.002)
        place(y, a * (c + 0.5 * burst), secs(at))
    hm = vox(0.5, [(0, 110), (0.5, 100)], [(0, "m"), (0.5, "m")], r, breath=0.2, q=(4.0, 3.0, 3.0),
             env_pts=[(0, 0), (0.08, 0.6), (0.5, 0)])
    place(y, 0.7 * hm, secs(0.68))
    return _finish(y, r), "sfx"


# ------------------------------------------------------------------ footsteps


def _step(r, surface):
    n = secs(0.3)
    if surface == "dirt":
        th = thump(n, 140, 70, 0.025, r, click=0.1, glide=0.01)
        grit = bp(white(n, r), 600, 4500) * exp_decay(n, 0.03, 0.002)
        grit *= 1 + 0.8 * (r.random(n) < 0.01)
        y = 0.7 * th + 0.7 * grit
    else:
        th = thump(n, 180, 90, 0.02, r, click=0.4, glide=0.008)
        tap = stone_strike(n, r.uniform(900, 1300), r, 0.3) * exp_decay(n, 0.03)
        scuff = bp(white(n, r), 1500, 6000) * exp_decay(n, 0.02, 0.003)
        y = 0.6 * th + 0.3 * tap + 0.4 * scuff
    return _finish(y, r, "small", 0.12)


def make_steps():
    out = {}
    for surface, seed in [("dirt", 700), ("stone", 710)]:
        for i in range(1, 5):
            r = rng(seed + i)
            out[f"foot_{surface}_{i}"] = (lambda r=r, s=surface: (_step(r, s), "sfx"))
    return out


SOUNDS = {name: fn for name, fn in globals().items()
          if callable(fn) and not name.startswith("_") and (
              name.startswith(("gaze_", "strain_", "cairn_", "loop_", "quest_", "amb_", "npc_"))
              or name in ("dialogue_open", "title_sting", "end_sting", "hit_heavy"))}
SOUNDS.update(make_steps())

# Active-RMS loudness targets (dBFS); everything is also limited to -1 dBFS peak.
TARGETS = {
    "amb_": -24.0, "loop_": -24.0,
    "strain_whisper": -24.0, "strain_breath": -21.0, "gaze_spot_enter": -22.0,
    "foot_": -24.0, "dialogue_open": -22.0,
    "quest_progress": -20.0,
}
DEFAULT_TARGET = -18.0

# Longest allowed length (s) including the reverb tail; longer renders get a 0.6 s fade.
MAX_LEN = {
    "gaze_open": 8.0, "gaze_close": 5.0, "gaze_spot_enter": 3.0, "strain_overwhelm": 5.0,
    "quest_accept": 3.0, "quest_progress": 1.8, "quest_complete": 5.0, "title_sting": 7.5,
    "end_sting": 9.5, "cairn_kindle": 3.0, "cairn_rest": 4.0, "strain_whisper": 3.8,
    "foot_": 0.32,
}


def target_for(name):
    for k, v in TARGETS.items():
        if name == k or (k.endswith("_") and name.startswith(k)):
            return v
    return DEFAULT_TARGET
