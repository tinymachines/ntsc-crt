//! Rungs B, C and D held to the geometry's own residues: the two-line
//! comb refused on the NES profile by name, the three-line comb's exact
//! cancellation shown on the stripe frame (the M2 gate), and the
//! temporal comb measured rather than believed: the spec's two-frame
//! claim fails confirmation, and the numbers that replace it are
//! asserted here.
//!
//! MUTATE=1 perturbs the three-line comb weights; the cancellation
//! tests must go red.

use ntsc_decode::{tables, Decoder, TemporalComb};
use ntsc_grid::{CompositeSource, FrameParity, Phase, Profile};
use ntsc_source_nes::{burst_axis_offset, encode_frame, levels, DotFrame, Levels, NesSource};

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

fn nes_decoder(mut d: Decoder) -> Decoder {
    if mutate() {
        d.comb_weights = [0.5, 0.3, 0.2];
    }
    d
}

fn notch() -> Decoder {
    Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2])
}

fn comb3() -> Decoder {
    nes_decoder(Decoder::comb_three_line(
        Profile::Nes,
        burst_axis_offset(),
        levels::LOW[1],
        levels::HIGH[2],
    ))
}

/// RMS of the chroma-band content of a row, via the same generated
/// bandpass Rung A uses (an instrument here, not the thing under test).
fn chroma_band_rms(row: &[f32]) -> f32 {
    let taps = &tables::CHROMA_BANDPASS;
    let half = taps.len() / 2;
    let n = row.len();
    let mut acc = 0.0f32;
    for i in half..n - half {
        let c: f32 = taps
            .iter()
            .enumerate()
            .map(|(k, t)| t * row[i + k - half])
            .sum();
        acc += c * c;
    }
    (acc / (n - 2 * half) as f32).sqrt()
}

#[test]
#[should_panic(expected = "Rung B (two-line comb) cannot work on the NES profile")]
fn the_two_line_comb_is_refused_on_the_nes_profile() {
    Decoder::comb_two_line(Profile::Nes, 0.0, 0.0, 1.0);
}

/// The subcarrier-LOCKED content of a row: synchronous demodulation
/// averaged over whole cycles. This, not band energy, is what "chroma"
/// means in the comb claims: the stripe frame's dot-rate luma square
/// (2.685 MHz) sits inside the chroma band and a comb rightly keeps it
/// in luma, and its off-carrier sidebands beat away under this
/// projection. The first version of this test used band RMS and
/// convicted a comb that was working.
fn locked_chroma(row: &[f32], frame: &ntsc_grid::CompositeFrame, line: usize, start: usize) -> f32 {
    let theta0 = burst_axis_offset();
    let (mut u, mut v) = (0.0f64, 0.0f64);
    for (i, s) in row.iter().enumerate() {
        let p = frame.phase_at(line, start + i).get() as f64;
        let th = std::f64::consts::TAU * p / 12.0 + theta0;
        u += *s as f64 * th.sin();
        v += *s as f64 * th.cos();
    }
    let n = row.len() as f64 / 2.0;
    ((u / n).powi(2) + (v / n).powi(2)).sqrt() as f32
}

#[test]
fn the_nes_three_line_comb_cancels_chroma_on_the_stripe_frame() {
    // The M2 gate: vertical stripes are vertically uniform colour, three
    // consecutive lines sit at 0/120/240 degrees, and the three-line sum
    // cancels the subcarrier-locked chroma exactly.
    let dots = ntsc_testgen::stripes(FrameParity::Even, 0x16, 0x2a);
    let frame = encode_frame(&Levels::transcribed(), &dots, Phase::new(0));
    let scale = levels::HIGH[2] - levels::LOW[1];
    let start = frame.lines[100].active_start;
    let raw: Vec<f32> = frame.lines[100].samples[start..][..2040]
        .iter()
        .map(|s| (s - levels::LOW[1]) / scale)
        .collect();
    let yuv = comb3().decode_yuv(&frame, 100, 1, 2040);
    let raw_locked = locked_chroma(&raw, &frame, 100, start);
    let luma_locked = locked_chroma(&yuv.y[..2040], &frame, 100, start);
    assert!(raw_locked > 0.05, "the stripe frame must carry chroma: {raw_locked}");
    assert!(
        luma_locked < raw_locked * 1e-2,
        "three-line luma keeps locked chroma: {luma_locked} of {raw_locked}"
    );
    // And the chroma path still sees it: the demodulated saturation is
    // in the same range the notch rung measures.
    let notch_yuv = notch().decode_yuv(&frame, 100, 1, 2040);
    let sat = |y: &ntsc_decode::YuvFrame, o: usize| {
        (y.u[o] * y.u[o] + y.v[o] * y.v[o]).sqrt()
    };
    let (a, b) = (sat(&yuv, 1024), sat(&notch_yuv, 1024));
    assert!(
        (a / b - 1.0).abs() < 0.2,
        "comb3 chroma {a} vs notch {b} on the stripes"
    );
}

