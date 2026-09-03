//! The capture source (handoff spec section 4.3): a raw sample stream at
//! a declared rate in, a broadcast-profile `CompositeFrame` on the grid
//! out. Sync detection, burst lock, resample. This is the only source
//! with genuine measurement uncertainty, and the only one that must
//! EARN its phase instead of deriving it: the burst is the reference.
//!
//! `capture_model` is the test rig's stand-in for a capture card
//! (handoff spec: anti-alias lowpass, rate mismatch, DC offset, noise),
//! and it is rig code under principle 5: deterministic, seeded, its
//! parameters recorded by every test that uses it.
//!
//! Recovery works in four steps, each a measurement:
//! 1. Sync: falling-edge times at a threshold anchored to the measured
//!    sync tip, walked at the nominal line period, then a least-squares
//!    line period over every matched edge. The period against the
//!    declared rate IS the rate-mismatch measurement.
//! 2. Fields: lines that are mostly low are the broad pulses the RGB
//!    encoder emits on frame lines 4..7; the first group anchors frame
//!    line numbering.
//! 3. Burst lock, per line: resample the burst window onto the grid,
//!    measure its phase against the geometry's own target (burst =
//!    -sin at origin Phase(0)), correct the sub-sample offset, and
//!    iterate until the residual is measured small. Lines without burst
//!    borrow the previous locked offset.
//! 4. Resample the whole line at the locked offset (8-tap windowed-sinc
//!    interpolation), then re-reference DC so the measured back porch
//!    sits at blanking.

pub mod ingest;

use ntsc_grid::{CompositeFrame, CompositeLine, FrameParity, Geometry, Phase};
use ntsc_source_rgb::layout;

/// A raw captured waveform: samples in volts at a declared rate that is
/// not trusted beyond parts per million.
#[derive(Clone, Debug)]
pub struct Capture {
    pub declared_rate_hz: f64,
    pub samples: Vec<f32>,
}

/// What recovery hands back beside the frame: the measurements.
pub struct Recovered {
    pub frame: CompositeFrame,
    /// Measured line period over declared expectation, as parts per
    /// million: the capture card's actual rate error.
    pub rate_error_ppm: f64,
    /// Worst per-line burst phase residual after locking, grid samples.
    pub worst_burst_residual: f64,
    /// Frame line index of the first sample of the recovered frame in
    /// the walked line list (diagnostic).
    pub anchor_line: usize,
}

const GRID_RATE: f64 = 472_500_000.0 / 11.0;

/// 8-tap Hamming-windowed sinc interpolation of `s` at fractional index
/// `x` (clamped at the ends).
fn interp8(s: &[f32], x: f64) -> f32 {
    let x0 = x.floor() as isize;
    let mut acc = 0.0f64;
    let mut wsum = 0.0f64;
    for k in -3..=4isize {
        let i = x0 + k;
        let d = x - i as f64;
        let sinc = if d.abs() < 1e-9 {
            1.0
        } else {
            (std::f64::consts::PI * d).sin() / (std::f64::consts::PI * d)
        };
        let w = 0.54 + 0.46 * (std::f64::consts::PI * d / 4.0).cos();
        let j = i.clamp(0, s.len() as isize - 1) as usize;
        acc += s[j] as f64 * sinc * w;
        wsum += sinc * w;
    }
    (acc / wsum) as f32
}

