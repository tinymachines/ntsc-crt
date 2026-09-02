# Performance report: the bench made fast

Run stamp: 2026-09-02, seventh commit of this repository, rustc 1.97.1,
AMD Ryzen 5 5600X (one thread), node v24.0.1 for the wasm rows.
Everything below rode under the full gate: 58 tests green including
every frozen blargg envelope, MUTATE=1 reddens 33, clippy clean,
check-self-counts 54 claims verified. The M2 throughput table stands as
the before; this is the after.

## The three levers, all named in docs/m2-report.md before being built

1. **Decimate before the U/V lowpass.** The demodulated chroma is 0.6
   MHz wide, so the raw product is decimated by 4 and filtered with 15
   taps at fs/4 instead of 101 taps at full rate (a 13x cut on the
   dominant stage), then interpolated back with Catmull-Rom (about a
   tenth of a percent of error on a 0.6 MHz signal at the 10.7 MHz
   decimated grid). The filter is generated with provenance like every
   other (`data/filters/rung-a.toml`, `[uv_lowpass_decimated]`), with
   its measured stopband at the folded image bands recorded (-50.3 dB
   at the folded 2.58 MHz).
2. **Convolutions restructured for the vectorizer.** Every FIR now runs
   tap-outer, sample-inner over edge-padded buffers: one elementwise
   multiply-add per tap, which LLVM vectorizes 8-wide natively and
   4-wide under wasm simd128. The per-sample phase lookup became a
   12-sample rotated table applied elementwise.
3. **The gamma round-trip removed from the wasm path.** decode()
   encodes signal RGB to linear light with a 2.2 power and the bench
   immediately undid it: three million powf calls per frame for a
   mathematical identity. push_frame now goes YUV to bytes directly
   through the same matrix and clamp.

## What the aliasing analysis missed, caught by the comb tests

Plain pick-every-4th decimation was correct for the notch rung (its
bandpass confines the product spectrum) and WRONG for the combs: their
separated chroma is wideband, and the square wave's fifth-harmonic
demodulation product sits at 21.48 MHz, exactly twice the decimated
rate, folding to DC. Measured before the fix: two-frame temporal ratio
0.897 against the phasor's 0.866, three-frame saturation 1.035 against
1.000, the stripe comb 58% high. The fix is block-average (boxcar-4)
decimation, whose nulls sit exactly on the frequencies that fold to DC
and to the decimated Nyquist, at four adds per sample and under half a
percent of passband droop. Every comb number returned to its envelope.

Also of record: the old MUTATE perturbation (+0.05 on the U/V centre
tap) faded below the hue ladder's tolerance because the decimated path
interpolates away the broadband ripple the old test was actually
detecting; the perturbation is now +0.15 and the red is back. A
mutation can quietly stop biting when the subject gets smoother; the
count in check-self-counts is what noticed.

## Measured, before and after

Full pipeline (encode + decode, 2048 x 240), frames/s, best of 3; wasm
rows show the spread over three fresh node processes:

| Path | M2 (before) | Now | Gain |
|---|---|---|---|
| NES notch, native | 7.54 | 40.69 | 5.4x |
| NES comb3, native | 8.49 | 44.30 | 5.2x |
| NES temporal-3 decode | 12.41 | 65.76 | 5.3x |
| Broadcast notch decode | 8.21 | 33.56 | 4.1x |
| Broadcast comb2 decode | 9.97 | 36.78 | 3.7x |
| NES notch, wasm | 4.87 | 27.9 to 28.6 | about 5.8x |
| NES comb3, wasm | 5.20 | 43.3 to 44.1 | about 8.4x |
| NES notch, wasm +simd128 | 4.97 | 41.5 to 42.2 | about 8.5x |
| NES comb3, wasm +simd128 | 5.18 | 44.4 to 47.5 | about 8.8x |

The simd128 flag, inert at M2 because nothing was written to
vectorize, now buys the notch rung another 1.5x, which is the point of
lever 2. The wasm bench sits at or near the source's own 60.09881 Hz
for comb3 and about 42 frames/s for the notch: a live instrument
rather than a slideshow. The remaining distance to a locked 60 is the
encoder (now roughly half the frame) and stays on the shelf until a
consumer needs it.
