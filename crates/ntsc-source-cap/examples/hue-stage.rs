//! Which stage owes the hue: a flat region of a real NES capture and the
//! same colour through the encoder, measured at TWO stages. The signal
//! stage is the raw chroma phase against the burst, read straight off
//! the composite samples with no decoder in the way (the projection of
//! the region's samples and of the burst's onto the subcarrier's sine
//! and cosine at the grid's own phase); the decoded stage is what
//! `score-real-region` scores, both sides through the one decoder. A
//! difference that is already there at the signal stage is the model's
//! signal (the encoder's wave table or its burst); one that appears only
//! at the decoded stage is the decoder's reading of the two signals.
//!
//!   hue-stage <capture.u8> <rate> <colour-hex> <row0> <row1> <x0> <x1> [burst_len]
//!
//! x0, x1 in decoded-grid samples (eight per console pixel); rows are
//! the recovered frame's lines, which for the NES profile are the
//! picture's rows from 0. burst_len is samples of burst projected from
//! each line's `burst_start` (default 96, eight cycles of twelve).
use ntsc_grid::{CompositeFrame, FrameParity, Phase};
use ntsc_source_cap::ingest::{auto_level_nes, read_capture};
use ntsc_source_cap::recover_nes;

/// The region's and the burst's chroma phase on each line of `rows`,
/// summed as vectors over the rows: (burst angle, region angle, region
/// magnitude), degrees.
fn raw_phase(f: &CompositeFrame, row0: usize, row1: usize, x0: usize, x1: usize, burst_len: usize) -> (f64, f64, f64) {
    let (mut bz, mut rz) = ((0.0f64, 0.0f64), (0.0f64, 0.0f64));
    for line in row0..row1 {
        let l = &f.lines[line];
        let proj = |from: usize, to: usize| -> (f64, f64) {
            let mut z = (0.0f64, 0.0f64);
            for i in from..to.min(l.samples.len()) {
                let th = std::f64::consts::TAU * f.phase_at(line, i).get() as f64 / 12.0;
                let s = l.samples[i] as f64;
                z.0 += s * th.cos();
                z.1 += s * th.sin();
            }
            z
        };
        let b = proj(l.burst_start, l.burst_start + burst_len);
        let r = proj(l.active_start + x0, l.active_start + x1);
        bz.0 += b.0; bz.1 += b.1;
        rz.0 += r.0; rz.1 += r.1;
    }
    let n = (row1 - row0) as f64;
    (bz.1.atan2(bz.0).to_degrees(), rz.1.atan2(rz.0).to_degrees(), (rz.0.hypot(rz.1)) / n / (x1 - x0) as f64)
}

fn wrap(d: f64) -> f64 {
    let mut d = d % 360.0;
    if d > 180.0 { d -= 360.0; }
    if d < -180.0 { d += 360.0; }
    d
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (path, rate) = (&a[1], a[2].parse::<f64>().unwrap());
    let colour = u8::from_str_radix(&a[3], 16).unwrap();
    let (row0, row1) = (a[4].parse::<usize>().unwrap(), a[5].parse::<usize>().unwrap());
    let (x0, x1) = (a[6].parse::<usize>().unwrap(), a[7].parse::<usize>().unwrap());
    let burst_len: usize = a.get(8).and_then(|s| s.parse().ok()).unwrap_or(96);

    let raw = read_capture(std::path::Path::new(path), "u8", Some(rate));
    let (cap, ..) = auto_level_nes(&raw);
    let rec = recover_nes(&cap);
    let levels = ntsc_source_nes::Levels::transcribed();
    let dots = ntsc_testgen::solid(FrameParity::Even, colour, 0);
    let synth = ntsc_source_nes::encode_frame(&levels, &dots, Phase::new(0));

    // The signal stage.
    let (rb, rr, rm) = raw_phase(&rec.frame, row0, row1, x0, x1, burst_len);
    let (sb, sr, sm) = raw_phase(&synth, row0, row1, x0, x1, burst_len);
    let (raw_real, raw_synth) = (wrap(rr - rb), wrap(sr - sb));

    // The decoded stage, as score-real-region measures it.
    let dec = ntsc_decode::Decoder::transcribed(ntsc_source_nes::burst_axis_offset(), ntsc_source_nes::levels::LOW[1], ntsc_source_nes::levels::HIGH[2]);
    let width = 2048usize;
    let hue = |f: &CompositeFrame| -> f64 {
        let y = dec.decode_yuv(f, row0, row1 - row0, width);
        let (mut u, mut v) = (0.0f64, 0.0f64);
        for r in 0..row1 - row0 {
            for x in x0..x1 {
                u += y.u[r * width + x] as f64;
                v += y.v[r * width + x] as f64;
            }
        }
        v.atan2(u).to_degrees()
    };
    let (dec_real, dec_synth) = (hue(&rec.frame), hue(&synth));

    println!("colour ${colour:02x}, rows {row0}..{row1}, x {x0}..{x1} (burst {burst_len} samples per line)");
    println!("  signal stage, chroma phase minus burst phase, degrees:");
    println!("    real   burst {rb:+7.1}  region {rr:+7.1}  region minus burst {raw_real:+7.1}  (chroma magnitude {rm:.4} V per sample)");
    println!("    synth  burst {sb:+7.1}  region {sr:+7.1}  region minus burst {raw_synth:+7.1}  (chroma magnitude {sm:.4})");
    println!("    real minus synth {:+.1} deg", wrap(raw_real - raw_synth));
    // The wave itself: the region's samples folded by subcarrier phase,
    // the mean level at each of the twelve, real beside synth, so a wave
    // that is not six high and six low, or not where the table puts it,
    // is seen rather than inferred from its fundamental.
    let fold = |f: &CompositeFrame| -> [f64; 12] {
        let (mut sum, mut n) = ([0.0f64; 12], [0u32; 12]);
        for line in row0..row1 {
            let l = &f.lines[line];
            for i in (l.active_start + x0)..(l.active_start + x1).min(l.samples.len()) {
                let p = f.phase_at(line, i).get() as usize;
                sum[p] += l.samples[i] as f64;
                n[p] += 1;
            }
        }
        let mut out = [0.0; 12];
        for p in 0..12 { out[p] = if n[p] > 0 { sum[p] / n[p] as f64 } else { 0.0 }; }
        out
    };
    let (fr, fs) = (fold(&rec.frame), fold(&synth));
    let show = |w: &[f64; 12]| -> String { w.iter().map(|v| format!("{v:5.3}")).collect::<Vec<_>>().join(" ") };
    println!("  the wave, mean level by subcarrier phase 0..11 (V):");
    println!("    real   {}", show(&fr));
    println!("    synth  {}", show(&fs));
    println!("  decoded stage, hue of the mean UV through the one decoder:");
    println!("    real {dec_real:+7.1}  synth {dec_synth:+7.1}  real minus synth {:+.1} deg", wrap(dec_real - dec_synth));
}
