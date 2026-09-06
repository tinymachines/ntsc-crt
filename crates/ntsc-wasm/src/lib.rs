//! The WASM bridge (handoff spec section 7): a dot frame in, RGBA on the
//! grid out, and the drift policy. The pipeline core is plain Rust,
//! compiled both natively (the benches) and to wasm32 (the page); the
//! `wasm-bindgen` surface is a thin shell gated on the target.
//!
//! Rates: the source runs at its own exact rate (the geometry's
//! rationals); the bridge presents the most recently completed frame on
//! each animation callback; duplicated and dropped frames are counted
//! and exposed, never resampled in time. Persistence (M3) advances by
//! the source's frame period, not wall clock.

use ntsc_decode::Decoder;
use ntsc_grid::{FrameParity, Phase, Profile};
use ntsc_source_nes::{burst_axis_offset, encode_frame, levels, DotFrame, Levels};

/// One NES pipeline: encode at the chained origin, decode, hand back
/// RGBA rows on the active sample grid (2048 x 240).
pub struct NesPipeline {
    levels: Levels,
    decoder: Decoder,
    origin: Phase,
    /// First decoded line: 0 for the notch, 1 for the three-line comb
    /// (it needs both neighbours; line 241 is still backdrop picture).
    row0: usize,
}

pub const OUT_WIDTH: usize = 2048;
pub const OUT_HEIGHT: usize = 240;

/// One encoded frame for a decoder elsewhere: `samples` is `lines` of
/// `line_len` each, `phases` the subcarrier phase at sample 0 of each
/// line (0..12), `active_start` the first active sample of a line.
pub struct EncodedFrame {
    pub samples: Vec<f32>,
    pub phases: Vec<u8>,
    pub line_len: usize,
    pub active_start: usize,
}

impl NesPipeline {
    /// `rung` is "notch" (Rung A) or "comb3" (Rung C); anything else is
    /// refused by name, not defaulted.
    pub fn new(rung: &str) -> NesPipeline {
        let (theta0, black, white) = (burst_axis_offset(), levels::LOW[1], levels::HIGH[2]);
        let decoder = match rung {
            "notch" => Decoder::transcribed(theta0, black, white),
            "comb3" => Decoder::comb_three_line(Profile::Nes, theta0, black, white),
            other => panic!("unknown rung {other:?}: this bridge runs \"notch\" or \"comb3\""),
        };
        NesPipeline {
            row0: if rung == "comb3" { 1 } else { 0 },
            levels: Levels::transcribed(),
            decoder,
            origin: Phase::new(0),
        }
    }

