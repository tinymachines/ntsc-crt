//! Times the CRT stages alone at scale 3, best of 3, for the M3 report.
use ntsc_crt::{CrtParams, CrtPipeline, GeometryParams, MaskParams};
use ntsc_decode::LinearRgbFrame;
use std::time::Instant;
fn main() {
    let input = LinearRgbFrame {
        width: 2048,
        height: 240,
        data: (0..2048 * 240 * 3).map(|i| (i % 7) as f32 / 7.0).collect(),
        display_gamma: 2.2,
    };
    let mut params = CrtParams::authored(3);
    params.mask = Some(MaskParams { pitch: 1, off_gain: 0.3 });
    params.geometry = Some(GeometryParams { barrel_k: 0.03, corner_radius: 12.0 });
    let mut pipe = CrtPipeline::new(params);
    pipe.process(&input); // warm
    let mut best = f64::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        pipe.process(&input);
        best = best.min(t.elapsed().as_secs_f64());
    }
    println!("crt stages at scale 3, all five on: {:.1} ms/frame", best * 1e3);
}
