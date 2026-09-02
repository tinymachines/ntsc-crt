//! The M2 throughput measurement, native side: frames and samples per
//! second through encode + decode for each rung, best of N because noise
//! only ever slows a run down. Run with --release; the numbers land in
//! docs/m2-report.md with this machine named.

use std::time::Instant;

use ntsc_decode::{Decoder, TemporalComb};
use ntsc_grid::{FrameParity, Phase, Profile};
use ntsc_source_nes::{burst_axis_offset, encode_frame, levels, Levels};
use ntsc_wasm::{NesPipeline, OUT_HEIGHT, OUT_WIDTH};

fn best_of<F: FnMut() -> usize>(n: usize, mut f: F) -> (f64, usize) {
    let mut best = f64::MAX;
    let mut work = 0;
    for _ in 0..n {
        let t = Instant::now();
        work = f();
        let dt = t.elapsed().as_secs_f64();
        if dt < best {
            best = dt;
        }
    }
    (best, work)
}

fn main() {
    let stripes = ntsc_testgen::stripes(FrameParity::Even, 0x16, 0x2a);
    let frames = 3usize;

    for rung in ["notch", "comb3"] {
        let mut pipe = NesPipeline::new(rung);
        let (dt, _) = best_of(3, || {
            for _ in 0..frames {
                pipe.push_frame(&stripes.colour, &stripes.emphasis, 0);
            }
            frames
        });
        let fps = frames as f64 / dt;
        println!(
            "nes {rung:6}: {:6.2} frames/s ({:5.1} Msamples/s decoded, full pipeline, {}x{})",
            fps,
            fps * (OUT_WIDTH * OUT_HEIGHT) as f64 / 1e6,
            OUT_WIDTH,
            OUT_HEIGHT,
        );
    }

    // Rung D: three-frame temporal comb over pre-encoded frames (the
    // encode is shared, so this times the comb + demod tail alone).
    let dec = Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
    let encoded: Vec<_> = (0..3)
        .map(|_| encode_frame(&Levels::transcribed(), &stripes, Phase::new(0)))
        .collect();
    let (dt, _) = best_of(3, || {
        let mut comb = TemporalComb::new(3);
        for f in &encoded {
            comb.push_and_decode(f.clone(), &dec, 0, OUT_HEIGHT, OUT_WIDTH);
        }
        1
    });
    println!("nes temporal3 decode: {:6.2} frames/s (demod tail only)", 1.0 / dt);

    // Broadcast bars through Rung A and Rung B.
    let bars = ntsc_testgen::smpte_bars75(700, 480);
    let frame = ntsc_source_rgb::encode_video_frame(&bars, 700, 480, Phase::new(0));
    let lay = ntsc_source_rgb::layout();
    let active = lay.line_len - lay.active_start;
    for (name, dec) in [
        (
            "notch",
            Decoder::transcribed(0.0, 7.5 / 140.0, 100.0 / 140.0),
        ),
        (
            "comb2",
            Decoder::comb_two_line(Profile::Broadcast, 0.0, 7.5 / 140.0, 100.0 / 140.0),
        ),
    ] {
        let (dt, _) = best_of(3, || {
            dec.decode(&frame, 21, 240, active);
            1
        });
        println!("broadcast {name:6} decode 240 lines: {:6.2} frames/s", 1.0 / dt);
    }
}
