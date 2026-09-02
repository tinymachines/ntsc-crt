# Notices

The code in this repository is MIT (see `LICENSE`). The data files under
`data/` are transcriptions of external sources and carry their own terms,
recorded per file in each file's provenance header. Current inventory:

- `data/nes-levels.toml`: measured NES composite levels from the nesdev
  wiki page "NTSC video" (revision 23864), measurements by lidnariq. The
  nesdev wiki declares no site-wide licence (MediaWiki API rightsinfo
  empty, no footer notice, checked 2026-09-01); the values are used as
  measurements of fact with attribution. Revisit before any
  redistribution decision beyond this repository.
- `data/yuv-matrix.toml`: SMPTE 170M-2004 matrix coefficients, transcribed
  at second hand from the same wiki revision (the ITU-R BT.1700 download
  was unreachable at transcription time). Marked in its header for
  confirmation against the primary at M2.

Planned for M1: blargg's `nes_ntsc` library as the decode oracle. It is
LGPL 2.1; it will be built natively inside the test-only `ntsc-oracle`
crate and never shipped, the same posture the 6502 repository takes with
its CC BY-NC-SA die data. Its licence terms will be recorded here when it
lands.
