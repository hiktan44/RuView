//! HAR feature extraction from temporal CSI amplitude windows.
//!
//! The input is a temporal window of per-subcarrier amplitude vectors
//! (e.g. 56 subcarriers sampled at ~20–100 Hz over ~1–3 s). We collapse the
//! per-subcarrier streams into a single mean-amplitude time series, build an
//! **amplitude spectrogram** by reusing [`crate::spectrogram::compute_spectrogram`]
//! (the same time-frequency representation a CNN would consume), and derive a
//! compact, deterministic feature vector from it.
//!
//! The feature vector is intentionally hand-engineered and dependency-free so
//! the [`super::HarClassifier`] needs no external ML runtime. It captures the
//! cues the literature relies on for single-antenna CSI HAR:
//!
//! - **Motion / Doppler band powers** (low/mid/high) — Walking vs. still.
//! - **Spectral entropy & centroid** — broadband motion vs. narrowband.
//! - **Temporal variance & velocity profile** — a simplified Widar3.0 BVP idea.
//! - **Fall-specific transients** — a sudden energy spike followed by stillness.

use crate::spectrogram::{compute_spectrogram, SpectrogramConfig, WindowFunction};
use crate::{Result, SignalError};
use serde::{Deserialize, Serialize};

/// Length of the HAR feature vector produced by [`HarFeatureExtractor`].
pub const N_HAR_FEATURES: usize = 12;

/// Configuration for [`HarFeatureExtractor`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarConfig {
    /// Sampling rate of the CSI stream in Hz (frames per second).
    pub sample_rate: f64,
    /// STFT window size (samples per frame) used for the spectrogram.
    pub window_size: usize,
    /// STFT hop size (step between frames).
    pub hop_size: usize,
    /// Minimum number of time steps required to extract features.
    pub min_window: usize,
}

impl Default for HarConfig {
    fn default() -> Self {
        // Defaults tuned for ESP32-S3 CSI at ~50 Hz over a ~2.5 s window.
        Self {
            sample_rate: 50.0,
            window_size: 32,
            hop_size: 8,
            min_window: 48,
        }
    }
}

/// A fixed-length, deterministic HAR feature vector.
///
/// Field order is stable and mirrored by [`HarFeatures::as_array`]; the
/// classifier depends on this ordering, so do not reorder without retraining.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarFeatures {
    /// Mean broadband amplitude variance over the window (presence cue).
    pub temporal_variance: f64,
    /// Fraction of spectral power in the low motion band (~0.5–2 Hz, breathing/sway).
    pub low_band_power: f64,
    /// Fraction of spectral power in the mid motion band (~2–8 Hz, limb motion).
    pub mid_band_power: f64,
    /// Fraction of spectral power in the high motion band (~8 Hz–Nyquist, fast motion).
    pub high_band_power: f64,
    /// Normalized spectral entropy in `[0,1]` (broadband motion → high entropy).
    pub spectral_entropy: f64,
    /// Spectral centroid in Hz (dominant Doppler frequency).
    pub spectral_centroid: f64,
    /// Mean per-frame "velocity" (frame-to-frame energy change) — simplified BVP.
    pub mean_velocity: f64,
    /// Peak per-frame velocity (fastest energy change in the window).
    pub peak_velocity: f64,
    /// Sustainedness: fraction of frames whose energy exceeds the window mean
    /// (sustained motion like Walking → high; bursty motion → low).
    pub motion_duty_cycle: f64,
    /// Fall transient score: magnitude of a sudden energy spike (0 if none).
    pub transient_spike: f64,
    /// Post-transient stillness: how much energy drops *after* the spike
    /// (a fall = spike followed by stillness, so this is high for falls).
    pub post_spike_stillness: f64,
    /// Overall window energy (log-scaled) — separates `Empty` from everything else.
    pub log_energy: f64,
}

impl HarFeatures {
    /// Flatten into a fixed-length array in stable feature order.
    pub fn as_array(&self) -> [f64; N_HAR_FEATURES] {
        [
            self.temporal_variance,
            self.low_band_power,
            self.mid_band_power,
            self.high_band_power,
            self.spectral_entropy,
            self.spectral_centroid,
            self.mean_velocity,
            self.peak_velocity,
            self.motion_duty_cycle,
            self.transient_spike,
            self.post_spike_stillness,
            self.log_energy,
        ]
    }
}

