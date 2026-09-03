//! The real-recording tool: a capture file in, the measurements and a
//! decoded picture out. Run with --release.
//!
//!   cargo run --release -p ntsc-source-cap --example recover-real -- \
//!       captures/real-bars.wav [wav|f32|i16|u8] [rate_hz] [--bars]
//!
//! Prints the recovery measurements (rate error, burst residual, levels
//! found), writes the decoded field as goldens/real-capture.ppm for
//! eyeballing, and with --bars checks the seven bar centres against the
//! published column at the stated real-recording tolerance (0.08 of
//! video level; consumer gear sits looser than Table 1's studio +/-1
//! IRE, and the number is stated here rather than implied).

use ntsc_decode::Decoder;
use ntsc_source_cap::ingest::{auto_level, auto_level_nes, read_capture};
use ntsc_source_cap::{recover, recover_nes};
use ntsc_source_rgb::{burst_axis_offset, layout, st170, FIELD1_FIRST_LINE};
use std::io::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: recover-real <file> [format] [rate_hz] [--bars] [--nes]");
    let format = args.get(2).map(|s| s.as_str()).unwrap_or("wav");
    let rate = args.get(3).and_then(|s| s.parse::<f64>().ok());
    let bars = args.iter().any(|a| a == "--bars");
    let nes = args.iter().any(|a| a == "--nes");

    let raw = read_capture(std::path::Path::new(path), format, rate);
    println!(
        "read {}: {} samples at a declared {:.1} Hz ({:.2} s)",
        path,
        raw.samples.len(),
        raw.declared_rate_hz,
        raw.samples.len() as f64 / raw.declared_rate_hz
    );
    let (cap, tip, blank) = if nes { auto_level_nes(&raw) } else { auto_level(&raw) };
    println!("auto-level: sync tip {tip:.4}, blanking {blank:.4} (original units)");
    let rec = if nes { recover_nes(&cap) } else { recover(&cap) };
    println!(
        "recovered: rate error {:+.1} ppm, worst burst residual {:.3} grid samples, anchor line {}",
        rec.rate_error_ppm, rec.worst_burst_residual, rec.anchor_line
    );

    let lay = layout();
    let (active_len, first_line) = if nes {
        (2048usize, 0usize)
    } else {
        (lay.line_len - lay.active_start, FIELD1_FIRST_LINE)
    };
    let dec = if nes {
        // The NES profile speaks the transcribed table's absolute
        // volts, so the decoder constants are the oracle's own.
        Decoder::transcribed(
            ntsc_source_nes::burst_axis_offset(),
            ntsc_source_nes::levels::LOW[1],
            ntsc_source_nes::levels::HIGH[2],
        )
    } else {
        Decoder::transcribed(
            burst_axis_offset(),
            st170::BLACK_IRE / st170::IRE_PER_VOLT,
            st170::WHITE_IRE / st170::IRE_PER_VOLT,
        )
    };
    // Decode the picture lines and write them for eyeballing.
    let rgb = dec.decode(&rec.frame, first_line, 240, active_len);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../goldens");
    std::fs::create_dir_all(&out).unwrap();
    let mut ppm = Vec::new();
    write!(ppm, "P6\n{} {}\n255\n", active_len, 240).unwrap();
    for i in 0..active_len * 240 {
        let s = rgb.signal_rgb(i);
        for c in s {
            ppm.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    let ppm_path = out.join("real-capture.ppm");
    std::fs::write(&ppm_path, ppm).unwrap();
    println!("wrote {} ({active_len} x 240, field 1)", ppm_path.display());

    if bars {
        let row = dec.decode_yuv(&rec.frame, FIELD1_FIRST_LINE + 5, 1, active_len);
        let rgbrow = dec.to_linear_rgb(&row);
        const BAR_RGB: [[f32; 3]; 7] = [
            [0.75, 0.75, 0.75],
            [0.75, 0.75, 0.0],
            [0.0, 0.75, 0.75],
            [0.0, 0.75, 0.0],
            [0.75, 0.0, 0.75],
            [0.75, 0.0, 0.0],
            [0.0, 0.0, 0.75],
        ];
        let mut worst = 0.0f32;
        for (bar, want) in BAR_RGB.iter().enumerate() {
            let center = (2 * bar + 1) * active_len / 14;
            let mut mean = [0.0f32; 3];
            for x in center - 60..center + 60 {
                let s = rgbrow.signal_rgb(x);
                for c in 0..3 {
                    mean[c] += s[c] / 120.0;
                }
            }
            for c in 0..3 {
                worst = worst.max((mean[c] - want[c]).abs());
            }
            println!(
                "bar {bar}: decoded ({:.3} {:.3} {:.3}) vs ({:.2} {:.2} {:.2})",
                mean[0], mean[1], mean[2], want[0], want[1], want[2]
            );
        }
        println!("worst channel error {worst:.3} against the stated tolerance 0.08");
        assert!(worst < 0.08, "the real recording does not decode to bars");
        println!("M4 real-recording gate: PASS");
    }
}