    /// Encode one frame at the chained origin and carry the phase: the
    /// composite samples, every line padded to the full line length
    /// with its last sample (a short line's tail is never decoded), for
    /// a decoder elsewhere (the console page's WebGPU decode). The
    /// per-line phases and the active start are `line_phases` and
    /// `active_start`; the parity codes are `push_frame`'s.
    pub fn encode(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> EncodedFrame {
        let dots = DotFrame {
            parity: Self::parity(parity),
            colour: colour.to_vec(),
            emphasis: emphasis.to_vec(),
        };
        let frame = encode_frame(&self.levels, &dots, self.origin);
        self.origin = frame.next_origin();
        let n = frame.lines[0].samples.len();
        let mut samples = Vec::with_capacity(frame.lines.len() * n);
        let mut phases = Vec::with_capacity(frame.lines.len());
        for (i, l) in frame.lines.iter().enumerate() {
            samples.extend_from_slice(&l.samples);
            samples.extend(std::iter::repeat_n(*l.samples.last().unwrap(), n - l.samples.len()));
            phases.push(frame.phase_at(i, 0).get());
        }
        EncodedFrame { samples, phases, line_len: n, active_start: frame.lines[0].active_start }
    }

    fn parity(parity: u8) -> FrameParity {
        match parity {
            0 => FrameParity::Even,
            1 => FrameParity::OddFull,
            2 => FrameParity::OddShort,
            other => panic!("parity {other} is not 0/1/2"),
        }
    }

    /// The phase the next frame is encoded at.
    pub fn origin(&self) -> Phase {
        self.origin
    }

    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    /// The first decoded line (1 on the comb).
    pub fn row0(&self) -> usize {
        self.row0
    }

    /// The encoder's constants, for an encoder elsewhere that must do
    /// this one's arithmetic: the transcribed levels [low x4, high x4,
    /// low attenuated x4, high attenuated x4, sync, burst low, burst
    /// high, blank], the three emphasis waves and the colourburst wave,
    /// then the grid: samples per dot, dots per line, lines, the short
    /// last line's deficit, and the phase step per line (the line length
    /// modulo the twelve-sample cycle). Never typed anywhere else.
    pub fn encoder_params(&self) -> Vec<f32> {
        let l = &self.levels;
        let geo = ntsc_grid::Geometry::nes();
        let full = geo.line_len(FrameParity::Even, 0);
        let short = geo.line_len(FrameParity::OddShort, ntsc_source_nes::LINES - 1);
        let mut v = Vec::new();
        v.extend_from_slice(&l.low);
        v.extend_from_slice(&l.high);
        v.extend_from_slice(&l.low_attenuated);
        v.extend_from_slice(&l.high_attenuated);
        v.extend_from_slice(&[l.sync, l.burst_low, l.burst_high, l.blank]);
        v.extend(levels::EMPHASIS_WAVES.iter().map(|w| *w as f32));
        v.push(levels::COLORBURST_WAVE as f32);
        v.push(ntsc_source_nes::SAMPLES_PER_DOT as f32);
        v.push(ntsc_source_nes::DOTS_PER_LINE as f32);
        v.push(ntsc_source_nes::LINES as f32);
        v.push((full - short) as f32);
        v.push((full % 12) as f32);
        v
    }

    /// Move the chained phase past one frame of `parity` without
    /// encoding it: what an encoder elsewhere calls after its frame.
    pub fn advance(&mut self, parity: u8) {
        self.origin = ntsc_grid::Geometry::nes().next_origin(self.origin, Self::parity(parity));
    }

    /// The decoder's constants, for a decoder elsewhere that must do this
    /// one's arithmetic: [comb w0, w1, w2, black, 1/(white-black), amp_k
    /// (the demodulation amplitude with the saturation correction and the
    /// chroma gain folded in), r_from_v, g_from_u, g_from_v, b_from_u,
    /// demod_offset, uv_decimation, first decoded row], then the
    /// decimated UV lowpass taps. Never typed anywhere else.
    pub fn decoder_params(&self) -> Vec<f32> {
        let d = &self.decoder;
        let scale = 1.0 / (d.white - d.black);
        let mut v = vec![
            d.comb_weights[0],
            d.comb_weights[1],
            d.comb_weights[2],
            d.black,
            scale,
            scale * ntsc_decode::tables::CHROMA_SAT_CORRECTION / d.chroma_gain,
            d.r_from_v,
            d.g_from_u,
            d.g_from_v,
            d.b_from_u,
            d.demod_offset as f32,
            d.uv_decimation as f32,
            self.row0 as f32,
        ];
        v.extend_from_slice(&d.uv_taps);
        v
    }

    /// Encode and decode one frame. `colour` and `emphasis` are 341 x
    /// 262 row-major dot planes; `parity` 0 = Even, 1 = OddFull,
    /// 2 = OddShort. Output is RGBA8, `OUT_WIDTH` x `OUT_HEIGHT`.
    pub fn push_frame(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> Vec<u8> {
        let dots = DotFrame {
            parity: Self::parity(parity),
            colour: colour.to_vec(),
            emphasis: emphasis.to_vec(),
        };
        let frame = encode_frame(&self.levels, &dots, self.origin);
        self.origin = frame.next_origin();
        // Straight from YUV to bytes: decode() encodes signal RGB to
        // linear light with a 2.2 power and signal_rgb() immediately
        // undoes it, which cost three million powf calls per frame for
        // a mathematical identity. The matrix and clamp here are the
        // same ones to_linear_rgb applies.
        let yuv = self.decoder.decode_yuv(&frame, self.row0, OUT_HEIGHT, OUT_WIDTH);
        let d = &self.decoder;
        let mut out = Vec::with_capacity(OUT_WIDTH * OUT_HEIGHT * 4);
        for i in 0..OUT_WIDTH * OUT_HEIGHT {
            let (y, u, v) = (yuv.y[i], yuv.u[i], yuv.v[i]);
            let r = (y + d.r_from_v * v).clamp(0.0, 1.0);
            let g = (y + d.g_from_u * u + d.g_from_v * v).clamp(0.0, 1.0);
            let b = (y + d.b_from_u * u).clamp(0.0, 1.0);
            out.push((r * 255.0 + 0.5) as u8);
            out.push((g * 255.0 + 0.5) as u8);
            out.push((b * 255.0 + 0.5) as u8);
            out.push(255);
        }
        out
    }
}

/// The drift policy, counted: the source advances at its own exact
/// period; each display callback advances the source by however many
/// whole frames elapsed. Zero advanced means the previous frame is
/// presented again (a duplicate); more than one means frames were never
/// presented (drops).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PacingStats {
    pub presented: u64,
    pub duplicated: u64,
    pub dropped: u64,
}

pub struct Pacing {
    /// Source frame period in nanoseconds, exact numerator/denominator.
    period_num: u128,
    period_den: u128,
    /// Elapsed wall time, and how many source frames have been consumed.
    elapsed_num: u128,
    consumed: u128,
    pub stats: PacingStats,
}

impl Pacing {
    /// The NES rendering-enabled pair rate: two frames per 714,736 +
    /// 714,728 samples at the grid rate, i.e. 60.09881 Hz.
    pub fn nes_rendering_enabled() -> Pacing {
        // period = pair_samples / (2 * grid_rate) seconds; grid rate is
        // 472,500,000/11 Hz, so period_ns = samples * 11e9 / (2 * 472.5e6).
        Pacing::from_samples_per_frame((714_736 + 714_728) as u128, 2)
    }

