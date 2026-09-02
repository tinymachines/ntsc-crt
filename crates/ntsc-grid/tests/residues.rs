//! M0 residue tests: every pre-computed claim in handoff spec section 3.2,
//! confirmed against the geometry rather than copied from it.
//!
//! MUTATE=1 swaps in a geometry whose line is two samples too long
//! (`Geometry::mutated_for_proof`); every test that claims a residue or a
//! rate must then go red, which is the proof the tests can tell. Three
//! tests stay green by design: the grid-rate identity (no geometry in it),
//! the closed-form-vs-prefix-sum consistency (it checks the phase formula,
//! not the constants), and the OddShort refusals (a refusal is not a
//! residue).

use ntsc_grid::{FrameParity, Geometry, Phase, Profile, SampleRate, SAMPLES_PER_CYCLE};
use num_rational::Ratio;

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

fn nes() -> Geometry {
    if mutate() {
        Geometry::mutated_for_proof(Profile::Nes)
    } else {
        Geometry::nes()
    }
}

fn broadcast() -> Geometry {
    if mutate() {
        Geometry::mutated_for_proof(Profile::Broadcast)
    } else {
        Geometry::broadcast()
    }
}

#[test]
fn the_grid_rate_is_twelve_times_the_subcarrier() {
    // f_sc = 315/88 MHz reduces to 39,375,000/11 Hz; the grid rate to
    // 472,500,000/11 Hz = 42.954545... MHz.
    assert_eq!(ntsc_grid::subcarrier_hz(), Ratio::new(39_375_000u64, 11));
    assert_eq!(SampleRate::grid().0, Ratio::new(472_500_000u64, 11));
}

#[test]
fn nes_line_residue_is_four_samples() {
    let g = nes();
    // 341 dots x 8 samples = 2728; 2728 mod 12 = 4: one third of a cycle
    // (120 degrees) per line, the three-line chroma pattern.
    assert_eq!(g.line_len(FrameParity::Even, 0), 2728);
    assert_eq!(g.line_len(FrameParity::Even, 0) % SAMPLES_PER_CYCLE, 4);
    // The same fact through phase_at: each line starts 4 later.
    for line in 0..g.lines() - 1 {
        let here = g.phase_at(FrameParity::Even, line, 0);
        let next = g.phase_at(FrameParity::Even, line + 1, 0);
        assert_eq!(next, here.advanced_by(4), "line {line} -> {}", line + 1);
    }
}

#[test]
fn nes_short_line_is_the_last_line_of_an_odd_short_frame_only() {
    let g = nes();
    let last = g.lines() - 1;
    assert_eq!(g.line_len(FrameParity::OddShort, last), 2728 - 8);
    assert_eq!(g.line_len(FrameParity::OddShort, last - 1), 2728);
    assert_eq!(g.line_len(FrameParity::Even, last), 2728);
    assert_eq!(g.line_len(FrameParity::OddFull, last), 2728);
}

#[test]
fn nes_full_frame_residue_is_four_samples() {
    let g = nes();
    // 262 x 2728 = 714,736; mod 12 = 4. Phase repeats every three frames
    // when the short frame is absent (rendering disabled: the Battletoads
    // case).
    assert_eq!(g.samples_per_frame(FrameParity::Even), 714_736);
    assert_eq!(g.frame_residue(FrameParity::Even), 4);
    assert_eq!(g.frame_residue(FrameParity::OddFull), 4);
    let start = Phase::new(0);
    let after_three = g.next_origin(
        g.next_origin(g.next_origin(start, FrameParity::Even), FrameParity::Even),
        FrameParity::Even,
    );
    assert_eq!(after_three, start);
    assert_ne!(g.next_origin(start, FrameParity::Even), start);
}

#[test]
fn nes_short_frame_residue_is_eight_samples() {
    let g = nes();
    assert_eq!(g.samples_per_frame(FrameParity::OddShort), 714_728);
    assert_eq!(g.frame_residue(FrameParity::OddShort), 8);
}

#[test]
fn nes_phase_repeats_every_two_frames_with_rendering_enabled() {
    let g = nes();
    // Full then short: 4 + 8 = 12 = 0. This is the mechanism behind the
    // skipped dot, and it must hold from every starting phase.
    for p in 0..SAMPLES_PER_CYCLE as u8 {
        let start = Phase::new(p);
        let after_even = g.next_origin(start, FrameParity::Even);
        let after_pair = g.next_origin(after_even, FrameParity::OddShort);
        assert_eq!(after_pair, start, "starting phase {p}");
        assert_ne!(after_even, start, "one frame alone must not return");
    }
    assert_eq!(
        (g.frame_residue(FrameParity::Even) + g.frame_residue(FrameParity::OddShort))
            % SAMPLES_PER_CYCLE as u8,
        0
    );
}

