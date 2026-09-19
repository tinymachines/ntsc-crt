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

use ntsc_grid::{FrameParity, Phase};
use ntsc_source_cap::ingest::{auto_level, read_capture};
use ntsc_source_cap::{capture_model, recover, recover_nes, Capture};
use ntsc_source_rgb::{burst_axis_offset, encode_video_frame, layout, st170, FIELD1_FIRST_LINE};

#[test]
fn a_synthetic_nes_capture_recovers_onto_the_original_grid() {
    // Three chained frames of a saturated pattern through the NES
    // encoder and the capture model. The recovery anchors on the frame
    // after the first vertical sync group, so the stream is arranged
    // with origins 8, 0, 4 (a valid chain: the frame residue is 4) and
    // the anchored frame is the origin-0 one; the recovered grid must
    // then sit on that frame's own samples, which also pins the
    // FIRST_BROAD_ROW anchor mapping (one row off would miss by a
    // whole line).
    // Eight 32-dot colour bands: wide enough that their luma sits far
    // inside the capture channel's 6.5 MHz. A per-dot pattern was tried
    // first and taught the comparison rules below: the channel
    // legitimately removes the square chroma's harmonics and softens
    // luma edges, so POINTWISE luma against the unfiltered encoder can
    // never match; pointwise CHROMA can (the decoder's bandpass leaves
    // both sides fundamental-only), and REGIONAL luma means can (band
    // interiors survive the channel exactly).
    let levels = ntsc_source_nes::Levels::transcribed();
    let mut dots = ntsc_testgen::solid(FrameParity::Even, 0x0f, 0);
    let bands = [0x16u8, 0x2a, 0x12, 0x28, 0x14, 0x26, 0x1a, 0x30];
    for row in 0..240 {
        for dot in 1..257 {
            dots.set(row, dot, bands[(dot - 1) / 32], 0);
        }
    }
    let fx = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(8));
    let f0 = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));
    let f1 = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(4));
    let cap = capture_model(&[&fx, &f0, &f1], 13_500_000.0, 20.0, 0.0, 0.001, 11);
    let rec = recover_nes(&cap);
    assert!((rec.rate_error_ppm - 20.0).abs() < 5.0, "ppm {}", rec.rate_error_ppm);
    assert!(
        rec.worst_burst_residual < 0.2,
        "burst residual {}",
        rec.worst_burst_residual
    );
    // The recovery speaks the table's absolute volts, so the decoder
    // constants are the oracle's own.
    let dec = ntsc_decode::Decoder::transcribed(
        ntsc_source_nes::burst_axis_offset(),
        ntsc_source_nes::levels::LOW[1],
        ntsc_source_nes::levels::HIGH[2],
    );
    let a = dec.decode_yuv(&rec.frame, 8, 224, 2000);
    let b = dec.decode_yuv(&f0, 8, 224, 2000);
    // Chroma, pointwise (measured 0.0023 on the run that pinned this).
    let chroma_mean = |x: &ntsc_decode::YuvFrame, y: &ntsc_decode::YuvFrame| -> f64 {
        let mut sum = 0.0f64;
        for o in 0..2000 * 224 {
            sum += ((x.u[o] - y.u[o]).abs() + (x.v[o] - y.v[o]).abs()) as f64;
        }
        sum / (2000.0 * 224.0)
    };
    let du = chroma_mean(&a, &b);
    assert!(du < 0.006, "decoded chroma mean {du} against the encoder's own frame");
    // Luma, per band interior (measured worst 0.0010).
    for (band, _) in bands.iter().enumerate() {
        let lo = (1 + band * 32) * 8 + 32 - 8;
        let hi = (1 + band * 32 + 32) * 8 - 32 - 8;
        let (mut ma, mut mb, mut n) = (0.0f64, 0.0f64, 0.0f64);
        for r in 8..216usize {
            for x in lo..hi {
                ma += a.y[r * 2000 + x] as f64;
                mb += b.y[r * 2000 + x] as f64;
                n += 1.0;
            }
        }
        let d = (ma / n - mb / n).abs();
        assert!(d < 0.005, "band {band} luma mean off by {d}");
    }

    // The mutation that started all of this: the same capture through
    // the BROADCAST recovery must not land on the frame. That is
    // exactly the first real console capture's failure (a broadcast
    // phase model chasing an NES signal), kept as the proof the
    // profile is load-bearing. The hue roll shows up as chroma error;
    // the level convention is equalized first so geometry is the only
    // thing under test.
    let wrong = std::panic::catch_unwind(|| recover(&cap));
    if let Ok(mut wrong) = wrong {
        for l in &mut wrong.frame.lines {
            for v in &mut l.samples {
                *v += ntsc_source_nes::levels::BLANK;
            }
        }
        let w = dec.decode_yuv(&wrong.frame, 8, 224, 2000);
        let wdu = chroma_mean(&w, &b);
        assert!(
            wdu > 10.0 * du,
            "the broadcast recovery matched an NES capture ({wdu} vs {du}): the profile is not load-bearing"
        );
    }
    // (A panic inside the broadcast path is an acceptable form of NO:
    // its 525-line frame does not fit this capture's geometry.)
}

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

