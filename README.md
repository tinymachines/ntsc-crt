# ntsc-crt

Signal-level NTSC encode, decode, and CRT display simulation. Rust core,
pure CPU, headed for WASM writing RGBA to a canvas. Companion to the
[tinymachines 6502](https://github.com/tinymachines/6502) engine ladder.

The design is `docs/ntsc-crt-handoff-v0_3.md` (ratified 2026-09-02; v0.2 kept for the record); the milestone log is
`docs/m0-report.md` onward. The short version of the design: the composite
sample (12 per colour subcarrier cycle, at exactly 12 x 315/88 MHz) is the
native unit, three sources (NES dot stream, RGB framebuffer, captured
waveform) converge on one `CompositeFrame` type, and every stage downstream
is source-agnostic, has an oracle, and has a way to fail.

## Status

All six milestones of the handoff spec are built and closed: M0 (grid
and contract), M1 (NES encode, Rung A decode, blargg golden), M2 (RGB
encode, the comb rungs, WASM, the measured budget), M3 (the CRT
stages), M4 (the capture source; its real-recording half waits on
hardware) and M5 (self-counts, `docs/divergences.md`, the spec's v0.3
draft). The milestone logs are `docs/m0-report.md` through
`docs/m5-report.md`. Nine crates:

| Crate | Role |
|---|---|
| `ntsc-grid` | Sample grid types, phase arithmetic, frame geometry: the contract every other crate is verified against. Subcarrier phase is a pure function of geometry; no source supplies phase as an input. |
| `ntsc-source-nes` | Per-dot (colour, emphasis) to `CompositeFrame`: direct synthesis like the PPU, levels from the gated transcription at build time, the signal function ported from the page's own C++. |
| `ntsc-testgen` | Deterministic dot streams (solids, the 64 x 8 sweep, both colour-cycle sets, stripes), so M1 through M3 close with no PPU existing anywhere. |
| `ntsc-source-rgb` | RGB framebuffer to `CompositeFrame`: the broadcast encoder per SMPTE ST 170M-2004 (the primary itself, fetched from SMPTE's repository and pinned by hash), held to the published 75% bar levels and a decode roundtrip. |
| `ntsc-decode` | The separation rungs: notch (A), two-line comb (B, refused by name on the NES profile), three-line comb (C, NES-native weights), and the temporal comb (D, measured: two frames attenuate to 0.866 and cannot cancel; three full frames cancel exactly). Shared QAM tail at the geometric phase, the transcribed inverse matrix, display gamma recorded. |
| `ntsc-wasm` | The browser bridge: dot frames to RGBA, the drift policy with counted duplicates and drops, plain-Rust core so the native bench measures the page's own code. |
| `ntsc-source-cap` | Captured waveform to `CompositeFrame`: sync detection, burst lock, sinc resample onto the grid, DC re-referenced. Proven by the synthetic-capture roundtrip against a modelled card (rate mismatch found to 5 ppm, burst lock proven load-bearing by its own mutation). |
| `ntsc-crt` | `LinearRgbFrame` to `DisplayFrame`: beam, scanlines, phosphor persistence, mask, geometry, in the fixed order, each optional. A model with analytic tests, every parameter authored and labelled so. |
| `ntsc-oracle` | Test-only, never shipped: blargg's nes_ntsc 0.2.2 built natively (fetched by hash, LGPL), his colour model ported and held to his own compiled palette, the comparison resampler, the recorded alignment, the golden comparison. |

`data/` holds the transcribed level tables, matrices and generated filter
taps, each with a provenance header; `data/nes-levels.toml` was accepted
only after two independent transcriptions of the same wiki revision
diffed clean on every numeric field.

## Commands

```bash
cargo test --workspace              # 56 tests: residues, data consistency,
                                    # encoder waveform, Rung A physics, and
                                    # (with the vendor fetched) the blargg
                                    # golden; the oracle tests SKIP without
                                    # it, REQUIRE_ORACLE=1 insists
MUTATE=1 cargo test --workspace --no-fail-fast   # must go red: 33 tests
bash tools/fetch-oracle.sh          # nes_ntsc 0.2.2 by pinned sha256
cargo run --release -p ntsc-oracle --example align   # the alignment run:
                                    # every frozen comparison constant,
                                    # re-measured and printed
cargo run --release -p ntsc-wasm --example bench     # the throughput table
cargo run --release -p ntsc-crt --example play-golden  # record + play the M3
                                    # golden: PPM frames and drift stats
                                    # into goldens/ (illustrative)
cargo run --release -p ntsc-crt --example crt-bench    # the CRT stages timed
wasm-pack build crates/ntsc-wasm --target nodejs --release   # the browser build
                                    # (add RUSTFLAGS='-C target-feature=+simd128'
                                    #  for the simd variant; both measured)
python3 tools/gen-filters.py        # regenerate data/filters/rung-a.toml
python3 tools/diff-transcriptions.py # the M0 two-transcription gate
python3 tools/check-self-counts.py   # every counted claim in the docs vs a
                                     # fresh measurement (--fast skips the
                                     # cargo rows; REQUIRE_ALL=1 insists)
cargo clippy --workspace --all-targets
```
