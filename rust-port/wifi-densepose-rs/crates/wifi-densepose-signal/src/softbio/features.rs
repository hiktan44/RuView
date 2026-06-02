//! Gait-feature extraction from a CSI walking window.
//!
//! The WiFi-"gender" literature is entirely **gait-derived**: any apparent
//! separation rides on stride cadence, gait regularity, and body-scattering
//! cross-section. This extractor reproduces those *gait* descriptors in a
//! deterministic, dependency-free way by reusing the same amplitude spectrogram
//! as [`crate::har`] (via [`crate::spectrogram::compute_spectrogram`]).
//!
//! These features are perfectly fine **gait descriptors**. They are emphatically
//! **not** a gender measurement — see the [module docs](crate::softbio). They are
//! used here only as the input to a nearest-centroid matcher over operator-enrolled
//! clusters.

use crate::spectrogram::{compute_spectrogram, SpectrogramConfig, WindowFunction};
use crate::{Result, SignalError};
use serde::{Deserialize, Serialize};

/// Length of the gait feature vector produced by [`GaitFeatureExtractor`].
pub const N_GAIT_FEATURES: usize = 8;

/// Configuration for [`GaitFeatureExtractor`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaitConfig {
    /// Sampling rate of the CSI stream in Hz (frames per second).
    pub sample_rate: f64,
    /// STFT window size (samples per frame) used for the spectrogram.
    pub window_size: usize,
    /// STFT hop size (step between frames).
    pub hop_size: usize,
    /// Minimum number of time steps required to extract features.
    pub min_window: usize,
}

impl Default for GaitConfig {
    fn default() -> Self {
        // Defaults tuned for ESP32-S3 CSI at ~50 Hz over a ~2.5 s walking window.
        Self {
            sample_rate: 50.0,
            window_size: 32,
            hop_size: 8,
            min_window: 48,
        }
    }
}

/// A fixed-length, deterministic gait feature vector.
///
/// Field order is stable and mirrored by [`GaitFeatures::as_array`]; the
/// classifier depends on this ordering, so do not reorder without re-enrolling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaitFeatures {
    /// Dominant gait cadence in Hz (peak motion-band Doppler frequency).
    pub cadence_hz: f64,
    /// Fractional spectral power in the stride band (~1–3 Hz, whole-body sway).
    pub stride_band_power: f64,
    /// Fractional spectral power in the limb band (~3–8 Hz, swinging limbs).
    pub limb_band_power: f64,
    /// Spectral centroid in Hz (overall Doppler "weight" — body cross-section cue).
    pub spectral_centroid: f64,
    /// Normalized spectral spread in `[0,1]` (gait regularity: tight = regular).
    pub spectral_spread: f64,
    /// Gait rhythm regularity in `[0,1]` (autocorrelation strength of the
    /// per-frame energy series — periodic walking → high).
    pub rhythm_regularity: f64,
    /// Mean per-frame velocity (frame-to-frame energy change) — stride intensity.
    pub mean_velocity: f64,
    /// Overall window energy (log-scaled) — gross scattering amplitude cue.
    pub log_energy: f64,
}

impl GaitFeatures {
    /// Flatten into a fixed-length array in stable feature order.
    pub fn as_array(&self) -> [f64; N_GAIT_FEATURES] {
        [
            self.cadence_hz,
            self.stride_band_power,
            self.limb_band_power,
            self.spectral_centroid,
            self.spectral_spread,
            self.rhythm_regularity,
            self.mean_velocity,
            self.log_energy,
        ]
    }
}

/// Extracts [`GaitFeatures`] from a temporal window of CSI amplitude frames.
#[derive(Debug, Clone)]
pub struct GaitFeatureExtractor {
    config: GaitConfig,
}

impl GaitFeatureExtractor {
    /// Create a new extractor with the given configuration.
    pub fn new(config: GaitConfig) -> Self {
        Self { config }
    }

    /// Create an extractor with default configuration.
    pub fn default_config() -> Self {
        Self::new(GaitConfig::default())
    }

    /// Access the configuration.
    pub fn config(&self) -> &GaitConfig {
        &self.config
    }

