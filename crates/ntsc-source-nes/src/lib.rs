//! The NES source: per-dot (colour_index, emphasis) at 341 x 262 in, a
//! `CompositeFrame` on the grid out. Direct synthesis, like the PPU: no
//! YUV anywhere, the "colour" of a pixel is the phase at which a two-level
//! square wave toggles (never a sinusoid).
//!
//! Levels come from `data/nes-levels.toml` through `build.rs`; the signal
//! function is the nesdev page's own `NTSCsignal` C++ (revision 23864),
//! ported. The horizontal segment map is the page's scanline timing table.
//! Two documented simplifications, both invisible to M1's oracles:
//!
//! - The one-dot delays and the border-pixels-belong-to-the-previous-line
//!   subtlety are ignored: dot d of row r is drawn from `dots[r][d]`, and
//!   the porch boundaries use the rendering-row table uniformly (the
//!   post-render table prints back porch at 302 vs 303).
//! - Vertical sync was modelled at line granularity until 2026-09-18
//!   (rows 245..247 low from dot 0, a blank window at dots 254..286);
//!   it is now the shape measured on the switch-level 2C02 (`2c02`'s
//!   `vsync-probe`, the sync-tip leg read every half-step through a
//!   frame) and confirmed on a real console's record (nes-bench run
//!   20260918-135721: the broad pulse begins exactly one line after the
//!   preceding horizontal sync, 0.934 line long, three of them a line
//!   apart): three broad pulses, each from the horizontal sync position
//!   of rows 244, 245 and 246 (dot 277 here; the die's DAC shows every
//!   sync three dots later than the table's numbering, sync onset
//!   included, so relative to the horizontal sync nothing moves) to dot
//!   253 of the row after, a 23-dot blank serration at 254..276 on rows
//!   245..247, no burst on rows 244..246, and row 247 closing with an
//!   ordinary horizontal sync and burst. The old placement put the onset
//!   64 dots (0.19 line) late and the serration 9 dots wide.

use ntsc_grid::{CompositeFrame, CompositeLine, CompositeSource, Geometry, Phase};

pub mod levels {
    //! Constants generated from the gated transcription. See build.rs.
    include!(concat!(env!("OUT_DIR"), "/levels.rs"));
}

/// The dot-stream waist moved to `nes-bus`, the console's contract
/// crate: the PPU ladder produces `DotFrame` and this crate consumes it,
/// so neither side may own it. These re-exports keep every existing path
/// compiling.
pub use nes_bus::{
    DotFrame, ACTIVE_DOTS, ACTIVE_FIRST_DOT, ACTIVE_ROWS, DOTS_PER_LINE, LINES, SAMPLES_PER_DOT,
};

/// Wave `w` (a colour number 1..=12; 0 and 12 coincide) is high at sample
/// phase `p`: the page's `InColorPhase`, `(w + p) mod 12 < 6`.
pub fn wave_high(wave: u8, p: u8) -> bool {
    (wave as u32 + p as u32) % 12 < 6
}

/// The level tables as a value, so a MUTATE proof can perturb a copy.
#[derive(Clone, Debug)]
pub struct Levels {
    pub low: [f32; 4],
    pub high: [f32; 4],
    pub low_attenuated: [f32; 4],
    pub high_attenuated: [f32; 4],
    pub sync: f32,
    pub burst_low: f32,
    pub burst_high: f32,
    pub blank: f32,
}

impl Levels {
    pub fn transcribed() -> Levels {
        Levels {
            low: levels::LOW,
            high: levels::HIGH,
            low_attenuated: levels::LOW_ATTENUATED,
            high_attenuated: levels::HIGH_ATTENUATED,
            sync: levels::SYNC,
            burst_low: levels::BURST_LOW,
            burst_high: levels::BURST_HIGH,
            blank: levels::BLANK,
        }
    }

    /// The momentary signal level for a pixel at sample phase `p`: the
    /// nesdev `NTSCsignal`, ported. Colour 0 emits only the high level,
    /// colours 13..=15 only the low; 14..=15 force luma row 1 (the $1D
    /// voltage); emphasis attenuates everything but $xE/$xF while any
    /// selected wave is high.
    pub fn signal(&self, colour: u8, emphasis: u8, p: u8) -> f32 {
        let color = colour & 0x0f;
        let mut level = (colour >> 4 & 3) as usize;
        if color > 13 {
            level = 1;
        }
        let attenuated = color < 0x0e
            && (0..3).any(|bit| {
                emphasis & (1 << bit) != 0 && wave_high(levels::EMPHASIS_WAVES[bit as usize], p)
            });
        let (mut lo, mut hi) = if attenuated {
            (self.low_attenuated[level], self.high_attenuated[level])
        } else {
            (self.low[level], self.high[level])
        };
        if color == 0 {
            lo = hi;
        }
        if color > 12 {
            hi = lo;
        }
        if wave_high(color, p) {
            hi
        } else {
            lo
        }
    }
}