#[test]
fn the_three_line_comb_removes_the_fundamental_the_notch_only_attenuates() {
    // On a solid colour: the comb's luma is the three-line mean, whose
    // fundamental cancels exactly; the notch's luma keeps (1 - gain) of
    // it plus every harmonic. Both keep the third harmonic (in phase
    // line to line), so the instrument is the chroma BAND here, where
    // only the fundamental lives on a solid frame.
    let dots = DotFrame::filled(FrameParity::Even, 0x16, 0);
    let frame = encode_frame(&Levels::transcribed(), &dots, Phase::new(0));
    let comb = chroma_band_rms(&comb3().decode_yuv(&frame, 100, 1, 2048).y);
    let notch_r = chroma_band_rms(&notch().decode_yuv(&frame, 100, 1, 2048).y);
    assert!(notch_r > 0.01, "the notch residue is the baseline: {notch_r}");
    assert!(comb < 1e-3, "comb3 luma fundamental {comb}");
}

/// Demodulated saturation of a solid-colour frame decoded through a
/// temporal comb over the given parity sequence, next to the same
/// frame's single-frame notch saturation.
fn temporal_sat_ratio(frames_window: usize, parities: &[FrameParity]) -> (f32, f32) {
    let colour = 0x16;
    let dots: Vec<DotFrame> = parities
        .iter()
        .map(|&p| DotFrame::filled(p, colour, 0))
        .collect();
    let mut src = NesSource::new(dots, Phase::new(0));
    let dec = notch();
    let mut comb = TemporalComb::new(frames_window);
    let mut last = None;
    let mut single_sat = 0.0f32;
    for _ in 0..parities.len() {
        let frame = src.next_frame();
        let one = dec.decode_yuv(&frame, 100, 1, 2048);
        single_sat = (one.u[1024] * one.u[1024] + one.v[1024] * one.v[1024]).sqrt();
        last = comb.push_and_decode(frame, &dec, 100, 1, 2048);
    }
    let yuv = last.expect("window must fill");
    let sat = (yuv.u[1024] * yuv.u[1024] + yuv.v[1024] * yuv.v[1024]).sqrt();
    // Chroma leakage left in the comb's luma: chroma-band content, which
    // on a solid frame is the fundamental alone (the harmonics are in
    // phase frame to frame and legitimately stay in luma, exactly as
    // they stay in every other rung's luma).
    let leak = chroma_band_rms(&yuv.y);
    (sat / single_sat, leak)
}

#[test]
fn the_two_frame_temporal_comb_attenuates_to_half_and_cannot_cancel() {
    // Spec v0.2 section 5 claims alternate frames are 180 degrees apart
    // with rendering enabled and that two-frame averaging cancels
    // chroma. Section 3.2's own residues say 4 and 8 samples: 120 and
    // 240 degrees, never 180. Measured: the averaged luma retains half
    // the chroma (|1 + e^(i120)| / 2), and the comb's chroma output is
    // 0.866 of a single frame's (|1 - e^(-i120)| / 2). Recorded for the
    // spec's v0.3; OddShort is exercised through decode here, which was
    // the spec's stated reason for wanting this rung.
    let (sat_ratio, leak) = temporal_sat_ratio(
        2,
        &[FrameParity::Even, FrameParity::OddShort, FrameParity::Even, FrameParity::OddShort],
    );
    assert!(
        (sat_ratio - 0.866).abs() < 0.03,
        "two-frame chroma ratio {sat_ratio}, phasors say 0.866"
    );
    assert!(leak > 0.05, "the averaged luma must retain chroma: {leak}");
}

#[test]
fn the_three_frame_temporal_comb_cancels_on_the_full_frame_pattern() {
    // Three full frames (rendering disabled: residue 4 each) put the
    // three copies at 0/120/240 degrees: the mean cancels chroma
    // exactly, with no vertical cost. This is the honest Rung D.
    let (sat_ratio, leak) = temporal_sat_ratio(
        3,
        &[FrameParity::Even, FrameParity::Even, FrameParity::Even],
    );
    assert!(
        (sat_ratio - 1.0).abs() < 0.02,
        "the chroma path must keep full saturation: {sat_ratio}"
    );
    assert!(leak < 2e-3, "three-frame luma must be clean: {leak}");
}
