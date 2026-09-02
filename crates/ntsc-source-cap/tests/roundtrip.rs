//! The M4 synthetic-capture roundtrip: the RGB source's bars through the
//! capture-card model (rate mismatch, DC offset, noise, all declared
//! here) and back through recovery, compared against the original ON THE
//! GRID, then decoded. The model's parameters are part of each test's
//! meaning and are stated at the call.
//!
//! MUTATE=1 disables the burst lock in the subject (recovery runs on
//! sync timing alone, via `recover_with`); the grid comparison and the
//! decode must go red, which is the proof the lock is load-bearing
//! rather than decorative. An earlier mutation shifted the capture a
//! quarter cycle instead, and the lock absorbed it, as it should: a
//! mutation the subject is designed to survive proves nothing.
//!
//! The spec's second M4 gate, a real recording, needs real hardware: no
//! captured NTSC waveform exists on this machine and synthesizing one
//! elsewhere would launder the same code through a file. Recorded as
//! open in docs/m4-report.md, the same posture as the 6502 repository's
//! "ask for a device before believing touch works".

use ntsc_grid::{CompositeFrame, Phase};
use ntsc_source_cap::{capture_model, recover_with, Capture};
use ntsc_source_rgb::{burst_axis_offset, encode_video_frame, layout, st170, FIELD1_FIRST_LINE};

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

/// The subject: burst lock on, except under MUTATE=1.
fn recover(cap: &Capture) -> ntsc_source_cap::Recovered {
    recover_with(cap, !mutate())
}

fn bars() -> CompositeFrame {
    let rgb = ntsc_testgen::smpte_bars75(700, 480);
    encode_video_frame(&rgb, 700, 480, Phase::new(0))
}



/// Mean and max absolute difference over the picture lines' active
/// regions, recovered vs original, volts.
fn grid_diff(rec: &CompositeFrame, orig: &CompositeFrame) -> (f32, f32) {
    let lay = layout();
    let (mut sum, mut n, mut worst) = (0.0f64, 0u64, 0.0f32);
    for line in (FIELD1_FIRST_LINE..260).chain(283..523) {
        for i in lay.active_start + 20..lay.line_len - 20 {
            let d = (rec.lines[line].samples[i] - orig.lines[line].samples[i]).abs();
            sum += d as f64;
            n += 1;
            worst = worst.max(d);
        }
    }
    ((sum / n as f64) as f32, worst)
}

#[test]
fn a_clean_capture_at_four_fsc_roundtrips_tightly() {
    // No noise, no DC, no rate error: what remains is the model's
    // anti-alias filter and two interpolations, so the tolerance is the
    // instrument floor, measured then frozen: mean under 4 mV (about
    // half an IRE), worst under 60 mV (bar edges, where the 6.5 MHz
    // anti-alias filter has removed real signal the original still has).
    let orig = bars();
    let cap = capture_model(&[&orig, &orig], 4.0 * 315e6 / 88.0, 0.0, 0.0, 0.0, 1);
    let rec = recover(&cap);
    assert!(rec.rate_error_ppm.abs() < 3.0, "ppm {}", rec.rate_error_ppm);
    assert!(
        rec.worst_burst_residual < 0.05,
        "burst residual {} grid samples",
        rec.worst_burst_residual
    );
    let (mean, worst) = grid_diff(&rec.frame, &orig);
    assert!(mean < 0.004, "mean {mean} V");
    assert!(worst < 0.060, "worst {worst} V");
}

#[test]
fn a_dirty_capture_still_recovers_and_measures_its_own_rate_error() {
    // The spec's named impairments, declared: +50 ppm rate error, +20 mV
    // DC, 2 mV uniform noise, at 13.5 MHz (Rec. 601's rate, the coarser
    // of the two typical rates). The rate measurement must find the 50
    // ppm; DC must be re-referenced away; the grid comparison loosens
    // only for the noise.
    let orig = bars();
    let cap = capture_model(&[&orig, &orig], 13_500_000.0, 50.0, 0.020, 0.002, 7);
    let rec = recover(&cap);
    assert!(
        (rec.rate_error_ppm - 50.0).abs() < 5.0,
        "measured {} ppm of an injected 50",
        rec.rate_error_ppm
    );
    let (mean, worst) = grid_diff(&rec.frame, &orig);
    assert!(mean < 0.006, "mean {mean} V");
    assert!(worst < 0.080, "worst {worst} V");

    // And the recovered frame decodes: bars within 0.04 of the input
    // video levels (looser than M2's 0.02: this signal has crossed a
    // capture card).
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let dec = ntsc_decode::Decoder::transcribed(
        burst_axis_offset(),
        st170::BLACK_IRE / st170::IRE_PER_VOLT,
        st170::WHITE_IRE / st170::IRE_PER_VOLT,
    );
    let rgb = dec.decode(&rec.frame, FIELD1_FIRST_LINE + 5, 1, active_len);
    const BAR_RGB: [[f32; 3]; 7] = [
        [0.75, 0.75, 0.75],
        [0.75, 0.75, 0.0],
        [0.0, 0.75, 0.75],
        [0.0, 0.75, 0.0],
        [0.75, 0.0, 0.75],
        [0.75, 0.0, 0.0],
        [0.0, 0.0, 0.75],
    ];
    for (bar, want) in BAR_RGB.iter().enumerate() {
        let center = (2 * bar + 1) * active_len / 14;
        let mut mean = [0.0f32; 3];
        for x in center - 60..center + 60 {
            let s = rgb.signal_rgb(x);
            for c in 0..3 {
                mean[c] += s[c];
            }
        }
        for c in 0..3 {
            mean[c] /= 120.0;
            assert!(
                (mean[c] - want[c]).abs() < 0.04,
                "bar {bar} channel {c}: {} vs {}",
                mean[c],
                want[c]
            );
        }
    }
}

#[test]
fn the_field_structure_is_found_where_it_was_put() {
    let orig = bars();
    let cap = capture_model(&[&orig, &orig], 4.0 * 315e6 / 88.0, 0.0, 0.0, 0.0, 3);
    let rec = recover(&cap);
    // The encoder puts broad pulses on frame lines 4..7 and 266..269;
    // the recovered frame's line 4 must be mostly sync-low and its line
    // 25 must not be.
    let low = |line: usize| {
        rec.frame.lines[line]
            .samples
            .iter()
            .filter(|v| **v < -0.1)
            .count() as f32
            / rec.frame.lines[line].samples.len() as f32
    };
    assert!(low(4) > 0.5, "line 4 low fraction {}", low(4));
    assert!(low(266) > 0.5, "line 266 low fraction {}", low(266));
    assert!(low(25) < 0.15, "line 25 low fraction {}", low(25));
}
