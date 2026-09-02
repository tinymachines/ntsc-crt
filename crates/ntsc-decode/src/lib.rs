//! Decode: `CompositeFrame` to `YuvFrame` to `LinearRgbFrame`.
//!
//! Rung A only, so far: chroma extracted by the generated bandpass, luma
//! as the exact complement (input minus chroma, delay-matched because the
//! kernel is symmetric), QAM demodulation by sin/cos at the phase from
//! `Geometry::phase_at` plus a caller-supplied burst axis offset, U/V
//! lowpassed, the transcribed inverse matrix, display gamma to linear.
//!
//! One recorded deviation from the handoff spec's naming: the spec calls
//! the intermediate `YiqFrame`; what Rung A implements is equiband YUV,
//! which is what the source page says real receivers decode. The
//! 33-degree I/Q rotation and split bandwidths are an encoder-side (M2)
//! concern and are not laundered in here.

use ntsc_grid::CompositeFrame;

pub mod tables {
    //! Constants generated from data/filters/rung-a.toml and
    //! data/yuv-matrix.toml. See build.rs.
    include!(concat!(env!("OUT_DIR"), "/tables.rs"));
}

/// Demodulated chroma plane plus luma, on the composite sample grid.
#[derive(Clone, Debug)]
pub struct YuvFrame {
    pub width: usize,
    pub height: usize,
    pub y: Vec<f32>,
    pub u: Vec<f32>,
    pub v: Vec<f32>,
}

/// Linear-light RGB on the same horizontal sample grid (no horizontal
/// resampling in decode). `display_gamma` records the curve that was
/// removed; `signal_rgb` reapplies it.
#[derive(Clone, Debug)]
pub struct LinearRgbFrame {
    pub width: usize,
    pub height: usize,
    /// r, g, b triples, row-major.
    pub data: Vec<f32>,
    pub display_gamma: f32,
}

impl LinearRgbFrame {
    /// Back to gamma-encoded signal RGB (what a framebuffer or the oracle
    /// comparison wants).
    pub fn signal_rgb(&self, i: usize) -> [f32; 3] {
        let inv = 1.0 / self.display_gamma;
        [
            self.data[i * 3].powf(inv),
            self.data[i * 3 + 1].powf(inv),
            self.data[i * 3 + 2].powf(inv),
        ]
    }
}

/// The Rung A decoder. All fields public so a MUTATE proof can perturb a
/// copy; `transcribed` is the honest constructor.
#[derive(Clone, Debug)]
pub struct RungA {
    pub chroma_taps: Vec<f32>,
    pub uv_taps: Vec<f32>,
    pub chroma_gain: f32,
    pub r_from_v: f32,
    pub g_from_u: f32,
    pub g_from_v: f32,
    pub b_from_u: f32,
    /// Demodulation angle offset putting the source's burst on -U. The
    /// source supplies it (`ntsc_source_nes::burst_axis_offset` for the
    /// NES); decode stays source-agnostic.
    pub demod_offset: f64,
    /// Black and white reference levels in volts. For the NES: the $1D
    /// and $20 voltages, per the page's normalization note.
    pub black: f32,
    pub white: f32,
    pub display_gamma: f32,
}

impl RungA {
    pub fn transcribed(demod_offset: f64, black: f32, white: f32) -> RungA {
        RungA {
            chroma_taps: tables::CHROMA_BANDPASS.to_vec(),
            uv_taps: tables::UV_LOWPASS.to_vec(),
            chroma_gain: tables::CHROMA_GAIN_AT_SUBCARRIER,
            r_from_v: tables::R_FROM_V,
            g_from_u: tables::G_FROM_U,
            g_from_v: tables::G_FROM_V,
            b_from_u: tables::B_FROM_U,
            demod_offset,
            black,
            white,
            display_gamma: 2.2,
        }
    }