#[test]
fn broadcast_line_residue_is_six_samples() {
    let g = broadcast();
    // 227.5 cycles x 12 = 2730; mod 12 = 6: 180 degrees per line, the
    // two-line pattern a 2-line comb relies on.
    assert_eq!(g.line_len(FrameParity::Even, 0), 2730);
    assert_eq!(g.line_len(FrameParity::Even, 0) % SAMPLES_PER_CYCLE, 6);
    for line in 0..g.lines() - 1 {
        let here = g.phase_at(FrameParity::Even, line, 0);
        let next = g.phase_at(FrameParity::Even, line + 1, 0);
        assert_eq!(next, here.advanced_by(6), "line {line} -> {}", line + 1);
    }
}

#[test]
fn broadcast_frame_residue_is_six_samples() {
    let g = broadcast();
    // 525 x 2730 = 1,433,250; mod 12 = 6. With the line residue this gives
    // the standard four-field colour sequence: two 525-line frames (four
    // fields) return the phase to the start, one does not.
    assert_eq!(g.samples_per_frame(FrameParity::Even), 1_433_250);
    assert_eq!(g.frame_residue(FrameParity::Even), 6);
    let start = Phase::new(0);
    let one = g.next_origin(start, FrameParity::Even);
    assert_ne!(one, start);
    assert_eq!(g.next_origin(one, FrameParity::Even), start);
}

#[test]
fn phase_at_closed_form_matches_the_prefix_sum() {
    // The closed form in Geometry::phase_at against a brute-force
    // accumulation over every line, both profiles, several samples.
    for (g, parity) in [
        (nes(), FrameParity::Even),
        (nes(), FrameParity::OddShort),
        (broadcast(), FrameParity::OddFull),
    ] {
        let mut acc = 0usize;
        for line in 0..g.lines() {
            for sample in [0usize, 1, 7, 11, 100] {
                let expect = Phase::new(((acc + sample) % SAMPLES_PER_CYCLE) as u8);
                assert_eq!(
                    g.phase_at(parity, line, sample),
                    expect,
                    "profile {:?} line {line} sample {sample}",
                    g.profile()
                );
            }
            acc += g.line_len(parity, line);
        }
        // And the accumulated total agrees with samples_per_frame.
        assert_eq!(acc, g.samples_per_frame(parity));
    }
}

#[test]
fn field_rates_exact() {
    let nes = nes();
    let bc = broadcast();

    // NES full frame: 472,500,000/11 / 714,736 = 29,531,250/491,381 Hz
    // = 60.09848 Hz. NOT the well-known 60.0988: that figure is the
    // two-frame average with the short frame alternating in (below).
    // Spec v0.2 section 3.2 quotes 60.0988 for full frames; the derivation
    // here disagrees in the fourth decimal, recorded in the M0 report.
    let full = nes.field_rate(FrameParity::Even);
    assert_eq!(full, Ratio::new(29_531_250u64, 491_381));
    assert!((ratio_f64(full) - 60.09848).abs() < 5e-5, "full {}", ratio_f64(full));

    // NES short frame: 472,500,000/11 / 714,728 = 8,437,500/140,393 Hz
    // = 60.09915 Hz, marginally faster.
    let short = nes.field_rate(FrameParity::OddShort);
    assert_eq!(short, Ratio::new(8_437_500u64, 140_393));
    assert!((ratio_f64(short) - 60.09915).abs() < 5e-5, "short {}", ratio_f64(short));

    // Rendering-enabled average, two frames per (714,736 + 714,728)
    // samples: 39,375,000/655,171 Hz = 60.09881 Hz, the famous figure.
    let pair = nes.samples_per_frame(FrameParity::Even) as u64
        + nes.samples_per_frame(FrameParity::OddShort) as u64;
    let avg = SampleRate::grid().0 * 2 / pair;
    assert_eq!(avg, Ratio::new(39_375_000u64, 655_171));
    assert!((ratio_f64(avg) - 60.09881).abs() < 5e-5, "avg {}", ratio_f64(avg));

    // Broadcast: frame 30,000/1,001 = 29.970 Hz, field 60,000/1,001
    // = 59.940 Hz. The canonical NTSC numbers falling out of 315/88 and
    // the geometry is itself a check on both.
    assert_eq!(bc.frame_rate(FrameParity::Even), Ratio::new(30_000u64, 1_001));
    assert_eq!(bc.field_rate(FrameParity::Even), Ratio::new(60_000u64, 1_001));
}

fn ratio_f64(r: Ratio<u64>) -> f64 {
    *r.numer() as f64 / *r.denom() as f64
}

#[test]
#[should_panic(expected = "OddShort is an NES frame parity")]
fn broadcast_refuses_odd_short_line_len() {
    Geometry::broadcast().line_len(FrameParity::OddShort, 0);
}

#[test]
#[should_panic(expected = "OddShort is an NES frame parity")]
fn broadcast_refuses_odd_short_phase() {
    Geometry::broadcast().phase_at(FrameParity::OddShort, 0, 0);
}