/// The capture-card model: concatenate the frame's lines, anti-alias
/// lowpass (windowed sinc, 6.5 MHz, 41 taps: a real card has one),
/// sample at `declared_rate_hz * (1 + rate_error_ppm/1e6)`, add a DC
/// offset and seeded uniform noise. Deterministic.
pub fn capture_model(
    frames: &[&CompositeFrame],
    declared_rate_hz: f64,
    rate_error_ppm: f64,
    dc_offset: f32,
    noise_amp: f32,
    seed: u64,
) -> Capture {
    let mut grid = Vec::new();
    for f in frames {
        for l in &f.lines {
            grid.extend_from_slice(&l.samples);
        }
    }
    // Anti-alias lowpass at the grid rate.
    let taps: Vec<f64> = {
        let n = 41usize;
        let m = n / 2;
        let fc = 6.5e6 / GRID_RATE;
        let mut t: Vec<f64> = (0..n)
            .map(|i| {
                let x = i as f64 - m as f64;
                let h = if x == 0.0 {
                    2.0 * fc
                } else {
                    (2.0 * std::f64::consts::PI * fc * x).sin() / (std::f64::consts::PI * x)
                };
                let w = 0.54
                    - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
                h * w
            })
            .collect();
        let s: f64 = t.iter().sum();
        for v in &mut t {
            *v /= s;
        }
        t
    };
    let m = taps.len() / 2;
    let filtered: Vec<f32> = (0..grid.len())
        .map(|i| {
            taps.iter()
                .enumerate()
                .map(|(k, t)| {
                    let j = (i as isize + k as isize - m as isize)
                        .clamp(0, grid.len() as isize - 1) as usize;
                    t * grid[j] as f64
                })
                .sum::<f64>() as f32
        })
        .collect();
    let actual_rate = declared_rate_hz * (1.0 + rate_error_ppm / 1e6);
    let n_out = (grid.len() as f64 * actual_rate / GRID_RATE) as usize - 8;
    let mut lcg = seed.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
    let mut samples = Vec::with_capacity(n_out);
    for k in 0..n_out {
        let g = k as f64 * GRID_RATE / actual_rate;
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let noise = ((lcg >> 33) as f32 / (1u64 << 31) as f32 - 0.5) * 2.0 * noise_amp;
        samples.push(interp8(&filtered, g) + dc_offset + noise);
    }
    Capture {
        declared_rate_hz,
        samples,
    }
}

/// The NES layout, in grid samples: ntsc-source-nes's segment map times
/// its eight samples per dot. Sync falls at dot 277; the burst is dots
/// 306..=320, ten full subcarrier cycles; the pre-sync blank at dots
/// 268..=276 is the DC window; active starts at dot 1, like `DotFrame`.
/// Rows 245..=247 carry vertical sync: no burst, and their sync edge
/// sits at the serration window's end rather than at dot 277, so their
/// DC window is skipped too.
mod nes_lay {
    pub const HREF: usize = 277 * 8;
    pub const BURST_START: usize = 306 * 8;
    pub const BURST_END: usize = 321 * 8;
    pub const DC_START: usize = 268 * 8 + 8;
    pub const DC_END: usize = 277 * 8 - 8;
    pub const ACTIVE_START: usize = 8;
    pub const LINE: usize = 2728;
    pub const VSYNC_FIRST: usize = 245;
    pub const VSYNC_LAST: usize = 247;
    /// The walked window that first reads mostly-low starts at row
    /// 244's sync edge: its span covers row 244's sync tail plus row
    /// 245's long vsync run. Measured on the synthetic NES capture and
    /// pinned by its test.
    pub const FIRST_BROAD_ROW: usize = 244;
}

/// Recover one full frame from a capture. The capture must contain at
/// least one complete frame after its first broad-pulse group.
pub fn recover(cap: &Capture) -> Recovered {
    recover_with(cap, true)
}

/// Recover one full NES-profile frame: 262 progressive lines of 2728
/// grid samples (227 and a third subcarrier cycles, so the burst phase
/// advances a third of a cycle per line where broadcast advances half),
/// anchored on the vertical sync rows the NES encoder models at rows
/// 245..=247. Found necessary by the first real console capture
/// (2026-09-02): decoded as broadcast, the burst lock chased the wrong
/// per-line phase target and every line landed two grid samples further
/// rotated than the last, a smooth hue roll down the whole frame.
pub fn recover_nes(cap: &Capture) -> Recovered {
    recover_nes_with(cap, true)
}

