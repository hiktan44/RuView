//! Trainable, dependency-free soft-biometric nearest-centroid estimator.
//!
//! Mirrors the [`crate::har::classifier`] style: a serde-serializable model with
//! per-class centroids, global feature normalization, and `save`/`load`. Unlike
//! HAR, it has **no heuristic fallback** — with no enrollment it deliberately
//! returns [`GenderGuess::Unknown`] with confidence `0.0`. It never guesses by
//! default.
//!
//! Everything is additionally gated behind [`SoftBioConfig::enabled`], which
//! defaults to `false`. See the [module docs](crate::softbio) for the full
//! honesty/ethics contract: this is per-cluster matching over operator-enrolled
//! clusters, not a gender measurement.

use super::features::{GaitFeatures, N_GAIT_FEATURES};
use super::types::{GenderGuess, SoftBioConfig, SoftBioEstimate};
use crate::{Result, SignalError};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Number of enrolled clusters (`A` and `B`).
const N_CLUSTERS: usize = 2;

/// A labeled enrollment window: extracted gait features + the neutral cluster
/// the operator assigned to it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabeledGaitWindow {
    /// The extracted gait feature vector.
    pub features: GaitFeatures,
    /// The operator-assigned neutral cluster (`A` or `B`). `Unknown` windows are
    /// rejected at enrollment time — you cannot enroll an unlabeled example.
    pub label: GenderGuess,
}

/// Enrolled centroids and normalization for the soft-biometric estimator.
///
/// An untrained model (`enrolled_windows == 0`) carries no usable centroids and
/// causes [`SoftBioClassifier::classify`] to return `Unknown`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftBioModel {
    /// Per-cluster centroids `[N_CLUSTERS][N_GAIT_FEATURES]` in normalized space.
    pub centroids: Vec<[f64; N_GAIT_FEATURES]>,
    /// Global per-feature mean used for normalization.
    pub global_mean: [f64; N_GAIT_FEATURES],
    /// Global per-feature stddev used for normalization.
    pub global_std: [f64; N_GAIT_FEATURES],
    /// Number of windows the model was enrolled on (0 = unenrolled).
    pub enrolled_windows: usize,
    /// Enrollment-set (in-sample) separation accuracy in `[0,1]`. Reported
    /// honestly; high in-sample numbers are expected and do **not** imply
    /// generalization.
    pub enrollment_accuracy: f64,
    /// Model schema version.
    pub version: u32,
}

impl Default for SoftBioModel {
    fn default() -> Self {
        Self {
            centroids: vec![[0.0; N_GAIT_FEATURES]; N_CLUSTERS],
            global_mean: [0.0; N_GAIT_FEATURES],
            global_std: [1.0; N_GAIT_FEATURES],
            enrolled_windows: 0,
            enrollment_accuracy: 0.0,
            version: 1,
        }
    }
}

impl SoftBioModel {
    /// Whether this model has been enrolled from data.
    pub fn is_enrolled(&self) -> bool {
        self.enrolled_windows > 0
    }

    /// Normalize a raw feature vector with the stored global statistics.
    fn normalize(&self, raw: &[f64; N_GAIT_FEATURES]) -> [f64; N_GAIT_FEATURES] {
        let mut x = [0.0f64; N_GAIT_FEATURES];
        for i in 0..N_GAIT_FEATURES {
            x[i] = (raw[i] - self.global_mean[i]) / (self.global_std[i] + 1e-9);
        }
        x
    }

    /// Nearest-centroid guess + a deliberately conservative confidence.
    ///
    /// Confidence is derived from the *relative* distance margin between the two
    /// clusters and capped low: even a clean in-sample separation should never
    /// look like a trustworthy result.
    fn nearest(&self, raw: &[f64; N_GAIT_FEATURES]) -> (GenderGuess, f64) {
        let x = self.normalize(raw);
        let mut dists = [0.0f64; N_CLUSTERS];
        for c in 0..N_CLUSTERS {
            let mut d = 0.0;
            for i in 0..N_GAIT_FEATURES {
                d += (x[i] - self.centroids[c][i]).powi(2);
            }
            dists[c] = d.sqrt();
        }
        let (best, &best_d) = dists
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, &0.0));
        let other_d = dists[1 - best];

        // Margin in [0,1]: 0 when equidistant, →1 when the other cluster is far.
        let margin = if (best_d + other_d) <= 1e-9 {
            0.0
        } else {
            ((other_d - best_d) / (other_d + best_d)).clamp(0.0, 1.0)
        };
        // Cap confidence hard: this is unreliable by construction.
        let confidence = (margin * 0.5).clamp(0.0, 0.5);
        (GenderGuess::from_index(best), confidence)
    }
}