    /// Broadcast frame rate (two fields), 30,000/1,001 Hz.
    pub fn broadcast() -> Pacing {
        Pacing::from_samples_per_frame(1_433_250, 1)
    }

    fn from_samples_per_frame(samples: u128, frames: u128) -> Pacing {
        Pacing {
            period_num: samples * 11 * 1_000_000_000,
            period_den: frames * 472_500_000,
            elapsed_num: 0,
            consumed: 0,
            stats: PacingStats::default(),
        }
    }

    /// One display callback, `dt_ns` since the previous. Returns how
    /// many source frames to advance before presenting.
    pub fn tick(&mut self, dt_ns: u64) -> u32 {
        self.elapsed_num += dt_ns as u128 * self.period_den;
        let due = self.elapsed_num / self.period_num;
        let advance = (due - self.consumed) as u32;
        self.consumed = due;
        self.stats.presented += 1;
        match advance {
            0 => self.stats.duplicated += 1,
            1 => {}
            n => self.stats.dropped += (n - 1) as u64,
        }
        advance
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct Pipeline {
        inner: super::NesPipeline,
        pacing: super::Pacing,
        last_phases: Vec<u8>,
        line_len: usize,
        active_start: usize,
    }

    #[wasm_bindgen]
    impl Pipeline {
        #[wasm_bindgen(constructor)]
        pub fn new(rung: &str) -> Pipeline {
            Pipeline {
                inner: super::NesPipeline::new(rung),
                pacing: super::Pacing::nes_rendering_enabled(),
                last_phases: Vec::new(),
                line_len: 0,
                active_start: 0,
            }
        }

        pub fn push_frame(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> Vec<u8> {
            self.inner.push_frame(colour, emphasis, parity)
        }

        /// The composite samples of one frame (every line padded to the
        /// full length), the phase carried; `line_phases` and the two
        /// geometry numbers describe the last one encoded.
        pub fn encode(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> Vec<f32> {
            let f = self.inner.encode(colour, emphasis, parity);
            self.last_phases = f.phases;
            self.line_len = f.line_len;
            self.active_start = f.active_start;
            f.samples
        }

        pub fn line_phases(&self) -> Vec<u8> {
            self.last_phases.clone()
        }

        pub fn line_len(&self) -> usize {
            self.line_len
        }

        pub fn active_start(&self) -> usize {
            self.active_start
        }

        pub fn decoder_params(&self) -> Vec<f32> {
            self.inner.decoder_params()
        }

        pub fn encoder_params(&self) -> Vec<f32> {
            self.inner.encoder_params()
        }

        /// The phase the next frame is encoded at, 0..12.
        pub fn origin(&self) -> u8 {
            self.inner.origin().get()
        }

        pub fn advance(&mut self, parity: u8) {
            self.inner.advance(parity);
        }

        pub fn tick(&mut self, dt_ns: f64) -> u32 {
            self.pacing.tick(dt_ns as u64)
        }

        pub fn stats(&self) -> Vec<f64> {
            let s = self.pacing.stats;
            vec![s.presented as f64, s.duplicated as f64, s.dropped as f64]
        }

        pub fn width(&self) -> usize {
            super::OUT_WIDTH
        }

        pub fn height(&self) -> usize {
            super::OUT_HEIGHT
        }
    }
}
