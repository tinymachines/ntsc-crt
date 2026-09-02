//! Deterministic dot-stream generators (handoff spec section 4.1). Every
//! generator's parameters are plain data, recorded in the golden run stamp
//! by whoever runs it; nothing here is random.

use ntsc_grid::FrameParity;
use ntsc_source_nes::{DotFrame, ACTIVE_DOTS, ACTIVE_FIRST_DOT, ACTIVE_ROWS};

/// The backdrop colour every generator paints outside its pattern: $0F,
/// canonical black.
pub const BACKDROP: u8 = 0x0f;

/// A frame that is one colour everywhere, borders included.
pub fn solid(parity: FrameParity, colour: u8, emphasis: u8) -> DotFrame {
    DotFrame::filled(parity, colour, emphasis)
}

/// The 64 x 8 solid sweep: every (colour, emphasis) pair, in ascending
/// order. Parameters for `solid`, one frame each.
pub fn sweep() -> Vec<(u8, u8)> {
    let mut out = Vec::with_capacity(64 * 8);
    for emphasis in 0..8u8 {
        for colour in 0..64u8 {
            out.push((colour, emphasis));
        }
    }
    out
}

/// A colour-cycle frame: twelve vertical bands across the active region,
/// hues $x1..$xC at the given luma row, backdrop elsewhere. Band edges at
/// `active_dot * 12 / 256`, so bands are 21 or 22 dots wide.
pub fn colour_cycle(parity: FrameParity, luma: u8) -> DotFrame {
    let mut f = DotFrame::filled(parity, BACKDROP, 0);
    for row in 0..ACTIVE_ROWS {
        for i in 0..ACTIVE_DOTS {
            let hue = (i * 12 / ACTIVE_DOTS) as u8 + 1;
            f.set(row, ACTIVE_FIRST_DOT + i, (luma << 4) | hue, 0);
        }
    }
    f
}

/// The two three-frame colour-cycle sets the spec names: same picture,
/// parity sequences Even/OddFull/Even and Even/OddShort/Even.
pub fn colour_cycle_set(short: bool) -> Vec<DotFrame> {
    let mid = if short { FrameParity::OddShort } else { FrameParity::OddFull };
    [FrameParity::Even, mid, FrameParity::Even]
        .into_iter()
        .map(|p| colour_cycle(p, 1))
        .collect()
}

/// Vertical stripes: two colours alternating every active dot, for comb
/// testing (M2's Rung C cancellation frame).
pub fn stripes(parity: FrameParity, a: u8, b: u8) -> DotFrame {
    let mut f = DotFrame::filled(parity, BACKDROP, 0);
    for row in 0..ACTIVE_ROWS {
        for i in 0..ACTIVE_DOTS {
            let c = if i % 2 == 0 { a } else { b };
            f.set(row, ACTIVE_FIRST_DOT + i, c, 0);
        }
    }
    f
}
