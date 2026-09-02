//! Rung A held to the encoder's physics without any external oracle yet:
//! greys carry no chroma, luma tracks the level table, the twelve hues
//! land at the angles the wave formula predicts with the amplitude a
//! square wave's fundamental predicts, and the matrix constants reach the
//! output. The blargg comparison lives in ntsc-oracle.
//!
//! MUTATE=1 perturbs the U/V lowpass centre tap and one inverse-matrix
//! coefficient on the decoder under test (spec section 5); the hue-ladder
//! amplitude and the matrix test must go red. The grey and luma tests
//! stay green by design: greys have no chroma for either perturbation to
//! touch, and luma passes through neither path.

use ntsc_decode::Decoder;
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{burst_axis_offset, encode_frame, levels, wave_high, DotFrame, Levels};

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

fn decoder() -> Decoder {
    let mut d = Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
    if mutate() {
        let mid = d.uv_taps.len() / 2;
        d.uv_taps[mid] += 0.05;
        d.r_from_v += 0.3;
    }
    d
}

fn solid_yuv(colour: u8) -> (f32, f32, f32) {
    let frame = encode_frame(
        &Levels::transcribed(),
        &DotFrame::filled(FrameParity::Even, colour, 0),
        Phase::new(0),
    );
    let yuv = decoder().decode_yuv(&frame, 100, 1, 2048);
    // Mid-line, clear of every filter edge.
    let o = 1024;
    (yuv.y[o], yuv.u[o], yuv.v[o])
}

/// The fundamental of wave `c` projected on the demodulation axes: the
/// prediction the decoder is held to, from the same wave formula the
/// encoder runs.
fn wave_uv(c: u8, theta0: f64) -> (f64, f64) {
    let mut u = 0.0;
    let mut v = 0.0;
    for p in 0..12u8 {
        let s = if wave_high(c, p) { 1.0 } else { -1.0 };
        let th = std::f64::consts::TAU * p as f64 / 12.0 + theta0;
        u += s * th.sin();
        v += s * th.cos();
    }
    (u, v)
}

#[test]
fn greys_decode_with_no_chroma() {
    for colour in [0x00u8, 0x10, 0x20, 0x0d, 0x3d] {
        let (_, u, v) = solid_yuv(colour);
        assert!(
            u.abs() < 0.01 && v.abs() < 0.01,
            "colour {colour:02x}: u {u} v {v}"
        );
    }
}

#[test]
fn luma_tracks_the_level_table() {
    // A solid grey's decoded luma is its voltage normalized black..white
    // ($1D..$20), straight from the generated constants.
    let scale = levels::HIGH[2] - levels::LOW[1];
    for (colour, volts) in [
        (0x00u8, levels::HIGH[0]),
        (0x10, levels::HIGH[1]),
        (0x20, levels::HIGH[2]),
        (0x0d, levels::LOW[0]),
        (0x3d, levels::LOW[3]),
    ] {
        let (y, ..) = solid_yuv(colour);
        let want = (volts - levels::LOW[1]) / scale;
        assert!(
            (y - want).abs() < 0.01,
            "colour {colour:02x}: y {y}, table {want}"
        );
    }
}

#[test]
fn the_hue_ladder_lands_where_the_wave_formula_points() {
    let theta0 = burst_axis_offset();
    // A square wave of amplitude a has fundamental (4/pi) a; a is half the
    // high-low swing, normalized like luma.
    let a = (levels::HIGH[1] - levels::LOW[1]) / 2.0 / (levels::HIGH[2] - levels::LOW[1]);
    let want_sat = 4.0 / std::f64::consts::PI * a as f64;
    for hue in 1..=12u8 {
        let (_, u, v) = solid_yuv(0x10 | hue);
        let (eu, ev) = wave_uv(hue, theta0);
        let angle = (v as f64).atan2(u as f64).to_degrees();
        let expect = ev.atan2(eu).to_degrees();
        let mut diff = (angle - expect).rem_euclid(360.0);
        if diff > 180.0 {
            diff -= 360.0;
        }
        assert!(diff.abs() < 3.0, "hue {hue}: angle {angle:.1}, wave {expect:.1}");
        let sat = ((u * u + v * v) as f64).sqrt();
        assert!(
            (sat - want_sat).abs() / want_sat < 0.10,
            "hue {hue}: saturation {sat:.4}, fundamental {want_sat:.4}"
        );
    }
}

#[test]
fn rgb_applies_the_transcribed_matrix() {
    let d = decoder();
    let frame = encode_frame(
        &Levels::transcribed(),
        &DotFrame::filled(FrameParity::Even, 0x16, 0),
        Phase::new(0),
    );
    let yuv = d.decode_yuv(&frame, 100, 1, 2048);
    let rgb = d.to_linear_rgb(&yuv);
    let o = 1024;
    let (y, u, v) = (yuv.y[o], yuv.u[o], yuv.v[o]);
    let want = [
        y + ntsc_decode::tables::R_FROM_V * v,
        y + ntsc_decode::tables::G_FROM_U * u + ntsc_decode::tables::G_FROM_V * v,
        y + ntsc_decode::tables::B_FROM_U * u,
    ];
    let got = rgb.signal_rgb(o);
    for c in 0..3 {
        assert!(
            (got[c] - want[c].clamp(0.0, 1.0)).abs() < 1e-3,
            "channel {c}: {} vs {}",
            got[c],
            want[c]
        );
    }
}
