"""Procedural sound generator for Duskhollow.

    python -I tools/sfxgen/sfxgen.py                 # all sounds
    python -I tools/sfxgen/sfxgen.py gaze_open amb_  # names or prefixes
    python -I tools/sfxgen/sfxgen.py --check         # analyse existing files only
    python -I tools/sfxgen/sfxgen.py --preview       # also write spectrograms

Writes 16-bit PCM mono WAVs at 44.1 kHz to custom_assets/content/sfx/ and
spectrograms to custom_assets/preview/sfx/ (gitignored). Designs live in sounds.py (ambience, gaze, story cues,
footsteps, Duskhollow voices) and sounds_game.py (combat, UI, spell kits, glade creatures).
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import numpy as np  # noqa: E402

from dsp import SR, active_rms_db, fade, hp, master, periodic_filter, read_wav, secs, trim_tail, write_wav  # noqa: E402
from sounds import MAX_LEN, SOUNDS, TARGETS, target_for  # noqa: E402
import sounds_game  # noqa: E402

# game sounds (combat, UI, spell kits, glade creatures) take precedence in the per-name tables
SOUNDS.update(sounds_game.SOUNDS)
MAX_LEN.update(sounds_game.MAX_LEN)
TARGETS.update(sounds_game.TARGETS)

ROOT = HERE.parent.parent
OUT = ROOT / "custom_assets/content/sfx"
PREVIEW = ROOT / "custom_assets/preview/sfx"
BUDGET = 16 * 1024 * 1024


def analyse(name, x, loop):
    """Returns (report line, problems)."""
    mono = x if x.ndim == 1 else x.mean(axis=1)
    peak = 20 * np.log10(np.max(np.abs(x)) + 1e-12)
    dc = float(np.mean(mono))
    act = active_rms_db(mono)
    d = np.diff(mono)
    typical = np.percentile(np.abs(d), 99.9) + 1e-9
    problems = []
    if peak > -0.99:
        problems.append(f"peak {peak:.2f} dBFS")
    if abs(dc) > 1e-3:
        problems.append(f"DC {dc:.4f}")
    seam = ""
    if loop:
        jump = abs(mono[0] - mono[-1])
        seam = f" seam {jump / typical:.2f}x"
        if jump > 1.5 * typical:
            problems.append(f"loop seam jump {jump:.4f} (p99.9 step {typical:.4f})")
        # periodicity of the spectrum across the seam: energy just before vs just after
        w = SR // 20
        e1, e2 = np.sqrt(np.mean(mono[-w:] ** 2)), np.sqrt(np.mean(mono[:w] ** 2))
        if max(e1, e2) / (min(e1, e2) + 1e-9) > 2.0:
            problems.append(f"loop level step {e1:.4f} -> {e2:.4f}")
    else:
        head = np.max(np.abs(mono[:8]))
        tail = np.max(np.abs(mono[-64:]))
        if head > 0.05 or tail > 0.01:
            problems.append(f"edge click head {head:.3f} tail {tail:.3f}")
    # isolated clicks: steps far above their local neighbourhood
    big = np.abs(d) > 8 * typical
    if np.any(big) and np.sum(big) > 0:
        problems.append(f"{int(np.sum(big))} sample steps > 8x p99.9")
    # high-frequency transients (clicks or intended attacks): 5 ms frames >12x their 0.3 s median
    hf = hp(mono, 7000, 4)
    w = SR // 200
    k = len(hf) // w
    e = np.sqrt(np.mean(hf[:k * w].reshape(k, w) ** 2, axis=1)) + 1e-7
    spikes = []
    for i in range(k):
        lo, hi_ = max(0, i - 30), min(k, i + 30)
        if e[i] > 12 * np.median(e[lo:hi_]) and e[i] > 1e-3:
            spikes.append(round(i * w / SR, 2))
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono))))
    f = np.fft.rfftfreq(len(mono), 1 / SR)
    centroid = float(np.sum(f * spec) / (np.sum(spec) + 1e-9))
    line = (f"{name:32s} {len(mono) / SR:5.2f}s peak {peak:6.2f} active {act:6.1f} dB "
            f"centroid {centroid:6.0f} Hz dc {dc:+.5f}{seam}"
            + (f" transients@{spikes[:6]}" if spikes else ""))
    return line, problems


def spectrogram(name, x):
    """Waveform + log-frequency spectrogram PNG (PIL only, no matplotlib)."""
    from PIL import Image, ImageDraw
    from scipy import signal as sg
    PREVIEW.mkdir(parents=True, exist_ok=True)
    mono = x if x.ndim == 1 else x.mean(axis=1)
    W, HW, HS = 900, 120, 360
    f, t, s = sg.spectrogram(mono, SR, nperseg=2048, noverlap=1536)
    db = 10 * np.log10(s + 1e-14)
    # rows: log frequency 20 Hz .. 16 kHz, top = high
    fr = np.geomspace(20, 16000, HS)[::-1]
    rows = np.array([np.interp(fr, f, db[:, i]) for i in range(db.shape[1])]).T
    cols = np.linspace(0, rows.shape[1] - 1, W).astype(int)
    img = np.clip((rows[:, cols] + 120) / 100, 0, 1)
    rgb = np.stack([np.clip(img * 1.6, 0, 1), np.clip(img * 1.4 - 0.45, 0, 1), np.clip(img * 2 - 1.3, 0, 1)
                    + 0.25 * np.clip(0.6 - np.abs(img - 0.35) * 2, 0, 1)], -1)
    spec = Image.fromarray((rgb * 255).astype(np.uint8))
    wave_img = Image.new("RGB", (W, HW), (16, 12, 14))
    d = ImageDraw.Draw(wave_img)
    k = max(1, len(mono) // W)
    for i in range(W):
        seg = mono[i * k:(i + 1) * k]
        if len(seg) == 0:
            break
        y0, y1 = HW / 2 * (1 - seg.max()), HW / 2 * (1 - seg.min())
        d.line([(i, y0), (i, y1)], fill=(200, 70, 60))
    d.text((4, 2), f"{name}  {len(mono) / SR:.2f}s", fill=(230, 220, 200))
    out = Image.new("RGB", (W, HW + HS))
    out.paste(wave_img, (0, 0))
    out.paste(spec, (0, HW))
    d = ImageDraw.Draw(out)
    for hz in (50, 100, 200, 500, 1000, 2000, 5000, 10000):
        yy = HW + int(np.interp(np.log(hz), np.log(fr[::-1]), np.arange(HS)[::-1]))
        d.line([(0, yy), (12, yy)], fill=(255, 255, 255))
        d.text((14, yy - 6), f"{hz}", fill=(255, 255, 255))
    out.save(PREVIEW / f"{name}.png")


def main(argv):
    check_only = "--check" in argv
    preview = "--preview" in argv
    picks = [a for a in argv if not a.startswith("--")]
    names = sorted(n for n in SOUNDS if not picks or any(n == p or n.startswith(p) for p in picks))
    OUT.mkdir(parents=True, exist_ok=True)
    failed = []
    for name in names:
        path = OUT / f"{name}.wav"
        loop = name.startswith(("amb_", "loop_"))
        if check_only:
            if not path.exists():
                failed.append(f"{name}: missing")
                continue
            x = read_wav(path)
        else:
            y, kind = SOUNDS[name]()
            loop = kind == "loop"
            if loop:
                y = periodic_filter(y, lambda v: hp(v, 28, 2))  # no subsonic drift
            else:
                y = trim_tail(y, -60)
                cap = next((v for k, v in MAX_LEN.items() if name == k or (k.endswith("_") and name.startswith(k))), None)
                if cap and len(y) > secs(cap):
                    y = fade(y[:secs(cap)], 0.0, min(0.6, cap / 4))
                y = fade(y, 0.002, 0.0)  # no click on the first sample
            x = master(y, target_for(name), loop=loop)
            if not loop:
                x = trim_tail(x, -60)
            write_wav(path, x)
            x = read_wav(path)
        line, problems = analyse(name, x, loop)
        print(line + ("" if not problems else "   !! " + "; ".join(problems)))
        failed += [f"{name}: {p}" for p in problems]
        if preview:
            spectrogram(name, x)
    total = sum(p.stat().st_size for p in OUT.glob("*.wav"))
    print(f"\n{len(list(OUT.glob('*.wav')))} files, {total / 1024 / 1024:.2f} MB (budget 16 MB)")
    if total > BUDGET:
        failed.append("over the size budget")
    if failed:
        print("PROBLEMS:\n  " + "\n  ".join(failed))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
