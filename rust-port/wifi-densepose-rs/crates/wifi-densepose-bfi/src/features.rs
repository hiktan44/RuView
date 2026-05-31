//! Temporal feature extraction over a window of BFI reports.
//!
//! Each [`crate::types::BfiReport`] is reduced to a flat feature vector via
//! [`crate::types::BfiReport::feature_vector`]. Over a window of consecutive
//! reports we compute per-dimension variance (the basic motion proxy) and the
//! band-limited power in the breathing (0.1–0.5 Hz) and gross-motion
//! (0.5–4 Hz) bands using a Goertzel evaluation of the DFT. These bands mirror
//! the v1 Python presence/breathing pipeline.

use crate::types::{BfiConfig, BfiError, BfiReport};

/// Motion band lower bound (Hz) — gross body movement / walking.
pub const MOTION_BAND_LOW_HZ: f64 = 0.5;
/// Motion band upper bound (Hz).
pub const MOTION_BAND_HIGH_HZ: f64 = 4.0;
/// Breathing band lower bound (Hz).
pub const BREATHING_BAND_LOW_HZ: f64 = 0.1;
/// Breathing band upper bound (Hz).
pub const BREATHING_BAND_HIGH_HZ: f64 = 0.5;

/// Aggregated temporal features for one analysis window.
#[derive(Debug, Clone, PartialEq)]
pub struct BfiFeatures {
    /// Number of frames the window was computed from.
    pub frames: usize,
    /// Per-dimension temporal variance of the angle feature vector.
    pub per_dim_variance: Vec<f64>,
    /// Sum of per-dimension variance — the presence proxy.
    pub total_variance: f64,
    /// Mean band power in the 0.5–4 Hz motion band (averaged over dims).
    pub motion_band_power: f64,
    /// Mean band power in the 0.1–0.5 Hz breathing band (averaged over dims).
    pub breathing_band_power: f64,
}

/// Compute the per-dimension temporal mean of a window of equal-length vectors.
fn column_means(window: &[Vec<f64>], dims: usize) -> Vec<f64> {
    let n = window.len() as f64;
    let mut means = vec![0.0; dims];
    for sample in window {
        for (m, &v) in means.iter_mut().zip(sample.iter()) {
            *m += v;
        }
    }
    for m in &mut means {
        *m /= n;
    }
    means
}

