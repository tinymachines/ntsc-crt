# The PPU project: handoff sketch v0.1

A switch-level Ricoh 2C02 (the NES PPU), the same way the family does
everything: the netlist in, the behaviour out, an oracle and a way to
fail at every stage, and a ladder above the switches when speed is
needed. Drafted 2026-09-02 by the implementing agent for the director
to ratify; the project's home is a new sibling repository (open
question 1). It lives in this repository's docs only because the
pipeline it feeds is defined here.

Status of the numbers: the resource table's counts and hashes were
**measured on 2026-09-02** from the fetched files and are stated with
their provenance; everything else marked (authored) is an expectation
for a milestone to replace.

## 1. Why, and why now

`ntsc-crt`'s input contract is the dot stream: per-dot
`(colour_index: u6, emphasis: u3)` at 341 x 262 plus frame parity
(`ntsc_source_nes::DotFrame`). Everything upstream of that waist is
"what colour is this dot"; everything downstream is physics, built and
gated M0..M5. The only honest producer of dots in this family is a
PPU whose behaviour falls out of switches, exactly as the 6502's does:
a behavioural PPU written from documentation would be the one organ in
the body whose behaviour is asserted rather than emergent.

The timing: once M4's real capture lands (issue #1), three artifacts
check each other in pairs, and the PPU joins as the fourth:

```
real NES waveform (scope)  <->  transcribed level table (nesdev rev 23864)
        ^                                   ^
        |                                   |
switch-level 2C02 dots -> ntsc-source-nes encoder -> the same waveform, simulated
```

## 2. Resources, verified

Every row checked live on 2026-09-02; nothing here is remembered.

| Resource | Where | Measured / recorded |
|---|---|---|
| Visual 2C02 netlist (Quietust) | `http://www.qmtpro.com/~nes/chipimages/visual2c02/` | segdefs.js sha256 `0322784743189ac75ba738630b97985dca507fe687a26f27333cc69ba774d87e` (4,123,728 B); transdefs.js `813ea9b73d833aa24fd3305d4734ddd378645c1a48d2c269cf9f54befa4a2471` (1,127,493 B); nodenames.js `7a6d41c271024d49544208b1507ff00bc837cc75105e03004d4a380e0bc7cd7a`. **16,871 transistors, 35,566 segdef entries over about 8,770 distinct nodes (max index 10,905), 858 names**: 4.8x the 6502's 3,510 transistors. JSSim-family format. |
| Visual 2A03 netlist (Quietust) | same host, `visual2a03/` | responds 200; not fetched or measured (P4 territory). |
| The JSSim simulator itself | same host (chipsim and friends beside the data) | the P0/P1 oracle: run it headless over the same data, compare every node every half-step, exactly the 6502 repository's `tools/golden-trace/gen.js` pattern. |
| RP2C02 die photography | already held locally: the 6502 repository's preservation archive (`archive/wayback/files/visual6502.org/images/RP2C02/`, from the 2012 blog post) | the imagery the netlist was traced from; CC BY-NC-SA, visual6502 team. |
| Breaking NES | `github.com/emu-russia/breaks` (217 stars) | reversed schematics and decoded logic; the independent second reading for disputes, the role the 6502 wiki's Hanson/Balazs names played. |
| nesdev wiki, revision 23864 | already transcribed and gated (`data/nes-levels.toml`) | the level table the PPU's own DAC nodes will be held against. |
| blargg PPU and sprite test ROMs | `github.com/christopherpow/nes-test-roms` (612 stars) | behavioural oracles for the LADDERED rungs (P3), not for rung 0 (section 6). |
| 240p test suite (NES port) | `github.com/pinobatch/240p-test-mini` | bars and patterns from real hardware or the laddered core, when literal bars are wanted. |
| `halfphi` | published, chip-agnostic, already loads the 6502, 6800 and Z80 through identical calls | the engine. Any parser accommodation the 2C02 variant needs flows through halfphi's own release gate (`tools/release-halfphi.sh`), never a fork. |

Licence posture (authored, for the record): the qmtpro pages carry no
explicit licence text (checked); the netlist derives from the
visual6502 team's CC BY-NC-SA imagery. Treat the data as NC-SA with
attribution to Quietust and visual6502.org: fetched by pinned hash or
submoduled, never committed, never shipped in an artifact that is not
similarly bound, the exact posture `extern/visual6502` and the blargg
oracle already take. Whether to write to Quietust as a courtesy is
open question 4.

