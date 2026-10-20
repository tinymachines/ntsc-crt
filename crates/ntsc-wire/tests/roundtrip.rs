//! The wire held to the only thing that matters: the waveform the
//! receiver synthesises is sample-for-sample the one the sender would
//! have. Then the behaviour around it: deltas chain, a late joiner is
//! refused until a key, a gap is reported, a corrupted packet is refused
//! whole, emphasis that varies within a row still survives, and the
//! sizes are what the design claims.
//!
//! MUTATE=1 sabotages two things: the picture mask drops the right border
//! (dots 326..=340), and the checksum is not checked. Every test that
//! compares a waveform or the mask goes red, and so does the corruption
//! test: **7 red, 4 green** (the still-scene size, the late joiner, the
//! gap and the header tests never touch either, and staying green is
//! itself a check that the sabotage is the one described).
//!
//! Measured sizes (`cargo test -p ntsc-wire print_sizes -- --ignored
//! --nocapture`): 68,486 picture dots raw; a key of the tiled scene
//! 4,152 bytes; a delta with one sprite walking at most 169 bytes; a
//! delta of a still scene at most 103 bytes, header included.

use ntsc_grid::{FrameParity, Phase};
use ntsc_source_nes::{encode_frame, segment, DotFrame, Levels, Segment, DOTS_PER_LINE, LINES};
use ntsc_wire::{mask, parse_header, Receiver, Sender, WireError, HEADER_LEN};

/// A deterministic generator; no crate for it, the repo takes none it
/// doesn't need.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

/// A frame shaped like a game: a background of 8x8 tiles drawn from a
/// four-colour palette, a backdrop colour in the borders, and a handful
/// of 8x8 "sprites" at (x, y) positions that later frames move.
fn scene(parity: FrameParity, sprites: &[(usize, usize)], emphasis: u8, seed: u64) -> DotFrame {
    let mut rng = Lcg(seed);
    let palette = [0x0f, 0x30, 0x16, 0x1a];
    let mut f = DotFrame::filled(parity, 0x22, emphasis);
    for row in 0..240 {
        for x in 0..256 {
            let tile = ((row / 8) * 32 + x / 8) as u64;
            let mut t = Lcg(seed ^ tile.wrapping_mul(0x9e37));
            let kind = t.next() % 4;
            let c = match kind {
                0 => 0x22,
                _ => palette[(x + row) / 4 % 4],
            };
            f.set(row, x + 1, c, emphasis);
        }
    }
    for &(sx, sy) in sprites {
        for dy in 0..8 {
            for dx in 0..8 {
                let (r, d) = (sy + dy, sx + dx + 1);
                if r < 240 && d < 257 {
                    f.set(r, d, 0x27 + (rng.next() % 2) as u8, emphasis);
                }
            }
        }
    }
    f
}

/// The waveform of a frame at `origin`, every line concatenated.
fn waveform(f: &DotFrame, origin: Phase) -> Vec<f32> {
    let frame = encode_frame(&Levels::transcribed(), f, origin);
    frame
        .lines
        .iter()
        .flat_map(|l| l.samples.iter().copied())
        .collect()
}

fn parities() -> [FrameParity; 3] {
    [
        FrameParity::Even,
        FrameParity::OddFull,
        FrameParity::OddShort,
    ]
}

#[test]
fn the_mask_is_exactly_what_the_encoder_reads() {
    // Rebuilt here independently of the crate's own loop, from the same
    // segment function, as a check that the crate loops the right way.
    let mut expect = Vec::new();
    for row in 0..LINES {
        for dot in 0..DOTS_PER_LINE {
            if segment(row, dot) == Segment::Picture {
                expect.push((row, dot));
            }
        }
    }
    let m = mask();
    assert_eq!(
        m.dots, expect,
        "the wire must carry every Picture dot in row-major order"
    );
    assert_eq!(m.rows.len(), 242);
    assert_eq!(m.dots.len(), 242 * (268 + 15));
}

#[test]
fn a_key_round_trips_the_waveform_exactly() {
    for (i, parity) in parities().into_iter().enumerate() {
        let origin = Phase::new((i * 5 % 12) as u8);
        let f = scene(parity, &[(40, 100), (200, 50)], 0, 7 + i as u64);
        let mut tx = Sender::new(1);
        let mut rx = Receiver::new();
        let packet = tx.encode(&f, origin);
        let got = rx.decode(&packet).unwrap();
        assert!(got.key);
        assert_eq!(got.origin, origin);
        assert_eq!(got.frame.parity, parity);
        assert_eq!(
            waveform(&f, origin),
            waveform(&got.frame, got.origin),
            "parity {parity:?}"
        );
    }
}

