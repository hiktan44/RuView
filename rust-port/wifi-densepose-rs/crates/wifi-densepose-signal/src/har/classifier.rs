//! Trainable, dependency-free HAR classifier.
//!
//! Mirrors the established [`adaptive_classifier`] style in
//! `wifi-densepose-sensing-server`: a softmax logistic-regression layer over a
//! fixed feature vector, with per-class centroids, global feature
//! normalization, serde (JSON) serialization, and `save`/`load`.
//!
//! It additionally provides a **rule-based heuristic default** so the
//! classifier produces sensible [`Activity`] estimates with *zero* training,
//! and a [`HarClassifier::train`] method to fit a softmax model from labeled
//! windows when data is available.
//!
//! [`adaptive_classifier`]: https://docs.rs/wifi-densepose-sensing-server

use super::features::{HarFeatures, N_HAR_FEATURES};
use super::taxonomy::{Activity, N_ACTIVITIES};
use crate::{Result, SignalError};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A labeled training window: extracted features + ground-truth activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledWindow {
    /// The extracted feature vector.
    pub features: HarFeatures,
    /// The ground-truth activity for this window.
    pub label: Activity,
}

/// Trained softmax weights and normalization for the HAR classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarModel {
    /// Per-class weights `[N_ACTIVITIES][N_HAR_FEATURES + 1]` (last entry = bias).
    pub weights: Vec<[f64; N_HAR_FEATURES + 1]>,
    /// Global per-feature mean used for normalization.
    pub global_mean: [f64; N_HAR_FEATURES],
    /// Global per-feature stddev used for normalization.
    pub global_std: [f64; N_HAR_FEATURES],
    /// Number of windows the model was trained on (0 = heuristic only).
    pub trained_windows: usize,
    /// Training-set accuracy in `[0,1]`.
    pub training_accuracy: f64,
    /// Model schema version.
    pub version: u32,
}

impl Default for HarModel {
    fn default() -> Self {
        Self {
            weights: vec![[0.0; N_HAR_FEATURES + 1]; N_ACTIVITIES],
            global_mean: [0.0; N_HAR_FEATURES],
            global_std: [1.0; N_HAR_FEATURES],
            trained_windows: 0,
            training_accuracy: 0.0,
            version: 1,
        }
    }
}

impl HarModel {
    /// Whether this model has been fit from data (vs. heuristic fallback).
    pub fn is_trained(&self) -> bool {
        self.trained_windows > 0
    }

    /// Softmax probabilities over all activities for a raw feature vector.
    fn probabilities(&self, raw: &[f64; N_HAR_FEATURES]) -> [f64; N_ACTIVITIES] {
        let mut x = [0.0f64; N_HAR_FEATURES];
        for i in 0..N_HAR_FEATURES {
            x[i] = (raw[i] - self.global_mean[i]) / (self.global_std[i] + 1e-9);
        }
        let mut logits = [0.0f64; N_ACTIVITIES];
        for c in 0..N_ACTIVITIES {
            let w = &self.weights[c];
            let mut z = w[N_HAR_FEATURES];
            for i in 0..N_HAR_FEATURES {
                z += w[i] * x[i];
            }
            logits[c] = z;
        }
        let max_l = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let exp_sum: f64 = logits.iter().map(|z| (z - max_l).exp()).sum::<f64>().max(1e-12);
        let mut probs = [0.0f64; N_ACTIVITIES];
        for c in 0..N_ACTIVITIES {
            probs[c] = (logits[c] - max_l).exp() / exp_sum;
        }
        probs
    }
}

/// High-level HAR classifier wrapping a [`HarModel`] (or heuristic fallback).
#[derive(Debug, Clone, Default)]
pub struct HarClassifier {
    model: HarModel,
}

impl HarClassifier {
    /// Create a classifier backed only by the rule-based heuristic (untrained).
    pub fn new_heuristic() -> Self {
        Self {
            model: HarModel::default(),
        }
    }

    /// Wrap an existing trained model.
    pub fn from_model(model: HarModel) -> Self {
        Self { model }
    }