/// Goertzel-style single-bin DFT magnitude-squared (power) for frequency `freq`.
fn bin_power(signal: &[f64], freq_hz: f64, sample_rate_hz: f64) -> f64 {
    let n = signal.len();
    if n == 0 || sample_rate_hz <= 0.0 {
        return 0.0;
    }
    let omega = 2.0 * std::f64::consts::PI * freq_hz / sample_rate_hz;
    let (cos_w, sin_w) = (omega.cos(), omega.sin());
    let coeff = 2.0 * cos_w;
    let (mut s_prev, mut s_prev2) = (0.0_f64, 0.0_f64);
    for &x in signal {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    let real = s_prev - s_prev2 * cos_w;
    let imag = s_prev2 * sin_w;
    (real * real + imag * imag) / (n as f64 * n as f64)
}

/// Total band power (summed over discrete bins) spanning `[low, high]`.
///
/// Summing rather than averaging keeps a narrow-band signal's energy
/// concentrated, so a single dominant tone is not diluted by the (many) empty
/// bins of a wide band such as the 0.5–4 Hz motion band.
fn band_power(signal: &[f64], low_hz: f64, high_hz: f64, sample_rate_hz: f64) -> f64 {
    let n = signal.len();
    if n < 2 || sample_rate_hz <= 0.0 {
        return 0.0;
    }
    let df = sample_rate_hz / n as f64;
    let nyquist = sample_rate_hz / 2.0;
    let hi = high_hz.min(nyquist);
    let mut total = 0.0;
    let mut k = 1usize;
    loop {
        let f = k as f64 * df;
        if f > hi {
            break;
        }
        if f >= low_hz {
            total += bin_power(signal, f, sample_rate_hz);
        }
        k += 1;
        if k > n / 2 {
            break;
        }
    }
    total
}

/// Extract [`BfiFeatures`] from a window of reports.
///
/// All reports must produce feature vectors of the same length (same MIMO
/// geometry and subcarrier count). The shortest vector length is used so that
/// mismatched reports degrade gracefully rather than panicking.
pub fn extract_features(window: &[BfiReport], config: &BfiConfig) -> Result<BfiFeatures, BfiError> {
    if window.len() < 2 {
        return Err(BfiError::WindowTooSmall {
            needed: 2,
            got: window.len(),
        });
    }
    let vectors: Vec<Vec<f64>> = window.iter().map(BfiReport::feature_vector).collect();
    let dims = vectors.iter().map(Vec::len).min().unwrap_or(0);
    if dims == 0 {
        return Ok(BfiFeatures {
            frames: window.len(),
            per_dim_variance: Vec::new(),
            total_variance: 0.0,
            motion_band_power: 0.0,
            breathing_band_power: 0.0,
        });
    }
    let trimmed: Vec<Vec<f64>> = vectors
        .into_iter()
        .map(|mut v| {
            v.truncate(dims);
            v
        })
        .collect();

    let means = column_means(&trimmed, dims);
    let n = trimmed.len() as f64;

    let mut per_dim_variance = vec![0.0; dims];
    let mut motion_accum = 0.0;
    let mut breathing_accum = 0.0;

    let mut series = vec![0.0; trimmed.len()];
    for d in 0..dims {
        let mean = means[d];
        let mut var = 0.0;
        for (t, sample) in trimmed.iter().enumerate() {
            let centered = sample[d] - mean;
            series[t] = centered;
            var += centered * centered;
        }
        per_dim_variance[d] = var / n;
        motion_accum += band_power(
            &series,
            MOTION_BAND_LOW_HZ,
            MOTION_BAND_HIGH_HZ,
            config.sample_rate_hz,
        );
        breathing_accum += band_power(
            &series,
            BREATHING_BAND_LOW_HZ,
            BREATHING_BAND_HIGH_HZ,
            config.sample_rate_hz,
        );
    }

    let total_variance = per_dim_variance.iter().sum();
    Ok(BfiFeatures {
        frames: trimmed.len(),
        per_dim_variance,
        total_variance,
        motion_band_power: motion_accum / dims as f64,
        breathing_band_power: breathing_accum / dims as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Bandwidth, MimoControl, SubcarrierAngles};

    fn report(values: &[f64]) -> BfiReport {
        BfiReport {
            source: None,
            control: MimoControl {
                nc: 1,
                nr: 2,
                bandwidth: Bandwidth::Bw20,
                codebook_large: false,
                mu_feedback: false,
            },
            subcarriers: vec![SubcarrierAngles {
                phi: values.to_vec(),
                psi: vec![],
            }],
        }
    }

    #[test]
    fn window_too_small_errors() {
        let cfg = BfiConfig::default();
        let err = extract_features(&[report(&[1.0])], &cfg).unwrap_err();
        assert!(matches!(err, BfiError::WindowTooSmall { .. }));
    }

    #[test]
    fn constant_window_has_zero_variance() {
        let cfg = BfiConfig::default();
        let window: Vec<_> = (0..16).map(|_| report(&[0.5, 0.7])).collect();
        let f = extract_features(&window, &cfg).unwrap();
        assert!(f.total_variance < 1e-9);
        assert!(f.motion_band_power < 1e-9);
    }

    #[test]
    fn oscillation_produces_band_power() {
        let cfg = BfiConfig {
            sample_rate_hz: 20.0,
            ..BfiConfig::default()
        };
        // 1 Hz sine -> in the motion band (0.5-4 Hz).
        let window: Vec<_> = (0..64)
            .map(|t| {
                let phase = 2.0 * std::f64::consts::PI * 1.0 * (t as f64) / 20.0;
                report(&[phase.sin(), 0.0])
            })
            .collect();
        let f = extract_features(&window, &cfg).unwrap();
        assert!(f.total_variance > 0.0);
        assert!(f.motion_band_power > f.breathing_band_power);
    }
}
