//! Ingestion of real recordings: file readers and auto-levelling, the
//! bridge between whatever a capture card writes and the volts
//! `recover` expects. Rig code under principle 5: the levelling is a
//! measurement (the sync depth is the ruler) and it is proven on a
//! synthetic capture in arbitrary units before any real file is trusted
//! to it.

use crate::Capture;

/// Read a mono capture file into arbitrary units. Formats:
/// - "wav": canonical RIFF/WAVE, PCM 8/16-bit or float 32, first
///   channel of interleaved data; the declared rate is taken from the
///   header (an explicit `rate_hz` overrides it).
/// - "f32", "i16", "u8": headerless raw samples, native-endian f32 or
///   little-endian integers; `rate_hz` required.
pub fn read_capture(path: &std::path::Path, format: &str, rate_hz: Option<f64>) -> Capture {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (samples, header_rate) = match format {
        "wav" => read_wav(&bytes),
        "f32" => (
            bytes
                .chunks_exact(4)
                .map(|c| f32::from_ne_bytes(c.try_into().unwrap()))
                .collect(),
            None,
        ),
        "i16" => (
            bytes
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f32)
                .collect(),
            None,
        ),
        "u8" => (bytes.iter().map(|b| *b as f32).collect(), None),
        other => panic!("unknown capture format {other:?}: wav, f32, i16 or u8"),
    };
    let declared_rate_hz = rate_hz
        .or(header_rate)
        .expect("no rate: raw formats need an explicit rate_hz");
    Capture {
        declared_rate_hz,
        samples,
    }
}

fn read_wav(bytes: &[u8]) -> (Vec<f32>, Option<f64>) {
    assert!(&bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE", "not a RIFF/WAVE file");
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (codec, channels, rate, bits)
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
        match id {
            b"fmt " => {
                fmt = Some((
                    u16::from_le_bytes(body[0..2].try_into().unwrap()),
                    u16::from_le_bytes(body[2..4].try_into().unwrap()),
                    u32::from_le_bytes(body[4..8].try_into().unwrap()),
                    u16::from_le_bytes(body[14..16].try_into().unwrap()),
                ));
            }
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    let (codec, channels, rate, bits) = fmt.expect("no fmt chunk");
    let data = data.expect("no data chunk");
    let ch = channels as usize;
    let samples: Vec<f32> = match (codec, bits) {
        (1, 8) => data.iter().step_by(ch).map(|b| *b as f32).collect(),
        (1, 16) => data
            .chunks_exact(2 * ch)
            .map(|c| i16::from_le_bytes(c[0..2].try_into().unwrap()) as f32)
            .collect(),
        (3, 32) => data
            .chunks_exact(4 * ch)
            .map(|c| f32::from_le_bytes(c[0..4].try_into().unwrap()))
            .collect(),
        other => panic!("unsupported WAV codec/bits {other:?}: PCM 8/16 or float 32"),
    };
    (samples, Some(rate as f64))
}

/// Normalize arbitrary capture units to the volts `recover` expects,
/// using the sync depth as the ruler: Table 1 puts the sync tip 40 IRE
/// (0.286 V at 1 V p-p) below blanking, and both levels are found as
/// the two lowest peaks of the sample histogram (sync tips and the
/// blanking-plus-porch plateau are the two most-populated low bands of
/// any composite signal). Returns the scaled capture and the measured
/// (tip, blank) in the original units, so the caller can report them.
pub fn auto_level(cap: &Capture) -> (Capture, f32, f32) {
    let mut sorted = cap.samples.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let lo = sorted[sorted.len() / 1000];
    let hi = sorted[sorted.len() - 1 - sorted.len() / 1000];
    let bins = 256usize;
    let width = (hi - lo).max(f32::EPSILON) / bins as f32;
    let mut hist = vec![0u32; bins];
    for s in &cap.samples {
        let b = (((s - lo) / width) as isize).clamp(0, bins as isize - 1) as usize;
        hist[b] += 1;
    }
    // The two lowest local maxima that each hold at least 1% of the
    // samples, separated by a real valley: sync tip, then blanking.
    let floor = (cap.samples.len() / 100) as u32;
    let mut peaks = Vec::new();
    let mut b = 0usize;
    while b < bins && peaks.len() < 2 {
        // Find the next bin range whose count clears the floor.
        if hist[b] >= floor {
            // Climb to the local crest of this band.
            let start = b;
            while b + 1 < bins && hist[b + 1] >= floor {
                b += 1;
            }
            let crest = (start..=b).max_by_key(|i| hist[*i]).unwrap();
            peaks.push(lo + (crest as f32 + 0.5) * width);
        }
        b += 1;
    }
    assert!(
        peaks.len() == 2,
        "could not find sync tip and blanking peaks in the histogram ({} found)",
        peaks.len()
    );
    let (tip, blank) = (peaks[0], peaks[1]);
    assert!(
        blank - tip > 4.0 * width,
        "sync tip and blanking are not separated: {tip} vs {blank}"
    );
    let scale = 0.286 / (blank - tip);
    let samples = cap.samples.iter().map(|s| (s - blank) * scale).collect();
    (
        Capture {
            declared_rate_hz: cap.declared_rate_hz,
            samples,
        },
        tip,
        blank,
    )
}
