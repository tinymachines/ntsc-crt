//! The RGB source: a framebuffer in, a broadcast-profile
//! `CompositeFrame` out, encoded per SMPTE ST 170M-2004 with every
//! constant flowing from the transcribed data files through build.rs.
//!
//! The chain (clauses in parentheses): video-level G'B'R' in 0..1
//! (either given directly, or produced from sRGB bytes via the sRGB EOTF
//! then the reference camera OETF, 5.1) -> the base matrix on video
//! signals (6.1) -> per-line resample onto the grid's active region ->
//! colour-difference lowpass (7.2/7.3; Y is unrestricted per 7.1) ->
//! clause 10's base equation with setup -> sync, blanking and burst per
//! Tables 1 and 2, burst inverted from the reference subcarrier (8.2).
//!
//! Recorded simplifications, invisible to M2's oracles:
//! - Resampling happens before bandlimiting (the spec's 4.2 orders them
//!   the other way); identical for band-limited content and exactly
//!   identical for flat bars.
//! - Sync, blanking and burst envelopes are rectangular: Table 2's rise
//!   times (140/300 ns) are not shaped. Vertical sync is modelled at
//!   line granularity: frame lines 4..7 and 266..269 carry broad pulses
//!   (low for each half line except a sync-width serration), enough for
//!   a field detector to key on, but not the standard's half-line
//!   equalizing structure. The nine burst-free lines per field are
//!   honoured.
//! - SC-H phase is whatever the frame origin gives; not tuned to 13.2.

use ntsc_grid::{CompositeFrame, CompositeLine, FrameParity, Geometry, Phase, SAMPLES_PER_CYCLE};

pub mod st170 {
    //! Constants generated from the transcribed data files. See build.rs.
    include!(concat!(env!("OUT_DIR"), "/st170.rs"));
}

/// Sample positions on a 2730-sample line, derived from Table 2 at the
/// grid rate, nearest sample. Line sample 0 is the start of horizontal
/// blanking; H-ref (sync leading edge, 50%) sits 1.5 us in.
pub struct Layout {
    pub href: usize,
    pub sync_end: usize,
    pub burst_start: usize,
    pub burst_end: usize,
    pub active_start: usize,
    pub line_len: usize,
}

pub fn layout() -> Layout {
    let fs = 12.0 * 315e6 / 88.0; // the grid rate, Hz
    let us = |t: f32| (t as f64 * 1e-6 * fs).round() as usize;
    let href = us(st170::BLANKING_TO_HREF_US);
    Layout {
        href,
        sync_end: href + us(st170::SYNC_US),
        burst_start: href + st170::BURST_START_CYCLES * SAMPLES_PER_CYCLE,
        burst_end: href + (st170::BURST_START_CYCLES + st170::BURST_CYCLES) * SAMPLES_PER_CYCLE,
        active_start: href + us(st170::HREF_TO_BLANKING_END_US),
        line_len: Geometry::broadcast().line_len(FrameParity::Even, 0),
    }
}

/// Clause 5.1: the reference camera's OETF, linear light to video.
pub fn camera_oetf(l: f32) -> f32 {
    if l < st170::CAMERA_LINEAR_BELOW {
        st170::CAMERA_LINEAR_SLOPE * l
    } else {
        st170::CAMERA_GAIN * l.powf(st170::CAMERA_POWER) - st170::CAMERA_OFFSET
    }
}

