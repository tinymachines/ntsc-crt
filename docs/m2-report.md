# M2 report: RGB encode, the comb rungs, WASM, and the measured budget

Run stamp: 2026-09-01, third commit of this repository, rustc 1.97.1,
`cargo test --workspace` 45 tests green, clippy clean, MUTATE=1 reddens
27 tests. Throughput measured on an AMD Ryzen 5 5600X (6 cores, one
thread used), node v24.0.1 for the wasm rows. Every number below is
pasted from the runs described; M5 puts them under a check-self-counts
scan.

## The primary source, finally in hand

SMPTE's own repository serves ST 170M-2004 (stabilized 2010):
`https://pub.smpte.org/latest/st170/st170-20041130-pub.zip`, zip sha256
`dc9a1f0487385b697de0caca0a8371d61d84bffe7352bcf707818cc9bd8df2bd`. The
M1 obligation is closed: clause 6.1 states the base matrix digit for
digit ("precise values; i.e., 0.587 G = 587/1000 G"), and clause 6.2's
I/Q formulas re-derive from the transcribed reduction factors and the
33-degree rotation to all four printed decimals. `data/yuv-matrix.toml`
records the confirmation; `data/broadcast-timing.toml` is a fresh
transcription of Table 1 (levels), Table 2 (horizontal timing) and
clauses 5.1, 7, 8, 10 and 11. Clause 11's own rationals land exactly on
M0's: f_sc = 5 MHz x 63/88 = 315/88 MHz, f_V = 59.94005994 Hz =
60,000/1,001.

Two findings that reshaped the plan, both from the primary:

- **Equiband U/V is the standard, not a compromise.** Clause 7.2's
  primary spec bandlimits B-Y and R-Y identically (less than 2 dB down
  at 1.3 MHz, at least 20 dB down at 3.6 MHz); the split I/Q bandwidths
  are its NTSC-1953 continuation note. The spec's section 4.2 step 2
  ("Y to 4.2 MHz, I to 1.3, Q to 0.4") should cite this at v0.3: Y is
  explicitly unrestricted (7.1) and the I/Q split is optional history.
- **Clause 10 is self-normalizing for the decoder.** The base equation
  scales luma and chroma by the same 0.925 and adds 7.5 IRE of setup, so
  a decoder referenced to black 7.5 / white 100 IRE recovers exactly
  0.492 (B-Y) and 0.877 (R-Y): the reduction-factor U and V the
  transcribed inverse matrix expects, with no extra constants anywhere.

## The RGB encoder, and the bars gate

`ntsc-source-rgb`: video-level G'B'R' (or sRGB bytes through the sRGB
EOTF and clause 5.1's camera OETF), the clause 6.1 matrix, linear
resample onto the 2271-sample active region, the colour-difference
lowpass (Blackman-windowed sinc, cutoff scanned to the 7.2 template with
most margin: 2.60 MHz, measured -0.00 dB at 1.3 MHz and -51.8 dB at 3.6,
recorded in `data/filters/rgb-encoder.toml`), clause 10's base equation,
and Table 1/2 sync, blanking and burst with the nine burst-free lines
per field honoured. Simplifications (rectangular envelopes,
line-granularity vertical sync, untuned SC-H) are recorded in the module
header and invisible to M2's oracles.

The known-answer gate (`tests/bars.rs`), all inside Table 1's +/-1 IRE:

- Each 75% bar's waveform mean lands on both the clause-10 derivation
  and the published waveform-monitor column (76.9 / 68.9 / 56.1 / 48.2 /
  36.1 / 28.2 / 15.4 IRE), which are compared against each other, so a
  wrong matrix or a wrong setup breaks the agreement.