    /// Borrow the underlying model (e.g. to inspect training accuracy).
    pub fn model(&self) -> &HarModel {
        &self.model
    }

    /// Classify a feature vector into `(Activity, confidence)`.
    ///
    /// Uses the trained softmax model if available; otherwise falls back to the
    /// deterministic [`heuristic_classify`] rules so it works untrained.
    pub fn classify(&self, features: &HarFeatures) -> (Activity, f64) {
        if self.model.is_trained() {
            let probs = self.model.probabilities(&features.as_array());
            let (best, &p) = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((Activity::Other.index(), &0.0));
            (Activity::from_index(best), p)
        } else {
            heuristic_classify(features)
        }
    }

    /// Fit a softmax model from labeled windows via deterministic SGD.
    ///
    /// On success the classifier switches from heuristic to the trained model.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] if `windows` is empty.
    pub fn train(&mut self, windows: &[LabeledWindow]) -> Result<()> {
        if windows.is_empty() {
            return Err(SignalError::FeatureExtraction(
                "cannot train HAR classifier on empty dataset".to_string(),
            ));
        }
        let n = windows.len();
        let samples: Vec<([f64; N_HAR_FEATURES], usize)> = windows
            .iter()
            .map(|w| (w.features.as_array(), w.label.index()))
            .collect();

        // Global normalization stats.
        let mut global_mean = [0.0f64; N_HAR_FEATURES];
        for (x, _) in &samples {
            for i in 0..N_HAR_FEATURES {
                global_mean[i] += x[i];
            }
        }
        for m in global_mean.iter_mut() {
            *m /= n as f64;
        }
        let mut global_std = [0.0f64; N_HAR_FEATURES];
        for (x, _) in &samples {
            for i in 0..N_HAR_FEATURES {
                global_std[i] += (x[i] - global_mean[i]).powi(2);
            }
        }
        for s in global_std.iter_mut() {
            *s = (*s / n as f64).sqrt().max(1e-9);
        }

        // Normalize.
        let mut norm: Vec<([f64; N_HAR_FEATURES], usize)> = samples
            .iter()
            .map(|(x, y)| {
                let mut z = [0.0f64; N_HAR_FEATURES];
                for i in 0..N_HAR_FEATURES {
                    z[i] = (x[i] - global_mean[i]) / (global_std[i] + 1e-9);
                }
                (z, *y)
            })
            .collect();

        // Deterministic SGD (LCG shuffle, matching adaptive_classifier style).
        let mut weights = vec![[0.0f64; N_HAR_FEATURES + 1]; N_ACTIVITIES];
        let lr = 0.1;
        let epochs = 300;
        let batch_size = 16;
        let mut rng_state: u64 = 42;
        let mut rng_next = move || -> u64 {
            rng_state = rng_state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            rng_state >> 33
        };

        for epoch in 0..epochs {
            for i in (1..norm.len()).rev() {
                let j = (rng_next() as usize) % (i + 1);
                norm.swap(i, j);
            }
            for start in (0..norm.len()).step_by(batch_size) {
                let end = (start + batch_size).min(norm.len());
                let batch = &norm[start..end];
                let mut grad = vec![[0.0f64; N_HAR_FEATURES + 1]; N_ACTIVITIES];
                for (x, target) in batch {
                    let probs = softmax_logits(&weights, x);
                    for c in 0..N_ACTIVITIES {
                        let delta = probs[c] - if c == *target { 1.0 } else { 0.0 };
                        for i in 0..N_HAR_FEATURES {
                            grad[c][i] += delta * x[i];
                        }
                        grad[c][N_HAR_FEATURES] += delta;
                    }
                }
                let bs = batch.len() as f64;
                let cur_lr = lr * (1.0 - epoch as f64 / epochs as f64);
                for c in 0..N_ACTIVITIES {
                    for i in 0..=N_HAR_FEATURES {
                        weights[c][i] -= cur_lr * grad[c][i] / bs;
                    }
                }
            }
        }

        // Training accuracy.
        let mut correct = 0;
        for (x, target) in &norm {
            let probs = softmax_logits(&weights, x);
            let pred = probs
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            if pred == *target {
                correct += 1;
            }
        }

        self.model = HarModel {
            weights,
            global_mean,
            global_std,
            trained_windows: n,
            training_accuracy: correct as f64 / n as f64,
            version: 1,
        };
        Ok(())
    }

