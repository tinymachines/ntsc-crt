//! The M2 known-answer tests: SMPTE 75% colour bars encoded by this
//! crate, held to the published bar levels, to clause 10's own numbers,
//! and to a decode roundtrip. Tolerances are justified where they are
//! used, not chosen to pass.
//!
//! MUTATE=1 scales the demodulated U by 1.3 in the roundtrip; it must go
//! red. The waveform tests carry their own perturbation: they compare
//! the published column against the derived one, so a wrong matrix or a
//! wrong setup breaks the agreement.

use ntsc_decode::Decoder;
use ntsc_grid::{Phase, Profile};
use ntsc_source_rgb::{burst_axis_offset, encode_video_frame, layout, st170, FIELD1_FIRST_LINE};

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

const BAR_RGB: [[f32; 3]; 7] = [
    [0.75, 0.75, 0.75],
    [0.75, 0.75, 0.0],
    [0.0, 0.75, 0.75],
    [0.0, 0.75, 0.0],
    [0.75, 0.0, 0.75],
    [0.75, 0.0, 0.0],
    [0.0, 0.0, 0.75],
];

/// The published luma levels of 75% bars with 7.5 IRE setup, as they
/// appear in every waveform-monitor chart: white, yellow, cyan, green,
/// magenta, red, blue. An external anchor, typed with intent: the test
/// also derives the same column from clause 10 and the base matrix, and
/// both must agree.
const PUBLISHED_BAR_IRE: [f32; 7] = [76.9, 69.0, 56.1, 48.2, 36.1, 28.2, 15.4];

fn bars_frame() -> (ntsc_grid::CompositeFrame, usize) {
    let width = 700;
    let height = 40;
    let rgb = ntsc_testgen::smpte_bars75(width, height);
    (encode_video_frame(&rgb, width, height, Phase::new(0)), width)
}

/// Mean voltage over one bar's centre, a whole number of subcarrier
/// cycles so chroma averages out.
fn bar_window(active_start: usize, active_len: usize, bar: usize) -> (usize, usize) {
    let center = active_start + (2 * bar + 1) * active_len / 14;
    (center - 60, center + 60) // 120 samples = 10 cycles
}

#[test]
fn the_waveform_carries_the_published_bar_levels() {
    let (frame, _) = bars_frame();
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let line = &frame.lines[FIELD1_FIRST_LINE + 5];
    #[allow(clippy::needless_range_loop)]
    for bar in 0..7 {
        let (lo, hi) = bar_window(lay.active_start, active_len, bar);
        let mean_ire = line.samples[lo..hi].iter().sum::<f32>() / (hi - lo) as f32
            * st170::IRE_PER_VOLT;
        // Derived from clause 10: 0.925 * 100 * Y + 7.5.
        let rgb = BAR_RGB[bar];
        let y = st170::BASE_Y[0] * rgb[0] + st170::BASE_Y[1] * rgb[1] + st170::BASE_Y[2] * rgb[2];
        let derived = st170::Y_SCALE * 100.0 * y + st170::SETUP_IRE;
        // Table 1's recommended tolerance is +/-1 IRE; the published
        // column is printed to one decimal, so both comparisons get 1.0.
        assert!(
            (mean_ire - derived).abs() < 1.0,
            "bar {bar}: waveform {mean_ire:.1} IRE, clause 10 {derived:.1}"
        );
        assert!(
            (mean_ire - PUBLISHED_BAR_IRE[bar]).abs() < 1.0,
            "bar {bar}: waveform {mean_ire:.1} IRE, published {:.1}",
            PUBLISHED_BAR_IRE[bar]
        );
    }
    // Sync tip and blanking, Table 1.
    assert!(
        (line.samples[lay.href + 100] * st170::IRE_PER_VOLT - st170::SYNC_IRE).abs() < 0.5
    );
    assert!((line.samples[10] * st170::IRE_PER_VOLT).abs() < 0.5, "blanking at 0 IRE");
}

