//! Score a flat region of a real NES capture against the same colour
//! synthesized through the encoder, both sides decoded identically.
//!   score-real-region <capture.u8> <rate> <colour-hex> <row0> <row1> <x0> <x1>
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_cap::ingest::{auto_level_nes, read_capture};
use ntsc_source_cap::recover_nes;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, rate) = (&a[1], a[2].parse::<f64>().unwrap());
    let colour = u8::from_str_radix(&a[3], 16).unwrap();
    let (row0, row1) = (a[4].parse::<usize>().unwrap(), a[5].parse::<usize>().unwrap());
    let (x0, x1) = (a[6].parse::<usize>().unwrap(), a[7].parse::<usize>().unwrap());

    let raw = read_capture(std::path::Path::new(path), "u8", Some(rate));
    let (cap, ..) = auto_level_nes(&raw);
    let rec = recover_nes(&cap);
    println!(
        "recovered: {:+.1} ppm, worst burst residual {:.3}",
        rec.rate_error_ppm, rec.worst_burst_residual
    );

    let levels = ntsc_source_nes::Levels::transcribed();
    let dots = ntsc_testgen::solid(FrameParity::Even, colour, 0);
    let synth = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));

    let dec = ntsc_decode::Decoder::transcribed(
        ntsc_source_nes::burst_axis_offset(),
        ntsc_source_nes::levels::LOW[1],
        ntsc_source_nes::levels::HIGH[2],
    );
    let width = 2048usize;
    let mean = |f: &ntsc_grid::CompositeFrame| -> (f64, f64, f64) {
        let y = dec.decode_yuv(f, row0, row1 - row0, width);
        let (mut my, mut mu, mut mv, mut n) = (0.0f64, 0.0, 0.0, 0.0);
        for r in 0..row1 - row0 {
            for x in x0..x1 {
                my += y.y[r * width + x] as f64;
                mu += y.u[r * width + x] as f64;
                mv += y.v[r * width + x] as f64;
                n += 1.0;
            }
        }
        (my / n, mu / n, mv / n)
    };
    let (ry, ru, rv) = mean(&rec.frame);
    let (sy, su, sv) = mean(&synth);
    let sat_r = (ru * ru + rv * rv).sqrt();
    let sat_s = (su * su + sv * sv).sqrt();
    let hue_r = rv.atan2(ru).to_degrees();
    let hue_s = sv.atan2(su).to_degrees();
    println!("region rows {row0}..{row1} x {x0}..{x1} vs solid ${colour:02x}:");
    println!("  real:  Y {ry:+.4}  U {ru:+.4}  V {rv:+.4}  sat {sat_r:.4}  hue {hue_r:+.1} deg");
    println!("  synth: Y {sy:+.4}  U {su:+.4}  V {sv:+.4}  sat {sat_s:.4}  hue {hue_s:+.1} deg");
    println!(
        "  delta: Y {:+.4}  sat {:+.4}  hue {:+.1} deg",
        ry - sy,
        sat_r - sat_s,
        hue_r - hue_s
    );
}
