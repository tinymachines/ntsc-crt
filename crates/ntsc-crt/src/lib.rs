//! The CRT stages (handoff spec section 6): `LinearRgbFrame` in,
//! `DisplayFrame` out, five stages in a fixed order, each optional:
//! beam, scanlines, phosphor persistence, mask, geometry.
//!
//! This crate is a model and the spec is honest about that: there is no
//! external oracle. Verification is internal and analytic (an impulse
//! through the beam must produce the declared Gaussian; a step through
//! persistence must decay with the declared constant), and **every
//! numeric parameter here is authored**, labelled so in `CrtParams`,
//! never presented as a measurement of any real tube.
//!
//! The grid finally becomes pixels in the beam stage, and the resampling
//! ratio is stated on the frame (`samples_per_pixel`). Output is 256 x
//! 240 times an integer scale, matching the shell's integer-scale rule;
//! the NES pixel aspect (8/7) is deliberately not applied, recorded
//! here. Persistence advances by the source's frame period, not wall
//! clock (spec section 7), so playback is deterministic.

use ntsc_decode::LinearRgbFrame;
use ntsc_grid::Stage;

/// Pixels on the way to a canvas: linear light RGB, integer-scaled
/// geometry, with the beam stage's resampling ratio recorded.
#[derive(Clone, Debug)]
pub struct DisplayFrame {
    pub width: usize,
    pub height: usize,
    /// r, g, b triples, row-major, linear light.
    pub data: Vec<f32>,
    /// Composite samples per output pixel column, stated by the beam.
    pub samples_per_pixel: f32,
}

impl DisplayFrame {
    fn zero(width: usize, height: usize, samples_per_pixel: f32) -> DisplayFrame {
        DisplayFrame {
            width,
            height,
            data: vec![0.0; width * height * 3],
            samples_per_pixel,
        }
    }

    pub fn at(&self, x: usize, y: usize) -> [f32; 3] {
        let o = (y * self.width + x) * 3;
        [self.data[o], self.data[o + 1], self.data[o + 2]]
    }

    /// Gamma-encode to RGBA bytes for a canvas (display gamma 2.2,
    /// recorded; the inverse of what decode removed).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for i in 0..self.width * self.height {
            for c in 0..3 {
                let v = self.data[i * 3 + c].clamp(0.0, 1.0).powf(1.0 / 2.2);
                out.push((v * 255.0 + 0.5) as u8);
            }
            out.push(255);
        }
        out
    }
}

/// Every knob of the model, all authored (spec section 6: "a modelled
/// parameter, documented as authored, not measured").
#[derive(Clone, Debug)]
pub struct CrtParams {
    /// Integer output scale: 256 x 240 input pixels become
    /// (256*scale) x (240*scale) output pixels.
    pub scale: usize,
    /// Horizontal Gaussian spot width, in composite samples.
    pub beam_sigma_samples: f32,
    /// Vertical scanline profile: sigma in output rows at zero beam
    /// current, and the bloom coefficient: sigma(v) = base * (1 +
    /// bloom * v) where v is the pixel's linear luminance. The
    /// bloom-current relationship is a modelled parameter.
    pub scanline_sigma_rows: f32,
    pub bloom: f32,
    /// Per-channel phosphor decay time constants, seconds.
    pub persistence_tau: [f32; 3],
    /// Source frame period, seconds: what persistence advances by.
    pub frame_period: f32,
    pub mask: Option<MaskParams>,
    pub geometry: Option<GeometryParams>,
}

#[derive(Clone, Debug)]
pub struct MaskParams {
    /// Output pixels per phosphor column; a triad is 3 * pitch wide.
    /// Integer-only tiling, per the shell's integer scale rule.
    pub pitch: usize,
    /// Gain of the two off channels in a phosphor column (the on
    /// channel keeps 1.0).
    pub off_gain: f32,
}

#[derive(Clone, Debug)]
pub struct GeometryParams {
    /// Barrel distortion coefficient: source = centre + d * (1 + k*r^2),
    /// r normalized to the half-diagonal.
    pub barrel_k: f32,
    /// Corner rounding radius, output pixels.
    pub corner_radius: f32,
}

impl CrtParams {
    /// The authored defaults: sigma covering about half a dot
    /// horizontally, a visible scanline gap, mild bloom, P22-ish decay
    /// ordering (blue fastest, green slowest), NES rendering-enabled
    /// frame period, mask and geometry off.
    pub fn authored(scale: usize) -> CrtParams {
        assert!(scale >= 1, "the scale is an integer, one or more");
        CrtParams {
            scale,
            beam_sigma_samples: 4.0,
            scanline_sigma_rows: 0.30 * scale as f32,
            bloom: 0.6,
            persistence_tau: [0.012, 0.020, 0.008],
            frame_period: (714_736.0 + 714_728.0) / 2.0 / (472_500_000.0 / 11.0),
            mask: None,
            geometry: None,
        }
    }
}

