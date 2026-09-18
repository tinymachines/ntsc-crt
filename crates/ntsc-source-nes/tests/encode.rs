//! Encoder tests: the waveform held to the page's own printed example, the
//! level identities the page states, burst and segment placement, and the
//! frame-chaining residues.
//!
//! MUTATE=1 perturbs the encoding level table (low luma row 1, the $1D /
//! blank voltage, and the burst high) while the expectations stay
//! transcribed; the waveform, burst and segment tests must go red.

use ntsc_grid::{CompositeSource, FrameParity, Phase};
use ntsc_source_nes::{encode_frame, levels, wave_high, DotFrame, Levels, NesSource};

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

/// The levels the encoder runs with: perturbed under MUTATE=1.
fn enc_levels() -> Levels {
    let mut l = Levels::transcribed();
    if mutate() {
        l.low[1] += 0.05;
        l.blank += 0.05;
        l.burst_high += 0.05;
    }
    l
}

#[test]
fn the_pages_example_waveform_reproduces() {
    // "4 cycles of color $18" from the differential-phase-distortion
    // section of the page (revision 23864): 48 samples, printed voltages.
    // The rotation is not printed there; (8 + p) % 12 < 6 puts the first
    // sample at phase 6 (four highs, then the six lows).
    let printed = [
        0.840, 0.840, 0.840, 0.840, 0.312, 0.312, 0.312, 0.312, 0.312, 0.312, 0.840, 0.840,
        0.840, 0.840, 0.840, 0.840, 0.312, 0.312, 0.312, 0.312, 0.312, 0.312, 0.840, 0.840,
        0.840, 0.840, 0.840, 0.840, 0.312, 0.312, 0.312, 0.312, 0.312, 0.312, 0.840, 0.840,
        0.840, 0.840, 0.840, 0.840, 0.312, 0.312, 0.312, 0.312, 0.312, 0.312, 0.840, 0.840,
    ];
    let l = enc_levels();
    for (i, want) in printed.iter().enumerate() {
        let p = ((6 + i) % 12) as u8;
        let got = l.signal(0x18, 0, p);
        assert!(
            (got - *want as f32).abs() < 1e-6,
            "sample {i} (phase {p}): encoder {got}, page {want}"
        );
    }
}

#[test]
fn the_pages_level_identities_hold() {
    let l = Levels::transcribed();
    for p in 0..12u8 {
        for e in 0..8u8 {
            // "$20 and $30 are exactly the same."
            assert_eq!(l.signal(0x20, e, p), l.signal(0x30, e, p));
            for row in 0..4u8 {
                // "$xE/$xF output the same voltage as $1D" and emphasis
                // "does not affect the black colors in columns $E or $F".
                for col in [0x0e, 0x0f] {
                    let c = (row << 4) | col;
                    assert_eq!(l.signal(c, e, p), levels::LOW[1], "colour {c:02x}");
                }
            }
        }
        // "...but it does affect all other columns, including the blacks
        // and greys in column $D": some phase must differ under emphasis.
        assert_eq!(l.signal(0x1d, 0, p), levels::LOW[1]);
    }
    assert!(
        (0..12u8).any(|p| l.signal(0x1d, 7, p) != l.signal(0x1d, 0, p)),
        "$1D must be attenuated by emphasis at some phase"
    );
    // Colour 0 emits only the high level, colour $xD only the low.
    for p in 0..12u8 {
        assert_eq!(l.signal(0x10, 0, p), levels::HIGH[1]);
        assert_eq!(l.signal(0x2d, 0, p), levels::LOW[2]);
    }
}

#[test]
fn burst_is_wave_8_at_the_frame_phase() {
    let frame = encode_frame(&enc_levels(), &DotFrame::filled(FrameParity::Even, 0x18, 0), Phase::new(3));
    let want = Levels::transcribed();
    let line = &frame.lines[0];
    for s in line.burst_start..line.burst_start + 15 * 8 {
        let p = frame.phase_at(0, s).get();
        let expect = if wave_high(levels::COLORBURST_WAVE, p) {
            want.burst_high
        } else {
            want.burst_low
        };
        assert_eq!(line.samples[s], expect, "sample {s} phase {p}");
    }
    // A full cycle of burst is six high, six low.
    let highs = (line.burst_start..line.burst_start + 12)
        .filter(|&s| line.samples[s] == want.burst_high)
        .count();
    assert_eq!(highs, 6);
}

