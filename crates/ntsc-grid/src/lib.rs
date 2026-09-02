//! The grid contract for the ntsc-crt pipeline (handoff spec section 3).
//!
//! The composite sample is the native unit: 12 samples per colour subcarrier
//! cycle, at an exact rational rate. Every source produces frames on this
//! grid, and the subcarrier phase at any sample is a pure function of
//! geometry -- no source supplies phase as an input.
//!
//! Two geometry profiles exist: the NES (341 dots x 8 samples, 262
//! progressive lines, one dot dropped on the pre-render line of odd frames
//! with rendering enabled) and broadcast NTSC (227.5 cycles per line, 525
//! interlaced lines). The residues that fall out of these lengths -- the
//! NES's 120-degree line step and three-line chroma pattern, broadcast's
//! 180-degree step and four-field sequence -- are confirmed by
//! `tests/residues.rs`, not asserted here.

use num_rational::Ratio;

/// Samples per colour subcarrier cycle: the base grid.
pub const SAMPLES_PER_CYCLE: usize = 12;

/// The colour subcarrier frequency, exact: 315/88 MHz, stored as a ratio
/// in Hz. Never a float.
pub fn subcarrier_hz() -> Ratio<u64> {
    Ratio::new(315_000_000, 88)
}

/// A sample rate in samples per second, exact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRate(pub Ratio<u64>);

impl SampleRate {
    /// The grid rate: 12 x f_sc = 42.954545... MHz.
    pub fn grid() -> Self {
        SampleRate(subcarrier_hz() * SAMPLES_PER_CYCLE as u64)
    }
}

/// A position within the subcarrier cycle, 0..12.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Phase(u8);

impl Phase {
    pub fn new(v: u8) -> Self {
        assert!(
            (v as usize) < SAMPLES_PER_CYCLE,
            "phase {v} is outside 0..12"
        );
        Phase(v)
    }

    pub fn get(self) -> u8 {
        self.0
    }

    /// The phase this many samples later.
    pub fn advanced_by(self, samples: usize) -> Phase {
        Phase(((self.0 as usize + samples) % SAMPLES_PER_CYCLE) as u8)
    }
}

/// Which geometry a frame is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Nes,
    Broadcast,
}

/// Frame parity: the PPU emits it and the encoder consumes it, so its
/// home moved to `nes-bus`, the console's contract crate; this re-export
/// keeps every existing path compiling. `OddShort` is the NES odd frame
/// with rendering enabled, which drops dot 340 from the pre-render line;
/// it does not exist on the broadcast profile, and the geometry refuses
/// it there by name.
pub use nes_bus::FrameParity;

/// Frame geometry: line count, line length, and where the short line is.
/// Phase arithmetic lives here and nowhere else.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Geometry {
    profile: Profile,
    lines: usize,
    samples_per_line: usize,
    // Samples missing from the LAST line of an OddShort frame (the NES
    // skipped dot, 8 samples). Zero on the broadcast profile.
    short_last_line_deficit: usize,
}

impl Geometry {
    /// The NES profile: 341 dots x 8 samples per line, 262 progressive
    /// lines, skipped dot on the pre-render line of OddShort frames.
    pub fn nes() -> Geometry {
        Geometry {
            profile: Profile::Nes,
            lines: 262,
            samples_per_line: 341 * 8,
            short_last_line_deficit: 8,
        }
    }

    /// The broadcast profile: 227.5 subcarrier cycles per line, 525
    /// interlaced lines (two fields of 262.5, modelled here as one
    /// 525-line frame; field structure is the encoder's concern).
    pub fn broadcast() -> Geometry {
        Geometry {
            profile: Profile::Broadcast,
            lines: 525,
            // 227.5 cycles x 12 samples
            samples_per_line: 455 * SAMPLES_PER_CYCLE / 2,
            short_last_line_deficit: 0,
        }
    }

