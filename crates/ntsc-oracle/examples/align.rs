//! The alignment run: measures every frozen constant in ntsc-oracle and
//! prints the table docs/m1-report.md records. Run with --release.
//!
//! 1. Fits the (U,V) -> (I,Q) map over the twelve hues.
//! 2. Holds the ported palette model to the compiled library.
//! 3. Scans blargg burst_phase and resampler offset for the colour-cycle
//!    comparison, minimizing mean abs difference.
//! 4. Prints the DC sweep envelope (matched-tail) and the full-pipeline
//!    figures the report attributes.

use ntsc_grid::{FrameParity, Phase};
use ntsc_oracle::{blargg_model, fit_iq_map, iq_map, nes_decoder, resample_row, OUT_WIDTH};
use ntsc_source_nes::{encode_frame, Levels};

fn main() {
    let (spread, rotation, scale) = fit_iq_map();
    println!("iq map: rotation {rotation:.3} deg (spread {spread:.3}), scale {scale:.5}");

    #[cfg(not(has_oracle))]
    println!("vendor library not fetched: run tools/fetch-oracle.sh for the rest");

    #[cfg(has_oracle)]
    {
        let oracle = ntsc_oracle::Oracle::composite(true);
        let mut worst = 0.0f32;
        let mut worst_entry = 0;
        for entry in 0..512u16 {
            let want = blargg_model::palette_rgb(entry);
            for (c, w) in want.iter().enumerate() {
                let d = (w - oracle.palette[entry as usize * 3 + c] as f32).abs();
                if d > worst {
                    worst = d;
                    worst_entry = entry;
                }
            }
        }
        println!("port vs compiled palette: worst {worst:.2} counts at entry {worst_entry:03x}");

        // DC sweep, matched tail: our solid DC wearing blargg's tail,
        // against his palette, split base colours vs emphasis entries.
        let map = iq_map();
        let mut groups = [(0.0f32, 0.0f32, 0u16); 2]; // (sum, worst, worst_entry)
        for entry in 0..512u16 {
            let (colour, emph) = ((entry & 0x3f) as u8, (entry >> 6) as u8);
            let (y, u, v) = ntsc_oracle::solid_dc(colour, emph);
            let (i, q) = map.apply(u as f64, v as f64);
            let got = blargg_model::rgb255(y, i as f32, q as f32);
            let mut d = 0.0f32;
            for (c, gc) in got.iter().enumerate() {
                d = d.max((gc - oracle.palette[entry as usize * 3 + c] as f32).abs());
            }
            let g = &mut groups[(emph != 0) as usize];
            g.0 += d;
            if d > g.1 {
                g.1 = d;
                g.2 = entry;
            }
        }
        println!(
            "dc sweep (matched tail): base colours mean-of-worst {:.2}, worst {:.2} at {:03x}; emphasis mean {:.2}, worst {:.2} at {:03x}",
            groups[0].0 / 64.0, groups[0].1, groups[0].2,
            groups[1].0 / 448.0, groups[1].1, groups[1].2
        );

        // The full honest pipeline (our SMPTE matrix, our 2.2 display
        // gamma, no borrowed tail) against his palette: the
        // decoder-choice divergence, reported rather than asserted.
        let dec = nes_decoder();
        let mut full = [(0.0f32, 0.0f32, 0u16); 2];
        for entry in 0..512u16 {
            let (colour, emph) = ((entry & 0x3f) as u8, (entry >> 6) as u8);
            let (y, u, v) = ntsc_oracle::solid_dc(colour, emph);
            let r = (y + dec.r_from_v * v).clamp(0.0, 1.0) * 255.0;
            let g = (y + dec.g_from_u * u + dec.g_from_v * v).clamp(0.0, 1.0) * 255.0;
            let b = (y + dec.b_from_u * u).clamp(0.0, 1.0) * 255.0;
            let mut d = 0.0f32;
            for (c, got) in [r, g, b].into_iter().enumerate() {
                d = d.max((got - oracle.palette[entry as usize * 3 + c] as f32).abs());
            }
            let g2 = &mut full[(emph != 0) as usize];
            g2.0 += d;
            if d > g2.1 {
                g2.1 = d;
                g2.2 = entry;
            }
        }
        println!(
            "dc sweep (full pipeline): base colours mean-of-worst {:.2}, worst {:.2} at {:03x}; emphasis mean {:.2}, worst {:.2} at {:03x}",
            full[0].0 / 64.0, full[0].1, full[0].2,
            full[1].0 / 448.0, full[1].1, full[1].2
        );

        // Row scans run against the UNMERGED oracle: the golden tests
        // compare with merge_fields off (per-frame phase is the thing
        // under test), and an earlier version of this example scanned
        // against the merged kernels, which moved the stripes minimum by
        // six samples and hid the true basin. The instrument now matches
        // the test.
        let oracle = ntsc_oracle::Oracle::composite(false);
        let cycle = ntsc_testgen::colour_cycle(FrameParity::Even, 1);
        let frame = encode_frame(&Levels::transcribed(), &cycle, Phase::new(0));
        let row0 = 8usize;
        let yuv = nes_decoder().decode_yuv(&frame, row0, 1, 2048);
        let ours: Vec<[f32; 3]> = (0..2048)
            .map(|x| {
                let (i, q) = map.apply(yuv.u[x] as f64, yuv.v[x] as f64);
                blargg_model::rgb255(yuv.y[x], i as f32, q as f32)
            })
            .collect();
        let blit = oracle.blit(&cycle.active_entries()[..256 * (row0 + 1)], row0 + 1, 0);
        let his = &blit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
        // b0 scanned by shifting which burst the blit starts at.
        let mut best = (f32::MAX, 0i32, 0.0f64);
        for b0 in 0..3i32 {
            let blit = oracle.blit(&cycle.active_entries()[..256 * (row0 + 1)], row0 + 1, b0);
            let his = &blit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
            for off10 in -200..=100i32 {
                let offset = off10 as f64 / 10.0;
                let rs = resample_row(&ours, offset);
                let margin = 12; // clear of blargg's row lead-in and tail
                let mut sum = 0.0f32;
                for x in margin..OUT_WIDTH - margin {
                    for c in 0..3 {
                        sum += (rs[x][c] - his[x][c] as f32).abs();
                    }
                }
                let mean = sum / ((OUT_WIDTH - 2 * margin) as f32 * 3.0);
                if mean < best.0 {
                    best = (mean, b0, offset);
                }
            }
        }
        println!(
            "colour-cycle row {row0}: best mean abs diff {:.3} counts at burst0 {} offset {:+.1} samples",
            best.0, best.1, best.2
        );
        let _ = his;

        // The stripes frame: edges at every dot, so the diff surface is
        // steep in the offset. This is the scan that actually constrains
        // the resampler alignment; the colour-cycle surface is nearly
        // flat (bands are 21 dots of DC).
        let stripes = ntsc_testgen::stripes(FrameParity::Even, 0x16, 0x2a);
        let frame = encode_frame(&Levels::transcribed(), &stripes, Phase::new(0));
        let yuv = nes_decoder().decode_yuv(&frame, row0, 1, 2048);
        let ours: Vec<[f32; 3]> = (0..2048)
            .map(|x| {
                let (i, q) = map.apply(yuv.u[x] as f64, yuv.v[x] as f64);
                blargg_model::rgb255(yuv.y[x], i as f32, q as f32)
            })
            .collect();
        let mut best = (f32::MAX, 0i32, 0.0f64);
        for b0 in 0..3i32 {
            let blit = oracle.blit(&stripes.active_entries()[..256 * (row0 + 1)], row0 + 1, b0);
            let his = &blit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
            for off10 in -200..=100i32 {
                let offset = off10 as f64 / 10.0;
                let rs = resample_row(&ours, offset);
                let margin = 12;
                let mut sum = 0.0f32;
                for x in margin..OUT_WIDTH - margin {
                    for c in 0..3 {
                        sum += (rs[x][c] - his[x][c] as f32).abs();
                    }
                }
                let mean = sum / ((OUT_WIDTH - 2 * margin) as f32 * 3.0);
                if mean < best.0 {
                    best = (mean, b0, offset);
                }
            }
        }
        println!(
            "stripes row {row0}: best mean abs diff {:.3} counts at burst0 {} offset {:+.1} samples",
            best.0, best.1, best.2
        );
        // The surface around the stripes minimum, and the colour-cycle
        // cost at the stripes-chosen offset: what freezing one offset for
        // both comparisons costs.
        let blit = oracle.blit(&stripes.active_entries()[..256 * (row0 + 1)], row0 + 1, best.1);
        let his = &blit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
        for d in [-4.0, -2.0, 0.0, 2.0, 4.0] {
            let rs = resample_row(&ours, best.2 + d);
            let margin = 12;
            let mut sum = 0.0f32;
            for x in margin..OUT_WIDTH - margin {
                for c in 0..3 {
                    sum += (rs[x][c] - his[x][c] as f32).abs();
                }
            }
            println!(
                "  stripes at offset {:+.1}: {:.3}",
                best.2 + d,
                sum / ((OUT_WIDTH - 2 * margin) as f32 * 3.0)
            );
        }
        let cycle_yuv = nes_decoder().decode_yuv(
            &encode_frame(&Levels::transcribed(), &cycle, Phase::new(0)),
            row0,
            1,
            2048,
        );
        let cycle_ours: Vec<[f32; 3]> = (0..2048)
            .map(|x| {
                let (i, q) = map.apply(cycle_yuv.u[x] as f64, cycle_yuv.v[x] as f64);
                blargg_model::rgb255(cycle_yuv.y[x], i as f32, q as f32)
            })
            .collect();
        let cblit = oracle.blit(&cycle.active_entries()[..256 * (row0 + 1)], row0 + 1, best.1);
        let chis = &cblit[row0 * OUT_WIDTH..(row0 + 1) * OUT_WIDTH];
        let rs = resample_row(&cycle_ours, best.2);
        let margin = 12;
        let mut sum = 0.0f32;
        for x in margin..OUT_WIDTH - margin {
            for c in 0..3 {
                sum += (rs[x][c] - chis[x][c] as f32).abs();
            }
        }
        println!(
            "  colour-cycle at stripes offset {:+.1}: {:.3}",
            best.2,
            sum / ((OUT_WIDTH - 2 * margin) as f32 * 3.0)
        );
        // The shift-sensitive instrument: edge windows on the cycle row.
        let chis_arr: Vec<[u8; 3]> = chis.to_vec();
        for d in [-4.0, -2.0, 0.0, 2.0, 4.0] {
            println!(
                "  cycle edge windows at offset {:+.1}: {:.3}",
                best.2 + d,
                ntsc_oracle::edge_diff(&cycle_ours, &chis_arr, best.2 + d)
            );
        }
    }
}
