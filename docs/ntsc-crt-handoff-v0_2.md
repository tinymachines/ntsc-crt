# ntsc-crt: handoff spec v0.2

Signal-level NTSC encode, decode, and CRT display simulator. Rust core, pure CPU, compiled to WASM, writing RGBA to a canvas. Verified against external oracles at every stage. Companion to the NES arcade shell (256x240 native canvas) and the halfphi engine ladder.

Status of every number in this document: **authored**, not measured. Each must be re-derived from the cited source by the implementing agent and recorded with provenance before it appears in prose or code comments. The check-self-counts pattern applies. Where this document pre-computes a value (section 3.2 residues, section 7 rates), the pre-computation is a claim for the agent's test to confirm, not a substitute for the test.

## Changes from v0.1

- Subcarrier phase is now derived inside `ntsc-grid` from geometry, not supplied per dot by an external PPU. Removes the shell dependency from M1.
- Line and frame residues pre-computed for both profiles, including the NES short odd frame.
- Section 5 states analytically which comb rungs can and cannot work on the NES profile, instead of asking the agent to discover it.
- Section 4.1 adds the blargg comparison resample as a first-class, provenance-carrying test fixture.
- Section 7 records the three distinct frame rates in play and sets a drift policy for the bridge.
- M1 and M3 gates no longer assume a live NES frame source exists.
- Open questions 1 and 4 resolved; one new question added.

---

## 1. Design principles

1. **The composite sample is the native unit.** Nothing resamples quietly. Every stage declares its input grid and output grid explicitly, and a mismatch is a compile error via the type system, not a runtime warning.
2. **Three sources, one waveform type.** NES dot stream, generic RGB framebuffer, and real captured waveform all converge on a single `CompositeFrame`. Everything downstream is source-agnostic.
3. **Every stage has an oracle and a way to fail.** `MUTATE=1` perturbs filter coefficients or level tables; the golden comparison must go red. A verification that cannot fail is not a verification.
4. **Measured vs authored stays separate.** Signal level tables, matrix coefficients, and filter taps are pulled from cited sources into data files with a provenance header, never hand-typed into code.
5. **Test rig is code too.** Anything that resamples, converts, or thresholds on the way to a comparison carries the same provenance and tolerance justification as the pipeline under test.
6. No em dashes in prose. Source data derived from nesdev wiki or blargg carries its original licence terms, recorded per file.

---

## 2. Crate layout

```
ntsc-crt/
  crates/
    ntsc-grid/        sample grid types, phase arithmetic, frame geometry (the contract)
    ntsc-source-nes/  NES PPU dot stream -> CompositeFrame
    ntsc-source-rgb/  RGB framebuffer -> CompositeFrame (full NTSC encoder)
    ntsc-source-cap/  captured waveform (WAV/raw f32) -> CompositeFrame (sync lock, resample)
    ntsc-decode/      CompositeFrame -> YiqFrame -> LinearRgbFrame
    ntsc-crt/         LinearRgbFrame -> DisplayFrame (beam, mask, phosphor, persistence)
    ntsc-oracle/      native-only: bindings to reference implementations, golden generation,
                      comparison resamplers
    ntsc-testgen/     synthetic NES dot streams and RGB test frames (no PPU required)
    ntsc-wasm/        wasm-bindgen bridge, canvas writer, frame pacing
  goldens/            recorded golden frames with run stamps
  data/               level tables, matrices, filter taps, each with provenance header
```

`ntsc-grid` plays the role `v6502-pins` plays in the engine ladder: the shared contract every other crate is verified against. `ntsc-testgen` exists so that M1 through M3 can close without any PPU implementation existing anywhere in the stack.

---

## 3. The grid contract (`ntsc-grid`)

### 3.1 Sample rate

Base grid: **12 samples per colour subcarrier cycle**.

- Subcarrier f_sc = 315/88 MHz = 3.579545... MHz (exact rational, store as ratio not float)
- Sample rate = 12 x f_sc = 42.954545... MHz
- NES master clock = 6 x f_sc; PPU dot clock = master / 4 = 1.5 x f_sc, so one dot spans 2/3 of a subcarrier cycle, which is **8 samples**
- Source: nesdev wiki, "NTSC video". Agent to confirm and record the page revision.

