# M0 report: grid and contract

Run stamp: 2026-09-01, first commit of this repository, rustc stable
(rustup toolchain), `cargo test --workspace` 16 tests green, clippy clean.
Every number below is pasted from that run or from the cited source, not
retyped from the spec; M5 puts them under a check-self-counts scan.

## What closed

`ntsc-grid` exists: exact rational sample rate (f_sc stored as
39,375,000/11 Hz, the grid at 472,500,000/11 Hz), `Phase`, both `Geometry`
profiles with `phase_at` and `line_len`, the `CompositeFrame` types, and
the `CompositeSource`/`Stage` traits from spec section 3.4. Phase is a
pure function of geometry, per section 3.3: the only free parameter is
`phase_at_origin`.

## Residues: every section 3.2 claim confirmed

`tests/residues.rs`, 12 tests. Confirmed by test, not copied:

- NES line: 2728 samples, residue 4 (120 degrees), each line starting 4
  later through `phase_at` itself.
- NES full frame: 714,736 samples, residue 4; three Even frames return the
  origin phase, one does not (the rendering-disabled three-frame pattern).
- NES short frame: 714,728 samples, residue 8; Even then OddShort returns
  the origin from all 12 starting phases, Even alone from none.
- Broadcast line: 2730, residue 6 (180 degrees); frame 1,433,250, residue
  6; two frames (four fields) return the origin, one does not.
- The closed-form `phase_at` matches a brute-force prefix sum over every
  line of both profiles, and the accumulated totals match
  `samples_per_frame`.
- `OddShort` on the broadcast profile is refused by name at `line_len` and
  `phase_at` (should-panic tests).

## Rates, exact

| Rate | Exact | Approx |
|---|---|---|
| NES full frame | 29,531,250 / 491,381 Hz | 60.09848 Hz |
| NES short frame | 8,437,500 / 140,393 Hz | 60.09915 Hz |
| NES two-frame average (rendering on) | 39,375,000 / 655,171 Hz | 60.09881 Hz |
| Broadcast frame | 30,000 / 1,001 Hz | 29.97003 Hz |
| Broadcast field | 60,000 / 1,001 Hz | 59.94006 Hz |

**Spec discrepancy, for v0.3:** section 3.2 states "Field rate:
(6 x f_sc) / (4 x 341 x 262) = 60.0988 Hz for full frames". The formula's
shape is right but its quoted value is not: it evaluates to 60.09848 Hz.
60.0988 is the *two-frame average* with the short frame alternating in,
i.e. the rendering-enabled rate a player actually sees, and the figure the
literature usually quotes. `field_rates_exact` pins all three NES rates
separately so the two cannot be conflated again. The broadcast rates
falling out as exactly 30,000/1,001 and 60,000/1,001 is itself a check
that 315/88 and the geometry are wired correctly.

## Data files and the transcription gate

`data/nes-levels.toml`: the terminated measurement table (lidnariq, 75
ohm, +/-2 IRE), the page's average emphasis attenuation factor 0.816328,
and the wave-number facts, from nesdev wiki "NTSC video" **revision
23864** (2026-06-01), transcribed from the raw wikitext. The wiki declares
no site-wide licence (API rightsinfo empty, no footer notice); recorded in
the file header and `NOTICE.md`.

Closing gate, run 2026-09-01: a second agent independently fetched the
same revision and transcribed to the same schema without sight of the
first copy. `tools/diff-transcriptions.py` compared **43 numeric values, 0
disagreements**; a deliberate one-digit perturbation of one copy makes the
gate exit 1, so the gate can tell. Both transcribers independently made
the same single judgement call: the $3x high slot copies the $2x row, per
the page's own levels[16] table and its "$20 and $30 are exactly the
same" statement. Transcription B is kept as
`data/nes-levels-transcription-b.toml`, labelled evidence, never read by
any crate.

`data/yuv-matrix.toml`: the SMPTE 170M base matrix, reduction factors
0.492111 / 0.877283, the stated inverse coefficients, the 33-degree I/Q
rotation and the demodulation constants. **Secondary source**: the ITU-R
BT.1700 download 404'd on 2026-09-01, so this is transcribed from the
same wiki revision, which cites BT.1700 pages 4, 16, 17. Marked in its
header for confirmation against the primary at M2.

`tests/data.rs`, 4 tests, holds the transcriptions to themselves:

- Every (volts, printed IRE) pair cross-checks through 1 IRE = 7.14 mV
  with $1D as the 0 IRE reference; worst row is CBH at 29.69 computed vs
  30 printed, inside the tolerance justified by the page's own +/-2 IRE.
- The page's 0.816328 average recomputes from its seven measured
  attenuation pairs to within 1e-4 (the duplicated $3x row excluded, or it
  would double-count).
- Every inverse-matrix coefficient re-derives from the base matrix and the
  reduction factors to within 1e-4 (actual agreement is within 5e-6).

## MUTATE=1, the red proof

`MUTATE=1 cargo test --workspace --no-fail-fast`: a geometry two samples
too long per line and a perturbed level voltage. Result: **11 tests red**
(all eight geometry-residue-and-rate tests; the IRE cross-check, the
attenuation recomputation and the matrix re-derivation), 5 green by
design and named in the test docstrings: the grid-rate identity (no
geometry in it), the closed-form-vs-prefix-sum consistency (it checks the
phase formula, not the constants), the two OddShort refusals, and the
wave-number range check.

## Carried forward

- Open questions from spec section 9, answered by the director 2026-09-01:
  blargg `nes_ntsc` pinned at 0.2.2 (last release), composite preset,
  recorded by hash at M1; CIEDE2000 under D65 for the RGB roundtrip at M2;
  Rung D (temporal comb) is wanted at M2.
- The yuv-matrix primary-source confirmation is an M2 obligation.
- The spec's full-frame field rate correction above belongs in v0.3.
