//! The M1 oracle: blargg's nes_ntsc 0.2.2 built natively, the ported
//! palette model, the comparison resampler, and the recorded alignment
//! between the two pipelines. Everything here is test rig, and test rig
//! is code too (handoff spec principle 5): the resampler and the
//! alignment carry provenance and are themselves mutation-tested.
//!
//! Without the fetched vendor library (tools/fetch-oracle.sh) the FFI and
//! `Oracle` are absent and the golden tests skip by name; the ported
//! model and the alignment fit still build, because they come from the
//! library's source text, not its binary.

pub mod blargg_model;

use ntsc_decode::Decoder;
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{burst_axis_offset, encode_frame, levels, DotFrame, Levels};

/// blargg's output width for a 256-pixel row: 3 in -> 7 out chunks.
pub const OUT_WIDTH: usize = ((256 - 1) / 3 + 1) * 7;

#[cfg(has_oracle)]
mod ffi {
    use std::os::raw::{c_int, c_long, c_void};

    #[repr(C)]
    pub struct Setup {
        pub hue: f64,
        pub saturation: f64,
        pub contrast: f64,
        pub brightness: f64,
        pub sharpness: f64,
        pub gamma: f64,
        pub resolution: f64,
        pub artifacts: f64,
        pub fringing: f64,
        pub bleed: f64,
        pub merge_fields: c_int,
        pub decoder_matrix: *const f32,
        pub palette_out: *mut u8,
        pub palette: *const u8,
        pub base_palette: *const u8,
    }

    extern "C" {
        // nes_ntsc_t is table[512][128] of unsigned long; passed as the
        // base pointer of an identically-sized allocation.
        pub fn nes_ntsc_init(ntsc: *mut u64, setup: *const Setup);
        pub fn nes_ntsc_blit(
            ntsc: *const u64,
            input: *const u16,
            in_row_width: c_long,
            burst_phase: c_int,
            in_width: c_int,
            in_height: c_int,
            rgb_out: *mut c_void,
            out_pitch: c_long,
        );
    }
}

/// The compiled library behind a safe face: composite preset, emphasis
/// enabled, 32-bit output. `merge_fields` is the one recorded deviation
/// from the preset when false: the preset merges fields, which averages
/// the three burst kernels pairwise, and the colour-cycle comparison is
/// about per-frame phase, so it runs unmerged and says so.
#[cfg(has_oracle)]
pub struct Oracle {
    table: Vec<u64>,
    /// 512 x 3 bytes: blargg's own DC colour per entry, captured at init.
    pub palette: Vec<u8>,
}

#[cfg(has_oracle)]
impl Oracle {
    pub fn composite(merge_fields: bool) -> Oracle {
        let mut palette = vec![0u8; 512 * 3];
        let mut table = vec![0u64; 512 * 128];
        let setup = ffi::Setup {
            hue: 0.0,
            saturation: 0.0,
            contrast: 0.0,
            brightness: 0.0,
            sharpness: 0.0,
            gamma: 0.0,
            resolution: 0.0,
            artifacts: 0.0,
            fringing: 0.0,
            bleed: 0.0,
            merge_fields: merge_fields as _,
            decoder_matrix: std::ptr::null(),
            palette_out: palette.as_mut_ptr(),
            palette: std::ptr::null(),
            base_palette: std::ptr::null(),
        };
        unsafe { ffi::nes_ntsc_init(table.as_mut_ptr(), &setup) };
        Oracle { table, palette }
    }

    /// Blit `height` rows of 256 9-bit entries; returns rows of
    /// `OUT_WIDTH` RGB triples in 0..255. blargg advances the burst phase
    /// internally by one per row, matching the geometry's own
    /// 120-degrees-per-line residue.
    pub fn blit(&self, entries: &[u16], height: usize, burst_phase: i32) -> Vec<[u8; 3]> {
        assert_eq!(entries.len(), 256 * height);
        let mut out = vec![0u32; OUT_WIDTH * height];
        unsafe {
            ffi::nes_ntsc_blit(
                self.table.as_ptr(),
                entries.as_ptr(),
                256,
                burst_phase,
                256,
                height as _,
                out.as_mut_ptr().cast(),
                (OUT_WIDTH * 4) as _,
            );
        }
        out.iter()
            .map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
            .collect()
    }
}

