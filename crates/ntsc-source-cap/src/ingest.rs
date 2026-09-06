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
    let (tip, blank) = find_tip_blank(cap);
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

/// The NES variant of [`auto_level`]: the same peak finding, but the
/// ruler is the transcribed NES table's own sync depth (BLANK - SYNC,
/// not Table 1's 0.286 V) and the output is re-referenced to the
/// table's ABSOLUTE voltages, blanking at `levels::BLANK`, so the
/// recovered frame decodes with the same decoder constants the oracle
/// uses on encoder output.
pub fn auto_level_nes(cap: &Capture) -> (Capture, f32, f32) {
    let (tip, blank) = find_tip_blank(cap);
    let nes_blank = ntsc_source_nes::levels::BLANK;
    let nes_sync = ntsc_source_nes::levels::SYNC;
    let scale = (nes_blank - nes_sync) / (blank - tip);
    let samples = cap
        .samples
        .iter()
        .map(|s| (s - blank) * scale + nes_blank)
        .collect();
    (
        Capture {
            declared_rate_hz: cap.declared_rate_hz,
            samples,
        },
        tip,
        blank,
    )
}

/// Sync tip and blanking, in the capture's original units. The
/// histogram gives a first look (the two lowest bands each holding at
/// least 1% of the samples) and that alone was the whole method until
/// 2026-09-06, when the console's capture gate found two things wrong
/// with it: a crest is only as fine as its bin (a 1 V span in 256 bins
/// is 4 mV, which read as a 3.7% luma gain), and a picture level below
/// blanking (the NES's darkest rows sit under it) is taken for blanking
/// as soon as it fills a percent of the record, which a colour-bars
/// frame does. So the histogram now only places a sync threshold, and
/// the levels are read where nothing but the signal's own structure can
/// put them: the sync tip as the median inside every sync pulse, the
/// blanking as the median of the front porch before every pulse.
fn find_tip_blank(cap: &Capture) -> (f32, f32) {
    let (tip0, blank0) = histogram_bands(cap);
    // Halfway from the tip to the second band, which is blanking or a
    // picture level below it: either way above the tip and below every
    // picture level, so only sync pulses cross it downward.
    let threshold = tip0 + (blank0 - tip0) / 2.0;
    let s = &cap.samples;
    let us = cap.declared_rate_hz / 1e6;
    // A horizontal sync is 4.7 us; a pulse shorter than 2 us is not one.
    let (min_pulse, porch_from, porch_to, tip_from, tip_to) =
        ((2.0 * us) as usize, (1.3 * us) as usize, (0.4 * us) as usize, (1.0 * us) as usize, (3.5 * us) as usize);
    let mut porch = Vec::new();
    let mut tips = Vec::new();
    let mut i = 1usize;
    while i < s.len() {
        if s[i - 1] >= threshold && s[i] < threshold {
            let mut j = i;
            while j < s.len() && s[j] < threshold {
                j += 1;
            }
            if j - i >= min_pulse && i >= porch_from && i + tip_to < s.len() {
                porch.extend_from_slice(&s[i - porch_from..i - porch_to]);
                tips.extend_from_slice(&s[i + tip_from..i + tip_to.min(j - i)]);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    assert!(porch.len() > 1000 && tips.len() > 1000, "too few sync pulses to level on: {} porch samples, {} tip samples", porch.len(), tips.len());
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    let (tip, blank) = (median(&mut tips), median(&mut porch));
    assert!(blank > tip, "sync tip and blanking are not separated: {tip} vs {blank}");
    (tip, blank)
}

/// The two lowest bands of the sample histogram each holding at least
/// 1% of the samples, as bin centres: the first look that places the
/// sync threshold for `find_tip_blank`.
fn histogram_bands(cap: &Capture) -> (f32, f32) {
    let mut sorted = cap.samples.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let lo = sorted[sorted.len() / 1000];
    let hi = sorted[sorted.len() - 1 - sorted.len() / 1000];
    let bins = 256usize;
    // Quantized captures comb a fine histogram: a u8 record spanning
    // 140 counts put integer values in sub-integer bins with empty bins
    // between them, so the sync tip's two adjacent values read as two
    // peaks and the scan stopped before real blanking (found on the
    // first real scope capture, 2026-09-02; the synthetic proof capture
    // is continuous f32 and could never show it). The bin width is
    // therefore never narrower than the data's own quantization step.
    let step = sorted
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 0.0)
        .fold(f32::INFINITY, f32::min);
    let step = if step.is_finite() { step } else { f32::EPSILON };
    let width = ((hi - lo).max(f32::EPSILON) / bins as f32).max(step);
    let bins = (((hi - lo) / width).ceil() as usize + 1).max(2);
    let mut hist = vec![0u32; bins];
    for s in &cap.samples {
        let b = (((s - lo) / width) as isize).clamp(0, bins as isize - 1) as usize;
        hist[b] += 1;
    }
    let floor = (cap.samples.len() / 100) as u32;
    let mut peaks = Vec::new();
    let mut b = 0usize;
    while b < bins && peaks.len() < 2 {
        if hist[b] >= floor {
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
    (tip, blank)
}