#[test]
fn burst_and_chroma_amplitudes_match_clause_10() {
    let (frame, _) = bars_frame();
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let line = &frame.lines[FIELD1_FIRST_LINE + 5];
    // Burst: 40 IRE peak to peak (Table 1). The grid samples a sine at
    // 12 points per cycle with phase offsets, so the sampled peak can
    // sit up to 15 degrees off the true peak: cos(15 deg) = 0.966 of
    // the amplitude. Tolerance covers exactly that.
    let burst: Vec<f32> = line.samples[lay.burst_start..lay.burst_end]
        .iter()
        .map(|v| v * st170::IRE_PER_VOLT)
        .collect();
    let pp = burst.iter().cloned().fold(f32::MIN, f32::max)
        - burst.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        (st170::BURST_PP_IRE * 0.966 - 0.1..=st170::BURST_PP_IRE + 0.1).contains(&pp),
        "burst peak-to-peak {pp:.2} IRE"
    );
    // Each bar's chroma amplitude: RMS over the window times sqrt(2),
    // against clause 10's U/V scales on the matrixed differences.
    // Tolerance 3%: the sampled sine's RMS is exact on whole cycles, so
    // the slack is for the window edges.
    #[allow(clippy::needless_range_loop)]
    for bar in 0..7 {
        let (lo, hi) = bar_window(lay.active_start, active_len, bar);
        let w: Vec<f32> = line.samples[lo..hi]
            .iter()
            .map(|v| v * st170::IRE_PER_VOLT)
            .collect();
        let mean = w.iter().sum::<f32>() / w.len() as f32;
        let rms = (w.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>()
            / w.len() as f32)
            .sqrt();
        let amp = rms * std::f32::consts::SQRT_2;
        let rgb = BAR_RGB[bar];
        let dot = |m: &[f32; 3]| m[0] * rgb[0] + m[1] * rgb[1] + m[2] * rgb[2];
        let want = ((st170::U_SCALE * 100.0 * dot(&st170::BASE_BMY)).powi(2)
            + (st170::V_SCALE * 100.0 * dot(&st170::BASE_RMY)).powi(2))
        .sqrt();
        if want < 1.0 {
            assert!(amp < 1.0, "bar {bar} should be near-grey: {amp:.2} IRE");
        } else {
            assert!(
                (amp / want - 1.0).abs() < 0.03,
                "bar {bar}: chroma {amp:.2} IRE, clause 10 {want:.2}"
            );
        }
    }
}

#[test]
fn bars_roundtrip_within_two_percent() {
    // Encode, decode with Rung A at the clause-10 references (black 7.5
    // IRE, white 100 IRE, demod offset 0 by the burst convention), and
    // the bar centres must come back within 0.02 of the video input.
    // Justification: the U/V lowpass is unity at DC, the chroma
    // bandpass's passband ripple and the luma complement's harmonic
    // leakage average out over the 10-cycle window, leaving encoder
    // lowpass edge effects, all measured below 2%.
    let (frame, _) = bars_frame();
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let dec = Decoder::transcribed(
        burst_axis_offset(),
        st170::BLACK_IRE / st170::IRE_PER_VOLT,
        st170::WHITE_IRE / st170::IRE_PER_VOLT,
    );
    let line = FIELD1_FIRST_LINE + 5;
    let mut yuv = dec.decode_yuv(&frame, line, 1, active_len);
    if mutate() {
        for u in &mut yuv.u {
            *u *= 1.3;
        }
    }
    let rgb = dec.to_linear_rgb(&yuv);
    #[allow(clippy::needless_range_loop)]
    for bar in 0..7 {
        let (lo, hi) = bar_window(0, active_len, bar);
        let mut mean = [0.0f32; 3];
        for x in lo..hi {
            let s = rgb.signal_rgb(x);
            for c in 0..3 {
                mean[c] += s[c];
            }
        }
        for (c, m) in mean.iter_mut().enumerate() {
            *m /= (hi - lo) as f32;
            assert!(
                (*m - BAR_RGB[bar][c]).abs() < 0.02,
                "bar {bar} channel {c}: decoded {m:.3}, input {:.3}",
                BAR_RGB[bar][c]
            );
        }
    }
}

#[test]
fn the_two_line_comb_works_on_broadcast_where_it_is_refused_on_nes() {
    // Rung B on its home profile: the comb luma's chroma-band energy on
    // a bars line collapses against the raw line's.
    let (frame, _) = bars_frame();
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let dec = Decoder::comb_two_line(
        Profile::Broadcast,
        burst_axis_offset(),
        st170::BLACK_IRE / st170::IRE_PER_VOLT,
        st170::WHITE_IRE / st170::IRE_PER_VOLT,
    );
    let line = FIELD1_FIRST_LINE + 5;
    let yuv = dec.decode_yuv(&frame, line, 1, active_len);
    let taps = &ntsc_decode::tables::CHROMA_BANDPASS;
    let half = taps.len() / 2;
    let band_rms = |row: &[f32]| {
        let mut acc = 0.0f32;
        for i in half..row.len() - half {
            let c: f32 = taps.iter().enumerate().map(|(k, t)| t * row[i + k - half]).sum();
            acc += c * c;
        }
        (acc / (row.len() - 2 * half) as f32).sqrt()
    };
    let scale = (st170::WHITE_IRE - st170::BLACK_IRE) / st170::IRE_PER_VOLT;
    let raw: Vec<f32> = frame.lines[line].samples[lay.active_start..]
        .iter()
        .map(|s| (s - st170::BLACK_IRE / st170::IRE_PER_VOLT) / scale)
        .collect();
    let raw_rms = band_rms(&raw);
    let luma_rms = band_rms(&yuv.y);
    assert!(raw_rms > 0.05, "bars must carry chroma: {raw_rms}");
    assert!(
        luma_rms < raw_rms * 0.02,
        "two-line comb luma keeps chroma on broadcast: {luma_rms} of {raw_rms}"
    );
}