All three sources must produce frames on this grid. The capture source resamples to it (section 4.3); the other two generate on it natively.

### 3.2 Frame geometry

Two geometry profiles, selected by source. Residues below are pre-computed claims; the residue tests in M0 confirm them.

**NES profile**
- 341 dots per line x 8 = 2728 samples per line; active region 256 dots x 8 = 2048 samples
- 262 lines per frame, progressive (no interlace)
- Odd frames with rendering enabled drop dot 340 from the pre-render line (line 261), giving 2720 samples on that line. The grid type carries a per-line sample count, not a constant.
- Line residue: 2728 mod 12 = **4** samples, i.e. one third of a subcarrier cycle (120 degrees) per line. This is why the NES has a three-line chroma pattern rather than the broadcast two-line pattern.
- Frame residue, full frame: 262 x 2728 = 714,736 samples, mod 12 = **4**. Phase repeats every three frames.
- Frame residue, short frame: 714,728 samples, mod 12 = **8**. A full frame followed by a short frame advances 4 + 8 = 12 = 0, so with rendering enabled the phase pattern repeats every two frames. This is the mechanism behind the skipped dot and must be reproduced exactly.
- Field rate: (6 x f_sc) / (4 x 341 x 262) = **60.0988 Hz** for full frames. Short frames are marginally faster; the agent computes and records both.

**Broadcast profile** (used by RGB and capture sources)
- 227.5 subcarrier cycles per line = 2730 samples per line
- 525 lines, interlaced, two fields of 262.5
- Line residue: 2730 mod 12 = **6** (180 degrees). Frame residue: 525 x 2730 = 1,433,250 samples, mod 12 = **6**. Together these give the standard four-field colour sequence.
- Field rate 59.94 Hz.
- Standard sync, blanking, and burst timings from SMPTE 170M. Agent to record which edition.

### 3.3 Phase is derived, not supplied

The subcarrier phase at any sample is a pure function of geometry: `phase_at_origin`, the profile, the frame parity, the line index, and the sample index within the line. `ntsc-grid` exposes this as

```rust
impl Geometry {
    pub fn phase_at(&self, parity: FrameParity, line: usize, sample: usize) -> Phase;
    pub fn line_len(&self, parity: FrameParity, line: usize) -> usize;
}
```

and every source uses it. No source accepts phase as an input. This matches the hardware: the NES PPU has no notion of its own colour phase, it simply runs off the master clock counter, and the phase is an emergent property of line and frame length. A source that needs a different starting phase changes `phase_at_origin` on the frame, which is the only free parameter.

Consequence: the NES source needs only `(colour_index, emphasis)` per dot plus `FrameParity` per frame. There is no PPU timing export dependency on the shell or the engine ladder.

### 3.4 Core types

```rust
pub struct SampleRate(Ratio<u64>);           // samples per second, exact
pub struct Phase(u8);                        // 0..12, position within subcarrier cycle

pub enum FrameParity { Even, OddFull, OddShort }   // OddShort only in NES profile

pub struct CompositeFrame {
    pub profile: Geometry,
    pub lines: Vec<CompositeLine>,           // per-line length may vary
    pub frame_parity: FrameParity,
    pub phase_at_origin: Phase,              // subcarrier phase at sample 0 of line 0
}

pub struct CompositeLine {
    pub samples: Vec<f32>,                   // volts, sync tip at the documented level
    pub sync_start: usize,                   // sample index
    pub burst_start: usize,
    pub active_start: usize,
}

pub trait CompositeSource {
    fn next_frame(&mut self) -> CompositeFrame;
}

pub trait Stage<In, Out> {
    fn process(&mut self, input: &In) -> Out;
}
```

Levels are in **volts** at the composite output (IRE is a derived display unit, converted at the boundary and never stored). The NES level table on nesdev is given in volts; the broadcast profile uses SMPTE levels converted to volts at 1 V p-p.

