# M4 report: the capture source

Run stamp: 2026-09-01, fifth commit of this repository, rustc 1.97.1,
`cargo test --workspace` 56 tests green, clippy clean, MUTATE=1 reddens
33. M5 puts the counts under a check-self-counts scan.

## What closed

`ntsc-source-cap`: a raw waveform at a declared-but-not-trusted rate in,
a broadcast `CompositeFrame` on the grid out. The only source that must
earn its phase instead of deriving it. Recovery is four measurements:

1. **Sync.** Falling edges at a threshold anchored to the measured sync
   tip (half of Table 1's 286 mV depth above the 1st percentile), walked
   at the nominal line period, then a least-squares period over every
   matched edge. That period against the declared rate IS the
   rate-mismatch measurement: an injected +50 ppm on a 13.5 MHz capture
   is recovered within 5 ppm.
2. **Fields.** Mostly-low lines are the broad pulses; the first group
   anchors frame line numbering. (The RGB encoder grew line-granularity
   broad pulses on frame lines 4..7 and 266..269 for this, recorded in
   its module doc: without them the M2 encoder had no vertical structure
   for a detector to find.)
3. **Burst lock, per line.** Resample the burst window onto the grid,
   project against the geometry's own target (burst = -sin at origin
   Phase(0)), correct the sub-sample offset by the measured phase
   residual (the algebra puts the residual at minus the offset, so the
   correction subtracts), iterate to under 0.01 grid samples. Burst-free
   and broad lines borrow the previous locked offset.
4. **Resample and re-reference.** The whole line at the locked offset
   through 8-tap windowed-sinc interpolation, then DC removed by pinning
   the measured back porch to blanking.

`capture_model` is the spec's capture-card stand-in, rig code under
principle 5: an anti-alias lowpass (6.5 MHz, a real card has one), the
declared rate error, DC offset, and seeded deterministic noise, every
parameter stated at each test's call site.

## The roundtrip gate

- **Clean capture at 4 x f_sc**: rate error measured under 3 ppm, burst
  residual under 0.05 grid samples, grid comparison mean under 4 mV
  (about half an IRE) with the worst 60 mV confined to bar edges, where
  the model's anti-alias filter removed real signal the original still
  carries: the instrument floor, stated as such.
- **Dirty capture at 13.5 MHz** (+50 ppm, +20 mV DC, 2 mV noise): the
  50 ppm found within 5, DC re-referenced away, grid mean under 6 mV,
  and the recovered frame decodes to bars within 0.04 of the input video
  levels (looser than M2's 0.02 because this signal has crossed a
  capture card).
- **Field structure** found where the encoder put it: recovered lines 4
  and 266 mostly sync-low, line 25 not.

## The mutation that had to be thrown away

The first MUTATE perturbation shifted the whole capture a quarter
subcarrier cycle, reasoning that sync would barely move while chroma
went 90 degrees wrong. The burst lock absorbed the shift, exactly as
designed, and the test went red only marginally, from the shift's own
interpolation loss: a mutation the subject is built to survive proves
nothing. The replacement disables the burst lock in the subject
(`recover_with`, whose doc says nothing but that test may pass false):
recovery then rests on sync edges alone, and both roundtrips go red at
five to seven times their tolerances (29 and 38 mV means against 4 and
6). That is the proof the lock is load-bearing rather than decorative.

## The open half of the gate

The spec's M4 closing gate has two parts. The synthetic roundtrip is
closed above. **The real recording is open and requires hardware**: no
captured NTSC waveform exists on this machine, and synthesizing one
anywhere else would launder this repository's own code through a file
and call it an oracle. The 6502 repository's touch lesson applies
verbatim: ask for a device before believing capture works. What is
needed, whenever hardware exists: any composite NTSC source recorded as
raw samples (WAV or f32) at a known nominal rate showing colour bars,
and the existing `recover` plus the M2 bar tests close the gate.

## Carried forward

- M5: self-counts, the licence registry, the consolidated divergence
  list, and the spec's v0.3 draft.
- The real-recording gate above, pending hardware.