#[test]
fn dots_outside_the_picture_are_not_carried_and_do_not_matter() {
    // Scribble on every non-picture dot: the waveform must not notice,
    // and the receiver's copy holds zeros there.
    let origin = Phase::new(3);
    let mut f = scene(FrameParity::Even, &[(10, 10)], 0, 1);
    let clean = f.clone();
    for row in 0..LINES {
        for dot in 0..DOTS_PER_LINE {
            if segment(row, dot) != Segment::Picture {
                f.set(row, dot, 0x3f, 7);
            }
        }
    }
    let mut tx = Sender::new(1);
    let mut rx = Receiver::new();
    let got = rx.decode(&tx.encode(&f, origin)).unwrap();
    assert_eq!(waveform(&clean, origin), waveform(&got.frame, origin));
    assert_eq!(got.frame.at(250, 100), (0, 0));
}

#[test]
fn deltas_chain_exactly_and_are_small() {
    let mut tx = Sender::new(1000);
    let mut rx = Receiver::new();
    let mut origin = Phase::new(0);
    let geo = ntsc_grid::Geometry::nes();
    let mut key_len = 0;
    let mut delta_lens = Vec::new();
    for i in 0..30usize {
        let parity = parities()[i % 3];
        // One sprite walks a dot a frame; the other sits still.
        let f = scene(parity, &[(40 + i, 100), (200, 50)], 0, 7);
        let packet = tx.encode(&f, origin);
        let got = rx.decode(&packet).unwrap();
        assert_eq!(got.seq, i as u32);
        assert_eq!(got.missed, 0);
        assert_eq!(got.key, i == 0);
        assert_eq!(
            waveform(&f, origin),
            waveform(&got.frame, got.origin),
            "frame {i}"
        );
        if i == 0 {
            key_len = packet.len();
        } else {
            delta_lens.push(packet.len());
        }
        origin = geo.next_origin(origin, parity);
    }
    let raw = mask().dots.len();
    let worst = *delta_lens.iter().max().unwrap();
    assert!(
        key_len < raw / 4,
        "a key of a tiled scene deflates well: {key_len} of {raw} raw"
    );
    assert!(
        worst < key_len / 10,
        "a one-sprite delta is tiny: {worst} vs key {key_len}"
    );
}

#[test]
fn a_still_scene_costs_almost_nothing() {
    let f = scene(FrameParity::Even, &[(40, 100)], 0, 7);
    let mut tx = Sender::new(1000);
    let _ = tx.encode(&f, Phase::new(0));
    let delta = tx.encode(&f, Phase::new(8));
    assert!(
        delta.len() - HEADER_LEN < 200,
        "identical frame delta: {} bytes",
        delta.len()
    );
}

#[test]
fn a_late_joiner_waits_for_a_key() {
    let f = scene(FrameParity::Even, &[], 0, 2);
    let mut tx = Sender::new(4);
    let origin = Phase::new(0);
    let packets: Vec<_> = (0..6).map(|_| tx.encode(&f, origin)).collect();
    let mut rx = Receiver::new();
    // Joins at seq 1: a delta.
    assert_eq!(
        rx.decode(&packets[1]).err(),
        Some(WireError::NeedsKey { seq: 1 })
    );
    assert_eq!(
        rx.decode(&packets[2]).err(),
        Some(WireError::NeedsKey { seq: 2 })
    );
    // seq 4 is the key: accepted, and the chain resumes from it.
    let got = rx.decode(&packets[4]).unwrap();
    assert!(got.key);
    assert_eq!(got.missed, 0, "no expectation yet, so nothing was missed");
    assert!(rx.decode(&packets[5]).is_ok());
}

#[test]
fn a_gap_is_reported_and_recovered_by_a_key() {
    let f = scene(FrameParity::Even, &[], 0, 2);
    let mut tx = Sender::new(3);
    let packets: Vec<_> = (0..7).map(|_| tx.encode(&f, Phase::new(0))).collect();
    let mut rx = Receiver::new();
    rx.decode(&packets[0]).unwrap();
    rx.decode(&packets[1]).unwrap();
    // 2 is lost; 4 is a delta across the gap: refused, state dropped.
    assert_eq!(
        rx.decode(&packets[4]).err(),
        Some(WireError::Gap {
            expected: 2,
            got: 4
        })
    );
    assert_eq!(
        rx.decode(&packets[5]).err(),
        Some(WireError::NeedsKey { seq: 5 })
    );
    // 6 is a key: accepted. The expectation was cleared by the gap, so
    // `missed` is 0 here; the caller already learned the gap's size.
    assert!(rx.decode(&packets[6]).unwrap().key);

    // A key straight across a gap reports how many were missed.
    let mut rx = Receiver::new();
    rx.decode(&packets[0]).unwrap();
    let got = rx.decode(&packets[3]).unwrap();
    assert!(got.key);
    assert_eq!(got.missed, 2);
}

#[test]
fn a_corrupted_packet_is_refused_whole() {
    let f = scene(FrameParity::Even, &[(1, 1)], 0, 3);
    let mut tx = Sender::new(1);
    let mut packet = tx.encode(&f, Phase::new(0));
    let mid = HEADER_LEN + (packet.len() - HEADER_LEN) / 2;
    packet[mid] ^= 0x40;
    let mut rx = Receiver::new();
    assert!(
        matches!(rx.decode(&packet), Err(WireError::Checksum { .. })),
        "a flipped payload byte must fail the checksum"
    );
    // Nothing was accepted: the next key is still the first frame.
    let good = tx.encode(&f, Phase::new(0));
    assert_eq!(rx.decode(&good).unwrap().seq, 1);
}

