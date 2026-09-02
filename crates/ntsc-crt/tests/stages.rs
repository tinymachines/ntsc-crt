//! The analytic stage tests (handoff spec section 6): no external
//! oracle exists for a model, so each stage is held to its own declared
//! mathematics, with the declarations typed HERE and the subject built
//! from `CrtParams`, so a drifted parameter is a red test rather than a
//! silently different look.
//!
//! MUTATE=1 perturbs the subject's beam width and one persistence time
//! constant (the spec's named perturbations); the beam-impulse and
//! decay tests must go red. The bloom and mask tests compare the
//! subject against its own params by construction and stay green under
//! those two perturbations; they are named here for that reason.

use ntsc_crt::{
    window_visible, Beam, CrtParams, CrtPipeline, DisplayFrame, GeometryParams, Mask, MaskParams,
    Persistence, Scanlines,
};
use ntsc_decode::LinearRgbFrame;
use ntsc_grid::Stage;

/// The authored declarations the tests hold the stages to.
const DECLARED_BEAM_SIGMA: f32 = 4.0;
const DECLARED_TAU: [f32; 3] = [0.012, 0.020, 0.008];

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

fn subject(scale: usize) -> CrtParams {
    let mut p = CrtParams::authored(scale);
    if mutate() {
        p.beam_sigma_samples *= 1.5;
        p.persistence_tau[0] *= 2.0;
    }
    p
}

fn frame(width: usize, height: usize) -> LinearRgbFrame {
    LinearRgbFrame {
        width,
        height,
        data: vec![0.0; width * height * 3],
        display_gamma: 2.2,
    }
}

#[test]
fn the_beam_impulse_is_the_declared_gaussian() {
    let mut input = frame(2048, 1);
    input.data[1024 * 3] = 1.0;
    let mut beam = Beam { params: subject(1) };
    let out = beam.process(&input);
    assert_eq!(out.width, 256);
    assert!((out.samples_per_pixel - 8.0).abs() < 1e-6, "the ratio is stated");
    let sigma = DECLARED_BEAM_SIGMA;
    let reach = (3.0 * sigma).ceil() as isize;
    for x in 120..136 {
        let center = (x as f32 + 0.5) * 8.0 - 0.5;
        let c0 = center.round() as isize;
        let mut wsum = 0.0f32;
        let mut want = 0.0f32;
        for i in c0 - reach..=c0 + reach {
            let d = i as f32 - center;
            let wt = (-d * d / (2.0 * sigma * sigma)).exp();
            wsum += wt;
            if i == 1024 {
                want = wt;
            }
        }
        let got = out.at(x, 0)[0];
        assert!(
            (got - want / wsum).abs() < 1e-5,
            "column {x}: beam {got:.6}, declared Gaussian {:.6}",
            want / wsum
        );
    }
}

#[test]
fn scanlines_bloom_with_beam_current() {
    // One line, a dim pixel and a bright one: each column's vertical
    // profile must be the declared Gaussian at sigma(v) = base * (1 +
    // bloom * v), so the bright column is measurably wider.
    let params = subject(4);
    let mut input = DisplayFrame {
        width: 2,
        height: 1,
        data: vec![0.1, 0.1, 0.1, 1.0, 1.0, 1.0],
        samples_per_pixel: 8.0,
    };
    input.data[3] = 1.0;
    let mut st = Scanlines { params: params.clone() };
    let out = st.process(&input);
    assert_eq!(out.height, 4);
    let center = 0.5 * 4.0 - 0.5;
    let mut widths = [0.0f32; 2];
    for (x, v) in [(0usize, 0.1f32), (1, 1.0)] {
        let sigma = params.scanline_sigma_rows * (1.0 + params.bloom * v);
        for y in 0..4 {
            let d = y as f32 - center;
            let want = v * (-d * d / (2.0 * sigma * sigma)).exp();
            let got = out.at(x, y)[0];
            assert!(
                (got - want).abs() < 1e-4,
                "column {x} row {y}: {got:.5} vs declared {want:.5}"
            );
        }
        widths[x.min(1)] = sigma;
    }
    assert!(widths[1] > widths[0], "brighter must bloom wider");
}

#[test]
fn persistence_decays_with_the_declared_constants() {
    let params = subject(1);
    let mut st = Persistence::new(&params);
    let white = DisplayFrame {
        width: 2,
        height: 1,
        data: vec![1.0; 6],
        samples_per_pixel: 8.0,
    };
    let black = DisplayFrame {
        width: 2,
        height: 1,
        data: vec![0.0; 6],
        samples_per_pixel: 8.0,
    };
    st.process(&white);
    // The declared per-frame decay, from the declared constants and the
    // NES rendering-enabled period, typed independently of CrtParams.
    let dt = (714_736.0 + 714_728.0) / 2.0 / (472_500_000.0 / 11.0);
    for n in 1..=4 {
        let out = st.process(&black);
        for (c, tau) in DECLARED_TAU.iter().enumerate() {
            let want = (-(n as f32) * dt / tau).exp();
            let got = out.at(0, 0)[c];
            assert!(
                (got - want).abs() < 1e-6,
                "frame {n} channel {c}: {got:.7} vs declared {want:.7}"
            );
        }
    }
}

#[test]
fn persistence_rises_immediately_and_resets() {
    let params = CrtParams::authored(1);
    let mut st = Persistence::new(&params);
    let mk = |v: f32| DisplayFrame {
        width: 1,
        height: 1,
        data: vec![v; 3],
        samples_per_pixel: 8.0,
    };
    st.process(&mk(0.2));
    let out = st.process(&mk(1.0));
    assert_eq!(out.at(0, 0), [1.0; 3], "excitation is instant");
    st.reset();
    let out = st.process(&mk(0.0));
    assert_eq!(out.at(0, 0), [0.0; 3], "reset clears the phosphor");
}

#[test]
fn the_mask_tiles_exactly_at_integer_pitch() {
    let mut mask = Mask::new(MaskParams { pitch: 2, off_gain: 0.25 });
    let white = DisplayFrame {
        width: 18,
        height: 1,
        data: vec![1.0; 18 * 3],
        samples_per_pixel: 8.0,
    };
    let out = mask.process(&white);
    for x in 0..18 {
        let on = (x / 2) % 3;
        for c in 0..3 {
            let want = if c == on { 1.0 } else { 0.25 };
            assert_eq!(out.at(x, 0)[c], want, "column {x} channel {c}");
        }
    }
    // The pattern tiles with period pitch * 3 exactly.
    for x in 0..12 {
        assert_eq!(out.at(x, 0), out.at(x + 6, 0), "period 6 at column {x}");
    }
}

#[test]
#[should_panic(expected = "the mask pitch is an integer, one or more")]
fn a_zero_pitch_mask_is_refused() {
    Mask::new(MaskParams { pitch: 0, off_gain: 0.5 });
}

#[test]
fn geometry_keeps_the_window_visible_and_rounds_the_corners() {
    let mut params = CrtParams::authored(2);
    params.mask = Some(MaskParams { pitch: 1, off_gain: 0.3 });
    params.geometry = Some(GeometryParams { barrel_k: 0.03, corner_radius: 8.0 });
    let mut pipe = CrtPipeline::new(params);
    let mut white = frame(2048, 240);
    white.data.fill(1.0);
    let out = pipe.process(&white);
    assert_eq!((out.width, out.height), (512, 480), "integer scale");
    assert!(window_visible(&out, 2), "the 224 x 224 window must stay lit");
    assert_eq!(out.at(1, 1), [0.0; 3], "the corner is rounded off");
}