/// What a dot position on a line carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Segment {
    Sync,
    Blank,
    Burst,
    Picture,
}

/// The horizontal segment map, from the page's scanline timing tables
/// (start/duration in dots). See the module doc for the two
/// simplifications.
pub fn segment(row: usize, dot: usize) -> Segment {
    match row {
        // Rendering and post-render rows: full picture structure.
        0..=241 => match dot {
            277..=301 => Segment::Sync,
            306..=320 => Segment::Burst,
            302..=305 | 321..=325 | 268..=276 => Segment::Blank,
            _ => Segment::Picture,
        },
        // The vertical sync as the die emits it (module doc): row 244's
        // horizontal sync widens into the first broad pulse, which runs
        // to dot 253 of row 245; rows 245 and 246 do the same; row 247
        // carries the last serration blank and then an ordinary sync
        // and burst. No burst on 244..246.
        244 => {
            if dot >= 277 {
                Segment::Sync
            } else {
                Segment::Blank
            }
        }
        245..=246 => match dot {
            0..=253 | 277.. => Segment::Sync,
            _ => Segment::Blank,
        },
        247 => match dot {
            0..=253 | 277..=301 => Segment::Sync,
            306..=320 => Segment::Burst,
            _ => Segment::Blank,
        },
        // Blanking rows before and after vsync: sync + burst, blank picture.
        _ => match dot {
            277..=301 => Segment::Sync,
            306..=320 => Segment::Burst,
            _ => Segment::Blank,
        },
    }
}

/// Encode one dot frame at the given origin phase.
pub fn encode_frame(levels: &Levels, dots: &DotFrame, origin: Phase) -> CompositeFrame {
    let geo = Geometry::nes();
    let mut lines = Vec::with_capacity(LINES);
    for row in 0..LINES {
        let len = geo.line_len(dots.parity, row);
        let mut samples = Vec::with_capacity(len);
        for dot in 0..len / SAMPLES_PER_DOT {
            let seg = segment(row, dot);
            let (colour, emphasis) = dots.at(row, dot);
            for s in 0..SAMPLES_PER_DOT {
                let p = origin
                    .advanced_by(
                        geo.phase_at(dots.parity, row, dot * SAMPLES_PER_DOT + s).get() as usize,
                    )
                    .get();
                samples.push(match seg {
                    Segment::Sync => levels.sync,
                    Segment::Blank => levels.blank,
                    Segment::Burst => {
                        if wave_high(levels::COLORBURST_WAVE, p) {
                            levels.burst_high
                        } else {
                            levels.burst_low
                        }
                    }
                    Segment::Picture => levels.signal(colour, emphasis, p),
                });
            }
        }
        debug_assert_eq!(samples.len(), len);
        lines.push(CompositeLine {
            samples,
            sync_start: 277 * SAMPLES_PER_DOT,
            burst_start: 306 * SAMPLES_PER_DOT,
            active_start: ACTIVE_FIRST_DOT * SAMPLES_PER_DOT,
        });
    }
    CompositeFrame {
        profile: geo,
        lines,
        frame_parity: dots.parity,
        phase_at_origin: origin,
    }
}

/// The demodulation angle that puts the colorburst on the -U axis: project
/// the burst's square wave (wave 8, the same `wave_high` the encoder uses)
/// onto sin/cos of the sample phase and rotate its fundamental to -U.
/// Derived from the shared formula, not typed.
pub fn burst_axis_offset() -> f64 {
    let mut a = 0.0f64;
    let mut b = 0.0f64;
    for p in 0..12u8 {
        let s = if wave_high(levels::COLORBURST_WAVE, p) { 1.0 } else { -1.0 };
        let theta = std::f64::consts::TAU * p as f64 / 12.0;
        a += s * theta.sin();
        b += s * theta.cos();
    }
    b.atan2(a) + std::f64::consts::PI
}

/// A source over a sequence of dot frames, chaining origin phase by the
/// geometry's own residues.
pub struct NesSource {
    levels: Levels,
    frames: std::vec::IntoIter<DotFrame>,
    origin: Phase,
}

impl NesSource {
    pub fn new(frames: Vec<DotFrame>, origin: Phase) -> NesSource {
        NesSource {
            levels: Levels::transcribed(),
            frames: frames.into_iter(),
            origin,
        }
    }
}

impl CompositeSource for NesSource {
    fn next_frame(&mut self) -> CompositeFrame {
        let dots = self.frames.next().expect("NesSource ran out of frames");
        let frame = encode_frame(&self.levels, &dots, self.origin);
        self.origin = frame.next_origin();
        frame
    }
}