/// The NES recovery with the burst lock switchable, so the mutation
/// test can prove the lock is load-bearing here too. Nothing but that
/// test should ever pass `false`.
pub fn recover_nes_with(cap: &Capture, lock_burst: bool) -> Recovered {
    use nes_lay as nl;
    let geo = Geometry::nes();
    let s = &cap.samples;

    // The NES levels are the transcribed table's ABSOLUTE volts
    // (blanking at BLANK, sync at SYNC, nothing negative), and this
    // recovery keeps that convention: the frame it hands back decodes
    // with the same Decoder::transcribed(.., LOW[1], HIGH[2]) the
    // oracle uses on encoder output. Both constants are the table's,
    // never typed here.
    let nes_blank = ntsc_source_nes::levels::BLANK;
    let nes_sync = ntsc_source_nes::levels::SYNC;
    let mut sorted: Vec<f32> = s.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let tip = sorted[s.len() / 100];
    let threshold = tip + (nes_blank - nes_sync) / 2.0;

    let mut edges = Vec::new();
    for i in 0..s.len() - 1 {
        if s[i] >= threshold && s[i + 1] < threshold {
            edges.push(i as f64 + (s[i] - threshold) as f64 / (s[i] - s[i + 1]) as f64);
        }
    }
    assert!(edges.len() > 500, "not enough sync edges: {}", edges.len());

    let nominal_h = cap.declared_rate_hz * nl::LINE as f64 / GRID_RATE;
    let mut line_starts = vec![edges[0]];
    let mut h_est = nominal_h;
    let mut ei = 0usize;
    loop {
        let want = line_starts.last().unwrap() + h_est;
        while ei + 1 < edges.len() && edges[ei + 1] < want - nominal_h / 4.0 {
            ei += 1;
        }
        let mut best: Option<f64> = None;
        for e in &edges[ei..] {
            if (*e - want).abs() < nominal_h / 4.0
                && best.is_none_or(|b: f64| (*e - want).abs() < (b - want).abs())
            {
                best = Some(*e);
            }
            if *e > want + nominal_h / 4.0 {
                break;
            }
        }
        match best {
            Some(e) => {
                h_est = 0.9 * h_est + 0.1 * (e - line_starts.last().unwrap());
                line_starts.push(e);
            }
            None => break,
        }
    }
    let n = line_starts.len() as f64;
    let mean_i = (n - 1.0) / 2.0;
    let mean_t = line_starts.iter().sum::<f64>() / n;
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (i, t) in line_starts.iter().enumerate() {
        num += (i as f64 - mean_i) * (t - mean_t);
        den += (i as f64 - mean_i) * (i as f64 - mean_i);
    }
    let h_fit = num / den;
    let rate_error_ppm = (h_fit / nominal_h - 1.0) * 1e6;
    let cap_per_grid = h_fit / nl::LINE as f64;

    let low_fraction = |t0: f64| {
        let a = t0 as usize;
        let b = ((t0 + h_fit) as usize).min(s.len());
        let low = s[a..b].iter().filter(|v| **v < threshold).count();
        low as f64 / (b - a) as f64
    };
    let broad: Vec<bool> = line_starts.iter().map(|t| low_fraction(*t) > 0.4).collect();
    let first_broad = broad
        .iter()
        .position(|b| *b)
        .expect("no broad-pulse group found");
    // The first broad window starts at row FIRST_BROAD_ROW's edge; the
    // next frame's row 0 is that many lines further on.
    let anchor_line = first_broad + (geo.lines() - nl::FIRST_BROAD_ROW);
    assert!(
        anchor_line + geo.lines() <= line_starts.len(),
        "capture ends before a full frame: {} lines from anchor {anchor_line}",
        line_starts.len()
    );

    // The burst target: the encoder's own wave 8 projected onto the
    // measurement's basis, derived rather than typed (the same move as
    // ntsc-source-nes's burst_axis_offset).
    let target = {
        let (mut u, mut v) = (0.0f64, 0.0f64);
        for p in 0..12u8 {
            let sgn = if ntsc_source_nes::wave_high(ntsc_source_nes::levels::COLORBURST_WAVE, p) {
                1.0
            } else {
                -1.0
            };
            let th = std::f64::consts::TAU * p as f64 / 12.0;
            u += sgn * th.sin();
            v += sgn * th.cos();
        }
        v.atan2(u)
    };

    let resample_line = |t0: f64, delta: f64, len: usize| -> Vec<f32> {
        (0..len)
            .map(|i| interp8(s, t0 + (i as f64 - nl::HREF as f64 + delta) * cap_per_grid))
            .collect()
    };
    let burst_free = |row: usize| (nl::VSYNC_FIRST..=nl::VSYNC_LAST).contains(&row);
    let mut lines = Vec::with_capacity(geo.lines());
    let mut delta = 0.0f64;
    let mut worst_residual = 0.0f64;
    let mut dc_estimates = Vec::new();
    for row in 0..geo.lines() {
        let t0 = line_starts[anchor_line + row];
        if lock_burst && !burst_free(row) {
            let mut residual = f64::MAX;
            for _ in 0..3 {
                let win = resample_line(t0, delta, nl::BURST_END + 12);
                let (mut u, mut v) = (0.0f64, 0.0f64);
                for (g, sample) in win
                    .iter()
                    .enumerate()
                    .take(nl::BURST_END - 12)
                    .skip(nl::BURST_START + 12)
                {
                    // The NES phase at row r, sample g is (4r + g) mod
                    // 12: a line is 227 and a third cycles.
                    let p = (4 * row + g) as f64;
                    let th = std::f64::consts::TAU * p / 12.0;
                    u += *sample as f64 * th.sin();
                    v += *sample as f64 * th.cos();
                }
                let mut err = v.atan2(u) - target;
                while err > std::f64::consts::PI {
                    err -= std::f64::consts::TAU;
                }
                while err < -std::f64::consts::PI {
                    err += std::f64::consts::TAU;
                }
                residual = err / std::f64::consts::TAU * 12.0;
                delta -= residual;
                if residual.abs() < 0.01 {
                    break;
                }
            }
            worst_residual = worst_residual.max(residual.abs());
        }
        let mut samples = resample_line(t0, delta, geo.line_len(FrameParity::Even, row));
        if !burst_free(row) {
            let porch: f32 = samples[nl::DC_START..nl::DC_END].iter().sum::<f32>()
                / (nl::DC_END - nl::DC_START) as f32;
            dc_estimates.push(porch);
            // Re-reference so the measured porch sits at the table's
            // own blanking voltage, keeping the absolute convention.
            for v in &mut samples {
                *v += nes_blank - porch;
            }
        }
        lines.push(CompositeLine {
            samples,
            sync_start: nl::HREF,
            burst_start: nl::BURST_START,
            active_start: nl::ACTIVE_START,
        });
    }
    dc_estimates.sort_by(|a, b| a.total_cmp(b));
    let dc = dc_estimates[dc_estimates.len() / 2];
    for (row, l) in lines.iter_mut().enumerate() {
        if burst_free(row) {
            for v in &mut l.samples {
                *v += nes_blank - dc;
            }
        }
    }

    Recovered {
        frame: CompositeFrame {
            profile: geo,
            lines,
            frame_parity: FrameParity::Even,
            phase_at_origin: Phase::new(0),
        },
        rate_error_ppm,
        worst_burst_residual: worst_residual,
        anchor_line,
    }
}

