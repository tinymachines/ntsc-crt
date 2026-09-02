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

M0 (grid and contract) is built and closed. One crate so far:

| Crate | Role |
|---|---|
| `ntsc-grid` | Sample grid types, phase arithmetic, frame geometry: the contract every other crate is verified against. Subcarrier phase is a pure function of geometry; no source supplies phase as an input. |

`data/` holds the transcribed level tables and matrices, each with a
provenance header; `data/nes-levels.toml` was accepted only after two
independent transcriptions of the same wiki revision diffed clean on every
numeric field.

## Commands

```bash
cargo test --workspace              # residue tests + data consistency
MUTATE=1 cargo test --workspace --no-fail-fast   # must go red: the proof
                                    # the tests can tell (a perturbed
                                    # geometry and a perturbed level)
cargo clippy --workspace --all-targets
```
