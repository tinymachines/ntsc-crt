# ntsc-crt handoff spec: v0.3 draft

Drafted 2026-09-01 by the implementing agent, from measurement, for the
director to ratify. This is a change list against v0.2
(`docs/ntsc-crt-handoff-v0_2.md`), in the spirit of that document's own
status line: every pre-computed value was a claim for a test to
confirm, and three did not survive confirmation. Citations point at the
milestone reports, which carry the run stamps.

## Corrections (v0.2 claims that failed confirmation)

1. **Section 3.2, NES field rate.** v0.2: "(6 x f_sc) / (4 x 341 x 262)
   = 60.0988 Hz for full frames." The formula's shape is right; its
   quoted value belongs to a different quantity. Full frames are
   **60.09848 Hz** (29,531,250/491,381 exactly); the famous 60.0988 is
   the **two-frame average** with the short frame alternating in
   (39,375,000/655,171 = 60.09881 Hz). Replacement text should give all
   three NES rates (full 60.09848, short 60.09915, rendering-enabled
   average 60.09881) so the two can never be conflated again.
   [docs/m0-report.md]

2. **Section 5, Rung D.** v0.2: "with rendering enabled the two-frame
   phase residue is zero, so alternate frames are 180 degrees apart at
   every sample ... and averaging two frames cancels chroma." The
   section 3.2 residues (4 and 8 samples) are 120 and 240 degrees;
   nothing is ever 180 degrees apart frame to frame. Measured: a
   two-frame average leaves **half** the chroma fundamental in luma
   (|1 + e^(i120)|/2) and its chroma path passes 0.866 of a single
   frame's. Replacement Rung D: a **three-frame temporal comb on the
   full-frame pattern** (rendering disabled, residue 4 each: the
   pattern v0.2 itself attributes to Battletoads), which cancels
   exactly with no vertical cost. The OddShort decode exercise v0.2
   wanted from this rung is preserved: the two-frame attenuation
   MEASUREMENT runs on the Even/OddShort alternation and is itself a
   test. [docs/m2-report.md]

3. **Section 4.2 step 2, bandwidths.** v0.2: "Bandlimit: Y to 4.2 MHz,
   I to 1.3 MHz, Q to 0.4 MHz (the classic values...)". The primary
   (SMPTE ST 170M-2004, in hand, pinned by hash) says: Y carries **no
   bandwidth restriction** (clause 7.1); the colour-difference signals
   are **equiband** (clause 7.2: under 2 dB down at 1.3 MHz, at least
   20 dB down at 3.6 MHz, Gaussian-like per 7.3); the split I/Q
   bandwidths are the clause 7 NOTE for NTSC-1953 continuation only.
   Replacement text should cite 7.1/7.2/7.3 and drop the split as the
   default. [docs/m2-report.md]

## Additions the implementation needed (for the record)

- **Section 4.2 (RGB source)**: the encoder carries line-granularity
  broad pulses on frame lines 4..7 and 266..269, because M4's field
  detection needs vertical structure to find; v0.2's capture section
  assumed serrations exist without requiring the RGB source to emit
  any. The full half-line equalizing structure remains unmodelled and
  is M4's recorded simplification.
- **Section 4.3 (capture)**: the sync-derived line period against the
  declared rate IS the rate-mismatch measurement (found an injected 50
  ppm within 5); the burst lock is per line, iterated below 0.01 grid
  samples, and proven load-bearing by a mutation that disables it.
- **Section 7 (budget)**: the authored "Rung A fits comfortably with
  SIMD" did not survive measurement (7.5 frames/s native, simd128 flag
  inside noise); v0.3 should carry the measured table and name the
  decimation lever instead. [docs/m2-report.md]

## Open questions from v0.2 section 9, all resolved

1. blargg pinned: nes_ntsc 0.2.2, composite preset, zip sha256
   ca1a420d721d83b944142c366a917ba199dbc10cf91ad6f21dc712ed1069d58e
   (the 2011-08-12 Wayback capture of the dead canonical URL).
2. CIEDE2000 under D65 (director, 2026-09-01). Not yet consumed: the
   photographic-roundtrip floor it was chosen for has no test yet.
3. Rung D at M2: taken, and the claim itself corrected (above).

## New open questions for the director

1. The real-recording half of M4's gate waits on hardware: any
   composite NTSC source recorded as raw samples at a known nominal
   rate, showing bars. Who has a capture card?
2. v0.2 named a companion NES arcade shell; the M3 gate was discharged
   with a reference player in this repository. Where does the shell
   live, and does the `ntsc-wasm` bridge's contract (dot planes in,
   RGBA and drift stats out) match what it wants?
3. The photographic test frames and PSNR/CIEDE2000 floors (v0.2
   section 5's RGB oracle) were not exercised by any milestone gate;
   fold into a future milestone or drop?