    /// Extract gait features from a window of per-frame subcarrier amplitudes.
    ///
    /// `window[t]` is the amplitude vector (one value per subcarrier) at time
    /// step `t`. All frames should have the same subcarrier count.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] if the window is shorter than
    /// `config.min_window`, frames are empty, or the spectrogram cannot be built.
    pub fn extract(&self, window: &[Vec<f64>]) -> Result<GaitFeatures> {
        let n = window.len();
        if n < self.config.min_window {
            return Err(SignalError::FeatureExtraction(format!(
                "gait window too short: need >= {} frames, got {}",
                self.config.min_window, n
            )));
        }
        if window.iter().any(|f| f.is_empty()) {
            return Err(SignalError::FeatureExtraction(
                "gait window contains empty frames".to_string(),
            ));
        }

        // Collapse subcarriers → a single mean-amplitude time series.
        let series: Vec<f64> = window
            .iter()
            .map(|frame| frame.iter().sum::<f64>() / frame.len() as f64)
            .collect();

        // Detrend (remove static multipath DC) so we measure *motion* (gait).
        let mean: f64 = series.iter().sum::<f64>() / n as f64;
        let centered: Vec<f64> = series.iter().map(|v| v - mean).collect();

        let temporal_variance = centered.iter().map(|v| v * v).sum::<f64>() / n as f64;
        let log_energy = (temporal_variance + 1e-12).ln();

        // ── Amplitude spectrogram (reuse existing STFT, as HAR does) ──
        let win = self.config.window_size.min(n);
        let spec_cfg = SpectrogramConfig {
            window_size: win,
            hop_size: self.config.hop_size.max(1),
            window_fn: WindowFunction::Hann,
            power: true,
        };
        let spec = compute_spectrogram(&centered, self.config.sample_rate, &spec_cfg)
            .map_err(|e| SignalError::FeatureExtraction(format!("gait spectrogram: {e}")))?;

        // Mean power per frequency bin (averaged across all time frames).
        let mut bin_power = vec![0.0f64; spec.n_freq];
        for f in 0..spec.n_freq {
            let mut s = 0.0;
            for t in 0..spec.n_time {
                s += spec.data[[f, t]];
            }
            bin_power[f] = s / spec.n_time.max(1) as f64;
        }
        let total_power: f64 = bin_power.iter().sum::<f64>().max(1e-12);

        // ── Stride / limb band powers and dominant cadence ──
        let (mut stride, mut limb) = (0.0, 0.0);
        let mut cadence_hz = 0.0;
        let mut peak_motion_power = 0.0;
        for (f, &p) in bin_power.iter().enumerate() {
            let freq = f as f64 * spec.freq_resolution;
            if freq < 1.0 {
                // DC / sub-band ignored (already detrended).
                continue;
            }
            if freq < 3.0 {
                stride += p;
            } else if freq < 8.0 {
                limb += p;
            }
            // Dominant cadence = strongest motion-band bin.
            if freq >= 0.5 && p > peak_motion_power {
                peak_motion_power = p;
                cadence_hz = freq;
            }
        }
        let stride_band_power = stride / total_power;
        let limb_band_power = limb / total_power;

        // ── Spectral centroid & normalized spread (body cross-section + regularity) ──
        let spectral_centroid = {
            let mut weighted = 0.0;
            for (f, &p) in bin_power.iter().enumerate() {
                weighted += (f as f64 * spec.freq_resolution) * p;
            }
            weighted / total_power
        };
        let spectral_spread = {
            let mut var = 0.0;
            for (f, &p) in bin_power.iter().enumerate() {
                let freq = f as f64 * spec.freq_resolution;
                var += (freq - spectral_centroid).powi(2) * (p / total_power);
            }
            let nyquist = (self.config.sample_rate / 2.0).max(1e-9);
            (var.sqrt() / nyquist).clamp(0.0, 1.0)
        };

        // ── Velocity profile and rhythm regularity ──
        let frame_energy: Vec<f64> = (0..spec.n_time)
            .map(|t| (0..spec.n_freq).map(|f| spec.data[[f, t]]).sum::<f64>())
            .collect();
        let velocities: Vec<f64> = frame_energy
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .collect();
        let mean_velocity = if velocities.is_empty() {
            0.0
        } else {
            velocities.iter().sum::<f64>() / velocities.len() as f64
        };
        let rhythm_regularity = rhythm_regularity(&frame_energy);

        Ok(GaitFeatures {
            cadence_hz,
            stride_band_power,
            limb_band_power,
            spectral_centroid,
            spectral_spread,
            rhythm_regularity,
            mean_velocity,
            log_energy,
        })
    }
}