/// Encode a solid frame and decode `rows` lines from `row0`, on the
/// transcribed pipeline. The workhorse of every comparison here.
pub fn decode_solid(colour: u8, emphasis: u8, rows: usize) -> ntsc_decode::YuvFrame {
    let frame = encode_frame(
        &Levels::transcribed(),
        &DotFrame::filled(FrameParity::Even, colour, emphasis),
        Phase::new(0),
    );
    nes_decoder().decode_yuv(&frame, 100, rows, 2048)
}

/// The DC of a decoded solid: (y, u, v) averaged over one full subcarrier
/// cycle mid-line. The averaging is part of the rig's meaning, not a
/// smoothing convenience: Rung A's luma is the complement of the chroma
/// bandpass, so it keeps the square wave's third and higher harmonics as
/// per-sample ripple (a real notch TV does too), while blargg's Y is
/// clean DC by construction. One cycle averages the harmonics to zero
/// and leaves the number his palette actually claims.
pub fn solid_dc(colour: u8, emphasis: u8) -> (f32, f32, f32) {
    let yuv = decode_solid(colour, emphasis, 1);
    let (mut y, mut u, mut v) = (0.0, 0.0, 0.0);
    for o in 1024..1036 {
        y += yuv.y[o];
        u += yuv.u[o];
        v += yuv.v[o];
    }
    (y / 12.0, u / 12.0, v / 12.0)
}

/// The NES-profile Rung A decoder with the transcribed references.
pub fn nes_decoder() -> Decoder {
    Decoder::transcribed(burst_axis_offset(), levels::LOW[1], levels::HIGH[2])
}

/// The rigid map from this pipeline's (U, V) onto blargg's (I, Q),
/// measured by `examples/align.rs` over the twelve hues and frozen here.
/// The golden test re-fits it every run and holds it to these numbers, so
/// a drift in either pipeline shows up as a failed fit rather than a
/// silently refitted comparison.
pub struct IqMap {
    /// blargg's IQ plane is mirrored relative to (U, V): his hue angles
    /// run clockwise where the demodulated ladder runs counterclockwise.
    pub mirror_v: bool,
    /// Rotation applied after the mirror, degrees.
    pub rotation_deg: f64,
    /// Scale: his half-swing chroma amplitude convention over the
    /// demodulator's 4/pi square-wave fundamental.
    pub scale: f64,
}

/// Frozen from the alignment run recorded in docs/m1-report.md.
pub fn iq_map() -> IqMap {
    IqMap {
        mirror_v: true,
        rotation_deg: FITTED_ROTATION_DEG,
        scale: FITTED_SCALE,
    }
}

/// Measured by examples/align.rs (see docs/m1-report.md for the run):
/// rotation fitted with spread 0.028 degrees over the twelve hues.
pub const FITTED_ROTATION_DEG: f64 = 120.0;
pub const FITTED_SCALE: f64 = 0.78799;

impl IqMap {
    pub fn apply(&self, u: f64, v: f64) -> (f64, f64) {
        let v = if self.mirror_v { -v } else { v };
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        (
            self.scale * (u * c - v * s),
            self.scale * (u * s + v * c),
        )
    }
}

/// Fit the map afresh from twelve decoded hue solids against the ported
/// model's own (i, q): returns (per-hue rotation spread in degrees, mean
/// rotation, mean scale). The golden test asserts the spread is tight and
/// the means match the frozen constants.
pub fn fit_iq_map() -> (f64, f64, f64) {
    let mut angles = Vec::new();
    let mut scales = Vec::new();
    for hue in 1..=12u8 {
        let yuv = decode_solid(0x10 | hue, 0, 1);
        let (u, v) = (yuv.u[1024] as f64, -(yuv.v[1024] as f64));
        let (_, i, q) = blargg_model::yiq((0x10 | hue) as u16);
        let ours = v.atan2(u);
        let his = (q as f64).atan2(i as f64);
        let mut d = (his - ours).to_degrees().rem_euclid(360.0);
        if d > 180.0 {
            d -= 360.0;
        }
        angles.push(d);
        scales.push(((i * i + q * q) as f64).sqrt() / (u * u + v * v).sqrt());
    }
    let mean = angles.iter().sum::<f64>() / 12.0;
    let spread = angles
        .iter()
        .map(|a| {
            let mut d = (a - mean).rem_euclid(360.0);
            if d > 180.0 {
                d -= 360.0;
            }
            d.abs()
        })
        .fold(0.0f64, f64::max);
    (spread, mean, scales.iter().sum::<f64>() / 12.0)
}

