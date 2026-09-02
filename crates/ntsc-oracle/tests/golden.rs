//! The M1 golden comparison against blargg's nes_ntsc 0.2.2. Every
//! envelope here was measured by examples/align.rs on 2026-09-01 (the run
//! is in docs/m1-report.md), attributed, then frozen; the tests hold both
//! pipelines to those envelopes and prove they can tell.
//!
//! SKIPS (by name) without the fetched vendor library; REQUIRE_ORACLE=1
//! makes its absence a failure. MUTATE=1 perturbs the pipeline's chroma
//! filter on the sweep test; the resampler and burst alignment carry
//! their own always-on red proofs inside the row test.

#[cfg(not(has_oracle))]
#[test]
fn oracle_vendor_missing() {
    if std::env::var("REQUIRE_ORACLE").map(|v| v == "1").unwrap_or(false) {
        panic!("REQUIRE_ORACLE=1 but crates/ntsc-oracle/vendor is not fetched: run tools/fetch-oracle.sh");
    }
    eprintln!("SKIP: nes_ntsc vendor library not fetched (tools/fetch-oracle.sh); golden comparison not run");
}

#[cfg(has_oracle)]
mod golden {
    use ntsc_grid::{CompositeSource, Phase};
    use ntsc_oracle::{
        blargg_model, burst_for_origin, fit_iq_map, iq_map, nes_decoder, resample_row, solid_dc,
        Oracle, FITTED_ROTATION_DEG, FITTED_SCALE, OUT_WIDTH, RESAMPLE_OFFSET,
    };
    use ntsc_source_nes::NesSource;

    fn mutate() -> bool {
        std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
    }

    #[test]
    fn the_port_reproduces_blarggs_own_palette() {
        // The ported model against the compiled library, all 512 entries:
        // agreement within integer packing (measured worst 1.00).
        let oracle = Oracle::composite(true);
        for entry in 0..512u16 {
            let want = blargg_model::palette_rgb(entry);
            for (c, w) in want.iter().enumerate() {
                let got = oracle.palette[entry as usize * 3 + c] as f32;
                assert!(
                    (w - got).abs() <= 2.5,
                    "entry {entry:03x} channel {c}: port {w} vs compiled {got}"
                );
            }
        }
    }

    #[test]
    fn the_iq_map_refits_to_its_frozen_constants() {
        // One rigid rotation maps our demodulated (U,V) onto blargg's
        // (I,Q) for all twelve hues (measured spread 0.028 degrees), and
        // it is the recorded one. A drift in either pipeline breaks this
        // before it can silently re-align the comparison.
        let (spread, rotation, scale) = fit_iq_map();
        assert!(spread < 0.5, "per-hue rotation spread {spread} degrees");
        assert!(
            (rotation - FITTED_ROTATION_DEG).abs() < 0.5,
            "rotation {rotation} vs frozen {FITTED_ROTATION_DEG}"
        );
        assert!(
            (scale / FITTED_SCALE - 1.0).abs() < 0.01,
            "scale {scale} vs frozen {FITTED_SCALE}"
        );
    }

    /// Our solid DC wearing blargg's own tail, against his palette.
    fn dc_diff(entry: u16, chroma_gain_fudge: f32) -> f32 {
        let oracle_palette = PALETTE.with(|p| p.clone());
        let (colour, emph) = ((entry & 0x3f) as u8, (entry >> 6) as u8);
        let (y, u, v) = solid_dc(colour, emph);
        let (u, v) = (u * chroma_gain_fudge, v * chroma_gain_fudge);
        let (i, q) = iq_map().apply(u as f64, v as f64);
        let got = blargg_model::rgb255(y, i as f32, q as f32);
        let mut d = 0.0f32;
        for c in 0..3 {
            d = d.max((got[c] - oracle_palette[entry as usize * 3 + c] as f32).abs());
        }
        d
    }

    thread_local! {
        static PALETTE: Vec<u8> = Oracle::composite(true).palette;
    }

    #[test]
    fn the_sweep_matches_blargg_at_dc() {
        // Representative subset in the test (the full 512 runs in
        // examples/align.rs and is recorded in the report): all 64 base
        // colours, and all eight emphasis settings of colour $12 plus the
        // worst measured emphasis entry. Envelopes from the align run:
        // base worst 5.48 (blargg's two-decimal level rounding), emphasis
        // worst 45.81 (blargg's YIQ-space emphasis approximation against
        // the measured attenuated voltages).
        let fudge = if mutate() { 1.3 } else { 1.0 };
        for colour in 0..64u16 {
            let d = dc_diff(colour, fudge);
            assert!(d <= 8.0, "base colour {colour:02x}: {d} counts");
        }
        for emph in 1..8u16 {
            let d = dc_diff(emph << 6 | 0x12, fudge);
            assert!(d <= 60.0, "emphasis {emph} colour 12: {d} counts");
        }
        assert!(dc_diff(0x1d2, fudge) <= 60.0);
    }