/// The re-referencing's precision, NES profile: a modelled capture with
/// a DC offset must come back with blanking within 1 mV of the table's
/// and the sync depth within half a percent, or every luma the capture
/// gate scores carries the error as gain (the console's N6 gate read
/// 3.7% before the levels were read off the sync pulses and porches),
/// and a picture level below blanking must not be taken for it.
#[test]
fn nes_auto_level_finds_the_levels_finer_than_a_histogram_bin() {
    // A solid dark colour: half its picture samples sit below blanking
    // (row-1 low level), the case the histogram alone took for it.
    let levels = ntsc_source_nes::Levels::transcribed();
    let dots = ntsc_testgen::solid(FrameParity::Even, 0x16, 0);
    let f = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));
    let volts = capture_model(&[&f, &f, &f], 125_000_000.0, 5.0, 0.020, 0.002, 6);
    let (levelled, tip, blank) = ntsc_source_cap::ingest::auto_level_nes(&volts);
    let (nes_blank, nes_sync) = (ntsc_source_nes::levels::BLANK, ntsc_source_nes::levels::SYNC);
    // The model added 20 mV: the peaks it finds are the table's plus that.
    let blank_err = (blank - 0.020 - nes_blank).abs();
    let depth_err = ((blank - tip) / (nes_blank - nes_sync) - 1.0).abs();
    let tol = if std::env::var("MUTATE").is_ok_and(|v| v == "1") { (0.004, 0.02) } else { (0.001, 0.005) };
    assert!(blank_err < tol.0, "blanking found {:.2} mV off", blank_err * 1000.0);
    assert!(depth_err < tol.1, "sync depth {:.2}% off", depth_err * 100.0);
    // And the re-referenced blanking is the table's.
    let lay_blank = levelled.samples.iter().filter(|s| (**s - nes_blank).abs() < 0.01).count();
    assert!(lay_blank > levelled.samples.len() / 20, "re-referenced blanking sits at the table's level");
    eprintln!("blanking {:.2} mV off, sync depth {:.3}% off", blank_err * 1000.0, depth_err * 100.0);
}

/// The same precision on a QUANTISED record, the scope's own: u8 codes
/// sized so the sync is 22.5 codes deep, as the bench's records at 200
/// mV a division read it (the console's output lands on the scope
/// below the table's volts), about a code of noise, and blanking placed 0.45 of a code above a code
/// boundary. A level read as a median can only land on a whole code,
/// which moves the sync depth by a code in 22 and every scored luma by
/// 4.5% (nes-bench's warm-up series stepped exactly so, 2026-09-18);
/// the depth must come back within half a percent. MUTATE_LEVEL=1 reads
/// the median again and must be red.
#[test]
fn nes_auto_level_reads_between_the_codes_of_a_quantised_record() {
    let levels = ntsc_source_nes::Levels::transcribed();
    let dots = ntsc_testgen::solid(FrameParity::Even, 0x16, 0);
    let f = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));
    let (nes_blank, nes_sync) = (ntsc_source_nes::levels::BLANK, ntsc_source_nes::levels::SYNC);
    let code = (nes_blank - nes_sync) / 22.5;
    let volts = capture_model(&[&f, &f, &f], 50_000_000.0, 5.0, 0.0, code, 7);
    // Blanking at code 88.45: the offset that puts it there.
    let offset = 88.45 - nes_blank / code;
    let adc = Capture {
        declared_rate_hz: volts.declared_rate_hz,
        samples: volts.samples.iter().map(|v| (v / code + offset).round().clamp(0.0, 255.0)).collect(),
    };
    let (_, tip, blank) = ntsc_source_cap::ingest::auto_level_nes(&adc);
    let want = (nes_blank - nes_sync) / code;
    let depth_err = ((blank - tip) / want - 1.0).abs();
    eprintln!("quantised: blanking at code {blank:.3} (placed 88.45), sync depth {:.3} codes of {want:.3}, {:.2}% off", blank - tip, depth_err * 100.0);
    assert!((blank - 88.45).abs() < 0.1, "blanking read at code {blank}, placed at 88.45");
    assert!(depth_err < 0.005, "sync depth {:.2}% off on a quantised record (MUTATE_LEVEL=1 is the median: red)", depth_err * 100.0);
}

