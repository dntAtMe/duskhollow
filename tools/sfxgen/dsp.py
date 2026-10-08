"""Small DSP toolkit for the procedural sound generator (numpy + scipy).

Everything works on float64 arrays at SR. Loops are made periodic by construction:
sources have period L and every filter / reverb is applied to a tiled copy (or circularly),
so the last sample flows into the first one without a crossfade.
"""

import wave

import numpy as np
from scipy import signal

SR = 44100


# ------------------------------------------------------------------ basics


def secs(n):
    return int(round(n * SR))


def t_axis(dur):
    return np.arange(secs(dur)) / SR


def rng(seed):
    return np.random.default_rng(seed)


def white(n, r):
    return r.standard_normal(n)


def colored(n, r, slope_db_oct):
    """Noise with a spectral slope (-3 dB/oct pink, -6 brown). Periodic over n samples."""
    spec = np.fft.rfft(r.standard_normal(n))
    f = np.fft.rfftfreq(n, 1 / SR)
    f[0] = f[1]
    spec *= (f / 1000.0) ** (slope_db_oct / 6.0206)
    x = np.fft.irfft(spec, n)
    return x / (np.std(x) + 1e-12)


def pink(n, r):
    return colored(n, r, -3.0)


def brown(n, r):
    return colored(n, r, -6.0)


def pad(x, n):
    return np.concatenate([x, np.zeros(max(0, n - len(x)))])[:n]


def place(dst, src, at, gain=1.0):
    """Mixes src into dst starting at sample `at` (clipped)."""
    at = int(at)
    if at >= len(dst):
        return dst
    end = min(len(dst), at + len(src))
    s0 = max(0, -at)
    dst[max(at, 0):end] += gain * _tail_fade(src)[s0:end - at]
    return dst


def _tail_fade(src, n=None):
    """Short fade-out on the end of a segment so truncated decays do not click."""
    n = min(len(src), n or secs(0.004))
    if n < 2:
        return src
    src = src.copy()
    src[-n:] *= np.cos(np.linspace(0, np.pi / 2, n)) ** 2
    return src


def place_wrap(dst, src, at, gain=1.0):
    """Like place, but wraps around the end (for loops)."""
    n = len(dst)
    idx = (int(at) + np.arange(len(src))) % n
    np.add.at(dst, idx, gain * _tail_fade(src))
    return dst


# ------------------------------------------------------------------ envelopes


def adsr(n, a, d, s, r, sustain_level=0.7, curve=2.0):
    a, d, r = secs(a), secs(d), secs(r)
    sus = max(0, n - a - d - r)
    env = np.concatenate([
        np.linspace(0, 1, max(a, 1)) ** (1 / curve),
        1 - (1 - sustain_level) * (np.linspace(0, 1, max(d, 1)) ** (1 / curve)),
        np.full(sus, sustain_level),
        sustain_level * (1 - np.linspace(0, 1, max(r, 1))) ** curve,
    ])
    return pad(env, n)


def exp_decay(n, tau, attack=0.002):
    t = np.arange(n) / SR
    env = np.exp(-t / tau)
    a = secs(attack)
    if a > 1:
        env[:a] *= np.linspace(0, 1, a)
    return env


def ramp(n, points):
    """Piecewise linear envelope through (time_s, value) points."""
    t = np.arange(n) / SR
    ts, vs = zip(*points)
    return np.interp(t, ts, vs)