/// Stage 1: the beam. Horizontal Gaussian integration of composite
/// samples into output pixel columns; where the grid becomes pixels.
pub struct Beam {
    pub params: CrtParams,
}

impl Stage<LinearRgbFrame, DisplayFrame> for Beam {
    fn process(&mut self, input: &LinearRgbFrame) -> DisplayFrame {
        let w = 256 * self.params.scale;
        let ratio = input.width as f32 / w as f32;
        let sigma = self.params.beam_sigma_samples;
        let reach = (3.0 * sigma).ceil() as isize;
        let mut out = DisplayFrame::zero(w, input.height, ratio);
        for y in 0..input.height {
            for x in 0..w {
                let center = (x as f32 + 0.5) * ratio - 0.5;
                let c0 = center.round() as isize;
                let mut acc = [0.0f32; 3];
                let mut wsum = 0.0f32;
                for i in c0 - reach..=c0 + reach {
                    let d = i as f32 - center;
                    let wt = (-d * d / (2.0 * sigma * sigma)).exp();
                    let j = i.clamp(0, input.width as isize - 1) as usize;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += wt * input.data[(y * input.width + j) * 3 + c];
                    }
                    wsum += wt;
                }
                let o = (y * w + x) * 3;
                for (c, a) in acc.iter().enumerate() {
                    out.data[o + c] = a / wsum;
                }
            }
        }
        out
    }
}

/// Stage 2: scanlines. Each input line paints a vertical Gaussian whose
/// width grows with the pixel's luminance (brighter lines bloom wider);
/// the output has `scale` rows per input line and the gaps fall out of
/// the profile rather than being drawn.
pub struct Scanlines {
    pub params: CrtParams,
}

impl Stage<DisplayFrame, DisplayFrame> for Scanlines {
    fn process(&mut self, input: &DisplayFrame) -> DisplayFrame {
        let s = self.params.scale;
        let out_h = input.height * s;
        let mut out = DisplayFrame::zero(input.width, out_h, input.samples_per_pixel);
        let base = self.params.scanline_sigma_rows;
        let bloom = self.params.bloom;
        // A line can reach a few rows past its own band at maximum bloom.
        let max_sigma = base * (1.0 + bloom);
        let reach = (3.0 * max_sigma).ceil() as isize + s as isize;
        for line in 0..input.height {
            let center = (line as f32 + 0.5) * s as f32 - 0.5;
            let y0 = center.round() as isize;
            for x in 0..input.width {
                let px = input.at(x, line);
                // Rec-luma of the pixel as the beam current.
                let v = (0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2]).clamp(0.0, 1.0);
                let sigma = base * (1.0 + bloom * v);
                for y in y0 - reach..=y0 + reach {
                    if y < 0 || y >= out_h as isize {
                        continue;
                    }
                    let d = y as f32 - center;
                    let wt = (-d * d / (2.0 * sigma * sigma)).exp();
                    if wt < 1e-4 {
                        continue;
                    }
                    let o = (y as usize * input.width + x) * 3;
                    for (c, p) in px.iter().enumerate() {
                        out.data[o + c] += wt * p;
                    }
                }
            }
        }
        out
    }
}

/// Stage 3: phosphor persistence. Per-channel exponential decay across
/// frames: the screen holds max(excitation, previous * decay). Stateful;
/// frame order matters, and the decay step is the source frame period.
pub struct Persistence {
    decay: [f32; 3],
    state: Option<Vec<f32>>,
}

impl Persistence {
    pub fn new(params: &CrtParams) -> Persistence {
        let decay = std::array::from_fn(|c| {
            (-params.frame_period / params.persistence_tau[c]).exp()
        });
        Persistence { decay, state: None }
    }

    pub fn reset(&mut self) {
        self.state = None;
    }
}

impl Stage<DisplayFrame, DisplayFrame> for Persistence {
    fn process(&mut self, input: &DisplayFrame) -> DisplayFrame {
        let mut out = input.clone();
        if let Some(prev) = &self.state {
            assert_eq!(prev.len(), out.data.len(), "frame size changed mid-run");
            for (i, v) in out.data.iter_mut().enumerate() {
                let held = prev[i] * self.decay[i % 3];
                if held > *v {
                    *v = held;
                }
            }
        }
        self.state = Some(out.data.clone());
        out
    }
}

/// Stage 4: the mask. An aperture-grille attenuation pattern tiled at an
/// integer pitch: each phosphor column passes one channel at full gain
/// and the other two at `off_gain`.
pub struct Mask {
    pub params: MaskParams,
}