/// The comparison resampler's alignment offset in input samples, and
/// blargg's burst_phase for a frame whose origin is Phase(0): both
/// measured by examples/align.rs and frozen (the run is in
/// docs/m1-report.md). Three independent instruments agree on the offset
/// at their own minima: the stripes-row mean (18.0 counts, a sharp basin
/// against 30+ one dot away), the colour-cycle row mean (5.7), and the
/// band-edge windows (6.4). The value is the net of blargg's kernel
/// lead-in (his pixels are drawn from kernels reaching up to 9 samples
/// back; "first pixel will be cut off a bit") against our filters'
/// centred delay; recorded as measured, not derived.
pub const RESAMPLE_OFFSET: f64 = -11.8;
pub const BURST0: i32 = 2;

/// blargg's burst_phase for a frame at the given origin. Origins on the
/// NES profile move in whole 4-sample steps (the line and frame
/// residues), and 4 samples is one blargg burst step (120 degrees).
pub fn burst_for_origin(origin: Phase) -> i32 {
    assert!(origin.get().is_multiple_of(4), "NES origins move in 4-sample steps");
    (BURST0 + origin.get() as i32 / 4) % 3
}

/// Mean abs diff restricted to windows around the colour-cycle band
/// edges: the shift-sensitive instrument. A whole-row mean is dominated
/// by the model differences between the pipelines (blargg's authored
/// artifact and fringing weights against our sincs) and moves only a few
/// percent under a multi-sample misalignment; the edges move with the
/// shift itself. Windows are +/-5 output pixels around each interior
/// band edge, edge positions derived from the same 24/7 mapping.
pub fn edge_diff(ours: &[[f32; 3]], his: &[[u8; 3]], offset: f64) -> f32 {
    let rs = resample_row(ours, offset);
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for k in 1..12 {
        let dot = k * 256 / 12; // band edge in active dots
        let x_e = ((dot * 8) as f64 - offset) * 7.0 / 24.0 - 0.5;
        let lo = (x_e - 5.0).max(0.0) as usize;
        let hi = ((x_e + 5.0) as usize).min(his.len() - 1);
        for x in lo..=hi {
            for c in 0..3 {
                sum += (rs[x][c] - his[x][c] as f32).abs();
            }
            n += 3;
        }
    }
    sum / n as f32
}

pub fn resample_row(signal_rgb: &[[f32; 3]], offset: f64) -> Vec<[f32; 3]> {
    // blargg's blit emits 7 output pixels per 3 input dots (24 samples),
    // padding the row with black lead-in and tail pixels, so his 602
    // pixels span 258 dots of signal. The slope is therefore exactly
    // 24/7 samples per output pixel; the first version of this function
    // used 2048/602, which drifts 16 samples across the row and made
    // every offset scan surface flat. The residual constant shift is the
    // recorded `offset`.
    let ratio = 24.0 / 7.0;
    (0..OUT_WIDTH)
        .map(|x| {
            let center = (x as f64 + 0.5) * ratio - 0.5 + offset;
            let mut acc = [0.0f64; 3];
            let mut wsum = 0.0f64;
            let lo = (center - ratio).floor() as isize;
            let hi = (center + ratio).ceil() as isize;
            for i in lo..=hi {
                let w = 1.0 - ((i as f64 - center) / ratio).abs();
                if w <= 0.0 {
                    continue;
                }
                let j = i.clamp(0, signal_rgb.len() as isize - 1) as usize;
                for c in 0..3 {
                    acc[c] += w * signal_rgb[j][c] as f64;
                }
                wsum += w;
            }
            [
                (acc[0] / wsum) as f32,
                (acc[1] / wsum) as f32,
                (acc[2] / wsum) as f32,
            ]
        })
        .collect()
}
