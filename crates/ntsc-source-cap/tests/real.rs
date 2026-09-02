//! The real-recording half of M4's gate, plus the machine-side proof of
//! the ingestion path.
//!
//! `arbitrary_units_auto_level_and_recover` runs now, hardware or not:
//! a synthetic capture rescaled into make-believe ADC units (gain 0.31,
//! offset 0.7) must auto-level back to volts and recover as tightly as
//! the volts capture does, or a real file could never be trusted to the
//! same path.
//!
//! `the_real_recording_decodes_to_bars` SKIPS by name until a real
//! capture exists at captures/real-bars.* beside captures/real-bars.toml
//! (see docs/capture-instructions.md); REQUIRE_REAL=1 makes its absence
//! a failure. Tolerance for the real gate: bar channels within 0.08 of
//! video level, stated in docs/m4-report.md (consumer gear sits looser
//! than Table 1's studio +/-1 IRE).

use ntsc_grid::Phase;
use ntsc_source_cap::ingest::{auto_level, read_capture};
use ntsc_source_cap::{capture_model, recover, Capture};
use ntsc_source_rgb::{burst_axis_offset, encode_video_frame, layout, st170, FIELD1_FIRST_LINE};

#[test]
fn arbitrary_units_auto_level_and_recover() {
    let rgb = ntsc_testgen::smpte_bars75(700, 480);
    let orig = encode_video_frame(&rgb, 700, 480, Phase::new(0));
    let volts = capture_model(&[&orig, &orig], 13_500_000.0, 20.0, 0.0, 0.001, 11);
    // Pretend ADC: arbitrary gain and offset.
    let adc = Capture {
        declared_rate_hz: volts.declared_rate_hz,
        samples: volts.samples.iter().map(|s| s * 0.31 + 0.7).collect(),
    };
    let (levelled, tip, blank) = auto_level(&adc);
    // The measured levels must be the transformed originals: blanking
    // was 0 V -> 0.7 units, the tip -0.286 V -> 0.6113 units.
    assert!((blank - 0.7).abs() < 0.01, "blank found at {blank}");
    assert!((tip - 0.6113).abs() < 0.01, "tip found at {tip}");
    let rec = recover(&levelled);
    assert!((rec.rate_error_ppm - 20.0).abs() < 5.0, "ppm {}", rec.rate_error_ppm);
    let lay = layout();
    let (mut sum, mut n) = (0.0f64, 0u64);
    for line in FIELD1_FIRST_LINE..260 {
        for i in lay.active_start + 20..lay.line_len - 20 {
            sum += (rec.frame.lines[line].samples[i] - orig.lines[line].samples[i]).abs() as f64;
            n += 1;
        }
    }
    let mean = sum / n as f64;
    assert!(mean < 0.007, "grid mean {mean} V after auto-levelling");
}

#[test]
fn the_real_recording_decodes_to_bars() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../captures");
    let meta = dir.join("real-bars.toml");
    if !meta.exists() {
        if std::env::var("REQUIRE_REAL").map(|v| v == "1").unwrap_or(false) {
            panic!("REQUIRE_REAL=1 but captures/real-bars.toml is absent: see docs/capture-instructions.md");
        }
        eprintln!("SKIP: no real recording at captures/real-bars.* (docs/capture-instructions.md); the M4 real gate is not exercised");
        return;
    }
    // Minimal metadata: file = "real-bars.wav", format = "wav",
    // rate_hz optional (WAV header wins when absent).
    let text = std::fs::read_to_string(&meta).unwrap();
    let field = |k: &str| {
        text.lines()
            .find(|l| l.trim_start().starts_with(k))
            .and_then(|l| l.split('=').nth(1))
            .map(|v| v.trim().trim_matches('"').to_string())
    };
    let file = field("file").expect("real-bars.toml: file = \"...\"");
    let format = field("format").unwrap_or_else(|| "wav".into());
    let rate = field("rate_hz").and_then(|v| v.parse::<f64>().ok());
    let raw = read_capture(&dir.join(file), &format, rate);
    let (cap, ..) = auto_level(&raw);
    let rec = recover(&cap);
    assert!(
        rec.worst_burst_residual < 0.2,
        "burst residual {} grid samples on the real recording",
        rec.worst_burst_residual
    );
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
                mean[c] += s[c] / 120.0;
            }
        }
        for c in 0..3 {
            assert!(
                (mean[c] - want[c]).abs() < 0.08,
                "bar {bar} channel {c}: {} vs {} (stated real tolerance 0.08)",
                mean[c],
                want[c]
            );
        }
    }
}