---

## 4. Sources

### 4.1 NES (`ntsc-source-nes`)

Input: per-dot `(colour_index: u6, emphasis: u3)` at 341 x 262 per frame, plus `FrameParity`. Phase comes from `Geometry::phase_at` (section 3.3). A debug assertion checks that the caller's line lengths match `line_len`.

Encode per dot: 8 samples. For each sample, look up (luma level, hue phase, emphasis) in the level table and emit the voltage. The hue is a 12-phase square wave, not a sine; the "colour" of an NES pixel is the phase at which a two-level signal toggles. Do not approximate with a sinusoid.

Data file: `data/nes-levels.toml`, values transcribed from nesdev with URL, revision, and transcription date in the header. Two independent transcriptions by two agents, diffed, before acceptance.

Test input: `ntsc-testgen` produces dot streams without a PPU. Required generators: solid frame for any (colour_index, emphasis); the 64 x 8 solid sweep; a three-frame colour-cycle set with parity sequence Even, OddFull, Even and a second set with Even, OddShort, Even; a vertical-stripe frame with alternating colours for comb testing. Each generator is deterministic and records its parameters in the golden run stamp.

Oracle: blargg's `nes_ntsc` C library, built natively in `ntsc-oracle`. Record blargg's version by hash and the exact preset (composite only; S-video, RGB, and monochrome presets are not targets). `nes_ntsc` does not emit a waveform. It maps 9-bit pixels (6-bit colour plus 3 emphasis) directly to RGB through a precomputed kernel, producing 602 output pixels per 256 input pixels, with its preset's gamma, sharpness, and fringing baked in. Comparison is therefore in decoded RGB, whole pipeline against whole pipeline, at M1; later milestones isolate stages.

The comparison resample: this crate's decoder outputs on the 2048-sample active grid; blargg outputs 602 pixels. A resampler in `ntsc-oracle` maps one onto the other for comparison. It is part of the test rig and is subject to principle 5: its filter design, its alignment offset, and its tolerance contribution are documented in the M1 report, and `MUTATE=1` also perturbs it to show that a wrong resampler is detectable. Any disagreement attributed to the resampler must be shown, not asserted.

Known divergence to document, not hide: `nes_ntsc` is itself an approximation with its own filter design and its own gamma. Where the two disagree, the agent must attribute the disagreement to a specific stage with evidence before the milestone closes. "Close enough" is not a closing condition; "differs here, because of this, and here is the test that shows it" is.

### 4.2 RGB framebuffer (`ntsc-source-rgb`)

Input: any width x height RGB8 frame plus a declared pixel aspect. Full NTSC encoder:

1. sRGB to linear, then to YIQ via the NTSC matrix (SMPTE 170M, recorded)
2. Bandlimit: Y to 4.2 MHz, I to 1.3 MHz, Q to 0.4 MHz (agent to confirm figures from source and record; these are the classic values and may need adjustment for the implemented filter)
3. Resample horizontally onto the 2730-sample broadcast line, active region only
4. Modulate I and Q onto the subcarrier at the phase from `Geometry::phase_at`, add to Y
5. Insert sync, blanking, and burst per profile
6. Interlace into two fields

