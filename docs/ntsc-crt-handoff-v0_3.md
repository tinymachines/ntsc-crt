# ntsc-crt: handoff spec v0.3

Signal-level NTSC encode, decode, and CRT display simulator. Rust core,
pure CPU, compiled to WASM, writing RGBA to a canvas. Verified against
external oracles at every stage. Companion to the NES arcade shell
(256x240 native canvas) and the halfphi engine ladder.

Ratified by the director 2026-09-02, from the change list drafted by
the implementing agent (`docs/spec-v0_3-draft.md`). v0.2
(`docs/ntsc-crt-handoff-v0_2.md`) is kept for the record.

Status of the numbers in this document: unlike v0.2's, they are now
**measured or confirmed**, each carrying a pointer to the milestone
report that holds its run stamp, and the counted ones are under
`tools/check-self-counts.py`. Where v0.2 pre-computed a claim, the
tests confirmed it or this revision corrects it; three did not survive
(sections 3.2, 4.2 and 5 below, marked CORRECTED).

## Changes from v0.2

- Section 3.2's NES field rate corrected: 60.0988 Hz is the two-frame
  average, not the full-frame rate (60.09848 Hz). All three NES rates
  now given exactly. [m0-report]
- Section 4.2's bandwidth step corrected against the primary standard,
  now in hand: Y unrestricted, colour-difference equiband; the I/Q
  split demoted to the historical note it is in ST 170M itself.
  [m2-report]
- Section 5's Rung D corrected: the two-frame temporal comb cannot
  cancel (the residues are 120/240 degrees, never 180); the honest
  rung is three-frame on the full-frame pattern. [m2-report]
- Section 4.2 gains the broad-pulse vertical sync lines M4's field
  detection needs; section 4.3 records how recovery actually measures.
- Section 7's authored budget replaced by the measured table and the
  named optimization levers.
- Section 8's milestones carry their closing records; section 9 carries
  the current open questions. All of v0.2's open questions are
  resolved.

---

## 1. Design principles

1. **The composite sample is the native unit.** Nothing resamples
   quietly. Every stage declares its input grid and output grid
   explicitly, and a mismatch is a compile error via the type system,
   not a runtime warning.
2. **Three sources, one waveform type.** NES dot stream, generic RGB
   framebuffer, and real captured waveform all converge on a single
   `CompositeFrame`. Everything downstream is source-agnostic.
3. **Every stage has an oracle and a way to fail.** `MUTATE=1` perturbs
   filter coefficients or level tables; the golden comparison must go
   red. A verification that cannot fail is not a verification. A
   mutation the subject is designed to survive proves nothing (the
   capture source's first mutation was thrown away for exactly this;
   m4-report).
4. **Measured vs authored stays separate.** Signal level tables, matrix
   coefficients, and filter taps are pulled from cited sources into
   data files with a provenance header, never hand-typed into code.
5. **Test rig is code too.** Anything that resamples, converts, or
   thresholds on the way to a comparison carries the same provenance
   and tolerance justification as the pipeline under test. (Three of
   M1's defects and three of M2's were in the rig, all recorded at the
   fix.)
6. No em dashes in prose. Source data carries its original licence
   terms, recorded per file in `NOTICE.md`.

---

## 2. Crate layout

```
ntsc-crt/
  crates/
    ntsc-grid/        sample grid types, phase arithmetic, frame geometry (the contract)
    ntsc-source-nes/  NES PPU dot stream -> CompositeFrame
    ntsc-source-rgb/  RGB framebuffer -> CompositeFrame (full ST 170M encoder)
    ntsc-source-cap/  captured waveform -> CompositeFrame (sync lock, burst lock, resample)
    ntsc-decode/      CompositeFrame -> YuvFrame -> LinearRgbFrame (rungs A/B/C/D)
    ntsc-crt/         LinearRgbFrame -> DisplayFrame (beam, scanlines, persistence, mask, geometry)
    ntsc-oracle/      native-only, test-only: blargg bindings, golden generation,
                      comparison resampler, the recorded alignment
    ntsc-testgen/     synthetic NES dot streams and RGB test frames (no PPU required)
    ntsc-wasm/        wasm-bindgen bridge, RGBA out, frame pacing with counted drift
  goldens/            recorded goldens with run stamps (generated, gitignored)
  data/               level tables, matrices, timing, filter taps, each with provenance
```

