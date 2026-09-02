#!/usr/bin/env python3
"""Generates data/filters/rung-a.toml: the Rung A FIR taps, with every
design parameter recorded in the output header (handoff spec section 5:
hand-tuned taps are not accepted).

Design, all authored and stated here:
- Chroma bandpass: difference of two Hamming-windowed sincs around f_sc,
  half-width 1.0 MHz, 51 taps. Luma is the exact complement (input minus
  extracted chroma), so one kernel serves both and the split is
  delay-matched by symmetry.
- Post-demodulation U/V lowpass: Hamming-windowed sinc, 0.6 MHz cutoff,
  101 taps (an equiband receiver; the page notes real receivers decode
  YUV, not split-bandwidth YIQ).

The script also measures the bandpass's actual gain at f_sc by DFT and
records it, so the decoder normalizes by a measured number rather than an
assumed 1.0.
"""
import math
from datetime import date
from fractions import Fraction
from pathlib import Path

FSC = Fraction(315_000_000, 88)  # Hz, exact
FS = 12 * FSC
CHROMA_HALF_WIDTH_HZ = 1_000_000
CHROMA_TAPS = 51
UV_CUTOFF_HZ = 600_000
UV_TAPS = 101


def lowpass(fc, n):
    """Hamming-windowed sinc, cutoff fc in cycles/sample, DC gain 1."""
    m = n // 2
    taps = []
    for i in range(n):
        x = i - m
        h = 2 * fc if x == 0 else math.sin(2 * math.pi * fc * x) / (math.pi * x)
        w = 0.54 - 0.46 * math.cos(2 * math.pi * i / (n - 1))
        taps.append(h * w)
    s = sum(taps)
    return [t / s for t in taps]


def gain_at(taps, f):
    """SIGNED response at f cycles/sample, evaluated in zero-phase form
    (taps centered). A symmetric FIR's response is real, and its sign is
    part of the answer: the first version of this function returned a
    magnitude, which hid a bandpass built as narrow-minus-wide (passband
    gain -0.907) until the decoder's hue ladder came out 180 degrees
    rotated. Measured, then fixed; the sign stays recorded."""
    m = len(taps) // 2
    return sum(t * math.cos(2 * math.pi * f * (i - m)) for i, t in enumerate(taps))


def blackman_lowpass(fc, n):
    """Blackman-windowed sinc: sidelobes near -58 dB, so effectively no
    ringing, the spirit of ST 170M clause 7.3."""
    m = n // 2
    taps = []
    for i in range(n):
        x = i - m
        h = 2 * fc if x == 0 else math.sin(2 * math.pi * fc * x) / (math.pi * x)
        w = 0.42 - 0.5 * math.cos(2 * math.pi * i / (n - 1)) + 0.08 * math.cos(
            4 * math.pi * i / (n - 1)
        )
        taps.append(h * w)
    s = sum(taps)
    return [t / s for t in taps]


def gen_encoder_chroma(fs):
    """The RGB encoder's colour-difference lowpass, held to ST 170M
    clause 7.2's template: less than 2 dB down at 1.3 MHz, at least
    20 dB down at 3.6 MHz. The cutoff is chosen by scanning for the
    template with the most margin; the scan rule, the chosen cutoff and
    the measured attenuations are all recorded in the output. Refuses to
    write a filter that fails the template."""
    n = 101
    best = None
    fc_hz = 1_600_000
    while fc_hz <= 2_600_000:
        taps = blackman_lowpass(fc_hz / fs, n)
        db13 = 20 * math.log10(gain_at(taps, 1_300_000 / fs))
        db36 = 20 * math.log10(max(abs(gain_at(taps, 3_600_000 / fs)), 1e-12))
        if db13 > -2.0 and db36 < -20.0:
            margin = min(db13 + 2.0, -20.0 - db36)
            if best is None or margin > best[0]:
                best = (margin, fc_hz, taps, db13, db36)
        fc_hz += 50_000
    if best is None:
        raise SystemExit("no Blackman-sinc cutoff meets the clause 7.2 template")
    return n, best


