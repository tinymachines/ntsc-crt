//! A `DotFrame` on a wire.
//!
//! The composite waveform is the wrong thing to send: 42.95 million
//! samples a second, and every one of them a pure function of the dot
//! stream. So the wire carries dots, and the receiver runs the same
//! encoder (`ntsc_source_nes::encode_frame`) to get the identical
//! waveform back. The contract this crate makes is exactly that: for any
//! frame, `encode_frame(decode(encode(frame)))` is sample-for-sample what
//! `encode_frame(frame)` is. `tests/roundtrip.rs` holds it to that, not to
//! "the dots look the same".
//!
//! Three decisions, each with the reason beside it:
//!
//! - **Which dots.** The encoder reads a dot only where
//!   `ntsc_source_nes::segment` says `Picture`; sync, blank and burst
//!   dots are synthesised from the level table and whatever the
//!   `DotFrame` holds there is never looked at. The wire carries the
//!   `Picture` dots in row-major order and nothing else, and the mask is
//!   built from `segment` itself at first use, so if the segment map ever
//!   moves the wire moves with it. 68,486 dots a frame (rows 0..=241,
//!   dots 0..=267 and 326..=340), not 89,342.
//! - **Emphasis per row, unless it can't be.** Emphasis comes from one
//!   PPUMASK register and is the same across a row in every game that
//!   does not race the beam. The wire carries one byte per picture row
//!   (242 bytes) and says so in a flag; a frame whose emphasis differs
//!   *within* a row is sent with a full per-dot emphasis plane and the
//!   flag clear. Exact either way; never "close enough".
//! - **Delta then deflate, with a key you can ask for.** A frame is sent
//!   as a key (its own dots) or a delta (xor against the previous frame's
//!   dots), then deflated. NES scenes barely change between frames, so a
//!   delta of a still scene deflates to a few dozen bytes. A receiver
//!   that has nothing to xor against (it just joined, or it missed a
//!   packet) refuses a delta with `WireError::NeedsKey` rather than
//!   decode garbage; the sender's `Sender::key_every` decides how long
//!   that wait is.
//!
//! The header also carries the parity and the origin phase the frame was
//! encoded at. The origin is chained across frames inside the encoder,
//! and a viewer who joins mid-stream has nothing to chain from: the wire
//! hands it to them, and a steady viewer can check its own chain against
//! the sender's every frame. Every packet carries a CRC-32 of the payload;
//! a packet that fails it is refused whole.
//!
//! Wire layout, all integers little-endian:
//!
//! ```text
//! 0   "NDOT"        magic
//! 4   u8  version   1
//! 5   u8  flags     bit 0 KEY (else delta), bit 1 EMPHASIS_PER_ROW
//! 6   u8  parity    0 Even, 1 OddFull, 2 OddShort
//! 7   u8  origin    subcarrier phase at the frame's first sample, 0..12
//! 8   u32 seq       frame number, increments by one per frame sent
//! 12  u32 crc32     of the payload
//! 16  u32 len       payload length
//! 20  payload       deflate(colour dots ++ emphasis), xor'd if delta
//! ```

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{segment, DotFrame, Segment, DOTS_PER_LINE, LINES};
use std::io::{Read, Write};
use std::sync::OnceLock;

pub const MAGIC: &[u8; 4] = b"NDOT";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 20;

const FLAG_KEY: u8 = 1 << 0;
const FLAG_EMPHASIS_PER_ROW: u8 = 1 << 1;

/// The picture mask: the `(row, dot)` positions the encoder reads, in
/// row-major order, and the rows that have any. Built once from
/// `segment` itself, never typed.
pub struct Mask {
    pub dots: Vec<(usize, usize)>,
    pub rows: Vec<usize>,
}

pub fn mask() -> &'static Mask {
    static MASK: OnceLock<Mask> = OnceLock::new();
    MASK.get_or_init(|| {
        let mut dots = Vec::new();
        let mut rows = Vec::new();
        for row in 0..LINES {
            let before = dots.len();
            for dot in 0..DOTS_PER_LINE {
                if segment(row, dot) == Segment::Picture && !dropped_for_proof(dot) {
                    dots.push((row, dot));
                }
            }
            if dots.len() != before {
                rows.push(row);
            }
        }
        Mask { dots, rows }
    })
}