`ntsc-grid` plays the role `v6502-pins` plays in the engine ladder: the
shared contract every other crate is verified against. `ntsc-testgen`
let M1 through M3 close without any PPU existing anywhere in the stack,
as intended.

One naming change from v0.2, recorded in the decode crate: the
intermediate is `YuvFrame`, not `YiqFrame`, because what is implemented
is equiband YUV, which ST 170M-2004 clause 7.2 makes the primary
encoding (section 4.2).

---

## 3. The grid contract (`ntsc-grid`)

### 3.1 Sample rate

Base grid: **12 samples per colour subcarrier cycle**.

- Subcarrier f_sc = 315/88 MHz = 3.579545... MHz, stored as an exact
  ratio (39,375,000/11 Hz), never a float. Confirmed against ST
  170M-2004 clause 11.1 (f_sc = 5 MHz x 63/88) and the nesdev wiki,
  revision 23864.
- Sample rate = 12 x f_sc = 472,500,000/11 Hz = 42.954545... MHz.
- NES master clock = 6 x f_sc; one dot spans 2/3 of a subcarrier
  cycle, which is **8 samples**.

All three sources produce frames on this grid. The capture source
resamples to it (section 4.3); the other two generate on it natively.

### 3.2 Frame geometry (CORRECTED)

Two geometry profiles, selected by source. Every residue below is
confirmed by `ntsc-grid/tests/residues.rs` and recomputed by
`check-self-counts.py`; none is merely quoted.

**NES profile**
- 341 dots x 8 = 2728 samples per line; active 256 dots x 8 = 2048.
- 262 lines per frame, progressive. Odd frames with rendering enabled
  drop dot 340 from the pre-render line, giving 2720 samples on that
  line; the grid type carries a per-line sample count.
- Line residue: 2728 mod 12 = **4** samples (120 degrees) per line:
  the three-line chroma pattern.
- Frame residue, full frame: 714,736 samples, mod 12 = **4**. Phase
  repeats every three frames when rendering is disabled.
- Frame residue, short frame: 714,728 samples, mod 12 = **8**. Full
  then short advances 4 + 8 = 0: with rendering enabled the phase
  pattern repeats every two frames.
- Rates, exact (v0.2 conflated the first two; m0-report):
  - full frames: 29,531,250/491,381 Hz = **60.09848 Hz**;
  - short frames: 8,437,500/140,393 Hz = **60.09915 Hz**;
  - rendering-enabled two-frame average: 39,375,000/655,171 Hz =
    **60.09881 Hz**, the figure the literature usually quotes as
    "60.0988". The three are pinned separately by `field_rates_exact`
    so they cannot be conflated again.

**Broadcast profile** (RGB and capture sources)
- 227.5 subcarrier cycles per line = 2730 samples; 525 lines,
  interlaced, two fields of 262.5.
- Line residue: 2730 mod 12 = **6** (180 degrees). Frame residue:
  1,433,250 mod 12 = **6**. Together: the standard four-field
  sequence.
- Frame rate 30,000/1,001 Hz; field rate 60,000/1,001 = 59.94006 Hz,
  matching ST 170M clause 11.3 digit for digit. That the canonical
  numbers fall out of 315/88 and the geometry is itself a check.
- Levels, sync, blanking and burst timings from ST 170M-2004 (the
  primary itself, retrieved from pub.smpte.org and pinned by hash;
  `data/broadcast-timing.toml`).

### 3.3 Phase is derived, not supplied

The subcarrier phase at any sample is a pure function of geometry:
`phase_at_origin`, the profile, the frame parity, the line index, and
the sample index. `ntsc-grid` exposes `Geometry::phase_at` and
`line_len`, and every source uses them. No source accepts phase as an
input; a source needing a different starting phase changes
`phase_at_origin`, the only free parameter. The capture source is the
one that must EARN alignment to this contract (section 4.3).

