//! The M3 closing gate, pipeline side: a recorded dot-stream golden
//! (the spec-named Even/OddShort colour-cycle set) played through
//! encode, Rung A decode and all five CRT stages at integer scale, with
//! the 224 x 224 guaranteed-visible window intact, the drift stats
//! counted, and the whole run bit-deterministic (persistence advances
//! by the source period, so a replay is a replay). The arcade shell
//! itself is the companion project; examples/play-golden.rs is the
//! reference player that writes the illustrative frames.

use ntsc_crt::{window_visible, CrtParams, CrtPipeline, GeometryParams, MaskParams};
use ntsc_decode::Decoder;
use ntsc_grid::{CompositeSource, Phase};
use ntsc_source_nes::{burst_axis_offset, levels, NesSource};
use ntsc_wasm::Pacing;

fn run(pipe: &mut CrtPipeline) -> (Vec<Vec<u8>>, Pacing) {
    let frames = ntsc_testgen::colour_cycle_set(true);
    let mut src = NesSource::new(frames[..2].to_vec(), Phase::new(0));
    let dec = Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
    let mut pacing = Pacing::nes_rendering_enabled();
    let mut out = Vec::new();
    for _ in 0..2 {
        let frame = src.next_frame();
        let rgb = dec.decode(&frame, 0, 240, 2048);
        let display = pipe.process(&rgb);
        assert_eq!((display.width, display.height), (512, 480), "integer scale");
        assert!(window_visible(&display, 2), "the 224 x 224 window must stay lit");
        pacing.tick(16_666_667);
        out.push(display.to_rgba8());
    }
    (out, pacing)
}

#[test]
fn the_recorded_golden_plays_through_the_full_pipeline() {
    let mut params = CrtParams::authored(2);
    params.mask = Some(MaskParams { pitch: 1, off_gain: 0.3 });
    params.geometry = Some(GeometryParams { barrel_k: 0.03, corner_radius: 8.0 });
    let mut pipe = CrtPipeline::new(params);
    let (first, pacing) = run(&mut pipe);
    assert_eq!(pacing.stats.presented, 2);
    // Two 60 Hz ticks against the 60.0988 Hz source: the drift needs
    // about ten seconds to accumulate one frame, so this short run must
    // show clean stats (the long-run counts are pacing.rs's tests).
    assert_eq!(pacing.stats.duplicated + pacing.stats.dropped, 0);
    // Determinism: reset the phosphor, play the same golden, get the
    // same bytes. Wall clock is nowhere in the pipeline.
    pipe.reset();
    let (second, _) = run(&mut pipe);
    assert_eq!(first, second, "a replay must be a replay");
}
