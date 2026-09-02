//! blargg's colour generation, ported line for line from nes_ntsc.c and
//! nes_ntsc_impl.h (nes_ntsc 0.2.2, Shay Green, LGPL 2.1 or later; see
//! NOTICE.md). Test-only, never shipped. The port exists so every
//! comparison constant comes from the oracle's own source text rather
//! than from a paraphrase, and it is itself held to the compiled
//! library's palette_out in tests/golden.rs.
//!
//! Composite-preset conditions are baked in, recorded: hue 0, saturation
//! 1, contrast 1, brightness 0, no custom palettes, so STD_HUE_CONDITION
//! holds, the gamma term is +0.1333 and the decoder hue is
//! std_decoder_hue = -15 degrees.

/// lo_levels / hi_levels from nes_ntsc.c: blargg's two-decimal
/// approximation of the same terminated measurement our level table
/// transcribes, prenormalized black-to-white. A named divergence source.
pub const LO_LEVELS: [f32; 4] = [-0.12, 0.00, 0.31, 0.72];
pub const HI_LEVELS: [f32; 4] = [0.40, 0.68, 1.00, 1.00];

/// phases[i] = cos(i * pi / 6); TO_ANGLE_SIN(c) = phases[c],
/// TO_ANGLE_COS(c) = phases[c + 3].
const PHASES: [f32; 19] = [
    -1.0, -0.866025, -0.5, 0.0, 0.5, 0.866025, 1.0, 0.866025, 0.5, 0.0, -0.5, -0.866025, -1.0,
    -0.866025, -0.5, 0.0, 0.5, 0.866025, 1.0,
];

/// The pre-decoder (y, i, q) for a 9-bit entry (emphasis << 6 | colour):
/// nes_ntsc.c lines 106..178. Chroma is the square wave's HALF-SWING as
/// a clean phasor: blargg models the fundamental only, with amplitude
/// (hi - lo) / 2 rather than the fundamental's 4/pi times that. Both
/// facts are what the fitted IqMap scale measures.
pub fn yiq(entry: u16) -> (f32, f32, f32) {
    let level = (entry >> 4 & 3) as usize;
    let mut lo = LO_LEVELS[level];
    let mut hi = HI_LEVELS[level];
    let color = (entry & 0x0f) as usize;
    if color == 0 {
        lo = hi;
    }
    if color == 0x0d {
        hi = lo;
    }
    if color > 0x0d {
        hi = 0.0;
        lo = 0.0;
    }
    let sat = (hi - lo) * 0.5;
    let mut i = PHASES[color] * sat;
    let mut q = PHASES[color + 3] * sat;
    let mut y = (hi + lo) * 0.5;

    // Colour emphasis: blargg's own approximation in YIQ space, against
    // our measured per-level attenuated voltages. A named divergence.
    let tint = (entry >> 6 & 7) as usize;
    if tint != 0 && color <= 0x0d {
        const ATTEN_MUL: f32 = 0.79399;
        const ATTEN_SUB: f32 = 0.0782838;
        if tint == 7 {
            y = y * (ATTEN_MUL * 1.13) - (ATTEN_SUB * 1.13);
        } else {
            const TINTS: [usize; 8] = [0, 6, 10, 8, 2, 4, 0, 0];
            let tint_color = TINTS[tint];
            let mut sat = hi * (0.5 - ATTEN_MUL * 0.5) + ATTEN_SUB * 0.5;
            y -= sat * 0.5;
            if tint >= 3 && tint != 4 {
                sat *= 0.6;
                y -= sat;
            }
            i += PHASES[tint_color] * sat;
            q += PHASES[tint_color + 3] * sat;
        }
    }
    (y, i, q)
}

/// default_decoder from nes_ntsc_impl.h: the classic YIQ-to-RGB rows.
pub const DEFAULT_DECODER: [f32; 6] = [0.956, 0.621, -0.272, -0.647, -1.105, 1.702];

/// The composite preset's whole tail, nes_ntsc.c lines 191..219: the
/// brightness epsilon, the unrotated matrix, the fast quadratic gamma
/// (factor pow(0.1333, 0.73)), re-encode to YIQ, then the -15-degree
/// rotated matrix scaled to bytes. Returns 0..255 floats; the compiled
/// library's integer packing truncates, so hold comparisons to about two
/// counts.
pub fn rgb255(y: f32, i: f32, q: f32) -> [f32; 3] {
    let y = y - 0.5 / 256.0;
    let d = DEFAULT_DECODER;
    let r = y + d[0] * i + d[1] * q;
    let g = y + d[2] * i + d[3] * q;
    let b = y + d[4] * i + d[5] * q;
    let gf = 0.1333f32.powf(0.73);
    let gam = |c: f32| (c * gf - gf) * c + c;
    let (r, g, b) = (gam(r), gam(g), gam(b));
    let y2 = r * 0.299 + g * 0.587 + b * 0.114;
    let i2 = r * 0.596 - g * 0.275 - b * 0.321;
    let q2 = r * 0.212 - g * 0.523 + b * 0.311;
    let (s, c) = (-15.0f32.to_radians()).sin_cos();
    let rot = [
        d[0] * c - d[1] * s,
        d[0] * s + d[1] * c,
        d[2] * c - d[3] * s,
        d[2] * s + d[3] * c,
        d[4] * c - d[5] * s,
        d[4] * s + d[5] * c,
    ];
    let out = |m0: f32, m1: f32| ((y2 + m0 * i2 + m1 * q2) * 256.0).clamp(0.0, 255.0);
    [out(rot[0], rot[1]), out(rot[2], rot[3]), out(rot[4], rot[5])]
}

/// blargg's DC colour for an entry, through the whole ported chain.
pub fn palette_rgb(entry: u16) -> [f32; 3] {
    let (y, i, q) = yiq(entry);
    rgb255(y, i, q)
}