/// The same recovery with the burst lock switchable, so the mutation
/// test can prove the lock is load-bearing: without it, sub-sample
/// alignment rests on sync edges alone and the chroma phase drifts.
/// Nothing but that test should ever pass `false`.
pub fn recover_with(cap: &Capture, lock_burst: bool) -> Recovered {
    let geo = Geometry::broadcast();
    let lay = layout();
    let s = &cap.samples;

    // 1. Sync threshold from the measured tip: half the Table 1 sync
    // depth (286 mV) above the 1st percentile.
    let mut sorted: Vec<f32> = s.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let tip = sorted[s.len() / 100];
    let threshold = tip + 0.143;

    // All falling crossings, sub-sample by linear interpolation.
    let mut edges = Vec::new();
    for i in 0..s.len() - 1 {
        if s[i] >= threshold && s[i + 1] < threshold {
            edges.push(i as f64 + (s[i] - threshold) as f64 / (s[i] - s[i + 1]) as f64);
        }
    }
    assert!(edges.len() > 500, "not enough sync edges: {}", edges.len());

    // Walk at the nominal line period, then least-squares the period.
    let nominal_h = cap.declared_rate_hz * 2730.0 / GRID_RATE;
    let mut line_starts = vec![edges[0]];
    let mut h_est = nominal_h;
    let mut ei = 0usize;
    loop {
        let want = line_starts.last().unwrap() + h_est;
        while ei + 1 < edges.len() && edges[ei + 1] < want - nominal_h / 4.0 {
            ei += 1;
        }
        let mut best: Option<f64> = None;
        for e in &edges[ei..] {
            if (*e - want).abs() < nominal_h / 4.0
                && best.is_none_or(|b: f64| (*e - want).abs() < (b - want).abs())
            {
                best = Some(*e);
            }
            if *e > want + nominal_h / 4.0 {
                break;
            }
        }
        match best {
            Some(e) => {
                h_est = 0.9 * h_est + 0.1 * (e - line_starts.last().unwrap());
                line_starts.push(e);
            }
            None => break,
        }
    }
    // Least-squares slope of start time over line index.
    let n = line_starts.len() as f64;
    let mean_i = (n - 1.0) / 2.0;
    let mean_t = line_starts.iter().sum::<f64>() / n;
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (i, t) in line_starts.iter().enumerate() {
        num += (i as f64 - mean_i) * (t - mean_t);
        den += (i as f64 - mean_i) * (i as f64 - mean_i);
    }
    let h_fit = num / den;
    let rate_error_ppm = (h_fit / nominal_h - 1.0) * 1e6;
    let cap_per_grid = h_fit / 2730.0;

    // 2. Fields: mostly-low lines are the broad pulses on frame lines
    // 4..7; anchor the frame there.
    let low_fraction = |t0: f64| {
        let a = t0 as usize;
        let b = ((t0 + h_fit) as usize).min(s.len());
        let low = s[a..b].iter().filter(|v| **v < threshold).count();
        low as f64 / (b - a) as f64
    };
    let broad: Vec<bool> = line_starts.iter().map(|t| low_fraction(*t) > 0.4).collect();
    let first_broad = broad
        .iter()
        .position(|b| *b)
        .expect("no broad-pulse group found");
    let anchor_line = if first_broad >= 4 { first_broad - 4 } else { first_broad };
    assert!(
        anchor_line + geo.lines() <= line_starts.len(),
        "capture ends before a full frame: {} lines from anchor {anchor_line}",
        line_starts.len()
    );

    // 3 + 4. Per line: burst-lock the sub-sample offset, then resample.
    let burst_free = |line: usize| line < 9 || (262..271).contains(&line) || broad[anchor_line + line];
    let resample_line = |t0: f64, delta: f64, len: usize| -> Vec<f32> {
        (0..len)
            .map(|i| {
                interp8(
                    s,
                    t0 + (i as f64 - lay.href as f64 + delta) * cap_per_grid,
                )
            })
            .collect()
    };
    let mut lines = Vec::with_capacity(geo.lines());
    let mut delta = 0.0f64;
    let mut worst_residual = 0.0f64;
    let mut dc_estimates = Vec::new();
    for line in 0..geo.lines() {
        let t0 = line_starts[anchor_line + line];
        if lock_burst && !burst_free(line) {
            // Iterate: resample the burst window, measure phase against
            // the geometry's target (burst = -sin at origin 0), correct.
            // A waveform read delta_g grid samples early projects to a
            // residual of -delta_g, so the correction subtracts.
            let mut residual = f64::MAX;
            for _ in 0..3 {
                let row = resample_line(t0, delta, lay.burst_end + 12);
                let (mut u, mut v) = (0.0f64, 0.0f64);
                for (g, sample) in row
                    .iter()
                    .enumerate()
                    .take(lay.burst_end - 12)
                    .skip(lay.burst_start + 12)
                {
                    let p = (6 * line + g) as f64;
                    let th = std::f64::consts::TAU * p / 12.0;
                    u += *sample as f64 * th.sin();
                    v += *sample as f64 * th.cos();
                }
                // Perfect lock: (u, v) proportional to (-1, 0).
                let err = (-v).atan2(-u);
                residual = err / std::f64::consts::TAU * 12.0;
                delta -= residual;
                if residual.abs() < 0.01 {
                    break;
                }
            }
            worst_residual = worst_residual.max(residual.abs());
        }
        let mut samples = resample_line(t0, delta, geo.line_len(FrameParity::Even, line));
        // DC: the back porch (between burst end and active start) sits
        // at blanking, i.e. zero volts.
        if !broad[anchor_line + line] {
            let porch: f32 = samples[lay.burst_end + 10..lay.active_start - 10]
                .iter()
                .sum::<f32>()
                / (lay.active_start - 10 - (lay.burst_end + 10)) as f32;
            dc_estimates.push(porch);
            for v in &mut samples {
                *v -= porch;
            }
        }
        lines.push(CompositeLine {
            samples,
            sync_start: lay.href,
            burst_start: lay.burst_start,
            active_start: lay.active_start,
        });
    }
    // Broad lines get the frame's median DC.
    dc_estimates.sort_by(|a, b| a.total_cmp(b));
    let dc = dc_estimates[dc_estimates.len() / 2];
    for (line, l) in lines.iter_mut().enumerate() {
        if broad[anchor_line + line] {
            for v in &mut l.samples {
                *v -= dc;
            }
        }
    }

    Recovered {
        frame: CompositeFrame {
            profile: geo,
            lines,
            frame_parity: FrameParity::Even,
            phase_at_origin: Phase::new(0),
        },
        rate_error_ppm,
        worst_burst_residual: worst_residual,
        anchor_line,
    }
}