/// High-level experimental soft-biometric classifier.
///
/// Holds a [`SoftBioModel`] and the opt-in [`SoftBioConfig`]. Construction does
/// **not** enable anything: with the default config disabled, or with no
/// enrollment, [`SoftBioClassifier::classify`] always returns `Unknown`.
#[derive(Debug, Clone)]
pub struct SoftBioClassifier {
    model: SoftBioModel,
    config: SoftBioConfig,
}

impl Default for SoftBioClassifier {
    fn default() -> Self {
        Self {
            model: SoftBioModel::default(),
            config: SoftBioConfig::default(),
        }
    }
}

impl SoftBioClassifier {
    /// Create an unenrolled classifier with the given (opt-in) configuration.
    ///
    /// Pass [`SoftBioConfig::default`] (disabled) unless an operator has
    /// explicitly and knowingly opted in.
    pub fn new(config: SoftBioConfig) -> Self {
        Self {
            model: SoftBioModel::default(),
            config,
        }
    }

    /// Wrap an existing enrolled model with the given config.
    pub fn from_model(model: SoftBioModel, config: SoftBioConfig) -> Self {
        Self { model, config }
    }

    /// Borrow the underlying model (e.g. to inspect enrollment accuracy).
    pub fn model(&self) -> &SoftBioModel {
        &self.model
    }

    /// Borrow the opt-in configuration.
    pub fn config(&self) -> &SoftBioConfig {
        &self.config
    }

    /// Replace the configuration (e.g. to flip the opt-in gate).
    pub fn set_config(&mut self, config: SoftBioConfig) {
        self.config = config;
    }

    /// Whether this classifier would currently emit a non-`Unknown` guess:
    /// requires both the opt-in gate **and** an enrolled model.
    pub fn is_active(&self) -> bool {
        self.config.enabled && self.model.is_enrolled()
    }

    /// Classify a CSI walking window into a [`SoftBioEstimate`].
    ///
    /// Returns `Unknown` (confidence `0.0`) when **any** of these hold:
    /// - the opt-in gate [`SoftBioConfig::enabled`] is `false` (the default);
    /// - the model has not been enrolled;
    /// - the features cannot be extracted from `gait`.
    ///
    /// The returned estimate **always** carries the experimental marker and a
    /// non-empty disclaimer, even when `Unknown`.
    pub fn classify(&self, gait: &GaitFeatures) -> Result<SoftBioEstimate> {
        // Hard opt-in gate: off → always Unknown, never guess.
        if !self.config.enabled {
            return Ok(SoftBioEstimate::unknown());
        }
        // No enrollment → never guess.
        if !self.model.is_enrolled() {
            return Ok(SoftBioEstimate::unknown());
        }
        let (guess, confidence) = self.model.nearest(&gait.as_array());
        Ok(SoftBioEstimate::guessed(guess, confidence))
    }