#[test]
fn segments_carry_the_right_levels() {
    let colour = 0x2a;
    let frame = encode_frame(&enc_levels(), &DotFrame::filled(FrameParity::Even, colour, 0), Phase::new(0));
    let want = Levels::transcribed();
    let line = &frame.lines[10];
    // Active picture: the signal function at the frame phase.
    for s in line.active_start..line.active_start + 32 {
        let p = frame.phase_at(10, s).get();
        assert_eq!(line.samples[s], want.signal(colour, 0, p), "active sample {s}");
    }
    // Sync tip and back porch blank.
    assert_eq!(line.samples[line.sync_start], want.sync);
    assert_eq!(line.samples[line.sync_start + 24 * 8], want.sync);
    assert_eq!(line.samples[302 * 8], want.blank);
    // A pre-render row shows blank where the picture would be.
    assert_eq!(frame.lines[250].samples[line.active_start], want.blank);
    // A vsync row is low outside the serration window, blank inside it.
    assert_eq!(frame.lines[246].samples[0], want.sync);
    assert_eq!(frame.lines[246].samples[260 * 8], want.blank);
}

/// The vertical sync's shape is the die's (ntsc-source-nes module doc):
/// the broad pulse begins where row 244's horizontal sync begins and
/// runs to dot 253 of the next row, the serration blank is 254..276,
/// rows 244..246 carry no burst, and row 247 closes with an ordinary
/// sync and burst. Each edge is pinned one dot either side.
#[test]
fn the_vertical_sync_is_the_dies() {
    let want = Levels::transcribed();
    let frame = encode_frame(&enc_levels(), &DotFrame::filled(FrameParity::Even, 0x0f, 0), Phase::new(0));
    let at = |row: usize, dot: usize| frame.lines[row].samples[dot * 8];
    let burst = |row: usize| {
        let v = at(row, 310);
        v == want.burst_high || v == want.burst_low
    };
    // Row 243: an ordinary blanking row, sync 277..301 and a burst.
    assert_eq!(at(243, 301), want.sync);
    assert_eq!(at(243, 302), want.blank);
    assert!(burst(243));
    // Row 244: blank to the onset, then low to the end of the row, no burst.
    assert_eq!(at(244, 276), want.blank);
    assert_eq!(at(244, 277), want.sync, "the onset is the horizontal sync position of row 244");
    assert_eq!(at(244, 340), want.sync);
    assert!(!burst(244), "the first broad pulse runs through row 244's burst");
    // Rows 245 and 246: low from dot 0 to 253, the serration blank 254..276, low again from 277.
    for row in [245, 246] {
        assert_eq!(at(row, 0), want.sync, "row {row}");
        assert_eq!(at(row, 253), want.sync, "row {row}");
        assert_eq!(at(row, 254), want.blank, "row {row}");
        assert_eq!(at(row, 276), want.blank, "row {row}");
        assert_eq!(at(row, 277), want.sync, "row {row}");
        assert_eq!(at(row, 340), want.sync, "row {row}");
        assert!(!burst(row), "row {row}");
    }
    // Row 247: the last serration, then an ordinary sync and burst.
    assert_eq!(at(247, 253), want.sync);
    assert_eq!(at(247, 254), want.blank);
    assert_eq!(at(247, 277), want.sync);
    assert_eq!(at(247, 301), want.sync);
    assert_eq!(at(247, 302), want.blank);
    assert!(burst(247), "row 247 closes with a burst");
    assert_eq!(at(248, 276), want.blank);
    assert!(burst(248));
}

#[test]
fn line_lengths_match_the_geometry() {
    for parity in [FrameParity::Even, FrameParity::OddFull, FrameParity::OddShort] {
        let frame = encode_frame(&Levels::transcribed(), &DotFrame::filled(parity, 0x0f, 0), Phase::new(0));
        for (i, line) in frame.lines.iter().enumerate() {
            assert_eq!(
                line.samples.len(),
                frame.profile.line_len(parity, i),
                "{parity:?} line {i}"
            );
        }
    }
}

#[test]
fn the_source_chains_origins_by_the_residues() {
    let frames = vec![
        DotFrame::filled(FrameParity::Even, 0x0f, 0),
        DotFrame::filled(FrameParity::OddShort, 0x0f, 0),
        DotFrame::filled(FrameParity::Even, 0x0f, 0),
    ];
    let mut src = NesSource::new(frames, Phase::new(5));
    assert_eq!(src.next_frame().phase_at_origin, Phase::new(5));
    assert_eq!(src.next_frame().phase_at_origin, Phase::new(9)); // +4
    assert_eq!(src.next_frame().phase_at_origin, Phase::new(5)); // +8
}

#[test]
fn the_burst_axis_lands_on_minus_u() {
    // Demodulating the burst square with the derived offset must put it on
    // -U: U strictly negative, V zero to numerical precision.
    let theta0 = ntsc_source_nes::burst_axis_offset();
    let mut u = 0.0f64;
    let mut v = 0.0f64;
    for p in 0..12u8 {
        let s = if wave_high(levels::COLORBURST_WAVE, p) { 1.0 } else { -1.0 };
        let theta = std::f64::consts::TAU * p as f64 / 12.0 + theta0;
        u += s * theta.sin();
        v += s * theta.cos();
    }
    assert!(u < -1.0, "burst U component {u}");
    assert!(v.abs() < 1e-9, "burst V component {v}");
}
