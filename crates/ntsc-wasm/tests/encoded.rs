//! `encode` hands a decoder elsewhere exactly what `push_frame` decodes:
//! the samples put back into a CompositeFrame at the phases it reports
//! decode, through this crate's own decoder, to the bytes push_frame
//! returns for the same frame at the same chained origin. And the
//! constants it exports are the decoder's fields, not a copy.

use ntsc_grid::{CompositeFrame, CompositeLine, Geometry, Phase};
use ntsc_wasm::NesPipeline;

#[test]
fn the_encoded_samples_decode_to_push_frames_bytes() {
    let mut a = NesPipeline::new("comb3");
    let mut b = NesPipeline::new("comb3");
    let dots = ntsc_testgen::solid(ntsc_grid::FrameParity::OddShort, 0x16, 0);
    let mut colour = dots.colour.clone();
    for (i, c) in colour.iter_mut().enumerate() {
        *c = [0x16u8, 0x2a, 0x30, 0x0f, 0x12][(i / 7) % 5];
    }
    let colour_full = {
        let mut v = colour.clone();
        v.resize(341 * 262, 0x0f);
        v
    };
    let emphasis = vec![0u8; colour_full.len()];
    // Two frames so the chained phase matters.
    for parity in [2u8, 0] {
        let want = b.push_frame(&colour_full, &emphasis, parity);
        let origin = a.origin();
        let e = a.encode(&colour_full, &emphasis, parity);
        assert_eq!(e.phases.len(), 262);
        assert_eq!(e.samples.len(), 262 * e.line_len);
        let geo = Geometry::nes();
        let lines = (0..262)
            .map(|i| CompositeLine { samples: e.samples[i * e.line_len..(i + 1) * e.line_len].to_vec(), sync_start: 0, burst_start: 0, active_start: e.active_start })
            .collect();
        let frame = CompositeFrame { profile: geo, lines, frame_parity: ntsc_grid::FrameParity::Even, phase_at_origin: origin };
        for i in 0..262 {
            assert_eq!(frame.phase_at(i, 0).get(), e.phases[i], "line {i} phase");
        }
        let yuv = a.decoder().decode_yuv(&frame, a.row0(), 240, 2048);
        let d = a.decoder();
        let mut worst = 0i32;
        for i in 0..2048 * 240 {
            let (y, u, v) = (yuv.y[i], yuv.u[i], yuv.v[i]);
            let px = [(y + d.r_from_v * v).clamp(0.0, 1.0), (y + d.g_from_u * u + d.g_from_v * v).clamp(0.0, 1.0), (y + d.b_from_u * u).clamp(0.0, 1.0)];
            for c in 0..3 {
                let got = (px[c] * 255.0 + 0.5) as u8;
                worst = worst.max((got as i32 - want[i * 4 + c] as i32).abs());
            }
        }
        assert_eq!(worst, 0, "parity {parity}: the padded samples decode differently from push_frame");
    }
    let p = a.decoder_params();
    assert_eq!(p[0], a.decoder().comb_weights[0]);
    assert_eq!(p[3], a.decoder().black);
    assert_eq!(p[11] as usize, a.decoder().uv_decimation);
    assert_eq!(&p[13..], &a.decoder().uv_taps[..]);
    let _ = Phase::new(0);
}

/// `advance` moves the phase exactly as encoding the frame would, and
/// the encoder's constants are the levels and the grid, not a copy.
#[test]
fn advance_carries_the_phase_as_encode_does() {
    let mut a = NesPipeline::new("comb3");
    let mut b = NesPipeline::new("comb3");
    let colour = vec![0x16u8; 341 * 262];
    let emphasis = vec![0u8; 341 * 262];
    for parity in [0u8, 2, 1, 2, 0] {
        a.encode(&colour, &emphasis, parity);
        b.advance(parity);
        assert_eq!(a.origin(), b.origin(), "after parity {parity}");
    }
    let p = a.encoder_params();
    let l = ntsc_source_nes::Levels::transcribed();
    assert_eq!(&p[0..4], &l.low[..]);
    assert_eq!(&p[12..16], &l.high_attenuated[..]);
    assert_eq!(p[16], l.sync);
    assert_eq!(p[19], l.blank);
    assert_eq!(p[24] as usize, ntsc_source_nes::SAMPLES_PER_DOT);
    assert_eq!(p[25] as usize, 341);
    assert_eq!(p[26] as usize, 262);
    assert_eq!(p[27] as usize, 8, "the short line's deficit");
    assert_eq!(p[28] as usize, 2728 % 12, "the phase step per line");
}
