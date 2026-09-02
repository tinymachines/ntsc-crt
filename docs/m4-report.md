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