/// IEC sRGB EOTF, byte to linear light.
pub fn srgb_eotf(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The demodulation angle that puts this encoder's burst on -U: zero by
/// construction. Chroma is U sin(theta) + V cos(theta) and the burst is
/// the reference subcarrier plus 180 degrees (clause 10 note 2), i.e.
/// -sin(theta): already the negative U axis.
pub fn burst_axis_offset() -> f64 {
    0.0
}

/// Where the picture rows land: 240 lines per field, field 1 on frame
/// lines 20..260, field 2 on 283..523 (2:1 interlace; input row r goes
/// to field r mod 2). An authored mapping, recorded, at line
/// granularity.
pub const FIELD1_FIRST_LINE: usize = 20;
pub const FIELD2_FIRST_LINE: usize = 283;
pub const ROWS_PER_FIELD: usize = 240;

/// Encode video-level G'B'R' triples (row-major, `width` x `height`,
/// values 0..1, no setup: clause 4.3) at the given origin phase.
/// `height` at most 480.
pub fn encode_video_frame(
    rgb: &[f32],
    width: usize,
    height: usize,
    origin: Phase,
) -> CompositeFrame {
    assert_eq!(rgb.len(), width * height * 3);
    assert!(height <= 2 * ROWS_PER_FIELD, "at most 480 rows");
    let geo = Geometry::broadcast();
    let lay = layout();
    let active_len = lay.line_len - lay.active_start;
    let half = st170::CHROMA_LOWPASS.len() / 2;

    // Which input row, if any, paints each frame line.
    let mut row_of_line = vec![usize::MAX; geo.lines()];
    for r in 0..height {
        let line = if r % 2 == 0 {
            FIELD1_FIRST_LINE + r / 2
        } else {
            FIELD2_FIRST_LINE + r / 2
        };
        row_of_line[line] = r;
    }
    // The nine burst-free lines at the top of each field.
    let burst_free = |line: usize| {
        line < st170::BURST_FREE_LINES_PER_FIELD
            || (262..262 + st170::BURST_FREE_LINES_PER_FIELD).contains(&line)
    };
    // Broad-pulse vertical sync lines (see the module doc): mostly low,
    // with one sync-width serration at the end of each half line.
    let broad = |line: usize| (4..7).contains(&line) || (266..269).contains(&line);

    let to_volts = |ire: f32| ire / st170::IRE_PER_VOLT;
    let mut lines = Vec::with_capacity(geo.lines());
    let mut y_row = vec![0.0f32; active_len];
    let mut u_row = vec![0.0f32; active_len];
    let mut v_row = vec![0.0f32; active_len];
    let mut u_f = vec![0.0f32; active_len];
    let mut v_f = vec![0.0f32; active_len];
    #[allow(clippy::needless_range_loop)]
    for line in 0..geo.lines() {
        let mut samples = vec![to_volts(st170::BLANK_IRE); lay.line_len];
        if broad(line) {
            let half = lay.line_len / 2;
            let serration = lay.sync_end - lay.href;
            for (s, sample) in samples.iter_mut().enumerate() {
                if (s % half) < half - serration {
                    *sample = to_volts(st170::SYNC_IRE);
                }
            }
            lines.push(CompositeLine {
                samples,
                sync_start: lay.href,
                burst_start: lay.burst_start,
                active_start: lay.active_start,
            });
            continue;
        }
        // Sync tip on every other line (line-granularity structure).
        for s in samples.iter_mut().take(lay.sync_end).skip(lay.href) {
            *s = to_volts(st170::SYNC_IRE);
        }
        // Burst: reference + 180 degrees, 40 IRE peak to peak.
        if !burst_free(line) {
            for (s, sample) in samples
                .iter_mut()
                .enumerate()
                .take(lay.burst_end)
                .skip(lay.burst_start)
            {
                let p = origin
                    .advanced_by(geo.phase_at(FrameParity::Even, line, s).get() as usize)
                    .get();
                let theta = std::f32::consts::TAU * p as f32 / 12.0;
                *sample = to_volts(
                    st170::BLANK_IRE - st170::BURST_PP_IRE / 2.0 * theta.sin(),
                );
            }
        }
        let row = row_of_line[line];
        if row != usize::MAX {
            // Video RGB resampled onto the active region (linear), then
            // the base matrix, then the colour-difference lowpass.
            for x in 0..active_len {
                let fx = x as f32 * (width - 1) as f32 / (active_len - 1) as f32;
                let x0 = fx as usize;
                let x1 = (x0 + 1).min(width - 1);
                let t = fx - x0 as f32;
                let mut rgb_i = [0.0f32; 3];
                for c in 0..3 {
                    let a = rgb[(row * width + x0) * 3 + c];
                    let b = rgb[(row * width + x1) * 3 + c];
                    rgb_i[c] = a + (b - a) * t;
                }
                let dot = |m: &[f32; 3]| m[0] * rgb_i[0] + m[1] * rgb_i[1] + m[2] * rgb_i[2];
                y_row[x] = dot(&st170::BASE_Y);
                u_row[x] = dot(&st170::BASE_BMY);
                v_row[x] = dot(&st170::BASE_RMY);
            }
            for x in 0..active_len {
                let conv = |src: &[f32]| {
                    st170::CHROMA_LOWPASS
                        .iter()
                        .enumerate()
                        .map(|(k, t)| {
                            let j = (x as isize + k as isize - half as isize)
                                .clamp(0, active_len as isize - 1)
                                as usize;
                            t * src[j]
                        })
                        .sum::<f32>()
                };
                u_f[x] = conv(&u_row);
                v_f[x] = conv(&v_row);
            }
            for x in 0..active_len {
                let s = lay.active_start + x;
                let p = origin
                    .advanced_by(geo.phase_at(FrameParity::Even, line, s).get() as usize)
                    .get();
                let theta = std::f32::consts::TAU * p as f32 / 12.0;
                // Clause 10 base equation, IRE domain, then volts.
                let n = st170::Y_SCALE * (100.0 * y_row[x])
                    + st170::SETUP_IRE
                    + st170::U_SCALE * (100.0 * u_f[x]) * theta.sin()
                    + st170::V_SCALE * (100.0 * v_f[x]) * theta.cos();
                samples[s] = to_volts(n);
            }
        }
        lines.push(CompositeLine {
            samples,
            sync_start: lay.href,
            burst_start: lay.burst_start,
            active_start: lay.active_start,
        });
    }
    CompositeFrame {
        profile: geo,
        lines,
        frame_parity: FrameParity::Even,
        phase_at_origin: origin,
    }
}

/// Encode an sRGB8 frame: EOTF to linear light, the reference camera
/// OETF (5.1) to video, then `encode_video_frame`.
pub fn encode_srgb_frame(
    rgb8: &[u8],
    width: usize,
    height: usize,
    origin: Phase,
) -> CompositeFrame {
    let video: Vec<f32> = rgb8
        .iter()
        .map(|&b| camera_oetf(srgb_eotf(b)))
        .collect();
    encode_video_frame(&video, width, height, origin)
}