### 3.4 Core types

As v0.2 specified and as built: `SampleRate` (exact ratio), `Phase`
(0..12), `FrameParity` (Even, OddFull, OddShort; OddShort refused by
name on the broadcast profile), `CompositeFrame` with per-line
`CompositeLine { samples, sync_start, burst_start, active_start }`,
`CompositeSource`, and `Stage<In, Out>`. Levels are volts at the
composite output; IRE is a derived display unit, converted at the
boundary and never stored.

---

## 4. Sources

### 4.1 NES (`ntsc-source-nes`)

Input: per-dot `(colour_index: u6, emphasis: u3)` at 341 x 262 plus
`FrameParity`. Phase from `Geometry::phase_at`. Encode per dot: 8
samples from the level table; the hue is a 12-phase square wave, never
a sinusoid.

Data: `data/nes-levels.toml`, the terminated measurement (lidnariq)
from nesdev wiki revision 23864, accepted through the two-transcription
gate (two agents, same revision, 43 numeric values, 0 disagreements;
re-runnable as `tools/diff-transcriptions.py`).

Oracle: blargg's `nes_ntsc` **0.2.2**, composite preset, zip sha256
pinned in `tools/fetch-oracle.sh` (the 2011-08-12 Wayback capture of
the dead canonical URL), built natively in `ntsc-oracle`, never
shipped. His colour generation is also ported line for line
(`blargg_model.rs`, LGPL, test-only) and held to his own compiled
palette within 1 count over all 512 entries, so comparison constants
come from the oracle's source text.

The comparison rig, per principle 5: the resampler's slope is blargg's
own 3-in/7-out chunk geometry (24/7 samples per output pixel), its
offset and the burst mapping are measured and frozen with the scan
recorded (m1-report), and one rigid rotation (spread 0.028 degrees)
maps the demodulated (U,V) onto his (I,Q), re-fitted by the tests every
run. Every disagreement is attributed by name in m1-report and
consolidated in `docs/divergences.md`; "close enough" closed nothing.

### 4.2 RGB framebuffer (`ntsc-source-rgb`) (CORRECTED)

Input: video-level G'B'R' in 0..1, or sRGB bytes through the sRGB EOTF
and the reference camera OETF (ST 170M clause 5.1). Chain, clauses in
parentheses:

1. Base matrix on video signals (6.1; `data/yuv-matrix.toml`,
   confirmed against the primary digit for digit).
2. Bandlimit (CORRECTED from v0.2's "Y 4.2, I 1.3, Q 0.4 MHz"): **Y is
   unrestricted** (7.1); the colour-difference signals are **equiband**
   (7.2: less than 2 dB down at 1.3 MHz, at least 20 dB down at 3.6
   MHz; near-Gaussian per 7.3). The split I/Q bandwidths are the
   standard's own NTSC-1953 continuation note and are not the default.
   The implemented filter is a Blackman-windowed sinc scanned to the
   7.2 template with most margin, its measured attenuations recorded
   in `data/filters/rgb-encoder.toml`.
3. Resample onto the 2271-sample active region (order swapped with 2
   in practice; identical for band-limited content, recorded).
4. Modulate at `Geometry::phase_at`; clause 10's base equation with
   setup (0.925 Y + 7.5 + 0.4552 (B-Y) sin + 0.8115 (R-Y) cos), which
   is self-normalizing for a decoder referenced to black 7.5 / white
   100 IRE.
5. Sync, blanking and burst per Tables 1 and 2; burst inverted from
   the reference subcarrier (8.2); the nine burst-free lines per field
   honoured. **Broad-pulse vertical sync on frame lines 4..7 and
   266..269** (new in v0.3): line-granularity, one sync-width
   serration per half line, enough for M4's field detector; the
   standard's half-line equalizing structure is a recorded non-goal.
6. Interlace into two fields (authored line mapping, recorded).

Oracle: known-answer signals. The 75% bars' waveform means land on the
published waveform-monitor column AND its derivation from clause 10
(76.9 / 69.0 / 56.1 / 48.2 / 36.1 / 28.2 / 15.4 IRE, the two compared
against each other), burst at 40 IRE p-p, chroma amplitudes within 3%
of clause 10's scales, decode roundtrip within 0.02. Tolerances
justified where used (m2-report).

### 4.3 Capture (`ntsc-source-cap`)

Input: a raw sample stream at a declared rate, trusted only to parts
per million. Typical rates 4 x f_sc or 13.5 MHz; both tested. Recovery
is four measurements (m4-report):

1. Sync: falling edges at a threshold anchored to the measured tip,
   walked at the nominal period, then least-squares. The fitted period
   against the declared rate IS the rate-mismatch measurement (an
   injected 50 ppm found within 5).
2. Fields: the broad-pulse group anchors frame line numbering.
3. Burst lock, per line: resample the burst window onto the grid,
   measure its phase against the geometry's own target (burst = -sin
   at origin Phase(0)), iterate the sub-sample offset below 0.01 grid
   samples. Proven load-bearing by the lock-disable mutation.
