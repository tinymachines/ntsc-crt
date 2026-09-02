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

    /// Encode and decode one frame. `colour` and `emphasis` are 341 x
    /// 262 row-major dot planes; `parity` 0 = Even, 1 = OddFull,
    /// 2 = OddShort. Output is RGBA8, `OUT_WIDTH` x `OUT_HEIGHT`.
    pub fn push_frame(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> Vec<u8> {
        let parity = match parity {
            0 => FrameParity::Even,
            1 => FrameParity::OddFull,
            2 => FrameParity::OddShort,
            other => panic!("parity {other} is not 0/1/2"),
        };
        let dots = DotFrame {
            parity,
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
    }

    #[wasm_bindgen]
    impl Pipeline {
        #[wasm_bindgen(constructor)]
        pub fn new(rung: &str) -> Pipeline {
            Pipeline {
                inner: super::NesPipeline::new(rung),
                pacing: super::Pacing::nes_rendering_enabled(),
            }
        }

        pub fn push_frame(&mut self, colour: &[u8], emphasis: &[u8], parity: u8) -> Vec<u8> {
            self.inner.push_frame(colour, emphasis, parity)
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