def main():
    fs = float(FS)
    fsc = float(FSC)
    lo = lowpass((fsc - CHROMA_HALF_WIDTH_HZ) / fs, CHROMA_TAPS)
    hi = lowpass((fsc + CHROMA_HALF_WIDTH_HZ) / fs, CHROMA_TAPS)
    # Wide lowpass minus narrow: positive passband gain at f_sc.
    chroma = [b - a for a, b in zip(lo, hi)]
    chroma_gain = gain_at(chroma, fsc / fs)
    uv = lowpass(UV_CUTOFF_HZ / fs, UV_TAPS)

    # The decimated U/V lowpass: the demodulated chroma is 0.6 MHz wide,
    # so the decoder decimates the raw product by 4 and filters at fs/4
    # with 15 taps instead of 101 at full rate. Decimating first folds
    # the product's image bands (chroma bandpass 2.58..4.58 MHz plus
    # f_sc lands at 6.16..8.16 MHz, folding to 2.58..4.58 MHz at fs/4);
    # the filter's measured attenuation there is recorded below and the
    # decoder's tests hold the end result to the same envelopes as the
    # full-rate design did.
    uv_dec_factor = 4
    fs_dec = fs / uv_dec_factor
    uv_dec = lowpass(UV_CUTOFF_HZ / fs_dec, 15)
    fold_lo_db = 20 * math.log10(max(abs(gain_at(uv_dec, 2_580_000 / fs_dec)), 1e-12))
    fold_hi_db = 20 * math.log10(max(abs(gain_at(uv_dec, 3_580_000 / fs_dec)), 1e-12))
    if fold_lo_db > -40.0:
        raise SystemExit(
            f"decimated UV lowpass leaks the folded image band: {fold_lo_db:.1f} dB at 2.58 MHz"
        )

    out = Path(__file__).resolve().parent.parent / "data/filters/rung-a.toml"
    out.parent.mkdir(parents=True, exist_ok=True)
    with open(out, "w") as f:
        f.write("# Generated by tools/gen-filters.py. Do not edit by hand;\n")
        f.write("# regenerate and rerun the decode tests instead.\n\n")
        f.write("[provenance]\n")
        f.write('generator = "tools/gen-filters.py"\n')
        f.write(f'generated = "{date.today().isoformat()}"\n')
        f.write('design = "Hamming-windowed sinc; chroma bandpass as difference of two lowpasses; luma is the exact complement in the decoder"\n')
        f.write(f"sample_rate_hz = \"{FS.numerator}/{FS.denominator}\"\n")
        f.write(f"subcarrier_hz = \"{FSC.numerator}/{FSC.denominator}\"\n")
        f.write(f"chroma_half_width_hz = {CHROMA_HALF_WIDTH_HZ}\n")
        f.write(f"uv_cutoff_hz = {UV_CUTOFF_HZ}\n\n")
        f.write("[chroma_bandpass]\n")
        f.write(f"taps = [{', '.join(f'{t:.10e}' for t in chroma)}]\n")
        f.write(f"gain_at_subcarrier = {chroma_gain:.10f}  # measured by DFT, used for normalization\n\n")
        f.write("[uv_lowpass]\n")
        f.write(f"taps = [{', '.join(f'{t:.10e}' for t in uv)}]\n\n")
        f.write("[uv_lowpass_decimated]\n")
        f.write(f"decimation = {uv_dec_factor}\n")
        f.write(f"taps = [{', '.join(f'{t:.10e}' for t in uv_dec)}]\n")
        f.write(f"measured_db_at_folded_2580000 = {fold_lo_db:.2f}\n")
        f.write(f"measured_db_at_folded_3580000 = {fold_hi_db:.2f}\n")
    print(
        f"wrote {out}: chroma {CHROMA_TAPS} taps (gain at f_sc {chroma_gain:.4f}), "
        f"uv {UV_TAPS} taps, decimated uv 15 taps at fs/{uv_dec_factor} "
        f"({fold_lo_db:.1f} dB at the folded 2.58 MHz)"
    )

    n, (margin, fc_hz, taps, db13, db36) = gen_encoder_chroma(fs)
    enc = Path(__file__).resolve().parent.parent / "data/filters/rgb-encoder.toml"
    with open(enc, "w") as f:
        f.write("# Generated by tools/gen-filters.py. Do not edit by hand.\n\n")
        f.write("[provenance]\n")
        f.write('generator = "tools/gen-filters.py"\n')
        f.write(f'generated = "{date.today().isoformat()}"\n')
        f.write('design = "Blackman-windowed sinc (sidelobes ~-58 dB, minimal ringing per ST 170M clause 7.3), cutoff scanned 1.6..2.6 MHz in 50 kHz steps for most margin against the clause 7.2 template"\n')
        f.write('template = "SMPTE ST 170M-2004 clause 7.2: < 2 dB down at 1.3 MHz, >= 20 dB down at 3.6 MHz (see data/broadcast-timing.toml)"\n')
        f.write(f"cutoff_hz = {fc_hz}\n")
        f.write(f"measured_db_at_1300000 = {db13:.4f}\n")
        f.write(f"measured_db_at_3600000 = {db36:.4f}\n\n")
        f.write("[chroma_lowpass]\n")
        f.write(f"taps = [{', '.join(f'{t:.10e}' for t in taps)}]\n")
    print(
        f"wrote {enc}: {n} taps, cutoff {fc_hz/1e6:.2f} MHz, "
        f"{db13:.2f} dB at 1.3 MHz, {db36:.2f} dB at 3.6 MHz"
    )


if __name__ == "__main__":
    main()