4. Resample the whole line (8-tap windowed sinc) and re-reference DC
   to the measured back porch.

Oracle: synthetic captures from the RGB source through the
capture-card model (anti-alias lowpass, rate mismatch, DC offset,
seeded noise; rig code with parameters stated at every call). Clean
roundtrip within 4 mV mean on the grid; dirty within 6 mV, decoding to
bars within 0.04. **The real-recording half of the gate is open,
pending hardware** (section 9).

---

## 5. Decode (`ntsc-decode`) (Rung D CORRECTED)

Rungs, each selectable at construction, each verified separately:

- **Rung A: notch.** Chroma by the generated bandpass (its signed
  response at f_sc recorded and used for normalization), luma the
  exact complement. Both profiles.
- **Rung B: 2-line comb.** Broadcast only, riding the 180-degree line
  residue; luma chroma-band energy on bars collapses below 2% of the
  raw line. On the NES profile it is **refused by name at
  construction**: the 120-degree residue means the two-line difference
  passes 0.866 of the chroma at a shifted phase and leaves half in
  luma (the factor v0.2 asked the agent to compute, measured in
  `tests/combs.rs`).
- **Rung C: 3-line comb.** Broadcast (1,2,1)/4; NES (1,1,1)/3, the
  native comb: three lines at 0/120/240 degrees sum to zero. Shown on
  the stripe frame: the subcarrier-locked chroma in the comb's luma is
  under 1% of the raw line's, measured by synchronous demodulation
  (band energy is NOT chroma: the dot-rate luma square lives inside
  the chroma band and a comb rightly keeps it; m2-report).
- **Rung D (CORRECTED): temporal comb, NES profile.** v0.2 claimed a
  two-frame average cancels chroma via a 180-degree frame
  relationship; the section 3.2 residues (120/240 degrees) forbid it,
  and measurement agrees: two frames attenuate the chroma path to
  0.866 and leave half the fundamental in luma. The rung as ratified
  is **three-frame on the full-frame pattern** (rendering disabled,
  residue 4 each: the Battletoads case), which cancels exactly with no
  vertical cost, at the price of motion smear. The two-frame
  measurement is kept as a test: it exercises OddShort through decode,
  which v0.2 wanted from this rung.

After separation, all rungs share one tail: QAM demodulation by
sin/cos at the phase from `Geometry::phase_at` plus the source's burst
axis offset, U/V lowpass, the transcribed inverse matrix, display
gamma (2.2, recorded) to `LinearRgbFrame` on the same horizontal grid.

Filters are FIR with taps in `data/filters/*.toml`, generated by
`tools/gen-filters.py`, which records cutoff, window, tap count and its
measured responses, signed. Hand-tuned taps are not accepted.

Oracles: blargg for the NES source (section 4.1); bars known-answer
and roundtrip for the RGB source; the synthetic-capture roundtrip for
the capture source. `MUTATE=1` perturbs a chroma tap and a matrix
coefficient and the decode goldens fail; the resampler and burst carry
always-on red proofs.