#[test]
fn headers_are_refused_before_payloads_are_touched() {
    let f = scene(FrameParity::Even, &[], 0, 3);
    let mut tx = Sender::new(1);
    let packet = tx.encode(&f, Phase::new(0));
    assert_eq!(
        parse_header(&packet[..10]),
        Err(WireError::Malformed("shorter than a header"))
    );
    let mut bad = packet.clone();
    bad[0] = b'X';
    assert_eq!(parse_header(&bad), Err(WireError::Malformed("bad magic")));
    let mut bad = packet.clone();
    bad[4] = 2;
    assert_eq!(parse_header(&bad), Err(WireError::Version(2)));
    let mut bad = packet.clone();
    bad[6] = 3;
    assert_eq!(parse_header(&bad), Err(WireError::Field("parity", 3)));
    let mut bad = packet.clone();
    bad[7] = 12;
    assert_eq!(parse_header(&bad), Err(WireError::Field("origin", 12)));
}

#[test]
fn emphasis_rides_per_row_until_a_row_disagrees_with_itself() {
    let origin = Phase::new(0);
    let mut tx = Sender::new(1000);
    let mut rx = Receiver::new();

    // Frame 0: uniform emphasis, per row.
    let a = scene(FrameParity::Even, &[], 0b010, 5);
    let p = tx.encode(&a, origin);
    assert!(parse_header(&p).unwrap().emphasis_per_row);
    let got = rx.decode(&p).unwrap();
    assert_eq!(waveform(&a, origin), waveform(&got.frame, origin));

    // Frame 1: a game that changes PPUMASK at a scanline: still per row.
    let mut b = a.clone();
    for row in 120..240 {
        for dot in 0..DOTS_PER_LINE {
            let (c, _) = b.at(row, dot);
            b.set(row, dot, c, 0b101);
        }
    }
    let p = tx.encode(&b, origin);
    assert!(parse_header(&p).unwrap().emphasis_per_row);
    let got = rx.decode(&p).unwrap();
    assert_eq!(waveform(&b, origin), waveform(&got.frame, origin));

    // Frame 2: emphasis flipped mid-row (a beam race): per dot, exact.
    let mut c = b.clone();
    let (col, _) = c.at(60, 128);
    c.set(60, 128, col, 0b111);
    let p = tx.encode(&c, origin);
    assert!(!parse_header(&p).unwrap().emphasis_per_row);
    let got = rx.decode(&p).unwrap();
    assert_eq!(got.frame.at(60, 128).1, 0b111);
    assert_eq!(waveform(&c, origin), waveform(&got.frame, origin));

    // Frame 3: back to uniform after a per-dot frame: the delta is per
    // dot (the previous frame's rows are not derivable) and still exact.
    let p = tx.encode(&b, origin);
    assert!(!parse_header(&p).unwrap().emphasis_per_row);
    let got = rx.decode(&p).unwrap();
    assert_eq!(waveform(&b, origin), waveform(&got.frame, origin));

    // Frame 4: and the frame after that is per row again.
    let p = tx.encode(&b, origin);
    assert!(parse_header(&p).unwrap().emphasis_per_row);
    let got = rx.decode(&p).unwrap();
    assert_eq!(waveform(&b, origin), waveform(&got.frame, origin));
}

#[test]
fn a_requested_key_resets_the_delta_chain() {
    let f = scene(FrameParity::OddShort, &[(3, 3)], 0, 9);
    let mut tx = Sender::new(1000);
    let _ = tx.encode(&f, Phase::new(0));
    let _ = tx.encode(&f, Phase::new(0));
    let key = tx.encode_key(&f, Phase::new(4));
    let mut rx = Receiver::new();
    let got = rx.decode(&key).unwrap();
    assert!(got.key);
    assert_eq!(got.seq, 2);
    assert_eq!(got.origin, Phase::new(4));
    assert_eq!(
        waveform(&f, Phase::new(4)),
        waveform(&got.frame, Phase::new(4))
    );
}

#[test]
#[ignore]
fn print_sizes() {
    let geo = ntsc_grid::Geometry::nes();
    let mut tx = Sender::new(60);
    let mut origin = Phase::new(0);
    let (mut key, mut walk, mut still) = (0, 0, 0);
    for i in 0..61usize {
        let p = parities()[i % 3];
        let f = scene(p, &[(40 + i.min(30), 100), (200, 50)], 0, 7);
        let n = tx.encode(&f, origin).len();
        match i {
            0 => key = n,
            1..=30 => walk = walk.max(n),
            31..=59 => still = still.max(n),
            _ => {}
        }
        origin = geo.next_origin(origin, p);
    }
    eprintln!(
        "raw picture dots {} | key {key} B | walking delta <= {walk} B | still delta <= {still} B",
        mask().dots.len()
    );
}