/// Extracts [`HarFeatures`] from a temporal window of CSI amplitude frames.
#[derive(Debug, Clone)]
pub struct HarFeatureExtractor {
    config: HarConfig,
}

impl HarFeatureExtractor {
    /// Create a new extractor with the given configuration.
    pub fn new(config: HarConfig) -> Self {
        Self { config }
    }

    /// Create an extractor with default configuration.
    pub fn default_config() -> Self {
        Self::new(HarConfig::default())
    }

    /// Access the configuration.
    pub fn config(&self) -> &HarConfig {
        &self.config
    }

    /// Extract features from a window of per-frame subcarrier amplitudes.
    ///
    /// `window[t]` is the amplitude vector (one value per subcarrier) at time
    /// step `t`. All frames should have the same subcarrier count.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] if the window is shorter than
    /// `config.min_window`, frames are empty, or the spectrogram cannot be built.
    pub fn extract(&self, window: &[Vec<f64>]) -> Result<HarFeatures> {
        let n = window.len();
        if n < self.config.min_window {
            return Err(SignalError::FeatureExtraction(format!(
                "window too short: need >= {} frames, got {}",
                self.config.min_window, n
            )));
        }
        if window.iter().any(|f| f.is_empty()) {
            return Err(SignalError::FeatureExtraction(
                "window contains empty frames".to_string(),
            ));
        }

        // Collapse subcarriers → a single mean-amplitude time series.
        let series: Vec<f64> = window
            .iter()
            .map(|frame| frame.iter().sum::<f64>() / frame.len() as f64)
            .collect();

        // Detrend (remove static multipath DC) so we measure *motion*.
        let mean: f64 = series.iter().sum::<f64>() / n as f64;
        let centered: Vec<f64> = series.iter().map(|v| v - mean).collect();

        let temporal_variance = centered.iter().map(|v| v * v).sum::<f64>() / n as f64;
        let log_energy = (temporal_variance + 1e-12).ln();

        // ── Amplitude spectrogram (reuse existing STFT) ──
        let win = self.config.window_size.min(n);
        let spec_cfg = SpectrogramConfig {
            window_size: win,
            hop_size: self.config.hop_size.max(1),
            window_fn: WindowFunction::Hann,
            power: true,
        };
        let spec = compute_spectrogram(&centered, self.config.sample_rate, &spec_cfg)
            .map_err(|e| SignalError::FeatureExtraction(format!("spectrogram: {e}")))?;

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

        // ── Motion / Doppler band powers (fractions of total) ──
        let (mut low, mut mid, mut high) = (0.0, 0.0, 0.0);
        for (f, &p) in bin_power.iter().enumerate() {
            let freq = f as f64 * spec.freq_resolution;
            if freq < 0.5 {
                // DC / sub-band ignored (already detrended).
            } else if freq < 2.0 {
                low += p;
            } else if freq < 8.0 {
                mid += p;
            } else {
                high += p;
            }
        }
        let low_band_power = low / total_power;
        let mid_band_power = mid / total_power;
        let high_band_power = high / total_power;

        // ── Spectral entropy (normalized) and centroid ──
        let spectral_entropy = {
            let mut h = 0.0;
            for &p in &bin_power {
                let prob = p / total_power;
                if prob > 1e-12 {
                    h -= prob * prob.ln();
                }
            }
            // Normalize by log(n_freq) → [0,1].
            let norm = (spec.n_freq as f64).ln().max(1e-9);
            (h / norm).clamp(0.0, 1.0)
        };
        let spectral_centroid = {
            let mut weighted = 0.0;
            for (f, &p) in bin_power.iter().enumerate() {
                weighted += (f as f64 * spec.freq_resolution) * p;
            }
            weighted / total_power
        };

        // ── Velocity profile (simplified Widar BVP): per-frame energy then diff ──
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
        let peak_velocity = velocities.iter().cloned().fold(0.0, f64::max);

        let energy_mean = if frame_energy.is_empty() {
            0.0
        } else {
            frame_energy.iter().sum::<f64>() / frame_energy.len() as f64
        };
        let motion_duty_cycle = if frame_energy.is_empty() {
            0.0
        } else {
            frame_energy.iter().filter(|&&e| e > energy_mean).count() as f64
                / frame_energy.len() as f64
        };

        // ── Fall transient: a sharp spike followed by stillness ──
        let (transient_spike, post_spike_stillness) =
            fall_transient(&frame_energy, energy_mean);

        Ok(HarFeatures {
            temporal_variance,
            low_band_power,
            mid_band_power,
            high_band_power,
            spectral_entropy,
            spectral_centroid,
            mean_velocity,
            peak_velocity,
            motion_duty_cycle,
            transient_spike,
            post_spike_stillness,
            log_energy,
        })
    }
}