impl Mask {
    pub fn new(params: MaskParams) -> Mask {
        assert!(params.pitch >= 1, "the mask pitch is an integer, one or more");
        Mask { params }
    }
}

impl Stage<DisplayFrame, DisplayFrame> for Mask {
    fn process(&mut self, input: &DisplayFrame) -> DisplayFrame {
        let mut out = input.clone();
        let p = self.params.pitch;
        let g = self.params.off_gain;
        for y in 0..out.height {
            for x in 0..out.width {
                let on = (x / p) % 3;
                let o = (y * out.width + x) * 3;
                for c in 0..3 {
                    if c != on {
                        out.data[o + c] *= g;
                    }
                }
            }
        }
        out
    }
}

/// Stage 5: geometry. Barrel curvature and corner rounding, off by
/// default. The shell's guaranteed-visible window (the centred 224 x 224
/// of the 256 x 240 input, times the scale) must remain visible when on;
/// `window_visible` is the check the tests and the gate run.
pub struct Geometry {
    pub params: GeometryParams,
}

impl Stage<DisplayFrame, DisplayFrame> for Geometry {
    fn process(&mut self, input: &DisplayFrame) -> DisplayFrame {
        let mut out = DisplayFrame::zero(input.width, input.height, input.samples_per_pixel);
        let (w, h) = (input.width as f32, input.height as f32);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let half_diag = (cx * cx + cy * cy).sqrt();
        let k = self.params.barrel_k;
        let cr = self.params.corner_radius;
        for y in 0..input.height {
            for x in 0..input.width {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let r2 = (dx * dx + dy * dy) / (half_diag * half_diag);
                let sx = cx + dx * (1.0 + k * r2) - 0.5;
                let sy = cy + dy * (1.0 + k * r2) - 0.5;
                // Corner rounding: distance past the rounded rectangle.
                let ex = (dx.abs() - (cx - cr)).max(0.0);
                let ey = (dy.abs() - (cy - cr)).max(0.0);
                if (ex * ex + ey * ey).sqrt() > cr {
                    continue; // outside the rounded corner: stays black
                }
                if sx < 0.0 || sy < 0.0 || sx > w - 1.0 || sy > h - 1.0 {
                    continue; // curved off the tube: stays black
                }
                let (x0, y0) = (sx as usize, sy as usize);
                let (x1, y1) = ((x0 + 1).min(input.width - 1), (y0 + 1).min(input.height - 1));
                let (tx, ty) = (sx - x0 as f32, sy - y0 as f32);
                let o = (y * input.width + x) * 3;
                for c in 0..3 {
                    let f = |xx: usize, yy: usize| input.data[(yy * input.width + xx) * 3 + c];
                    let top = f(x0, y0) + (f(x1, y0) - f(x0, y0)) * tx;
                    let bot = f(x0, y1) + (f(x1, y1) - f(x0, y1)) * tx;
                    out.data[o + c] = top + (bot - top) * ty;
                }
            }
        }
        out
    }
}

/// True when every pixel of the centred 224 x 224 window (input-pixel
/// units, times the scale) is lit for a frame that was lit everywhere:
/// the shell's guaranteed-visible invariant.
pub fn window_visible(frame: &DisplayFrame, scale: usize) -> bool {
    let wx0 = (256 - 224) / 2 * scale;
    let wy0 = (240 - 224) / 2 * scale;
    let side = 224 * scale;
    for y in wy0..wy0 + side {
        for x in wx0..wx0 + side {
            let px = frame.at(x, y);
            if px[0] + px[1] + px[2] <= 0.0 {
                return false;
            }
        }
    }
    true
}

/// The five stages in their fixed order, driven off one `CrtParams`.
pub struct CrtPipeline {
    beam: Beam,
    scanlines: Scanlines,
    persistence: Persistence,
    mask: Option<Mask>,
    geometry: Option<Geometry>,
    pub scale: usize,
}

impl CrtPipeline {
    pub fn new(params: CrtParams) -> CrtPipeline {
        CrtPipeline {
            persistence: Persistence::new(&params),
            mask: params.mask.clone().map(Mask::new),
            geometry: params.geometry.clone().map(|params| Geometry { params }),
            scale: params.scale,
            beam: Beam { params: params.clone() },
            scanlines: Scanlines { params },
        }
    }

    pub fn reset(&mut self) {
        self.persistence.reset();
    }

    pub fn process(&mut self, input: &LinearRgbFrame) -> DisplayFrame {
        let mut f = self.beam.process(input);
        f = self.scanlines.process(&f);
        f = self.persistence.process(&f);
        if let Some(mask) = &mut self.mask {
            f = mask.process(&f);
        }
        if let Some(geometry) = &mut self.geometry {
            f = geometry.process(&f);
        }
        f
    }
}