---

## 6. CRT (`ntsc-crt`)

Five stages in a fixed order, each optional: beam (horizontal
Gaussian, where the grid becomes pixels, the ratio stated on the
frame), scanlines (vertical Gaussian whose width grows with the
pixel's luminance; the bloom-current relationship is a modelled
parameter, documented as authored), phosphor persistence (per-channel
exponential decay advanced by the source frame period, never wall
clock, so playback is bit-deterministic), mask (aperture-grille triads
at integer pitch, integer-only tiling), geometry (barrel plus corner
rounding, off by default; the 224x224 guaranteed-visible window is a
library invariant, `window_visible`).

Oracle: none external; the crate is a model and says so. Each stage is
held to its own declared mathematics, the declarations typed in the
tests. `MUTATE=1` perturbs the beam width and one time constant, the
two spec-named perturbations. Measured cost: 52.7 ms/frame at scale 3,
all five on (m3-report).

---

## 7. WASM bridge (`ntsc-wasm`) (budget CORRECTED to measurement)

Single entry: dot planes in, RGBA out. Builds with and without
simd128; both measured.

**Rates and drift.** Three rates are in play and none match: NES
60.09881 Hz (the rendering-enabled average, section 3.2), broadcast
59.94 Hz, and the display's refresh. The bridge does not resample
time: the source runs at its own exact rate, the bridge presents the
most recently completed frame per callback, and duplicated and dropped
frames are counted and exposed. Tested with derived, not retyped,
expectations: a 60 Hz display drops exactly the beat frequency; a
display at exactly 30,000/1,001 Hz never drifts.

**The budget, replaced by measurement** (Ryzen 5 5600X, one thread,
m2-report): NES notch 7.54 frames/s native full pipeline, 4.87 wasm;
comb3 8.49 / 5.20; the simd128 flag inside noise. v0.2's "Rung A
should fit comfortably in a single thread with SIMD" did not survive:
the naive FIRs cost about 253 multiply-adds per sample. The levers,
named and deliberately not built during the correctness milestones:
decimation before the U/V lowpass (chroma is 0.6 MHz wide after
demodulation), then explicit SIMD in the line convolutions.

---

## 8. Milestones, as closed

| Gate | Record |
|---|---|
| M0: grid and contract | Closed, commit ca09c95. Residue tests and the two-transcription gate (43/0). |
| M1: NES encode, Rung A, blargg | Closed, b8c057d. Every disagreement attributed; the port held to the compiled palette within 1 count. |
| M2: RGB encode, Rungs B/C/D, WASM, benchmark | Closed, 0ad9320. Primary standard in hand; Rung D corrected by measurement; throughput measured. |
| M3: CRT stages | Closed, ca69c8e. Analytic tests; the recorded golden plays end to end bit-deterministically. |
| M4: capture | Synthetic roundtrip closed, 8f7ae94. **Real recording open, pending hardware.** |
| M5: docs and self-counts | Closed, 5af32f0. 56 claims under `check-self-counts.py`; `NOTICE.md` is the per-source licence registry; divergences consolidated. |

## 9. Open questions for the director

1. **Hardware for M4's real recording**: any composite NTSC source
   captured as raw samples at a known nominal rate, showing bars;
   `recover` plus the existing bar tests then close the last gate.
2. **The NES arcade shell**: where does it live, and does
   `ntsc-wasm`'s contract (dot planes in; RGBA, pacing stats out)
   match its needs?
3. **The photographic roundtrip** (v0.2 section 5's PSNR and
   CIEDE2000-under-D65 floors, decided but never consumed by a gate):
   fold into a future milestone or drop?

## Resolved since v0.2

- All three of v0.2's section 9 questions: blargg pinned at 0.2.2 by
  hash; CIEDE2000 under D65 chosen (still unconsumed, question 3
  above); Rung D taken at M2 and corrected in the taking.
- The v0.2 corrections themselves: sections 3.2, 4.2 and 5, ratified
  here from the measurements in m0-report and m2-report.