- Burst peak-to-peak at 40 IRE (tolerance justified from the 12-sample
  grid's worst peak-sampling offset, cos 15 degrees), sync at -40,
  blanking at 0.
- Each bar's chroma amplitude within 3% of clause 10's own scales.
- The decode roundtrip returns every bar centre within 0.02 of the
  input video level.

## The comb rungs

- **Rung B** is refused by name on the NES profile at construction, per
  the spec: the 120-degree line residue means two adjacent lines cannot
  cancel; the attenuation a two-line comb would deliver there is 0.866
  in its chroma path with half the chroma left in luma (the phasor
  arithmetic the temporal-comb tests measure directly). On broadcast it
  works: the comb luma's chroma-band energy on a bars line collapses to
  under 2% of the raw line's.
- **Rung C** on the NES profile is the native comb, and the M2 gate is
  shown: on the stripe frame the three-line mean's subcarrier-locked
  chroma is under 1% of the raw line's, while the chroma path still
  demodulates the stripes' artifact colour at the notch rung's own
  saturation. On a solid frame the comb removes the fundamental to
  under 1e-3 where the notch leaves its (1 - 0.907) residue.
- **Rung D, measured against the spec's claim.** Spec v0.2 section 5
  says rendering-enabled alternate frames are 180 degrees apart and a
  two-frame average cancels chroma. Its own section 3.2 residues say 4
  and 8 samples: 120 and 240 degrees, never 180. Measured
  (`tests/combs.rs`): the two-frame comb on the Even/OddShort pattern
  attenuates its chroma path to 0.866 of a single frame (|1 - e^(-i120)|
  / 2) and leaves half the fundamental in luma; it cannot cancel. Three
  full frames (the rendering-disabled pattern, residue 4 each: the
  Battletoads case) cancel exactly: luma fundamental under 2e-3,
  saturation preserved to 1.000. **For v0.3: Rung D as specced does not
  exist; the honest temporal comb is three-frame on the full-frame
  pattern.** The OddShort path is exercised through decode either way,
  which was the spec's stated reason for the rung.

## Three more instrument lessons, recorded at the fix

1. **Band energy is not chroma.** The stripe frame's dot-rate luma
   square (2.685 MHz) lives inside the chroma band; a comb rightly
   keeps it, and a band-RMS instrument convicted a comb that was
   cancelling perfectly (the solid-frame probe measured 5e-4 of the
   raw). The instruments are now synchronous demodulation for
   subcarrier-locked claims, band RMS only where the fundamental is the
   only band occupant.
2. **A borrowed decoder carries its own gain.** The temporal comb run
   through a notch decoder inherited its 0.907 chroma-gain divisor and
   measured 1.103 saturation: exactly 1/0.907. The comb now overrides
   the gain to unity and says why.
3. **A truncated tick manufactured a slow display.** The broadcast
   pacing test's dt of 1,001e9/30,000 ns rounds down; the fix is a
   3-tick pattern summing to exactly three periods.

## Throughput, measured (the budget's replacement)

Ryzen 5 5600X, one thread, --release, best of 3, full pipeline = encode
+ decode at 2048 x 240 output:

| Path | frames/s | Note |
|---|---|---|
| NES notch, native | 7.54 | 3.7 Msamples/s decoded |
| NES comb3, native | 8.49 | |
| NES temporal-3 decode, native | 12.41 | demod tail only, encode shared |
| Broadcast notch decode, native | 8.21 | 240 lines of 2271 |
| Broadcast comb2 decode, native | 9.97 | |
| NES notch, wasm (node v24) | 4.87 | |
| NES comb3, wasm | 5.20 | |
| NES notch, wasm +simd128 | 4.97 | inside noise |
| NES comb3, wasm +simd128 | 5.18 | inside noise |

The spec's authored budget said Rung A "should fit comfortably in a
single thread with SIMD" at 42.95 Msamples/s; the measurement says
otherwise: the naive per-sample FIRs cost about 253 multiply-adds per
sample (51-tap chroma bandpass plus two 101-tap U/V lowpasses), an order
of magnitude off real time, and the simd128 flag alone changes nothing
because nothing is written to vectorize. The lever, named and not yet
built: chroma is 0.6 MHz wide after demodulation, so decimating before
the U/V lowpass cuts the dominant cost by the decimation factor, and the
line convolutions are the natural place for explicit SIMD. M3 can run on
these numbers (the CRT stages are per-pixel, not per-sample); real-time
display is an optimization milestone, not a correctness one.

The pacing policy (spec section 7) is implemented and tested with
derived, not retyped, expectations: a 60 Hz display against the NES
rendering-enabled rate drops exactly the beat frequency (about one
frame per ten seconds), a 120 Hz display duplicates every other
callback, and a display at exactly 30,000/1,001 Hz never drifts.

## Carried forward

- M3: the CRT stages, analytic tests, and the shell gate.
- Real-time decode (decimation, explicit SIMD) when a display consumer
  exists; the wasm bridge and stats are ready for it.
- Spec v0.3 notes from this milestone: the Rung D correction, the
  clause 7.2 equiband citation, and M1's field-rate correction.
