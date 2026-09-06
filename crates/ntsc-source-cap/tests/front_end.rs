//! `front_end` is the card model's lowpass and nothing else: a capture
//! modelled at the grid rate with no rate error, offset or noise carries,
//! frame by frame, the same samples `front_end` gives the frame (away
//! from the stream's own edges, where the model clamps to the stream and
//! the frame to itself); a flat line passes through at its level (unit
//! DC gain); and the square-wave chroma of a real colour loses amplitude
//! at the subcarrier's harmonics and keeps its fundamental, which is the
//! whole reason the function exists.

use ntsc_grid::{FrameParity, Phase};
use ntsc_source_cap::{anti_alias_taps, capture_model, front_end};

const GRID_RATE: f64 = 472_500_000.0 / 11.0;

#[test]
fn the_front_end_is_the_card_models_filter_frame_for_frame() {
    let levels = ntsc_source_nes::Levels::transcribed();
    let mut dots = ntsc_testgen::solid(FrameParity::Even, 0x0f, 0);
    for row in 0..240 {
        for dot in 1..257 {
            dots.set(row, dot, [0x16u8, 0x2a, 0x12, 0x28, 0x14, 0x26, 0x1a, 0x30][(dot - 1) / 32], 0);
        }
    }
    let f = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));
    let cap = capture_model(&[&f, &f, &f], GRID_RATE, 0.0, 0.0, 0.0, 1);
    let fe = front_end(&f);
    let n: usize = f.lines.iter().map(|l| l.samples.len()).sum();
    let fe_flat: Vec<f32> = fe.lines.iter().flat_map(|l| l.samples.iter().copied()).collect();
    // The middle frame of the three against the front end, its first and
    // last two kernel widths left out.
    let half = anti_alias_taps().len();
    let worst = fe_flat[half..n - half].iter().zip(&cap.samples[n + half..2 * n - half]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "the model at the grid rate and the front end differ by {worst}");
    // Unit DC gain: the sync tip is a flat run and comes through at its level.
    let tip = fe.lines[10].samples[277 * 8 + 40];
    assert!((tip - levels.sync).abs() < 1e-6, "sync {tip} vs {}", levels.sync);
    // The chroma square wave: its fundamental survives, its harmonics do
    // not. Project one active run of colour $16 onto the fundamental and
    // the third harmonic before and after.
    let project = |samples: &[f32], k: usize| -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in samples.iter().enumerate() {
            let th = 2.0 * std::f64::consts::PI * (k as f64) * i as f64 / 12.0;
            re += *s as f64 * th.cos();
            im += *s as f64 * th.sin();
        }
        (re * re + im * im).sqrt() / samples.len() as f64
    };
    let raw = &f.lines[10].samples[8 + 16..8 + 16 + 192];
    let flt = &fe.lines[10].samples[8 + 16..8 + 16 + 192];
    let (fund_raw, fund_flt) = (project(raw, 1), project(flt, 1));
    let (h3_raw, h3_flt) = (project(raw, 3), project(flt, 3));
    assert!((fund_flt / fund_raw - 1.0).abs() < 0.02, "the fundamental: {fund_flt} of {fund_raw}");
    assert!(h3_flt / h3_raw < 0.2, "the third harmonic: {h3_flt} of {h3_raw}");
    eprintln!("front end: model vs front end within {worst:.1e}; fundamental {:.3} of raw, third harmonic {:.3} of raw", fund_flt / fund_raw, h3_flt / h3_raw);
}