def smooth_random(n, r, rate_hz, lo=0.0, hi=1.0, periodic=False):
    """Slow random curve (interpolated random points, smoothed)."""
    k = max(2, int(n / SR * rate_hz) + 2)
    pts = r.uniform(lo, hi, k)
    if periodic:
        pts[-1] = pts[0]
    xs = np.linspace(0, n, k)
    y = np.interp(np.arange(n), xs, pts)
    # cosine-ish smoothing via a moving average of ~1/rate
    w = max(1, int(SR / rate_hz / 3))
    if periodic:
        y = np.real(np.fft.ifft(np.fft.fft(y) * np.fft.fft(np.ones(w) / w, n)))
    else:
        yp = np.pad(y, (w, w), mode="edge")
        c = np.cumsum(np.concatenate([[0.0], yp]))
        ma = (c[w:] - c[:-w]) / w  # moving average, len(yp) - w + 1
        y = ma[w // 2:w // 2 + len(y)]
    return y


def periodic_lfo(n, r, cycles_list, amps=None):
    """Sum of sinusoids with an integer number of cycles over n samples (loopable)."""
    t = np.arange(n) / n
    y = np.zeros(n)
    amps = amps or [1.0] * len(cycles_list)
    for c, a in zip(cycles_list, amps):
        y += a * np.sin(2 * np.pi * c * t + r.uniform(0, 2 * np.pi))
    return y


# ------------------------------------------------------------------ filters


def _sos(kind, f, order=2):
    nyq = SR / 2
    if kind in ("lowpass", "highpass"):
        return signal.butter(order, min(f / nyq, 0.999), btype=kind, output="sos")
    lo, hi = f
    return signal.butter(order, [max(lo / nyq, 1e-5), min(hi / nyq, 0.999)], btype="bandpass", output="sos")


def lp(x, f, order=2):
    return signal.sosfilt(_sos("lowpass", f, order), x)


def hp(x, f, order=2):
    return signal.sosfilt(_sos("highpass", f, order), x)


def bp(x, lo, hi, order=2):
    return signal.sosfilt(_sos("bandpass", (lo, hi), order), x)


def resonator(x, f, q):
    """Peaking band-pass (constant 0 dB peak gain)."""
    b, a = signal.iirpeak(min(f, SR / 2 * 0.98), q, fs=SR)
    return signal.lfilter(b, a, x)


def svf(x, cutoff, q=0.707, mode="lp"):
    """Time-varying TPT state-variable filter; cutoff may be an array (Hz)."""
    n = len(x)
    cutoff = np.broadcast_to(np.asarray(cutoff, dtype=float), (n,))
    q = np.broadcast_to(np.asarray(q, dtype=float), (n,))
    g = np.tan(np.pi * np.clip(cutoff, 10, SR * 0.45) / SR)
    k = 1.0 / q
    a1 = 1.0 / (1.0 + g * (g + k))
    a2 = g * a1
    a3 = g * a2
    out = np.empty(n)
    ic1 = ic2 = 0.0
    m = {"lp": 0, "bp": 1, "hp": 2}[mode]
    xl = x.tolist()
    a1l, a2l, a3l, kl = a1.tolist(), a2.tolist(), a3.tolist(), k.tolist()
    for i in range(n):
        v0 = xl[i]
        v3 = v0 - ic2
        v1 = a1l[i] * ic1 + a2l[i] * v3
        v2 = ic2 + a2l[i] * ic1 + a3l[i] * v3
        ic1 = 2 * v1 - ic1
        ic2 = 2 * v2 - ic2
        if m == 0:
            out[i] = v2
        elif m == 1:
            out[i] = v1 * kl[i]  # unity peak gain band-pass
        else:
            out[i] = v0 - kl[i] * v1 - v2
    return out


def formants(x, freqs, bws, gains=None):
    """Parallel formant bank (static)."""
    gains = gains or [1.0] * len(freqs)
    y = np.zeros_like(x)
    for f, bw, g in zip(freqs, bws, gains):
        y += g * resonator(x, f, f / bw)
    return y


def formants_tv(x, tracks, q_list, gains=None):
    """Time-varying parallel formants; tracks = list of per-sample frequency arrays."""
    gains = gains or [1.0] * len(tracks)
    y = np.zeros_like(x)
    for tr, q, g in zip(tracks, q_list, gains):
        y += g * svf(x, tr, q, "bp")
    return y


def comb(x, delay_s, fb):
    d = max(1, secs(delay_s))
    a = np.zeros(d + 1)
    a[0] = 1
    a[d] = -fb
    return signal.lfilter([1.0], a, x)


def periodic_filter(x, fn, reps=3):
    """Applies fn to a tiled copy and returns the middle period (seamless for loops)."""
    n = len(x)
    y = fn(np.tile(x, reps))
    return y[n * (reps // 2):n * (reps // 2 + 1)]


# ------------------------------------------------------------------ oscillators


def osc(freq, n=None, phase=0.0, shape="sine"):
    """Oscillator; freq may be a per-sample array (pitch drift)."""
    if np.isscalar(freq):
        freq = np.full(n, float(freq))
    ph = phase + 2 * np.pi * np.cumsum(freq) / SR
    if shape == "sine":
        return np.sin(ph)
    if shape == "saw":
        return 2 * ((ph / (2 * np.pi)) % 1.0) - 1
    if shape == "tri":
        return 2 * np.abs(2 * ((ph / (2 * np.pi)) % 1.0) - 1) - 1
    raise ValueError(shape)


def glottal(f0, r, jitter=0.01, shimmer=0.05, open_q=0.6):
    """Band-limited-ish glottal pulse train (Rosenberg-like) from a per-sample f0 curve."""
    n = len(f0)
    f0 = f0 * (1 + jitter * smooth_random(n, r, 30, -1, 1))
    ph = np.cumsum(f0) / SR
    frac = ph % 1.0
    pulse = np.where(frac < open_q, np.sin(np.pi * frac / open_q) ** 2, 0.0)
    d = np.diff(pulse, prepend=0.0) * SR / np.maximum(f0, 20) / 4  # derivative = excitation
    cyc = np.floor(ph).astype(int)
    amp = 1 + shimmer * r.standard_normal(cyc.max() + 2)[cyc]
    return lp(d * amp, 5000, 2)


def fm(carrier, ratio, index, n, index_env=None):
    """Two-operator FM; index_env scales the modulation index over time."""
    t = np.arange(n) / SR
    ie = index if index_env is None else index * index_env
    return np.sin(2 * np.pi * carrier * t + ie * np.sin(2 * np.pi * carrier * ratio * t))


def modal(n, f0, ratios, decays, amps, r=None, detune=0.0):
    """Struck object: sum of exponentially decaying inharmonic partials."""
    t = np.arange(n) / SR
    y = np.zeros(n)
    for k, (ra, dc, am) in enumerate(zip(ratios, decays, amps)):
        f = f0 * ra * (1 + (detune * r.standard_normal() if r is not None else 0))
        if f >= SR / 2 * 0.95:
            continue
        ph = r.uniform(0, 2 * np.pi) if r is not None else 0
        y += am * np.exp(-t / dc) * np.sin(2 * np.pi * f * t + ph)
    return y


# ------------------------------------------------------------------ space + colour


def reverb_ir(dur, r, decay_lo=None, decay_hi=None, predelay=0.01, density_lp=9000, early=8):
    """Synthetic room IR: filtered noise whose highs decay faster than its lows."""
    n = secs(dur)
    t = np.arange(n) / SR
    decay_lo = decay_lo or dur / 4
    decay_hi = decay_hi or dur / 10
    nz = r.standard_normal(n)
    low = lp(nz, 900) * np.exp(-t / decay_lo)
    high = hp(nz, 900) * np.exp(-t / decay_hi)
    ir = lp(low + 0.7 * high, density_lp)
    ir[:secs(0.004)] *= np.linspace(0, 1, secs(0.004))
    for _ in range(early):  # a few discrete early reflections
        at = secs(r.uniform(0.004, 0.05))
        ir[at] += r.uniform(-1, 1) * 3
    ir = np.concatenate([np.zeros(secs(predelay)), ir])
    return ir / np.sqrt(np.sum(ir ** 2))


def reverb(x, ir, wet=0.3, dry=1.0, tail=True):
    y = signal.fftconvolve(x, ir)
    if not tail:
        y = y[:len(x)]
    out = wet * y
    out[:len(x)] += dry * x
    return out


def reverb_circular(x, ir, wet=0.3, dry=1.0):
    """Circular convolution: the tail wraps around (seamless loops)."""
    n = len(x)
    ir = ir[:n]
    y = np.real(np.fft.ifft(np.fft.fft(x) * np.fft.fft(pad(ir, n))))
    return dry * x + wet * y


def saturate(x, drive=2.0):
    return np.tanh(drive * x) / np.tanh(drive)


def fade(x, fin=0.005, fout=0.02):
    x = x.copy()
    a, b = secs(fin), secs(fout)
    if a > 1:
        x[:a] *= np.sin(np.linspace(0, np.pi / 2, a)) ** 2
    if b > 1:
        x[-b:] *= np.cos(np.linspace(0, np.pi / 2, b)) ** 2
    return x


def trim_tail(x, thresh_db=-70):
    """Drops trailing near-silence (keeps a short fade)."""
    lvl = 10 ** (thresh_db / 20) * np.max(np.abs(x))
    idx = np.nonzero(np.abs(x) > lvl)[0]
    if len(idx) == 0:
        return x
    return fade(x[:idx[-1] + secs(0.02)], 0.0, 0.02)


def dc_block(x):
    return hp(x, 18, 2)


# ------------------------------------------------------------------ loudness + output


def rms_db(x):
    return 20 * np.log10(np.sqrt(np.mean(x ** 2)) + 1e-12)


def active_rms_db(x, win=0.05, top=0.5):
    """RMS of the loudest `top` fraction of 50 ms windows (ignores silence/tails)."""
    w = secs(win)
    k = max(1, len(x) // w)
    frames = x[:k * w].reshape(k, w)
    e = np.sort(np.mean(frames ** 2, axis=1))[::-1]
    m = max(1, int(k * top))
    return 10 * np.log10(np.mean(e[:m]) + 1e-12)


def master(x, target_db, ceiling_db=-1.0, loop=False):
    """DC removal, loudness to target (active RMS), soft-knee limiting to the ceiling."""
    if loop:
        x = x - np.mean(x)
    else:
        x = dc_block(x)
        x = x - np.mean(x)
    x = x * 10 ** ((target_db - active_rms_db(x)) / 20)
    ceil = 10 ** (ceiling_db / 20)
    knee = 0.7 * ceil
    a = np.abs(x)
    over = a > knee
    if np.any(over):
        # soft limiter: compress everything above the knee into [knee, ceil)
        y = knee + (ceil - knee) * np.tanh((a[over] - knee) / (ceil - knee))
        x = x.copy()
        x[over] = np.sign(x[over]) * y
    return x


def write_wav(path, x):
    """16-bit PCM WAV; x is (n,) mono or (n, 2) stereo in [-1, 1]."""
    x = np.asarray(x)
    ch = 1 if x.ndim == 1 else x.shape[1]
    pcm = np.clip(np.round(x * 32767), -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(ch)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def read_wav(path):
    with wave.open(str(path), "rb") as w:
        ch, n = w.getnchannels(), w.getnframes()
        x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64) / 32768
    return x.reshape(-1, ch) if ch > 1 else x
