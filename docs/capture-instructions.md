# Closing M4's real-recording gate: what to capture, and how to hand it over

The machine side is built and waiting; one file plus one command closes
the last gate. This page is the human side.

## What to record

1. **Source**: any device that outputs SMPTE colour bars over composite
   NTSC. Ideal: a camcorder, DVD player, or pattern generator with a
   bars mode. Second best: any test disc or console title showing full
   bars. The gate checks the seven 75% bars, so the pattern should be
   the classic seven vertical bars (white, yellow, cyan, green,
   magenta, red, blue).
2. **Connection**: composite (the yellow RCA jack), straight into the
   capture card. No S-Video, no RF.
3. **Capture**: raw samples of the composite WAVEFORM, not a decoded
   video file. This is the one thing most capture software does not do
   by default: an AVI or MP4 has already been decoded by the card's own
   chip and is useless here. What works:
   - a card or scope that can dump raw ADC samples (many cheap
     RTL/backhaul tools, instrument capture modes, or vhs-decode-style
     rigs can);
   - sample rate: anything at or above about 12 MHz; 4 x f_sc
     (14,318,181.8 Hz) or 13.5 MHz are the classic choices and both are
     tested. Note the nominal rate the software reports; recovery
     measures the true rate itself.
4. **Length**: half a second is plenty (the tool needs one complete
   frame after the first vertical sync; more is fine).

## Formats accepted

- **WAV** (mono or first channel of stereo; PCM 8-bit, PCM 16-bit, or
  float 32): the rate is read from the header.
- **Raw** headerless samples: f32 (native-endian), i16 or u8 (little
  endian); the rate must then be given.

Levels and polarity do not matter: the tool auto-levels using the sync
depth as the ruler, and that path is proven on a synthetic capture in
arbitrary units (`tests/real.rs`).

## Handing it over

Put the file in `captures/` (gitignored) beside a three-line
`captures/real-bars.toml`:

```toml
file = "real-bars.wav"
format = "wav"        # or f32 / i16 / u8
# rate_hz = 14318181.8  # only needed for raw formats
```

Then either of:

```bash
# The tool: measurements, a decoded field written for eyeballing
# (goldens/real-capture.ppm), and the bar check with --bars.
cargo run --release -p ntsc-source-cap --example recover-real -- \
    captures/real-bars.wav wav '' --bars

# The gate itself: SKIPS without the file, runs with it.
REQUIRE_REAL=1 cargo test -p ntsc-source-cap --test real
```

Stated tolerance for the real gate (also in docs/m4-report.md): each
bar channel within 0.08 of its video level. Consumer gear sits looser
than Table 1's studio +/-1 IRE; if a clean capture lands outside 0.08,
that is a finding to investigate, not a number to widen quietly.

## If it fails

The tool prints which measurement gave out: no sync edges (wrong file
or not raw composite), no broad-pulse group (capture too short or
non-standard vsync), sync tip and blanking not separable (clipped or
AC-coupled beyond recognition), a large burst residual (chroma mangled
or the rate declared very wrong). Each failure names itself; send the
printout.