    /// Save the underlying model as JSON.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] on serialization or I/O failure.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.model)
            .map_err(|e| SignalError::FeatureExtraction(format!("serialize HAR model: {e}")))?;
        std::fs::write(path, json)
            .map_err(|e| SignalError::FeatureExtraction(format!("write HAR model: {e}")))
    }

    /// Load a model from JSON, returning a ready-to-use classifier.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] on I/O or deserialization failure.
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| SignalError::FeatureExtraction(format!("read HAR model: {e}")))?;
        let model: HarModel = serde_json::from_str(&json)
            .map_err(|e| SignalError::FeatureExtraction(format!("parse HAR model: {e}")))?;
        Ok(Self::from_model(model))
    }
}

/// Softmax over per-class linear logits for an (already normalized) vector.
fn softmax_logits(
    weights: &[[f64; N_HAR_FEATURES + 1]],
    x: &[f64; N_HAR_FEATURES],
) -> [f64; N_ACTIVITIES] {
    let mut logits = [0.0f64; N_ACTIVITIES];
    for c in 0..N_ACTIVITIES {
        let mut z = weights[c][N_HAR_FEATURES];
        for i in 0..N_HAR_FEATURES {
            z += weights[c][i] * x[i];
        }
        logits[c] = z;
    }
    let max_l = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exp_sum: f64 = logits.iter().map(|z| (z - max_l).exp()).sum::<f64>().max(1e-12);
    let mut probs = [0.0f64; N_ACTIVITIES];
    for c in 0..N_ACTIVITIES {
        probs[c] = (logits[c] - max_l).exp() / exp_sum;
    }
    probs
}