## Addendum, 2026-09-02: the ingestion path, built and waiting

The director has a capture card, so the machine side of the real gate
is now complete (`crates/ntsc-source-cap/src/ingest.rs`,
`examples/recover-real.rs`, `tests/real.rs`):

- Readers for WAV (PCM 8/16, float 32; rate from the header) and raw
  f32/i16/u8 (rate declared).
- Auto-levelling from arbitrary ADC units to volts, the sync depth as
  the ruler (Table 1: tip 40 IRE below blanking), the two levels found
  as the two lowest histogram peaks. Proven before any real file
  touches it: a synthetic capture rescaled to gain 0.31 offset 0.7
  levels back and recovers within 7 mV mean on the grid
  (`arbitrary_units_auto_level_and_recover`).
- The gate test SKIPS by name until `captures/real-bars.*` exists;
  REQUIRE_REAL=1 insists. **Stated tolerance for the real recording:
  each bar channel within 0.08 of video level** (consumer gear sits
  looser than Table 1's studio +/-1 IRE); a clean capture outside
  that is a finding to investigate, not a number to widen.

The human side is one page: `docs/capture-instructions.md`. One file
plus one command closes the last gate.

## Addendum, 2026-09-02 evening: first real console captures, and the NES profile the recovery was missing

Five captures from a real front-loader NES (Super Mario Bros. / Duck
Hunt cart) taken with `tools/scope-capture.py` on the family's DS1054Z:
12 M points each at 125 MSa/s, about 5.8 frames per record, backed up
off-repo. Three findings, each fixed and proven the same day:

- **The scope ignores a memory-depth set while stopped** and the
  capture tool issued it stopped; it stayed AUTO and the readback had
  no record length. Set while running and asserted now.
- **The recovery's histogram combed on quantized data**: integer u8
  samples in sub-integer bins made the sync tip read as two peaks, and
  "blanking" was found one count above the tip. Bin width is clamped
  to the data's own quantization step now; the synthetic f32 proof
  capture could never have caught either of these.
- **The recovery decoded every capture as broadcast NTSC, and the NES
  is not broadcast NTSC.** Its line is 227 and a third subcarrier
  cycles (2728 grid samples, not 2730), so the burst phase advances a
  third of a cycle per line where broadcast advances half, over 262
  progressive lines anchored on vsync rows 245..=247. Decoded as
  broadcast, every line landed two grid samples further rotated than
  the last: a smooth hue roll down the whole frame, invisible on the
  mostly-monochrome menu screen and unmissable on World 1-1. The
  broadcast assumption also mismeasured the scope clock at -740 ppm
  (2 in 2730 is 733 ppm); under the NES profile the same records
  measure -7 to -19 ppm. `recover_nes` is the fix: NES geometry, the
  burst target derived from the encoder's own wave-8 square, levels in
  the transcribed table's ABSOLUTE volts so the recovered frame
  decodes with the oracle's own constants. Proven on a synthetic NES
  capture (chained origins 8, 0, 4 so the anchored frame is the
  origin-0 one): chroma pointwise within 0.006, band luma means within
  0.005, and the broadcast recovery on the same capture is the built-in
  mutation, which must miss by 10x or refuse.

**The first region scored against the family's own synthesis**
(`examples/score-real-region.rs`, the paused World 1-1 sky vs a solid
$22 frame through the identical decoder): luma within 0.003, hue
within 0.6 degrees, and saturation 28 percent HOT (0.573 vs 0.448).
That last number is a finding, not a tolerance to widen: either the
unterminated probe run flatters the chroma swing, or the real DAC's
AC swing genuinely exceeds the table's DC-measured levels, and one
75-ohm terminated re-capture decides which.

The bars gate stays open: the combo cart draws no SMPTE bars, so
`captures/real-bars.*` still waits on a test ROM. The five banked
captures (menu, paused World 1-1, title-plus-early-demo, Duck Hunt
menu, Duck Hunt in play) decode recognizably end to end, and the
paused 1-1 frame is the keeper: the $22 sky, flat and stable, off
real silicon through this repository's whole path.
