# ntsc-crt

Signal-level NTSC encode, decode, and CRT display simulation. Rust core,
pure CPU, headed for WASM writing RGBA to a canvas. Companion to the
[tinymachines 6502](https://github.com/tinymachines/6502) engine ladder.

The design is `docs/ntsc-crt-handoff-v0_2.md`; the milestone log is
`docs/m0-report.md` onward. The short version of the design: the composite
sample (12 per colour subcarrier cycle, at exactly 12 x 315/88 MHz) is the
native unit, three sources (NES dot stream, RGB framebuffer, captured
waveform) converge on one `CompositeFrame` type, and every stage downstream
is source-agnostic, has an oracle, and has a way to fail.

## Status

M0 (grid and contract) and M1 (NES encode, Rung A decode, blargg golden)
are built and closed; the milestone logs are `docs/m0-report.md` and
`docs/m1-report.md`. Five crates:

| Crate | Role |
|---|---|
| `ntsc-grid` | Sample grid types, phase arithmetic, frame geometry: the contract every other crate is verified against. Subcarrier phase is a pure function of geometry; no source supplies phase as an input. |
| `ntsc-source-nes` | Per-dot (colour, emphasis) to `CompositeFrame`: direct synthesis like the PPU, levels from the gated transcription at build time, the signal function ported from the page's own C++. |
| `ntsc-testgen` | Deterministic dot streams (solids, the 64 x 8 sweep, both colour-cycle sets, stripes), so M1 through M3 close with no PPU existing anywhere. |
| `ntsc-decode` | Rung A: chroma bandpass with luma as its exact complement, QAM demodulation at the geometric phase, the transcribed inverse matrix, display gamma recorded. |
| `ntsc-oracle` | Test-only, never shipped: blargg's nes_ntsc 0.2.2 built natively (fetched by hash, LGPL), his colour model ported and held to his own compiled palette, the comparison resampler, the recorded alignment, the golden comparison. |

`data/` holds the transcribed level tables, matrices and generated filter
taps, each with a provenance header; `data/nes-levels.toml` was accepted
only after two independent transcriptions of the same wiki revision
diffed clean on every numeric field.

## Commands

```bash
cargo test --workspace              # 33 tests: residues, data consistency,
                                    # encoder waveform, Rung A physics, and
                                    # (with the vendor fetched) the blargg
                                    # golden; the oracle tests SKIP without
                                    # it, REQUIRE_ORACLE=1 insists
MUTATE=1 cargo test --workspace --no-fail-fast   # must go red: 17 tests
bash tools/fetch-oracle.sh          # nes_ntsc 0.2.2 by pinned sha256
cargo run --release -p ntsc-oracle --example align   # the alignment run:
                                    # every frozen comparison constant,
                                    # re-measured and printed
python3 tools/gen-filters.py        # regenerate data/filters/rung-a.toml
python3 tools/diff-transcriptions.py # the M0 two-transcription gate
cargo clippy --workspace --all-targets
```
