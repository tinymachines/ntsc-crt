//! Internal-consistency checks on the transcribed data files. A wrong
//! transcription of one column should disagree with the other column, so
//! every (volts, IRE) pair is cross-checked, the page's own average
//! attenuation factor is recomputed from the measured pairs, and every
//! inverse-matrix coefficient is re-derived from the base matrix and the
//! reduction factors.
//!
//! These tests live in ntsc-grid only because it is the sole M0 crate;
//! they move beside the crate that consumes the data when one exists.
//!
//! MUTATE=1 perturbs one parsed voltage by +0.05 after reading the file;
//! the IRE cross-check and the attenuation recomputation must both go red.

use toml::Value;

fn load(name: &str) -> Value {
    let path = format!("{}/../../data/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.parse::<Value>().unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn mutate() -> bool {
    std::env::var("MUTATE").map(|v| v == "1").unwrap_or(false)
}

fn f(v: &Value, table: &str, key: &str) -> f64 {
    v[table][key]
        .as_float()
        .or_else(|| v[table][key].as_integer().map(|i| i as f64))
        .unwrap_or_else(|| panic!("{table}.{key} is not a number"))
}

fn fs(v: &Value, table: &str, key: &str) -> Vec<f64> {
    v[table][key]
        .as_array()
        .unwrap_or_else(|| panic!("{table}.{key} is not an array"))
        .iter()
        .map(|x| x.as_float().or_else(|| x.as_integer().map(|i| i as f64)).unwrap())
        .collect()
}

/// The measured (volts, printed IRE) pairs, with MUTATE's perturbation
/// applied to the first one.
fn level_pairs(v: &Value) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    for key in ["sync", "burst_low", "burst_high"] {
        out.push((
            key.to_string(),
            f(v, "levels", &format!("{key}_volts")),
            f(v, "levels", &format!("{key}_ire")),
        ));
    }
    for key in ["low", "high", "low_attenuated", "high_attenuated"] {
        let volts = fs(v, "levels", &format!("{key}_volts"));
        let ire = fs(v, "levels", &format!("{key}_ire"));
        assert_eq!(volts.len(), 4, "{key}_volts");
        assert_eq!(ire.len(), 4, "{key}_ire");
        for i in 0..4 {
            out.push((format!("{key}[{i}]"), volts[i], ire[i]));
        }
    }
    if mutate() {
        // low[0]: a row both the IRE cross-check and the attenuation
        // recomputation consume, so one perturbation reddens both.
        out[3].1 += 0.05;
    }
    out
}

#[test]
fn every_voltage_agrees_with_its_printed_ire() {
    let v = load("nes-levels.toml");
    // 1 IRE = 7.14 mV (714 mV over 100 IRE, the standard-video table on
    // the same page); $1D is the 0 IRE reference.
    let zero = 0.312;
    let per_ire = 0.00714;
    for (name, volts, printed) in level_pairs(&v) {
        let computed = (volts - zero) / per_ire;
        assert!(
            (computed - printed).abs() < 0.65,
            "{name}: {volts} V computes to {computed:.2} IRE, printed {printed}"
        );
    }
}

#[test]
fn the_pages_average_attenuation_factor_recomputes_from_its_own_pairs() {
    let v = load("nes-levels.toml");
    let pairs = level_pairs(&v);
    let get = |n: &str| pairs.iter().find(|(name, ..)| name == n).unwrap().1;
    // The seven measured pairs; high[3]/high_attenuated[3] is the page's
    // own duplication of the $2x row and would double-count.
    let mut ratios = Vec::new();
    for i in 0..4 {
        ratios.push(get(&format!("low_attenuated[{i}]")) / get(&format!("low[{i}]")));
    }
    for i in 0..3 {
        ratios.push(get(&format!("high_attenuated[{i}]")) / get(&format!("high[{i}]")));
    }
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    let claimed = f(&v, "attenuation", "average_factor");
    assert!(
        (mean - claimed).abs() < 1e-4,
        "mean of measured ratios {mean:.6} vs the page's {claimed}"
    );
}

#[test]
fn phase_waves_are_wave_numbers() {
    let v = load("nes-levels.toml");
    for key in [
        "colorburst_wave",
        "emphasis_bit7_wave",
        "emphasis_bit6_wave",
        "emphasis_bit5_wave",
    ] {
        let w = f(&v, "phase", key);
        assert!((1.0..=12.0).contains(&w), "{key} = {w}");
    }
}

#[test]
fn inverse_matrix_re_derives_from_base_and_reduction() {
    let v = load("yuv-matrix.toml");
    let y = fs(&v, "base", "y");
    let bmy = fs(&v, "base", "b_minus_y");
    let rmy = fs(&v, "base", "r_minus_y");
    // Row identities: Y sums to 1; B-Y and R-Y are unit vectors minus Y.
    assert!((y.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    for i in 0..3 {
        let e_b = if i == 2 { 1.0 } else { 0.0 };
        let e_r = if i == 0 { 1.0 } else { 0.0 };
        assert!((bmy[i] - (e_b - y[i])).abs() < 1e-9, "b_minus_y[{i}]");
        assert!((rmy[i] - (e_r - y[i])).abs() < 1e-9, "r_minus_y[{i}]");
    }

    let uf = f(&v, "reduction", "u_factor");
    let vf = f(&v, "reduction", "v_factor");
    let r_from_v = f(&v, "inverse", "r_from_v");
    let b_from_u = f(&v, "inverse", "b_from_u");
    let g_from_u = f(&v, "inverse", "g_from_u");
    let g_from_v = f(&v, "inverse", "g_from_v");
    // R = Y + V/vf, B = Y + U/uf; G falls out of the Y row.
    let mut derived = [
        ("r_from_v", 1.0 / vf, r_from_v),
        ("b_from_u", 1.0 / uf, b_from_u),
        ("g_from_u", -(1.0 / uf) * y[2] / y[1], g_from_u),
        ("g_from_v", -(1.0 / vf) * y[0] / y[1], g_from_v),
    ];
    if mutate() {
        derived[0].2 += 0.01;
    }
    for (name, want, got) in derived {
        assert!(
            (want - got).abs() < 1e-4,
            "{name}: derived {want:.6}, transcribed {got}"
        );
    }
}