/// Deterministic rule-based classifier used when no model is trained.
///
/// Thresholds are conservative and derived from the qualitative cues described
/// in the [`super::features`] docs. They give a usable zero-training baseline;
/// the trained softmax model is expected to outperform it in-domain.
pub fn heuristic_classify(f: &HarFeatures) -> (Activity, f64) {
    // 1. Fall: strong transient spike followed by stillness dominates.
    if f.transient_spike > 1.5 && f.post_spike_stillness > 0.6 {
        let conf = (0.5 + 0.25 * f.post_spike_stillness).min(0.95);
        return (Activity::Falling, conf);
    }

    // 2. Empty: almost no motion energy.
    if f.temporal_variance < 1e-4 && f.mid_band_power < 0.1 {
        return (Activity::Empty, 0.8);
    }

    // 3. Walking: sustained, broadband motion (high duty cycle + mid/high band).
    if f.motion_duty_cycle > 0.4 && (f.mid_band_power + f.high_band_power) > 0.45 {
        let conf = (0.45 + 0.4 * f.motion_duty_cycle).min(0.9);
        return (Activity::Walking, conf);
    }

    // 4. Waving: bursty mid-band motion that is NOT sustained.
    if f.mid_band_power > 0.3 && f.motion_duty_cycle <= 0.4 && f.peak_velocity > f.mean_velocity * 2.0 {
        return (Activity::Waving, 0.55);
    }

    // 5. Sitting vs Standing: present but low motion. Use low-band (sway)
    //    energy as a weak tie-breaker; both are low-confidence in-domain.
    if f.temporal_variance >= 1e-4 && (f.mid_band_power + f.high_band_power) < 0.3 {
        if f.low_band_power > 0.5 {
            return (Activity::Standing, 0.45);
        }
        return (Activity::Sitting, 0.45);
    }

    // 6. Fallback.
    (Activity::Other, 0.3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn feat(
        var: f64,
        low: f64,
        mid: f64,
        high: f64,
        duty: f64,
        spike: f64,
        still: f64,
    ) -> HarFeatures {
        HarFeatures {
            temporal_variance: var,
            low_band_power: low,
            mid_band_power: mid,
            high_band_power: high,
            spectral_entropy: 0.5,
            spectral_centroid: 3.0,
            mean_velocity: 1.0,
            peak_velocity: 2.0,
            motion_duty_cycle: duty,
            transient_spike: spike,
            post_spike_stillness: still,
            log_energy: var.ln(),
        }
    }

    fn empty() -> HarFeatures {
        feat(1e-6, 0.05, 0.02, 0.01, 0.0, 0.0, 0.0)
    }
    fn walking() -> HarFeatures {
        feat(0.5, 0.1, 0.4, 0.3, 0.7, 0.0, 0.0)
    }
    fn falling() -> HarFeatures {
        feat(0.3, 0.1, 0.3, 0.2, 0.3, 3.0, 0.85)
    }

    #[test]
    fn heuristic_separates_classes() {
        assert_eq!(heuristic_classify(&empty()).0, Activity::Empty);
        assert_eq!(heuristic_classify(&walking()).0, Activity::Walking);
        assert_eq!(heuristic_classify(&falling()).0, Activity::Falling);
    }

    #[test]
    fn untrained_classifier_uses_heuristic() {
        let clf = HarClassifier::new_heuristic();
        assert!(!clf.model().is_trained());
        assert_eq!(clf.classify(&falling()).0, Activity::Falling);
    }

    #[test]
    fn train_separates_empty_walking_falling() {
        // Build a labeled set with clear separation.
        let mut data = Vec::new();
        for k in 0..20 {
            let jitter = k as f64 * 1e-7;
            data.push(LabeledWindow {
                features: feat(1e-6 + jitter, 0.05, 0.02, 0.01, 0.0, 0.0, 0.0),
                label: Activity::Empty,
            });
            data.push(LabeledWindow {
                features: feat(0.5 + jitter, 0.1, 0.4, 0.3, 0.7, 0.0, 0.0),
                label: Activity::Walking,
            });
            data.push(LabeledWindow {
                features: feat(0.3 + jitter, 0.1, 0.3, 0.2, 0.3, 3.0, 0.85),
                label: Activity::Falling,
            });
        }
        let mut clf = HarClassifier::new_heuristic();
        clf.train(&data).unwrap();
        assert!(clf.model().is_trained());
        assert!(
            clf.model().training_accuracy > 0.9,
            "expected >90% train acc, got {}",
            clf.model().training_accuracy
        );
        assert_eq!(clf.classify(&empty()).0, Activity::Empty);
        assert_eq!(clf.classify(&walking()).0, Activity::Walking);
        assert_eq!(clf.classify(&falling()).0, Activity::Falling);
    }

    #[test]
    fn train_rejects_empty_dataset() {
        let mut clf = HarClassifier::new_heuristic();
        assert!(clf.train(&[]).is_err());
    }

    #[test]
    fn train_save_load_roundtrip() {
        let mut data = Vec::new();
        for _ in 0..10 {
            data.push(LabeledWindow {
                features: empty(),
                label: Activity::Empty,
            });
            data.push(LabeledWindow {
                features: walking(),
                label: Activity::Walking,
            });
        }
        let mut clf = HarClassifier::new_heuristic();
        clf.train(&data).unwrap();

        let mut path = std::env::temp_dir();
        path.push(format!("har_model_test_{}.json", std::process::id()));
        clf.save(&path).unwrap();

        let loaded = HarClassifier::load(&path).unwrap();
        assert!(loaded.model().is_trained());
        assert_eq!(
            loaded.model().trained_windows,
            clf.model().trained_windows
        );
        // Same prediction after round-trip.
        assert_eq!(
            loaded.classify(&walking()).0,
            clf.classify(&walking()).0
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_garbage() {
        let mut path = std::env::temp_dir();
        path.push(format!("har_bad_{}.json", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "not json").unwrap();
        assert!(HarClassifier::load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
