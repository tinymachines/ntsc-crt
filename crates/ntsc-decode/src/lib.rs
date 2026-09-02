//! Decode: `CompositeFrame` to `YuvFrame` to `LinearRgbFrame`.
//!
//! Four Y/C separation rungs, each selectable at construction and
//! verified separately (handoff spec section 5):
//!
//! - **Notch** (Rung A): chroma by the generated bandpass, luma as its
//!   exact complement. Works on both profiles.
//! - **Two-line comb** (Rung B): relies on the broadcast profile's
//!   180-degree line residue. **Refused by name on the NES profile at
//!   construction**: there the residue is 120 degrees, so the two-line
//!   difference attenuates chroma to exactly half (|1 - e^{i120}|/2 = ...
//!   measured in the tests) at a shifted phase instead of extracting it.
//! - **Three-line comb** (Rung C): the broadcast (1,2,1)/4 adaptive
//!   weights, or on the NES profile the native (1,1,1)/3 comb: three
//!   lines at 0/120/240 degrees are three equal phasors summing to zero.
//! - **Temporal comb** (Rung D, `TemporalComb`): frame averaging on the
//!   NES profile. The spec's two-frame claim fails confirmation (the
//!   frame residues are 120/240 degrees, never 180); the tests measure
//!   the truth: two frames attenuate chroma to half, and it takes three
//!   full frames (the rendering-disabled pattern, residue 4 each) to
//!   cancel exactly.
//!
//! After separation, all rungs share the same tail: QAM demodulation by
//! sin/cos at the phase from `Geometry::phase_at` plus a caller-supplied
//! burst axis offset, U/V lowpass, the transcribed inverse matrix,
//! display gamma to linear.
//!
//! One recorded naming deviation from the spec: the intermediate is
//! `YuvFrame`, not `YiqFrame`: what is implemented is equiband YUV,
//! which ST 170M-2004 clause 7.2 makes the primary encoding spec (the
//! split I/Q bandwidths are its NTSC-1953 continuation note).

use std::collections::VecDeque;

use ntsc_grid::{CompositeFrame, Profile};

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
    /// Back to gamma-encoded signal RGB (what a framebuffer or the
    /// oracle comparison wants).
    pub fn signal_rgb(&self, i: usize) -> [f32; 3] {
        let inv = 1.0 / self.display_gamma;
        [
            self.data[i * 3].powf(inv),
            self.data[i * 3 + 1].powf(inv),
            self.data[i * 3 + 2].powf(inv),
        ]
    }
}

/// How luma and chroma are pulled apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Separation {
    Notch,
    CombTwoLine,
    CombThreeLine,
}

/// The decoder. All fields public so a MUTATE proof can perturb a copy;
/// the named constructors are the honest ones.
#[derive(Clone, Debug)]
pub struct Decoder {
    pub separation: Separation,
    /// Comb weights over (previous, current, next) line; unused by Notch.
    pub comb_weights: [f32; 3],
    pub chroma_taps: Vec<f32>,
    pub uv_taps: Vec<f32>,
    /// What the separation stage's chroma path gains at the subcarrier:
    /// the measured bandpass response for Notch, exactly 1 for combs.
    pub chroma_gain: f32,
    pub r_from_v: f32,
    pub g_from_u: f32,
    pub g_from_v: f32,
    pub b_from_u: f32,
    /// Demodulation angle offset putting the source's burst on -U. The
    /// source supplies it; decode stays source-agnostic.
    pub demod_offset: f64,
    /// Black and white reference levels in volts.
    pub black: f32,
    pub white: f32,
    pub display_gamma: f32,
}