    /// Decode rows `row0..row0+height` of the frame, `width` samples from
    /// each line's `active_start`. Convolution edges clamp to the line's
    /// first and last sample, which is a real waveform (porch, border)
    /// rather than an invented zero.
    pub fn decode_yuv(
        &self,
        frame: &CompositeFrame,
        row0: usize,
        height: usize,
        width: usize,
    ) -> YuvFrame {
        let mut yf = YuvFrame {
            width,
            height,
            y: vec![0.0; width * height],
            u: vec![0.0; width * height],
            v: vec![0.0; width * height],
        };
        let scale = 1.0 / (self.white - self.black);
        // Sin/cos of the twelve grid phases plus the burst axis offset.
        let mut sin12 = [0.0f32; 12];
        let mut cos12 = [0.0f32; 12];
        for (p, (s, c)) in sin12.iter_mut().zip(cos12.iter_mut()).enumerate() {
            let theta = std::f64::consts::TAU * p as f64 / 12.0 + self.demod_offset;
            *s = theta.sin() as f32;
            *c = theta.cos() as f32;
        }
        let ct_half = self.chroma_taps.len() / 2;
        let uv_half = self.uv_taps.len() / 2;
        let mut chroma = Vec::new();
        let mut u_raw = Vec::new();
        let mut v_raw = Vec::new();
        for row in 0..height {
            let line = &frame.lines[row0 + row];
            let samples = &line.samples;
            let n = samples.len();
            let at = |i: isize| samples[i.clamp(0, n as isize - 1) as usize];
            // Chroma by bandpass over the whole line; luma as complement.
            chroma.clear();
            chroma.extend((0..n).map(|i| {
                self.chroma_taps
                    .iter()
                    .enumerate()
                    .map(|(k, t)| t * at(i as isize + k as isize - ct_half as isize))
                    .sum::<f32>()
            }));
            // Demodulate over the whole line so the U/V lowpass has real
            // neighbours at the active edges.
            u_raw.clear();
            v_raw.clear();
            for (i, c) in chroma.iter().enumerate() {
                let p = frame.phase_at(row0 + row, i).get() as usize;
                let amp = c * scale * tables::CHROMA_SAT_CORRECTION / self.chroma_gain;
                u_raw.push(amp * sin12[p]);
                v_raw.push(amp * cos12[p]);
            }
            let start = line.active_start;
            for x in 0..width {
                let i = start + x;
                let conv = |raw: &[f32]| {
                    self.uv_taps
                        .iter()
                        .enumerate()
                        .map(|(k, t)| {
                            let j = (i as isize + k as isize - uv_half as isize)
                                .clamp(0, n as isize - 1) as usize;
                            t * raw[j]
                        })
                        .sum::<f32>()
                };
                let o = row * width + x;
                yf.y[o] = (samples[i] - chroma[i] - self.black) * scale;
                yf.u[o] = conv(&u_raw);
                yf.v[o] = conv(&v_raw);
            }
        }
        yf
    }

    /// The inverse matrix, clamp to [0, 1], display gamma removed.
    pub fn to_linear_rgb(&self, yuv: &YuvFrame) -> LinearRgbFrame {
        let mut data = Vec::with_capacity(yuv.width * yuv.height * 3);
        for o in 0..yuv.width * yuv.height {
            let (y, u, v) = (yuv.y[o], yuv.u[o], yuv.v[o]);
            let r = y + self.r_from_v * v;
            let g = y + self.g_from_u * u + self.g_from_v * v;
            let b = y + self.b_from_u * u;
            for c in [r, g, b] {
                data.push(c.clamp(0.0, 1.0).powf(self.display_gamma));
            }
        }
        LinearRgbFrame {
            width: yuv.width,
            height: yuv.height,
            data,
            display_gamma: self.display_gamma,
        }
    }

    pub fn decode(
        &self,
        frame: &CompositeFrame,
        row0: usize,
        height: usize,
        width: usize,
    ) -> LinearRgbFrame {
        self.to_linear_rgb(&self.decode_yuv(frame, row0, height, width))
    }
}
