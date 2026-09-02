//! The reference player: records a dot-stream golden with a run stamp,
//! plays it back through the whole pipeline (encode, Rung A decode, all
//! five CRT stages at integer scale 3), writes the display frames as PPM
//! into goldens/, and prints the drift stats of a simulated 60 Hz
//! playback. The images are illustrative, not verification (spec
//! section 8, M3); the verification is tests/stages.rs and tests/gate.rs.
//!
//! Run with --release.

use std::io::Write as _;
use std::path::PathBuf;

use ntsc_crt::{window_visible, CrtParams, CrtPipeline, GeometryParams, MaskParams};
use ntsc_decode::Decoder;
use ntsc_grid::{CompositeSource, Phase};
use ntsc_source_nes::{burst_axis_offset, levels, DotFrame, NesSource, DOTS_PER_LINE, LINES};
use ntsc_wasm::Pacing;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../goldens");
    std::fs::create_dir_all(&root).unwrap();

    // Record the golden: the Even/OddShort colour-cycle set plus the
    // stripe frame, byte-serialized with its parameters in the stamp.
    let mut frames = ntsc_testgen::colour_cycle_set(true);
    frames.push(ntsc_testgen::stripes(ntsc_grid::FrameParity::Even, 0x16, 0x2a));
    let mut blob = Vec::new();
    for f in &frames {
        blob.push(match f.parity {
            ntsc_grid::FrameParity::Even => 0u8,
            ntsc_grid::FrameParity::OddFull => 1,
            ntsc_grid::FrameParity::OddShort => 2,
        });
        blob.extend_from_slice(&f.colour);
        blob.extend_from_slice(&f.emphasis);
    }
    std::fs::write(root.join("dotstream-m3.bin"), &blob).unwrap();
    std::fs::write(
        root.join("dotstream-m3.stamp.txt"),
        format!(
            "ntsc-crt M3 dot-stream golden\ngenerators: colour_cycle_set(short=true) + stripes(Even, 16, 2a)\n\
             frames: {} of {} x {} dots (parity byte + colour plane + emphasis plane each)\n\
             recorded: 2026-09-01 by examples/play-golden.rs\n",
            frames.len(),
            DOTS_PER_LINE,
            LINES,
        ),
    )
    .unwrap();

    // Play it back: read the recorded blob, not the in-memory frames.
    let blob = std::fs::read(root.join("dotstream-m3.bin")).unwrap();
    let stride = 1 + 2 * DOTS_PER_LINE * LINES;
    let played: Vec<DotFrame> = blob
        .chunks_exact(stride)
        .map(|c| DotFrame {
            parity: match c[0] {
                0 => ntsc_grid::FrameParity::Even,
                1 => ntsc_grid::FrameParity::OddFull,
                _ => ntsc_grid::FrameParity::OddShort,
            },
            colour: c[1..1 + DOTS_PER_LINE * LINES].to_vec(),
            emphasis: c[1 + DOTS_PER_LINE * LINES..].to_vec(),
        })
        .collect();
    let n = played.len();

    let mut src = NesSource::new(played, Phase::new(0));
    let dec = Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
    let mut params = CrtParams::authored(3);
    params.mask = Some(MaskParams { pitch: 1, off_gain: 0.3 });
    params.geometry = Some(GeometryParams { barrel_k: 0.03, corner_radius: 12.0 });
    let mut pipe = CrtPipeline::new(params);
    let mut pacing = Pacing::nes_rendering_enabled();

    for k in 0..n {
        let frame = src.next_frame();
        let rgb = dec.decode(&frame, 0, 240, 2048);
        let display = pipe.process(&rgb);
        assert!(window_visible(&display, 3), "frame {k}: window lost");
        pacing.tick(16_666_667);
        let rgba = display.to_rgba8();
        let mut ppm = Vec::with_capacity(display.width * display.height * 3 + 32);
        write!(ppm, "P6\n{} {}\n255\n", display.width, display.height).unwrap();
        for px in rgba.chunks_exact(4) {
            ppm.extend_from_slice(&px[..3]);
        }
        let path = root.join(format!("m3-frame-{k}.ppm"));
        std::fs::write(&path, ppm).unwrap();
        println!("wrote {} ({} x {})", path.display(), display.width, display.height);
    }
    println!(
        "drift stats after {n} presentations at 60 Hz: presented {} duplicated {} dropped {}",
        pacing.stats.presented, pacing.stats.duplicated, pacing.stats.dropped
    );
}
