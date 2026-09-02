# M3 report: the CRT stages

Run stamp: 2026-09-01, fourth commit of this repository, rustc 1.97.1,
`cargo test --workspace` 53 tests green, clippy clean, MUTATE=1 reddens
30. M5 puts the counts under a check-self-counts scan.

## What closed

`ntsc-crt`: `LinearRgbFrame` to `DisplayFrame`, five stages in the
spec's fixed order, each optional: beam, scanlines, phosphor
persistence, mask, geometry. The crate is a model and says so: **every
numeric parameter is authored** (`CrtParams::authored`, so labelled),
none presented as a measurement of any tube. The grid becomes pixels in
the beam stage and the frame states its resampling ratio
(`samples_per_pixel`); output is 256 x 240 times an integer scale, the
shell's rule, with the NES 8/7 pixel aspect deliberately not applied
and recorded as such.

Stage facts, each held by an analytic test (spec: no external oracle
exists for a model, so each stage is held to its own declared
mathematics, the declarations typed in the tests rather than read back
from the subject):

- **Beam**: an impulse through it produces the declared Gaussian
  (sigma 4.0 samples) within 1e-5, column by column, and the ratio on
  the frame is 2048 over 256 x scale.
- **Scanlines**: each column's vertical profile is the declared
  Gaussian at sigma(v) = 0.30 x scale x (1 + 0.6 v), v the pixel's
  luminance: the bloom-current relationship is a modelled parameter and
  the brighter column is measurably wider.
- **Persistence**: max(excitation, previous x decay) per channel, decay
  exp(-dt/tau) with authored tau (12, 20, 8 ms: P22-ish ordering, blue
  fastest) and dt the source frame period, never wall clock. A step
  down decays with the declared constants to 1e-6 over four frames; a
  step up is instant; reset clears.
- **Mask**: an aperture-grille triad tiled at integer pitch, exact
  values asserted across two periods; pitch zero refused by name.
- **Geometry**: barrel (source = centre + d(1 + k r^2)) plus corner
  rounding, off by default. At the authored k = 0.03 the full frame's
  corners curve off the tube while the centred 224 x 224 window stays
  lit (`window_visible`, the shell's guaranteed-visible invariant, is a
  library function the gate reuses).

## The gate

`tests/gate.rs`: the recorded dot-stream golden (the spec-named
Even/OddShort colour-cycle set) through encode, Rung A decode and all
five stages at scale 2, mask and geometry on: integer dimensions
asserted, the 224 x 224 window lit in every frame, drift stats counted
(two 60 Hz ticks against the 60.0988 Hz source show clean stats; the
long-run beat-frequency counts are ntsc-wasm's pacing tests), and the
whole two-frame run **bit-deterministic on replay** after a phosphor
reset, which is what "persistence advances by the source period" buys.

`examples/play-golden.rs` is the reference player standing in for the
companion shell: it records the golden (colour-cycle set plus stripes)
to `goldens/dotstream-m3.bin` with a run stamp, plays it back **from
the recorded bytes**, writes the four display frames as PPM beside it,
and prints the drift stats. The frames are illustrative, not
verification, per the spec; one was eyeballed at 768 x 720 (scale 3,
mask pitch 1, barrel on) and shows the twelve hue bands stepping around
the wheel inside a curved, corner-rounded, scanlined tube.

## MUTATE

The spec names the perturbations for this milestone: the beam width and
one time constant. MUTATE=1 applies both to the subject
(`tests/stages.rs`); the beam-impulse and decay tests go red, and the
tests that compare the subject against its own parameters by
construction (bloom, mask) are named as staying green.

## Carried forward

- M4: the capture source (sync and burst lock, resample, the
  synthetic-capture roundtrip): the spec's own ordering gate is
  satisfied, decode has been passing against both other sources since
  M1/M2.
- M5: documentation and self-counts over every number in docs/.
- Spec v0.3 queue unchanged: the full-frame rate, Rung D, the clause
  7.2 equiband citation.
- Real-time decode remains the named optimization (M2's table). The
  CRT stages measure 52.7 ms/frame native release at scale 3 with all
  five on (`examples/crt-bench.rs`, same Ryzen 5 5600X), so at display
  scale they join decode on the optimization list; both are correctness
  milestones' non-goals.