    /// Test-only: the same profile with a deliberately wrong line length,
    /// so MUTATE=1 runs can prove the residue tests can go red. Using this
    /// anywhere but a mutation proof is a bug by name.
    #[doc(hidden)]
    pub fn mutated_for_proof(profile: Profile) -> Geometry {
        let mut g = match profile {
            Profile::Nes => Geometry::nes(),
            Profile::Broadcast => Geometry::broadcast(),
        };
        g.samples_per_line += 2;
        g
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn lines(&self) -> usize {
        self.lines
    }

    fn check_parity(&self, parity: FrameParity) {
        if self.profile == Profile::Broadcast && parity == FrameParity::OddShort {
            panic!("OddShort is an NES frame parity; the broadcast profile has no short frame");
        }
    }

    /// Length in samples of one line of a frame with this parity.
    pub fn line_len(&self, parity: FrameParity, line: usize) -> usize {
        self.check_parity(parity);
        assert!(line < self.lines, "line {line} is outside 0..{}", self.lines);
        if parity == FrameParity::OddShort && line == self.lines - 1 {
            self.samples_per_line - self.short_last_line_deficit
        } else {
            self.samples_per_line
        }
    }

    /// Subcarrier phase at (line, sample), relative to the frame origin
    /// (i.e. assuming sample 0 of line 0 is at Phase(0)). A frame combines
    /// this with its own `phase_at_origin`.
    ///
    /// The short line is the LAST line of an OddShort frame, so no line's
    /// starting phase inside the frame is affected by parity; parity is
    /// taken so the sample bound can be checked against the right length.
    pub fn phase_at(&self, parity: FrameParity, line: usize, sample: usize) -> Phase {
        self.check_parity(parity);
        assert!(line < self.lines, "line {line} is outside 0..{}", self.lines);
        debug_assert!(
            sample < self.line_len(parity, line),
            "sample {sample} is outside line {line} of length {}",
            self.line_len(parity, line)
        );
        let step = self.samples_per_line % SAMPLES_PER_CYCLE;
        Phase::new(((line * step + sample) % SAMPLES_PER_CYCLE) as u8)
    }

    /// Total samples in a frame of this parity.
    pub fn samples_per_frame(&self, parity: FrameParity) -> usize {
        self.check_parity(parity);
        let deficit = if parity == FrameParity::OddShort {
            self.short_last_line_deficit
        } else {
            0
        };
        self.lines * self.samples_per_line - deficit
    }

    /// How far the subcarrier phase advances over one frame of this
    /// parity: `samples_per_frame` mod 12.
    pub fn frame_residue(&self, parity: FrameParity) -> u8 {
        (self.samples_per_frame(parity) % SAMPLES_PER_CYCLE) as u8
    }

    /// The origin phase of the frame after one of this parity.
    pub fn next_origin(&self, origin: Phase, parity: FrameParity) -> Phase {
        origin.advanced_by(self.samples_per_frame(parity))
    }

    /// Frames per second for this parity, exact: grid rate over frame
    /// length. On the NES profile (progressive) this is also the field
    /// rate; on broadcast one frame is two fields.
    pub fn frame_rate(&self, parity: FrameParity) -> Ratio<u64> {
        SampleRate::grid().0 / self.samples_per_frame(parity) as u64
    }

    /// Fields per second, exact. NES: same as `frame_rate` (progressive,
    /// "double struck"). Broadcast: two interlaced fields per frame.
    pub fn field_rate(&self, parity: FrameParity) -> Ratio<u64> {
        match self.profile {
            Profile::Nes => self.frame_rate(parity),
            Profile::Broadcast => self.frame_rate(parity) * 2,
        }
    }
}

/// One line of composite samples, in volts at the composite output. IRE is
/// a derived display unit, converted at the boundary and never stored.
#[derive(Clone, Debug)]
pub struct CompositeLine {
    pub samples: Vec<f32>,
    pub sync_start: usize,
    pub burst_start: usize,
    pub active_start: usize,
}

/// One frame of composite video on the grid. `lines[i].samples.len()` must
/// equal `profile.line_len(frame_parity, i)`; sources assert this.
#[derive(Clone, Debug)]
pub struct CompositeFrame {
    pub profile: Geometry,
    pub lines: Vec<CompositeLine>,
    pub frame_parity: FrameParity,
    /// Subcarrier phase at sample 0 of line 0: the one free parameter.
    pub phase_at_origin: Phase,
}

impl CompositeFrame {
    /// Absolute subcarrier phase at (line, sample) of this frame.
    pub fn phase_at(&self, line: usize, sample: usize) -> Phase {
        self.phase_at_origin
            .advanced_by(self.profile.phase_at(self.frame_parity, line, sample).get() as usize)
    }

    /// The origin phase of the frame that follows this one.
    pub fn next_origin(&self) -> Phase {
        self.profile.next_origin(self.phase_at_origin, self.frame_parity)
    }
}

/// Anything that produces composite frames on the grid: the NES encoder,
/// the RGB encoder, the capture front end.
pub trait CompositeSource {
    fn next_frame(&mut self) -> CompositeFrame;
}

/// One pipeline stage. Input and output grids are part of the types, so a
/// grid mismatch is a compile error, not a runtime warning.
pub trait Stage<In, Out> {
    fn process(&mut self, input: &In) -> Out;
}