    /// Enroll (train) per-cluster centroids from labeled gait windows.
    ///
    /// On success the classifier gains usable centroids; it still only emits
    /// guesses when [`SoftBioConfig::enabled`] is `true`.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] if `windows` is empty, if any
    /// window is labeled [`GenderGuess::Unknown`] (unlabeled enrollment is
    /// rejected), or if any cluster received no examples.
    pub fn train(&mut self, windows: &[LabeledGaitWindow]) -> Result<()> {
        if windows.is_empty() {
            return Err(SignalError::FeatureExtraction(
                "cannot enroll soft-bio classifier on empty dataset".to_string(),
            ));
        }
        if windows.iter().any(|w| w.label == GenderGuess::Unknown) {
            return Err(SignalError::FeatureExtraction(
                "cannot enroll an Unknown-labeled window; assign cluster A or B".to_string(),
            ));
        }

        let n = windows.len();
        let samples: Vec<([f64; N_GAIT_FEATURES], usize)> = windows
            .iter()
            .filter_map(|w| w.label.index().map(|c| (w.features.as_array(), c)))
            .collect();

        // Global normalization stats.
        let mut global_mean = [0.0f64; N_GAIT_FEATURES];
        for (x, _) in &samples {
            for i in 0..N_GAIT_FEATURES {
                global_mean[i] += x[i];
            }
        }
        for m in global_mean.iter_mut() {
            *m /= n as f64;
        }
        let mut global_std = [0.0f64; N_GAIT_FEATURES];
        for (x, _) in &samples {
            for i in 0..N_GAIT_FEATURES {
                global_std[i] += (x[i] - global_mean[i]).powi(2);
            }
        }
        for s in global_std.iter_mut() {
            *s = (*s / n as f64).sqrt().max(1e-9);
        }

        // Per-cluster centroids in normalized space.
        let mut centroids = vec![[0.0f64; N_GAIT_FEATURES]; N_CLUSTERS];
        let mut counts = [0usize; N_CLUSTERS];
        for (x, c) in &samples {
            counts[*c] += 1;
            for i in 0..N_GAIT_FEATURES {
                let z = (x[i] - global_mean[i]) / (global_std[i] + 1e-9);
                centroids[*c][i] += z;
            }
        }
        for c in 0..N_CLUSTERS {
            if counts[c] == 0 {
                return Err(SignalError::FeatureExtraction(format!(
                    "cluster {} received no enrollment examples; both A and B are required",
                    GenderGuess::from_index(c).label()
                )));
            }
            for v in centroids[c].iter_mut() {
                *v /= counts[c] as f64;
            }
        }

        let candidate = SoftBioModel {
            centroids,
            global_mean,
            global_std,
            enrolled_windows: n,
            enrollment_accuracy: 0.0,
            version: 1,
        };

        // In-sample separation accuracy (honest: high here means overfit, not skill).
        let mut correct = 0;
        for (x, target) in &samples {
            let (guess, _) = candidate.nearest(x);
            if guess.index() == Some(*target) {
                correct += 1;
            }
        }
        let enrollment_accuracy = correct as f64 / n as f64;

        self.model = SoftBioModel {
            enrollment_accuracy,
            ..candidate
        };
        Ok(())
    }

