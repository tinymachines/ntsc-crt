# Known divergences

The M5 consolidation (handoff spec section 8): every place this
pipeline knowingly differs from its oracles and sources, with where
each was measured. Nothing here is hidden inside a tolerance; each has
a name, a magnitude and a home.

## From blargg's nes_ntsc 0.2.2 (measured in docs/m1-report.md)

| Divergence | Magnitude at DC | Whose choice |
|---|---|---|
| Level table: his two-decimal normalized levels vs the measured voltages | up to 5.48 counts (base colours) | his rounding |
| Emphasis: his YIQ-space approximation vs the measured attenuated rows | up to 45.81 counts | his approximation |
| Chroma: fundamental-only phasor vs the real square (harmonics) | our luma carries the harmonics as ripple; his cannot | physics vs kernel |
| Decoder: his classic YIQ matrix rotated -15 degrees + quadratic gamma vs our SMPTE UV + 2.2 | full-pipeline sweep mean 23.0, worst 68.9 counts | both legitimate; the site will show ours and say so |
| Filters: his DSF-sinc luma + Gaussian chroma kernels vs our windowed sincs | row means 5.7 (bands) to 18.0 (stripes) counts | different authored designs |
| Chroma-vs-luma delay skew between the pipelines | a few samples; no single resampler offset zeroes every instrument | his kernels are asymmetric by design |

## From SMPTE ST 170M-2004 (recorded in the encoder module docs)

- Rise times (140/300 ns, Table 2) not shaped: rectangular envelopes.
- Vertical sync at line granularity: broad pulses on frame lines 4..7
  and 266..269 only, no half-line equalizing pulses (recovery keys on
  the broad group; the standard's full structure is unmodelled).
- SC-H phase (13.2) not tuned; the frame origin is the free parameter.
- Resample-then-bandlimit order (identical for band-limited content).
- Equiband U/V at 1.3 MHz is clause 7.2 compliance, NOT a divergence;
  the I/Q split is the standard's own NTSC-1953 continuation note.

## From the nesdev page's model (measured in docs/m1-report.md)

- Differential phase distortion (the page's own IIR approximation of
  the DAC impedance effect, about -2.5 to -5 degrees per luma row) is
  not modelled; the encoder emits ideal edges.

## Internal, deliberate (each recorded where it lives)

- The intermediate is equiband YUV, named `YuvFrame`; the spec said
  `YiqFrame` (ntsc-decode module doc; clause 7.2 makes YUV primary).
- The NES 8/7 pixel aspect is not applied in the CRT stages
  (ntsc-crt module doc; the shell's integer-scale canvas rule wins).
- The CRT model's every parameter is authored, none measured from a
  tube (CrtParams doc, docs/m3-report.md).
- The capture-card model is rig, not a card: anti-alias at 6.5 MHz,
  uniform noise, linear rate error only (ntsc-source-cap doc); a real
  recording remains the open half of M4's gate.