/// Detect a fall-style transient: locate the max-energy frame, measure how
/// sharply it spikes above the window mean, and how much energy *drops* in the
/// frames after it (a fall ends in stillness).
///
/// Returns `(spike_score, post_stillness)`, both >= 0.
fn fall_transient(frame_energy: &[f64], energy_mean: f64) -> (f64, f64) {
    if frame_energy.len() < 3 {
        return (0.0, 0.0);
    }
    let denom = energy_mean.max(1e-9);

    // Peak frame.
    let (peak_idx, &peak_val) = frame_energy
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((0, &0.0));

    // Spike score: how far above the mean the peak is (relative).
    let spike_score = ((peak_val - energy_mean) / denom).max(0.0);

    // Post-spike stillness: mean energy after the peak vs. at the peak.
    let after = &frame_energy[(peak_idx + 1).min(frame_energy.len())..];
    let post_stillness = if after.is_empty() {
        0.0
    } else {
        let after_mean = after.iter().sum::<f64>() / after.len() as f64;
        // 1.0 = drops to zero, 0.0 = no drop. Only meaningful with a real spike.
        (1.0 - (after_mean / peak_val.max(1e-9))).clamp(0.0, 1.0)
    };

    (spike_score, post_stillness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn const_window(n: usize, sc: usize, level: f64) -> Vec<Vec<f64>> {
        (0..n).map(|_| vec![level; sc]).collect()
    }

    fn walking_window(n: usize, sc: usize, rate: f64) -> Vec<Vec<f64>> {
        // Sustained 4 Hz amplitude modulation = limb-motion Doppler.
        (0..n)
            .map(|t| {
                let m = 1.0 + 0.5 * (2.0 * PI * 4.0 * t as f64 / rate).sin();
                vec![m; sc]
            })
            .collect()
    }

    #[test]
    fn rejects_short_window() {
        let ex = HarFeatureExtractor::default_config();
        let w = const_window(4, 56, 1.0);
        assert!(ex.extract(&w).is_err());
    }

    #[test]
    fn rejects_empty_frames() {
        let cfg = HarConfig {
            min_window: 4,
            window_size: 4,
            hop_size: 2,
            ..Default::default()
        };
        let ex = HarFeatureExtractor::new(cfg);
        let mut w = const_window(8, 56, 1.0);
        w[2].clear();
        assert!(ex.extract(&w).is_err());
    }

    #[test]
    fn empty_room_has_low_motion() {
        let ex = HarFeatureExtractor::default_config();
        let w = const_window(64, 56, 2.0); // perfectly static
        let f = ex.extract(&w).unwrap();
        assert!(
            f.temporal_variance < 1e-6,
            "static room should have ~0 variance, got {}",
            f.temporal_variance
        );
        assert!(f.mid_band_power.is_finite());
    }

    #[test]
    fn walking_has_more_mid_band_than_empty() {
        let ex = HarFeatureExtractor::default_config();
        let empty = ex.extract(&const_window(64, 56, 2.0)).unwrap();
        let walk = ex.extract(&walking_window(64, 56, 50.0)).unwrap();
        assert!(
            walk.temporal_variance > empty.temporal_variance,
            "walking variance {} should exceed empty {}",
            walk.temporal_variance,
            empty.temporal_variance
        );
    }

    #[test]
    fn feature_array_is_stable_length() {
        let ex = HarFeatureExtractor::default_config();
        let f = ex.extract(&walking_window(64, 56, 50.0)).unwrap();
        assert_eq!(f.as_array().len(), N_HAR_FEATURES);
        assert!(f.as_array().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn fall_transient_detects_spike_then_stillness() {
        // Energy: flat, big spike, then near-zero.
        let mut e = vec![1.0; 10];
        e[5] = 50.0;
        for v in e.iter_mut().skip(6) {
            *v = 0.05;
        }
        let mean = e.iter().sum::<f64>() / e.len() as f64;
        let (spike, still) = fall_transient(&e, mean);
        assert!(spike > 1.0, "expected strong spike, got {spike}");
        assert!(still > 0.8, "expected post-spike stillness, got {still}");
    }
}