    /// Save the underlying model as JSON. The opt-in config is **not** persisted
    /// — re-enabling the feature is always an explicit, separate decision.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] on serialization or I/O failure.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.model)
            .map_err(|e| SignalError::FeatureExtraction(format!("serialize soft-bio model: {e}")))?;
        std::fs::write(path, json)
            .map_err(|e| SignalError::FeatureExtraction(format!("write soft-bio model: {e}")))
    }

    /// Load a model from JSON. The returned classifier is **disabled by default**
    /// ([`SoftBioConfig::default`]); the caller must explicitly opt in to emit
    /// guesses.
    ///
    /// # Errors
    /// Returns [`SignalError::FeatureExtraction`] on I/O or deserialization failure.
    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| SignalError::FeatureExtraction(format!("read soft-bio model: {e}")))?;
        let model: SoftBioModel = serde_json::from_str(&json)
            .map_err(|e| SignalError::FeatureExtraction(format!("parse soft-bio model: {e}")))?;
        Ok(Self::from_model(model, SoftBioConfig::default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn feat(
        cadence: f64,
        stride: f64,
        limb: f64,
        centroid: f64,
        spread: f64,
        rhythm: f64,
        vel: f64,
    ) -> GaitFeatures {
        GaitFeatures {
            cadence_hz: cadence,
            stride_band_power: stride,
            limb_band_power: limb,
            spectral_centroid: centroid,
            spectral_spread: spread,
            rhythm_regularity: rhythm,
            mean_velocity: vel,
            log_energy: vel.max(1e-6).ln(),
        }
    }

    // Two clearly-separated synthetic gait clusters (arbitrary, not gendered).
    fn cluster_a() -> GaitFeatures {
        feat(2.0, 0.6, 0.2, 2.5, 0.2, 0.8, 1.0)
    }
    fn cluster_b() -> GaitFeatures {
        feat(6.0, 0.2, 0.6, 6.0, 0.5, 0.6, 3.0)
    }

    fn enrollment_set() -> Vec<LabeledGaitWindow> {
        let mut data = Vec::new();
        for k in 0..20 {
            let j = k as f64 * 1e-6;
            data.push(LabeledGaitWindow {
                features: feat(2.0 + j, 0.6, 0.2, 2.5, 0.2, 0.8, 1.0),
                label: GenderGuess::A,
            });
            data.push(LabeledGaitWindow {
                features: feat(6.0 + j, 0.2, 0.6, 6.0, 0.5, 0.6, 3.0),
                label: GenderGuess::B,
            });
        }
        data
    }

    #[test]
    fn disabled_by_default_returns_unknown() {
        let clf = SoftBioClassifier::default();
        assert!(!clf.config().enabled);
        let est = clf.classify(&cluster_a()).unwrap();
        assert_eq!(est.guess, GenderGuess::Unknown);
        assert_eq!(est.confidence, 0.0);
        assert!(!est.disclaimer.is_empty());
    }

    #[test]
    fn enabled_but_unenrolled_returns_unknown() {
        // Opt-in on, but never enrolled → still never guesses.
        let clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        let est = clf.classify(&cluster_a()).unwrap();
        assert_eq!(est.guess, GenderGuess::Unknown);
        assert!(!est.disclaimer.is_empty());
    }

    #[test]
    fn enrolled_but_disabled_still_returns_unknown() {
        // Enrolled, but gate off → the gate alone suppresses guessing.
        let mut clf = SoftBioClassifier::new(SoftBioConfig::default());
        clf.train(&enrollment_set()).unwrap();
        assert!(clf.model().is_enrolled());
        assert!(!clf.is_active());
        let est = clf.classify(&cluster_a()).unwrap();
        assert_eq!(est.guess, GenderGuess::Unknown);
    }

    #[test]
    fn enabled_and_enrolled_separates_two_clusters() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        clf.train(&enrollment_set()).unwrap();
        assert!(clf.is_active());
        // Proves it is just per-cluster matching of what we enrolled.
        let a = clf.classify(&cluster_a()).unwrap();
        let b = clf.classify(&cluster_b()).unwrap();
        assert_eq!(a.guess, GenderGuess::A);
        assert_eq!(b.guess, GenderGuess::B);
    }

    #[test]
    fn confidence_is_low_and_honest() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        clf.train(&enrollment_set()).unwrap();
        let est = clf.classify(&cluster_a()).unwrap();
        // Confidence is capped well below "trustworthy" by construction.
        assert!(
            est.confidence <= 0.5,
            "confidence must be capped low, got {}",
            est.confidence
        );
    }

    #[test]
    fn disclaimer_present_in_every_output() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        clf.train(&enrollment_set()).unwrap();
        for f in [cluster_a(), cluster_b()] {
            let est = clf.classify(&f).unwrap();
            assert!(!est.disclaimer.is_empty());
            assert!(est.disclaimer.to_lowercase().contains("experimental"));
        }
    }

    #[test]
    fn train_rejects_empty_dataset() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        assert!(clf.train(&[]).is_err());
    }

    #[test]
    fn train_rejects_unknown_labels() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        let data = vec![LabeledGaitWindow {
            features: cluster_a(),
            label: GenderGuess::Unknown,
        }];
        assert!(clf.train(&data).is_err());
    }

    #[test]
    fn train_rejects_single_cluster() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        // Only cluster A enrolled → B has no examples.
        let data: Vec<_> = (0..10)
            .map(|_| LabeledGaitWindow {
                features: cluster_a(),
                label: GenderGuess::A,
            })
            .collect();
        assert!(clf.train(&data).is_err());
    }

    #[test]
    fn save_load_roundtrip_preserves_decision() {
        let mut clf = SoftBioClassifier::new(SoftBioConfig::enabled());
        clf.train(&enrollment_set()).unwrap();

        let mut path = std::env::temp_dir();
        path.push(format!("softbio_model_test_{}.json", std::process::id()));
        clf.save(&path).unwrap();

        let mut loaded = SoftBioClassifier::load(&path).unwrap();
        // Loaded classifier is disabled by default — opt-in is never persisted.
        assert!(!loaded.config().enabled);
        assert_eq!(loaded.classify(&cluster_a()).unwrap().guess, GenderGuess::Unknown);

        // Re-enable explicitly, then the decision matches the original.
        loaded.set_config(SoftBioConfig::enabled());
        assert!(loaded.model().is_enrolled());
        assert_eq!(
            loaded.classify(&cluster_a()).unwrap().guess,
            clf.classify(&cluster_a()).unwrap().guess
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_rejects_garbage() {
        let mut path = std::env::temp_dir();
        path.push(format!("softbio_bad_{}.json", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "not json").unwrap();
        assert!(SoftBioClassifier::load(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