/// Horizontal registration, whatever the frame's subcarrier origin. The
/// NES starts each frame at one of three phases, a third of a cycle (4
/// grid samples) apart, rotating frame to frame. The recovery used to
/// assume origin 0 and let the burst lock slide every line of a frame
/// that began elsewhere by 4 or 8 samples: colour still decoded right
/// and every band INTERIOR still matched (the test above, whose chain
/// was arranged so the anchored frame had origin 0), but the picture
/// sat half a dot or a dot off. Found by nes's split-score, whose
/// synthetic roundtrip correlated 0.77 with the model's frame and
/// 1.0000 four samples over (2026-09-19). Here the anchored frame has
/// each origin in turn; its luma edges must come back where the
/// encoder put them (the best shift of a luma row within a sample) and
/// the recovered frame must name the origin. MUTATE_ORIGIN=1 assumes 0
/// and must be red.
#[test]
fn the_recovered_picture_sits_on_the_encoder_s_whatever_the_frame_s_origin() {
    let levels = ntsc_source_nes::Levels::transcribed();
    let mut dots = ntsc_testgen::solid(FrameParity::Even, 0x0f, 0);
    // Alternating light and dark bands of uneven widths: luma edges
    // every few dots, none of them periodic enough to alias a shift.
    let widths = [7usize, 13, 5, 19, 11, 9, 23, 6, 17, 10];
    for row in 0..240 {
        let (mut dot, mut k) = (1usize, 0usize);
        while dot < 257 {
            let c = if k % 2 == 0 { 0x30u8 } else { 0x0f };
            for d in dot..(dot + widths[k % widths.len()]).min(257) {
                dots.set(row, d, c, 0);
            }
            dot += widths[k % widths.len()];
            k += 1;
        }
    }
    let dec = ntsc_decode::Decoder::transcribed(
        ntsc_source_nes::burst_axis_offset(),
        ntsc_source_nes::levels::LOW[1],
        ntsc_source_nes::levels::HIGH[2],
    );
    let width = 2000usize;
    for anchored in [0u8, 4, 8] {
        let before = (anchored + 8) % 12;
        let after = (anchored + 4) % 12;
        let fb = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(before));
        let fa = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(anchored));
        let fn_ = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(after));
        let cap = capture_model(&[&fb, &fa, &fn_], 50_000_000.0, 0.0, 0.0, 0.0, 3);
        let rec = recover_nes(&cap);
        let a = dec.decode_yuv(&rec.frame, 60, 100, width);
        let b = dec.decode_yuv(&ntsc_source_cap::front_end(&fa), 60, 100, width);
        let mut best = (0isize, f64::MAX);
        for dx in -12isize..=12 {
            let mut sum = 0.0f64;
            for r in 0..100 {
                for x in 40..width - 40 {
                    sum += (a.y[r * width + (x as isize + dx) as usize] - b.y[r * width + x]).abs() as f64;
                }
            }
            if sum < best.1 {
                best = (dx, sum);
            }
        }
        eprintln!("origin {anchored}: recovered origin {}, best luma shift {} samples", rec.frame.phase_at_origin.get(), best.0);
        assert!(best.0.abs() <= 1, "origin {anchored}: the recovered picture sits {} samples off the encoder's (MUTATE_ORIGIN=1 assumes origin 0: red)", best.0);
        assert_eq!(rec.frame.phase_at_origin.get(), anchored, "the recovered frame names its origin");
    }
}