impl Decoder {
    /// Rung A: the notch/bandpass split, either profile.
    pub fn transcribed(demod_offset: f64, black: f32, white: f32) -> Decoder {
        Decoder {
            separation: Separation::Notch,
            comb_weights: [0.0; 3],
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

    /// Rung B: the two-line comb. Broadcast only, and the refusal is the
    /// point: on the NES profile the line residue is 120 degrees, not
    /// 180, so this rung cannot cancel there (it attenuates chroma to
    /// half at a shifted phase), and selecting it must be an error, not
    /// a silently worse picture.
    pub fn comb_two_line(profile: Profile, demod_offset: f64, black: f32, white: f32) -> Decoder {
        if profile == Profile::Nes {
            panic!(
                "Rung B (two-line comb) cannot work on the NES profile: the line residue is \
                 120 degrees, not 180, so adjacent lines cannot cancel chroma (Rung C's \
                 three-line comb is the NES-native comb)"
            );
        }
        Decoder {
            separation: Separation::CombTwoLine,
            comb_weights: [0.5, 0.5, 0.0],
            chroma_gain: 1.0,
            ..Decoder::transcribed(demod_offset, black, white)
        }
    }

    /// Rung C: the three-line comb, weights per profile: (1,1,1)/3 on
    /// the NES (three equal phasors 120 degrees apart sum to zero),
    /// (1,2,1)/4 on broadcast.
    pub fn comb_three_line(profile: Profile, demod_offset: f64, black: f32, white: f32) -> Decoder {
        let comb_weights = match profile {
            Profile::Nes => [1.0 / 3.0; 3],
            Profile::Broadcast => [0.25, 0.5, 0.25],
        };
        Decoder {
            separation: Separation::CombThreeLine,
            comb_weights,
            chroma_gain: 1.0,
            ..Decoder::transcribed(demod_offset, black, white)
        }
    }

    /// Split one line into (luma, chroma) per the configured separation.
    fn separate(&self, frame: &CompositeFrame, line: usize) -> (Vec<f32>, Vec<f32>) {
        let cur = &frame.lines[line].samples;
        let n = cur.len();
        match self.separation {
            Separation::Notch => {
                let taps = &self.chroma_taps;
                let half = taps.len() / 2;
                let at = |i: isize| cur[i.clamp(0, n as isize - 1) as usize];
                let chroma: Vec<f32> = (0..n)
                    .map(|i| {
                        taps.iter()
                            .enumerate()
                            .map(|(k, t)| t * at(i as isize + k as isize - half as isize))
                            .sum()
                    })
                    .collect();
                let luma = cur.iter().zip(&chroma).map(|(s, c)| s - c).collect();
                (luma, chroma)
            }
            Separation::CombTwoLine => {
                assert!(line >= 1, "the two-line comb needs a previous line");
                let prev = &frame.lines[line - 1].samples;
                let luma: Vec<f32> = (0..n)
                    .map(|i| self.comb_weights[0] * prev[i] + self.comb_weights[1] * cur[i])
                    .collect();
                let chroma = cur.iter().zip(&luma).map(|(s, y)| s - y).collect();
                (luma, chroma)
            }
            Separation::CombThreeLine => {
                assert!(
                    line >= 1 && line + 1 < frame.lines.len(),
                    "the three-line comb needs both neighbours"
                );
                let prev = &frame.lines[line - 1].samples;
                let next = &frame.lines[line + 1].samples;
                let w = self.comb_weights;
                let luma: Vec<f32> = (0..n)
                    .map(|i| w[0] * prev[i] + w[1] * cur[i] + w[2] * next[i])
                    .collect();
                let chroma = cur.iter().zip(&luma).map(|(s, y)| s - y).collect();
                (luma, chroma)
            }
        }
    }

    /// The shared demodulation tail for one row's (luma, chroma), at the
    /// frame's own phase.
    #[allow(clippy::too_many_arguments)]
    fn demod_row(
        &self,
        frame: &CompositeFrame,
        line: usize,
        luma: &[f32],
        chroma: &[f32],
        width: usize,
        out_row: usize,
        yf: &mut YuvFrame,
    ) {
        let n = chroma.len();
        let scale = 1.0 / (self.white - self.black);
        let mut sin12 = [0.0f32; 12];
        let mut cos12 = [0.0f32; 12];
        for (p, (s, c)) in sin12.iter_mut().zip(cos12.iter_mut()).enumerate() {
            let theta = std::f64::consts::TAU * p as f64 / 12.0 + self.demod_offset;
            *s = theta.sin() as f32;
            *c = theta.cos() as f32;
        }
        let uv_half = self.uv_taps.len() / 2;
        let mut u_raw = Vec::with_capacity(n);
        let mut v_raw = Vec::with_capacity(n);
        for (i, c) in chroma.iter().enumerate() {
            let p = frame.phase_at(line, i).get() as usize;
            let amp = c * scale * tables::CHROMA_SAT_CORRECTION / self.chroma_gain;
            u_raw.push(amp * sin12[p]);
            v_raw.push(amp * cos12[p]);
        }
        let start = frame.lines[line].active_start;
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
            let o = out_row * width + x;
            yf.y[o] = (luma[i] - self.black) * scale;
            yf.u[o] = conv(&u_raw);
            yf.v[o] = conv(&v_raw);
        }
    }

    /// Decode rows `row0..row0+height` of the frame, `width` samples
    /// from each line's `active_start`. Convolution edges clamp to the
    /// line's first and last sample, which is a real waveform (porch,
    /// border) rather than an invented zero.
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
        for row in 0..height {
            let (luma, chroma) = self.separate(frame, row0 + row);
            self.demod_row(frame, row0 + row, &luma, &chroma, width, row, &mut yf);
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

/// Rung D: the temporal comb. Averages the last `frames` frames per
/// sample: luma is the average, chroma is the newest frame minus it,
/// demodulated at the newest frame's own phase. Stateful, so frame
/// order matters. NES profile (the broadcast profile has no use for it:
/// its four-field sequence pairs at 180 degrees, but so do its lines,
/// which is what Rung B already exploits without the motion smear).
pub struct TemporalComb {
    frames: usize,
    history: VecDeque<CompositeFrame>,
}

impl TemporalComb {
    pub fn new(frames: usize) -> TemporalComb {
        assert!(frames >= 2, "a temporal comb averages at least two frames");
        TemporalComb {
            frames,
            history: VecDeque::new(),
        }
    }

    /// Push a frame; once enough history exists, decode the newest
    /// frame's rows through the given decoder's demodulation tail.
    /// Returns None until the window is full.
    pub fn push_and_decode(
        &mut self,
        frame: CompositeFrame,
        dec: &Decoder,
        row0: usize,
        height: usize,
        width: usize,
    ) -> Option<YuvFrame> {
        self.history.push_back(frame);
        if self.history.len() > self.frames {
            self.history.pop_front();
        }
        if self.history.len() < self.frames {
            return None;
        }
        let cur = self.history.back().unwrap();
        // The frame-mean split passes chroma at unity, whatever the
        // borrowed decoder's own separation gained: without this
        // override a notch decoder's 0.907 divisor inflates the
        // temporal comb's saturation by 10%, which is how it was found.
        let mut dec = dec.clone();
        dec.chroma_gain = 1.0;
        let dec = &dec;
        let mut yf = YuvFrame {
            width,
            height,
            y: vec![0.0; width * height],
            u: vec![0.0; width * height],
            v: vec![0.0; width * height],
        };
        let k = 1.0 / self.frames as f32;
        for row in 0..height {
            let line = row0 + row;
            let n = cur.lines[line].samples.len();
            let mut luma = vec![0.0f32; n];
            for f in &self.history {
                for (i, l) in luma.iter_mut().enumerate() {
                    *l += k * f.lines[line].samples[i];
                }
            }
            let chroma: Vec<f32> = cur.lines[line]
                .samples
                .iter()
                .zip(&luma)
                .map(|(s, y)| s - y)
                .collect();
            dec.demod_row(cur, line, &luma, &chroma, width, row, &mut yf);
        }
        Some(yf)
    }
}