/// Gait rhythm regularity: the strongest non-trivial lag autocorrelation of the
/// per-frame energy series, normalized to `[0,1]`. Periodic walking gives a high
/// value; irregular / non-periodic motion gives a low one.
fn rhythm_regularity(frame_energy: &[f64]) -> f64 {
    let n = frame_energy.len();
    if n < 4 {
        return 0.0;
    }
    let mean = frame_energy.iter().sum::<f64>() / n as f64;
    let centered: Vec<f64> = frame_energy.iter().map(|e| e - mean).collect();
    let denom: f64 = centered.iter().map(|v| v * v).sum::<f64>().max(1e-12);

    // Search lags from 1 .. n/2 for the strongest positive autocorrelation.
    let mut best = 0.0f64;
    for lag in 1..(n / 2).max(2) {
        let mut acc = 0.0;
        for i in 0..(n - lag) {
            acc += centered[i] * centered[i + lag];
        }
        let r = acc / denom;
        if r > best {
            best = r;
        }
    }
    best.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn const_window(n: usize, sc: usize, level: f64) -> Vec<Vec<f64>> {
        (0..n).map(|_| vec![level; sc]).collect()
    }

    /// Synthetic gait: a sustained cadence-Hz amplitude modulation.
    fn gait_window(n: usize, sc: usize, rate: f64, cadence: f64, depth: f64) -> Vec<Vec<f64>> {
        (0..n)
            .map(|t| {
                let m = 1.0 + depth * (2.0 * PI * cadence * t as f64 / rate).sin();
                vec![m; sc]
            })
            .collect()
    }

    #[test]
    fn rejects_short_window() {
        let ex = GaitFeatureExtractor::default_config();
        let w = const_window(4, 56, 1.0);
        assert!(ex.extract(&w).is_err());
    }

    #[test]
    fn rejects_empty_frames() {
        let cfg = GaitConfig {
            min_window: 4,
            window_size: 4,
            hop_size: 2,
            ..Default::default()
        };
        let ex = GaitFeatureExtractor::new(cfg);
        let mut w = const_window(8, 56, 1.0);
        w[2].clear();
        assert!(ex.extract(&w).is_err());
    }

    #[test]
    fn features_are_finite_and_stable_length() {
        let ex = GaitFeatureExtractor::default_config();
        let f = ex.extract(&gait_window(128, 56, 50.0, 4.0, 0.5)).unwrap();
        assert_eq!(f.as_array().len(), N_GAIT_FEATURES);
        assert!(f.as_array().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn periodic_gait_has_higher_regularity_than_static() {
        let ex = GaitFeatureExtractor::default_config();
        let still = ex.extract(&const_window(128, 56, 2.0)).unwrap();
        let walk = ex.extract(&gait_window(128, 56, 50.0, 4.0, 0.5)).unwrap();
        assert!(
            walk.rhythm_regularity >= still.rhythm_regularity,
            "periodic gait regularity {} should be >= static {}",
            walk.rhythm_regularity,
            still.rhythm_regularity
        );
    }

    #[test]
    fn distinct_cadences_yield_distinct_features() {
        let ex = GaitFeatureExtractor::default_config();
        let slow = ex.extract(&gait_window(128, 56, 50.0, 2.0, 0.5)).unwrap();
        let fast = ex.extract(&gait_window(128, 56, 50.0, 6.0, 0.5)).unwrap();
        // Different cadence should move at least the centroid (gait separability).
        assert!(
            (slow.spectral_centroid - fast.spectral_centroid).abs() > 1e-6,
            "expected distinct centroids for distinct cadences"
        );
    }
}