/// MUTATE=1 sabotage: the plausible mistake that the picture is only
/// dots 0..=267 (the left border and active region) and the right border
/// at 326..=340 is porch. It is not; the encoder reads it. Under the
/// sabotage those dots are dropped from the wire and the waveform
/// round trip must go red.
fn dropped_for_proof(dot: usize) -> bool {
    mutate() && dot >= 326
}

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

#[derive(Debug, PartialEq, Eq)]
pub enum WireError {
    /// Not an NDOT packet, or truncated.
    Malformed(&'static str),
    /// A version this crate does not speak.
    Version(u8),
    /// The payload's CRC-32 does not match the header's.
    Checksum { expected: u32, got: u32 },
    /// A delta arrived and the receiver has no previous frame to apply it
    /// to: it just joined, or `Gap` was reported and not recovered from.
    NeedsKey { seq: u32 },
    /// The sequence jumped and the packet is a delta: `expected` was
    /// next, `got` arrived, and a delta against a frame we never saw is
    /// garbage, so it is refused. The receiver drops its previous frame;
    /// the next packet must be a key. (A key arriving across a gap is
    /// decoded, with the gap reported in `Received::missed`.)
    Gap { expected: u32, got: u32 },
    /// The decompressed payload is not the size the flags promise.
    Length { expected: usize, got: usize },
    /// A parity or origin byte outside its range.
    Field(&'static str, u8),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WireError {}

/// Parsed header, exposed so a relay can route on it without inflating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub key: bool,
    pub emphasis_per_row: bool,
    pub parity: FrameParity,
    pub origin: Phase,
    pub seq: u32,
    pub crc: u32,
    pub len: usize,
}

pub fn parse_header(bytes: &[u8]) -> Result<Header, WireError> {
    if bytes.len() < HEADER_LEN {
        return Err(WireError::Malformed("shorter than a header"));
    }
    if &bytes[0..4] != MAGIC {
        return Err(WireError::Malformed("bad magic"));
    }
    if bytes[4] != VERSION {
        return Err(WireError::Version(bytes[4]));
    }
    let flags = bytes[5];
    let parity = match bytes[6] {
        0 => FrameParity::Even,
        1 => FrameParity::OddFull,
        2 => FrameParity::OddShort,
        other => return Err(WireError::Field("parity", other)),
    };
    if bytes[7] >= 12 {
        return Err(WireError::Field("origin", bytes[7]));
    }
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    Ok(Header {
        key: flags & FLAG_KEY != 0,
        emphasis_per_row: flags & FLAG_EMPHASIS_PER_ROW != 0,
        parity,
        origin: Phase::new(bytes[7]),
        seq: u32_at(8),
        crc: u32_at(12),
        len: u32_at(16) as usize,
    })
}

fn parity_byte(p: FrameParity) -> u8 {
    match p {
        FrameParity::Even => 0,
        FrameParity::OddFull => 1,
        FrameParity::OddShort => 2,
    }
}

/// The picture dots of a frame, in mask order: colour, then emphasis.
fn gather(frame: &DotFrame) -> (Vec<u8>, Vec<u8>) {
    let m = mask();
    let mut colour = Vec::with_capacity(m.dots.len());
    let mut emphasis = Vec::with_capacity(m.dots.len());
    for &(row, dot) in &m.dots {
        let (c, e) = frame.at(row, dot);
        colour.push(c);
        emphasis.push(e);
    }
    (colour, emphasis)
}

/// Emphasis as one byte per picture row, or `None` if any row disagrees
/// with itself.
fn emphasis_rows(emphasis: &[u8]) -> Option<Vec<u8>> {
    let m = mask();
    let mut rows = Vec::with_capacity(m.rows.len());
    let mut i = 0;
    for &row in &m.rows {
        let first = emphasis[i];
        while i < m.dots.len() && m.dots[i].0 == row {
            if emphasis[i] != first {
                return None;
            }
            i += 1;
        }
        rows.push(first);
    }
    Some(rows)
}

/// The sending side: holds the previous frame's dots to delta against and
/// the frame counter.
pub struct Sender {
    prev: Option<(Vec<u8>, Vec<u8>)>,
    seq: u32,
    /// Send a key every this many frames (1 = every frame is a key).
    pub key_every: u32,
    level: Compression,
}

impl Default for Sender {
    fn default() -> Self {
        Sender::new(60)
    }
}

impl Sender {
    pub fn new(key_every: u32) -> Sender {
        Sender {
            prev: None,
            seq: 0,
            key_every: key_every.max(1),
            level: Compression::new(6),
        }
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Frame a `DotFrame` encoded at `origin`. The next call is a delta
    /// against this one unless the key cadence says otherwise.
    pub fn encode(&mut self, frame: &DotFrame, origin: Phase) -> Vec<u8> {
        self.encode_as(
            frame,
            origin,
            self.seq.is_multiple_of(self.key_every) || self.prev.is_none(),
        )
    }

    /// Frame it as a key regardless of cadence: what a sender does when a
    /// receiver reports `NeedsKey`.
    pub fn encode_key(&mut self, frame: &DotFrame, origin: Phase) -> Vec<u8> {
        self.encode_as(frame, origin, true)
    }

    fn encode_as(&mut self, frame: &DotFrame, origin: Phase, key: bool) -> Vec<u8> {
        let (colour, emphasis) = gather(frame);
        let rows = emphasis_rows(&emphasis);

        let mut raw = Vec::with_capacity(colour.len() * 2);
        let mut flags = 0u8;
        if key {
            flags |= FLAG_KEY;
            raw.extend_from_slice(&colour);
        } else {
            let (pc, _) = self.prev.as_ref().expect("delta without a previous frame");
            raw.extend(colour.iter().zip(pc).map(|(a, b)| a ^ b));
        }
        match (&rows, key) {
            (Some(r), true) => {
                flags |= FLAG_EMPHASIS_PER_ROW;
                raw.extend_from_slice(r);
            }
            (Some(r), false) => {
                flags |= FLAG_EMPHASIS_PER_ROW;
                // Delta against the previous frame's per-row emphasis;
                // if the previous frame was per-dot, derive its rows (a
                // per-dot previous frame that is row-uniform is just a
                // per-row frame written long).
                let (_, pe) = self.prev.as_ref().unwrap();
                match emphasis_rows(pe) {
                    Some(prow) => raw.extend(r.iter().zip(&prow).map(|(a, b)| a ^ b)),
                    None => {
                        // Previous frame had mixed rows: fall back to a
                        // per-dot delta so the receiver never has to guess.
                        flags &= !FLAG_EMPHASIS_PER_ROW;
                        raw.extend(emphasis.iter().zip(pe).map(|(a, b)| a ^ b));
                    }
                }
            }
            (None, true) => raw.extend_from_slice(&emphasis),
            (None, false) => {
                let (_, pe) = self.prev.as_ref().unwrap();
                raw.extend(emphasis.iter().zip(pe).map(|(a, b)| a ^ b));
            }
        }

        let mut enc = DeflateEncoder::new(Vec::with_capacity(raw.len() / 4), self.level);
        enc.write_all(&raw).unwrap();
        let payload = enc.finish().unwrap();

        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.push(flags);
        out.push(parity_byte(frame.parity));
        out.push(origin.get());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&crc32fast::hash(&payload).to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);

        self.prev = Some((colour, emphasis));
        self.seq = self.seq.wrapping_add(1);
        out
    }
}

/// What a packet decoded to.
#[derive(Clone, Debug)]
pub struct Received {
    pub seq: u32,
    pub key: bool,
    /// Frames that went missing before this one (a key across a gap).
    /// The caller advances the encoder's phase once per missing frame,
    /// or, since this packet carries `origin`, simply sets it.
    pub missed: u32,
    /// The phase the sender encoded this frame at. Feed it to the
    /// encoder on a key (a cold start) and compare it on a delta.
    pub origin: Phase,
    /// The frame, with every picture dot as sent. Dots outside the
    /// picture (sync, blank, burst positions) are zero: the encoder never
    /// reads them.
    pub frame: DotFrame,
}

/// The receiving side: the previous frame's dots, the expected sequence.
#[derive(Default)]
pub struct Receiver {
    prev: Option<(Vec<u8>, Vec<u8>)>,
    expect: Option<u32>,
}

impl Receiver {
    pub fn new() -> Receiver {
        Receiver::default()
    }

    /// Decode one packet. On `Gap` or `NeedsKey` the receiver forgets its
    /// previous frame and will accept only a key until one arrives; on
    /// any other error nothing changes.
    pub fn decode(&mut self, bytes: &[u8]) -> Result<Received, WireError> {
        let h = parse_header(bytes)?;
        let payload = bytes
            .get(HEADER_LEN..HEADER_LEN + h.len)
            .ok_or(WireError::Malformed("payload shorter than len"))?;
        if !mutate() {
            let got = crc32fast::hash(payload);
            if got != h.crc {
                return Err(WireError::Checksum {
                    expected: h.crc,
                    got,
                });
            }
        }
        let mut missed = 0;
        if let Some(expected) = self.expect {
            if h.seq != expected {
                self.prev = None;
                self.expect = None;
                if !h.key {
                    return Err(WireError::Gap {
                        expected,
                        got: h.seq,
                    });
                }
                missed = h.seq.wrapping_sub(expected);
            }
        }
        if !h.key && self.prev.is_none() {
            return Err(WireError::NeedsKey { seq: h.seq });
        }

        let m = mask();
        let n = m.dots.len();
        let e_len = if h.emphasis_per_row { m.rows.len() } else { n };
        let mut raw = Vec::with_capacity(n + e_len);
        DeflateDecoder::new(payload)
            .read_to_end(&mut raw)
            .map_err(|_| WireError::Malformed("payload does not inflate"))?;
        if raw.len() != n + e_len {
            return Err(WireError::Length {
                expected: n + e_len,
                got: raw.len(),
            });
        }
        let (c_raw, e_raw) = raw.split_at(n);

        let colour: Vec<u8> = if h.key {
            c_raw.to_vec()
        } else {
            let (pc, _) = self.prev.as_ref().unwrap();
            c_raw.iter().zip(pc).map(|(a, b)| a ^ b).collect()
        };

        // Expand per-row emphasis to per-dot, applying the delta in
        // whichever domain it was sent.
        let emphasis: Vec<u8> = if h.emphasis_per_row {
            let rows: Vec<u8> = if h.key {
                e_raw.to_vec()
            } else {
                let (_, pe) = self.prev.as_ref().unwrap();
                let prow = emphasis_rows(pe).ok_or(WireError::Malformed(
                    "per-row delta against a per-dot frame",
                ))?;
                e_raw.iter().zip(&prow).map(|(a, b)| a ^ b).collect()
            };
            let mut out = Vec::with_capacity(n);
            let mut ri = 0;
            for (i, &(row, _)) in m.dots.iter().enumerate() {
                if i > 0 && m.dots[i - 1].0 != row {
                    ri += 1;
                }
                out.push(rows[ri]);
            }
            out
        } else if h.key {
            e_raw.to_vec()
        } else {
            let (_, pe) = self.prev.as_ref().unwrap();
            e_raw.iter().zip(pe).map(|(a, b)| a ^ b).collect()
        };

        let mut frame = DotFrame::filled(h.parity, 0, 0);
        for (i, &(row, dot)) in m.dots.iter().enumerate() {
            frame.set(row, dot, colour[i], emphasis[i]);
        }

        self.prev = Some((colour, emphasis));
        self.expect = Some(h.seq.wrapping_add(1));
        Ok(Received {
            seq: h.seq,
            key: h.key,
            missed,
            origin: h.origin,
            frame,
        })
    }

    /// Forget everything: what a caller does after a `Gap` once it has
    /// advanced the phase past the missing frames.
    pub fn reset(&mut self) {
        self.prev = None;
        self.expect = None;
    }
}