    /// Compare one decoded row of a frame against blargg, returning the
    /// mean abs diff in counts over the row clear of blargg's lead-in.
    fn row_diff(
        dots: &ntsc_source_nes::DotFrame,
        origin: Phase,
        burst: i32,
        offset: f64,
    ) -> f32 {
        let row0 = 8usize;
        let frame = ntsc_source_nes::encode_frame(&ntsc_source_nes::Levels::transcribed(), dots, origin);
        let yuv = nes_decoder().decode_yuv(&frame, row0, 1, 2048);
        let map = iq_map();
        let ours: Vec<[f32; 3]> = (0..2048)
            .map(|x| {
                let (i, q) = map.apply(yuv.u[x] as f64, yuv.v[x] as f64);
                blargg_model::rgb255(yuv.y[x], i as f32, q as f32)
            })
            .collect();
        let rs = resample_row(&ours, offset);
        let oracle = Oracle::composite(false);
        let blit = oracle.blit(&dots.active_entries()[..256 * (row0 + 1)], row0 + 1, burst);
        let his = &blit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
        let margin = 12;
        let mut sum = 0.0f32;
        for x in margin..OUT_WIDTH - margin {
            for c in 0..3 {
                sum += (rs[x][c] - his[x][c] as f32).abs();
            }
        }
        sum / ((OUT_WIDTH - 2 * margin) as f32 * 3.0)
    }

    #[test]
    fn both_colour_cycle_sets_match_along_their_frames() {
        // All six frames of the two spec-named sets, each frame decoded
        // at its chained origin and compared at its mapped burst.
        // Envelope: measured 5.8 counts mean on the first frame; band
        // edges differ by filter design (his DSF/Gaussian kernels, our
        // sincs), which is where most of the mean lives.
        for short in [false, true] {
            let frames = ntsc_testgen::colour_cycle_set(short);
            let mut src = NesSource::new(frames.clone(), Phase::new(0));
            for (k, dots) in frames.iter().enumerate() {
                let origin = src.next_frame().phase_at_origin;
                let burst = burst_for_origin(origin);
                let d = row_diff(dots, origin, burst, RESAMPLE_OFFSET);
                assert!(d <= 10.0, "set short={short} frame {k}: mean {d} counts");
            }
        }
    }

    #[test]
    fn the_two_frame_repeat_shows_through_the_mapping() {
        // Even/OddShort returns the origin, so frame 2 maps to frame 0's
        // burst; Even/OddFull does not.
        let origins = |short: bool| {
            let frames = ntsc_testgen::colour_cycle_set(short);
            let mut src = NesSource::new(frames, Phase::new(0));
            [
                src.next_frame().phase_at_origin,
                src.next_frame().phase_at_origin,
                src.next_frame().phase_at_origin,
            ]
        };
        let a = origins(false);
        let b = origins(true);
        assert_ne!(burst_for_origin(a[0]), burst_for_origin(a[2]));
        assert_eq!(burst_for_origin(b[0]), burst_for_origin(b[2]));
        assert_ne!(burst_for_origin(b[0]), burst_for_origin(b[1]));
    }

    #[test]
    fn a_wrong_resampler_or_burst_is_detected() {
        // The rig's own red proof, always on (spec: MUTATE must also
        // perturb the resampler to show a wrong resampler is detectable).
        // The instrument is the stripes frame, whose diff surface has a
        // sharp basin at the recorded offset (measured 18.0 counts, 30+
        // one dot away); the colour-cycle surface is flat and proves
        // nothing about the offset, which is why it is not used here.
        let burst = burst_for_origin(Phase::new(0));
        let stripes = ntsc_testgen::stripes(ntsc_grid::FrameParity::Even, 0x16, 0x2a);
        let good = row_diff(&stripes, Phase::new(0), burst, RESAMPLE_OFFSET);
        assert!(good <= 25.0, "aligned stripes comparison at {good} counts");
        for shift in [-8.0, 8.0] {
            let shifted = row_diff(&stripes, Phase::new(0), burst, RESAMPLE_OFFSET + shift);
            assert!(
                shifted > 1.8 * good,
                "a one-dot resampler shift ({shift}) must be loud: {shifted} vs {good}"
            );
        }
        // The wrong-burst proof also lives on stripes: blargg's
        // correct_errors forces every burst kernel to the same DC, so a
        // solid band looks identical under any burst phase and the
        // colour-cycle frame barely notices (measured 6.0 vs 5.7). The
        // burst shows only in edge artifacts, which stripes are made of
        // (measured 53.6 and 56.9 against 18.0).
        for wrong in [1, 2] {
            let wrong_burst =
                row_diff(&stripes, Phase::new(0), (burst + wrong) % 3, RESAMPLE_OFFSET);
            assert!(
                wrong_burst > 2.5 * good,
                "a wrong burst phase (+{wrong}) must be loud: {wrong_burst} vs {good}"
            );
        }
    }
}
