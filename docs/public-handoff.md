# Handoff: surfacing ntsc-crt on tinymachines.ai as a new project

For the session working in the public roof repository. Written
2026-09-02 from both sides: this repository's reports, and the roof's
own structure (`data/projects.json`, `PROJECTS.md`, `web/lib/projects.ts`,
the per-project silo pattern). The director wants ntsc-crt surfaced as
a **new project**, a sibling of 6502 and hotbits, not a page under
/6502.

## What this project is, in the site's voice

Signal-level NTSC: the composite waveform simulated at 12 samples per
subcarrier cycle on an exact-rational grid, encoded from three sources
(a NES dot stream, an SMPTE ST 170M framebuffer encoder, a real
captured waveform), decoded through four separation rungs, displayed
through a five-stage CRT model. Every stage has an oracle and a way to
fail; the spec's own numbers were treated as claims, and three did not
survive measurement. Public at github.com/tinymachines/ntsc-crt: six
milestones, 58 tests, 33 mutation reds, 56 documentation claims held to
fresh measurement by the repo's own scanner.

The kinship to 6502 is the pitch: the engine ladder simulates the chip
at the switches; this simulates the SIGNAL between a console and a
tube. The two meet at the NES: the ladder's machine emits dots, this
project turns dots into a waveform and the waveform into phosphor.

## Registration (the one-answer rule)

Add to `data/projects.json` (and the reasoning to `PROJECTS.md`): key
`ntsc` (recommended; short like the others; name it "ntsc-crt"),
landing `/ntsc`, its own silo stylesheet, status alongside hotbits'.
Initial surfaces:

1. `site` surface: the landing page, prerendered, nav: true. This is
   phase 1 and can ship alone.
2. (phase 2) `bench` surface: the live demo page, prerendered.

Both i18n'd (check-i18n covers en/ja). The roof's api/pieces.py checks
the manifest, so registration is the first commit, not an
afterthought.

## Phase 1: the landing page

A measurement-report page in the house voice, sourced from the
repository's own documents rather than retyped:

- The story: docs/m0-report.md through m5-report.md carry the arc
  (each has a one-paragraph "What closed"); docs/divergences.md is the
  honesty table; docs/ntsc-crt-handoff-v0_3.md is the ratified spec.
- Visuals: the repo generates its own (play-golden writes the CRT
  frames; the hue-band tube image is the money shot). Generate at
  authoring time from a named commit, store as roof assets with the
  generating command in a comment, and label them illustrative, per
  the repo's own rule.
- Numbers on the page: every figure must trace to a milestone report,
  and any figure the page states should be added to the roof's
  data/check-figures.py, the same discipline this repo runs as
  tools/check-self-counts.py. Do not retype; quote and pin.
- Link the GitHub repository prominently; it is public and MIT.

## Phase 2: the live bench (the demo surface)

`ntsc-wasm` is built for this. Contract, already tested:

- `wasm-pack build crates/ntsc-wasm --target web --release`. Pure MIT:
  unlike the 6502 fold there is NO licence boundary here (no die data,
  no NC-SA; the LGPL oracle is native-test-only and cannot reach a
  wasm bundle), so the roof may build and commit the bundle freely.
  Board from a tagged ntsc-crt commit and record the tag, mirroring
  the boarding discipline, but nothing needs the served-release
  indirection.
- API: `new Pipeline(rung)` with rung "notch" or "comb3" (anything
  else refuses by name); `push_frame(colour: Uint8Array 341*262,
  emphasis: same, parity 0|1|2) -> Uint8Array` RGBA 2048 x 240;
  `tick(dt_ns) -> frames to advance`; `stats() -> [presented,
  duplicated, dropped]`.
- Expectation to print, not hide: about 5 frames/s in the browser
  (measured, docs/m2-report.md). The bench is a laboratory instrument,
  not a game: show one frame advanced per click or a slow free-run
  with the drift counters visible. Patterns to offer: the hue bands,
  stripes (the artifact frame), solids; all generated in-page from the
  documented dot layout (341 x 262, colour u6 + emphasis u3).
- The page should say what the wall of numbers means: this is the
  whole signal path, encode to phosphor, at the switch-level project's
  standard of honesty.

## Phase 3, when it exists

The real-capture gallery: M4's hardware gate (docs/
capture-instructions.md) will produce a real recording and its decoded
frame beside the synthetic one. That page writes itself once the file
exists.

## What not to do

- Do not fold under /6502; the director wants a sibling project. The
  landing may cross-link to /6502 (and the 6502 pages may link back)
  as companions.
- Do not restate lists or numbers the repo already owns: link or pin.
- Do not ship anything from crates/ntsc-oracle or its vendor tree
  (LGPL, test-only); nothing else in the repo is encumbered.
- House style holds on the roof as here: no em dashes in shipped text,
  headings state facts, measured and authored kept apart and labelled.