Oracle: known-answer signals. SMPTE colour bars encoded by this crate must decode (section 5) to bar values within a stated tolerance, and the encoded waveform of the bars must match published bar-pattern sample values at named points (burst amplitude, each bar's Y level and chroma amplitude). Tolerance must be justified from quantisation and filter ripple, not chosen to pass.

### 4.3 Capture (`ntsc-source-cap`)

Input: a raw sample stream at a declared rate. Typical capture rates are 4 x f_sc (14.318 MHz) or 13.5 MHz (Rec. 601), neither of which is the grid rate. Steps:

1. Sync detection: find horizontal sync tips, measure line period, find vertical sync via the serration pattern
2. Burst lock: measure subcarrier phase and frequency from burst on each line; track with a PLL model
3. Resample each line onto the 12 x f_sc grid using the locked burst phase as the reference, so that sample 0 of the burst sits at Phase(0)
4. Emit `CompositeFrame` in the broadcast profile

This is the only source with genuine measurement uncertainty. Oracle: a captured recording of a known test pattern (colour bars, multiburst) where the expected decoded values are known. The capture, the resample, and the decode are all under test at once; isolating them requires synthetic captures generated by running the RGB source's output through a model of a capture card (added noise, rate mismatch, DC offset) and confirming the recovered frame matches the original on the grid.

Do not build this crate until decode (section 5) is passing against the other two sources. It is milestone M4, not earlier.

---

## 5. Decode (`ntsc-decode`)

Rungs, each selectable at runtime, each verified separately:

- **Rung A: notch filter.** Band-reject at f_sc for luma, band-pass for chroma. Simplest, worst dot crawl, closest to a cheap 1980s TV. Works on both profiles.
- **Rung B: 2-line comb.** Relies on the 180 degree line residue. **Broadcast profile only.** On the NES profile the line residue is 120 degrees, so summing two adjacent lines cannot cancel chroma; it attenuates it by a factor the agent computes and records, and leaves a residual at a shifted phase. Rung B on the NES profile is therefore a documented non-goal, and selecting it there must be a compile-time or construction-time error, not a silently worse picture.
- **Rung C: 3-line comb.** On the broadcast profile this is the usual adaptive luma/chroma separator. On the NES profile it is the native comb: three consecutive lines at 0, 120, and 240 degrees are three equal phasors summing to zero, so the three-line sum cancels chroma exactly for vertically uniform colour. Vertical resolution cost is one line each way, same as the broadcast case. The M2 report shows this cancellation on the stripe test frame.
- **Rung D (optional, NES profile only): 2-frame temporal comb.** With rendering enabled the two-frame phase residue is zero, so alternate frames are 180 degrees apart at every sample when the short frame is present, and averaging two frames cancels chroma with no vertical cost, at the price of motion smear. Included only if M2 has budget; it is the closest analogue to what some real TVs did with the NES signal and it exercises the OddShort path end to end.

After Y/C separation: QAM demodulate C by multiplying with sin and cos at the phase from `Geometry::phase_at` (or the burst-locked phase for the capture source), lowpass I and Q, apply the inverse NTSC matrix, apply the display gamma (2.2 or the SMPTE curve, recorded). Output is `LinearRgbFrame` on the same horizontal sample grid; no horizontal resampling yet.

Filters are FIR with taps stored in `data/filters/*.toml`, generated by a script that records its parameters (cutoff, window, tap count) in the header. Hand-tuned taps are not accepted.

Oracle: for the NES source, blargg (section 4.1). For the RGB source, roundtrip of colour bars and of a set of photographic test frames with a stated PSNR floor and a stated colour-difference floor in a perceptual space. Both floors must be justified in the milestone report.

`MUTATE=1` flips one tap in the chroma lowpass and one coefficient in the inverse matrix; all decode goldens must fail.

---

## 6. CRT (`ntsc-crt`)

Stages, each an optional `Stage<LinearRgbFrame, DisplayFrame>` in a fixed order:

1. **Beam.** Horizontal Gaussian spot with width in samples; each output pixel column integrates the beam over the samples it covers. This is where the grid finally becomes pixels, and the resampling ratio is stated on the frame.
2. **Scanlines.** Vertical Gaussian profile per line; gap intensity depends on beam current (brighter lines bloom wider). The bloom-current relationship is a modelled parameter, documented as authored, not measured.
3. **Phosphor persistence.** Per-channel exponential decay across frames, with time constants stored as authored parameters. Persistence state lives in the stage, so `process` is stateful and frame order matters.
4. **Mask.** Shadow mask or aperture grille as a tiled RGB attenuation pattern at a declared dot pitch relative to output pixels; integer-only tiling to match the shell's integer scale rule.
5. **Geometry.** Barrel curvature and corner rounding. Off by default; the shell's 224x224 guaranteed-visible window must remain visible when on.

Oracle: none external. The CRT stage is a model, and the spec is honest about that. Verification is internal: each stage has an analytic test (a single bright sample through the beam stage must produce the declared Gaussian to within float tolerance; a step function through persistence must decay with the declared constant). `MUTATE=1` perturbs the beam width and one time constant.

---

## 7. WASM bridge (`ntsc-wasm`)

- Single `wasm-bindgen` entry: `push_frame(source_frame) -> ImageData`
- Output buffer is a `Uint8ClampedArray` written directly; no intermediate copies
- Builds with and without `simd128`; both are benchmarked

**Rates and drift.** Three frame rates are in play and none of them match: NES 60.0988 Hz (section 3.2), broadcast 59.94 Hz, and the browser's `requestAnimationFrame`, nominally 60 Hz but actually the display's refresh. The bridge does not resample time. Policy: the source runs at its own rate; the bridge presents the most recently completed frame on each animation callback; duplicated and dropped frames are counted and exposed on a stats struct so the shell can show them. Persistence (section 6, stage 3) is advanced by the source's frame period, not by wall clock, so the picture is deterministic regardless of the host's refresh. Any future audio path will need a different policy and is out of scope here.

Budget, authored, to be replaced by measurement at M2: one NES frame is 714,736 samples. At 60.0988 Hz that is about 42.95 M samples per second through encode, separation, demod, three lowpasses, matrix, and beam integration. Rung A decode should fit comfortably in a single thread with SIMD; Rung C may not. The M2 report must contain measured throughput per rung, per build variant, on a named machine, and the site's numbers must come from that run.

---

## 8. Milestones

**M0: grid and contract.** `ntsc-grid` with both profiles, exact rational sample rate, `phase_at` and `line_len` with tests confirming every residue in section 3.2 (line, full frame, short frame, broadcast line, broadcast frame) and the two-frame repeat under Even/OddShort alternation. `CompositeFrame` types. Level tables and matrices transcribed with provenance headers. Closing gate: residue tests pass and a second agent's independent transcription of `nes-levels.toml` diffs clean.

**M1: NES encode, Rung A decode, blargg golden.** `ntsc-testgen` generators. End-to-end synthetic dot stream to `LinearRgbFrame`, compared to blargg on the 64 x 8 solid sweep and both colour-cycle sets. Comparison resampler documented per section 4.1. `MUTATE=1` demonstrated for the pipeline and for the resampler. Closing gate: every disagreement with blargg attributed to a stage with a test. No PPU is required.

**M2: RGB encode, Rungs B and C (D optional), WASM build, benchmark.** Colour bar known-answer tests. Rung C cancellation shown on the NES stripe frame; Rung B rejected at construction on the NES profile. Measured throughput table. Closing gate: numbers on the site trace to the M2 run.

**M3: CRT stages.** Analytic stage tests. Visual comparison frames published alongside the analytic results, labelled as illustrative, not as verification. Closing gate: the shell plays a recorded dot-stream golden (from `ntsc-testgen` or, if one exists by then, from a PPU) through the full pipeline at integer scale with the 224x224 window intact, with the drift stats visible.

**M4: capture source.** Sync and burst lock, resample, synthetic-capture roundtrip, then a real recording. Closing gate: synthetic roundtrip matches on the grid; real recording decodes bars within stated tolerance.

**M5: documentation and self-counts.** Every number in `docs/` covered by `check-self-counts.py`, including the residues and rates in this document. Licence file per data source. Known divergences from blargg and from SMPTE listed explicitly.

---

## 9. Open questions for the director

1. Which blargg version pins the oracle? Suggest the last released version, composite preset, recorded by hash.
2. Perceptual tolerance space for the RGB roundtrip: CIEDE2000 is the obvious choice but needs a white point decision (D65 assumed).
3. Is Rung D (temporal comb) wanted at M2, or deferred? It is the only rung that exercises `OddShort` through decode rather than just through the residue tests.

## Resolved since v0.1

- *NES phase source from the shell.* Not needed; phase is geometry (section 3.3).
- *Rung B on the NES profile.* Cannot work by construction; Rung C is the NES comb (section 5).