## 3. The contract

- **Output waist**: `ntsc_source_nes::DotFrame`, unchanged. The PPU
  project depends on ntsc-crt for the type (or the type moves to a
  tiny shared crate; director's taste).
- **The deeper tap** (P1 measurement, not a promise): the 2C02 drives
  its video DAC on-die. Reading the DAC-driving nodes at switch level
  yields per-dot levels that can be held DIRECTLY against
  `data/nes-levels.toml`'s measured voltages, one abstraction below
  the dot stream: silicon logic against lidnariq's bench. If the
  ratios disagree, that is a finding about the table, the netlist or
  the model, and it gets a named test either way.
- **Input**: the PPU's bus world, owned by a harness crate: VRAM,
  palette RAM, OAM, the register interface at $2000..$2007, clocking
  per the master/4 relationship the grid contract already states.

## 4. Repository and crate sketch

```
<new repo>/
  extern/visual2c02/   the data, submodule or hash-pinned fetch, read-only
  crates/
    v2c02-netlist/     parse + CSR via halfphi, counts asserted against
                       section 2's measured numbers, NC-SA quarantine
    v2c02-sim/         clock, bus, VRAM/OAM/palette harness, register file
    v2c02-dots/        video-node extraction -> DotFrame + DAC-level tap
  tools/golden-trace/  the JSSim headless runner (the 6502 pattern)
  goldens/             node-level and dot-stream goldens with run stamps
```

## 5. Milestones

**P0: the netlist loads and settles.** halfphi parses the 2C02 data;
counts in-tree must equal section 2's measured numbers (the
check-self-counts pattern from day one); power-on settles to a fixed
point; the JSSim golden harness runs and rung 0 matches it node for
node over a recorded trace. MUTATE=1 drops data and must go red.
Gate: bit-exact against JSSim, counts asserted, a first measured
half-steps-per-second number replacing section 6's estimate.

**P1: free-running render, and first light.** The harness loads a
known VRAM/palette image, pokes rendering on, runs frames. Extract the
dot stream; record it as a golden with a run stamp; play it through
ntsc-crt and eyeball the picture. Measure the DAC tap against the
level table (tolerances stated before measuring). Gate: a recorded
dot-stream golden, the picture, and the DAC comparison, each with a
number.

**P2: registers and the famous timings, by micro-trace.** Sprite-0
hit, the VBL flag race, OAM address corruption territory: measured
over CRAFTED short sequences driven through the register file, cycle
positions recorded and compared against Breaking NES's decoders and
the wiki's claims, the dpc-vs-wiki pattern. NOT whole test ROMs:
section 6 says why.

**P3: the ladder.** An authored fast PPU (the v6502-micro pattern:
tables measured out of rung 0, datapath authored from the proven
model) held to rung 0's node and dot goldens, THEN to the blargg and
nes-test-roms suites end to end with a real CPU attached, then to the
M4 real capture. Real-time is a P3 property, never a rung 0 one.

**P4: the console.** The laddered PPU beside the existing rung 3 6502
(the 2A03 is that core minus decimal, plus the APU, which is out of
scope until someone wants sound), the shell, and ntsc-crt as the video
path: a console whose picture is demodulated, not looked up.

## 6. Performance expectations (authored, P0 replaces them)

The 6502 (3,510 transistors) runs about 29,600 half-cycles/s at rung
0. The 2C02 is 4.8x the transistors with a wider active fraction
during rendering; expect rung 0 in the low thousands of half-cycles/s,
i.e. minutes per frame. That is the right speed for golden generation
and completely wrong for test ROMs, which need seconds of emulated
time: hence P2's micro-traces and P3's ladder. No number here survives
contact with P0's measurement, and none gets quoted anywhere until it
is one.

## 7. Open questions for the director

1. The repository's name and home: `tinymachines/2c02` (recommended:
   the chip, like `6502`), `ppu`, or under a NES umbrella?
2. Does `DotFrame` stay defined in ntsc-crt with the PPU depending on
   it, or move to a shared contract crate the way `v6502-pins` stands
   alone?
3. Who drives which milestone: P0/P1 are proven machinery (an agent
   can run the whole pattern); P2's micro-trace choices and P4's shell
   are the places a human hand is most valuable.
4. Write to Quietust about the netlist's licence terms as a courtesy,
   or rely on the NC-SA-derived posture recorded above?
