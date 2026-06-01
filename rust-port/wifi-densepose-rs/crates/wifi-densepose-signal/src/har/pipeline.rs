//! High-level streaming HAR pipeline.
//!
//! [`HarPipeline`] maintains a sliding window of CSI amplitude frames, runs the
//! [`HarFeatureExtractor`] + [`HarClassifier`] when the window is full, and
//! emits an [`HarEstimate`] carrying the predicted [`Activity`], a confidence,
//! and — honestly — a `cross_domain_warning` flag.
//!
//! ## Why the cross-domain warning matters
//! Single-antenna ESP32-S3 CSI HAR reaches ~85–92% **in-domain** (same
//! subject, same room) but degrades sharply cross-subject / cross-room without
//! domain adaptation (Strohmayer & Kampel 2024; CSI-Bench; Widar3.0). When
//! confidence is low we set `cross_domain_warning = true` so the UI can tell
//! the operator the estimate may not generalize, rather than presenting a
//! falsely-confident label.

use super::classifier::HarClassifier;
use super::features::{HarConfig, HarFeatureExtractor};
use super::taxonomy::Activity;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Tunable parameters for [`HarPipeline`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Number of CSI frames held in the sliding window.
    pub window_len: usize,
    /// How many new frames to advance before re-classifying (stride).
    pub hop_len: usize,
    /// Confidence below which `cross_domain_warning` is raised.
    pub low_confidence_threshold: f64,
    /// Feature-extractor configuration.
    pub feature_config: HarConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        let feature_config = HarConfig::default();
        Self {
            window_len: 128,
            hop_len: 32,
            low_confidence_threshold: 0.6,
            feature_config,
        }
    }
}

/// A single HAR estimate emitted by the pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarEstimate {
    /// Predicted coarse activity.
    pub activity: Activity,
    /// Classifier confidence in `[0,1]`.
    pub confidence: f64,
    /// True when confidence is below threshold: the estimate may not generalize
    /// across subjects/rooms. Surfaced to the UI for honest reporting.
    pub cross_domain_warning: bool,
}

/// Streaming HAR pipeline over CSI amplitude frames.
#[derive(Debug)]
pub struct HarPipeline {
    config: PipelineConfig,
    extractor: HarFeatureExtractor,
    classifier: HarClassifier,
    window: VecDeque<Vec<f64>>,
    /// Frames pushed since the last classification (drives the hop/stride).
    frames_since_emit: usize,
}

impl HarPipeline {
    /// Build a pipeline from a config and a classifier (trained or heuristic).
    pub fn new(config: PipelineConfig, classifier: HarClassifier) -> Self {
        let extractor = HarFeatureExtractor::new(config.feature_config.clone());
        let window = VecDeque::with_capacity(config.window_len);
        Self {
            config,
            extractor,
            classifier,
            window,
            frames_since_emit: 0,
        }
    }

    /// Build a pipeline with default config and the zero-training heuristic.
    pub fn default_heuristic() -> Self {
        Self::new(PipelineConfig::default(), HarClassifier::new_heuristic())
    }

    /// Number of frames currently buffered in the sliding window.
    pub fn buffered(&self) -> usize {
        self.window.len()
    }

    /// Replace the classifier (e.g. after loading a trained model).
    pub fn set_classifier(&mut self, classifier: HarClassifier) {
        self.classifier = classifier;
    }

    /// Push one CSI amplitude frame (one value per subcarrier).
    ///
    /// Returns `Some(estimate)` when the window is full and the hop boundary is
    /// reached; otherwise `None`. Returns an error only if feature extraction
    /// of a full window fails.
    pub fn push_frame(&mut self, frame: Vec<f64>) -> Result<Option<HarEstimate>> {
        self.window.push_back(frame);
        while self.window.len() > self.config.window_len {
            self.window.pop_front();
        }
        self.frames_since_emit += 1;

        let ready = self.window.len() >= self.config.window_len
            && self.frames_since_emit >= self.config.hop_len;
        if !ready {
            return Ok(None);
        }
        self.frames_since_emit = 0;
        Ok(Some(self.classify_current_window()?))
    }

    /// Classify the current window immediately (ignoring the hop schedule).
    ///
    /// Useful for flushing a final estimate at end-of-stream.
    ///
    /// # Errors
    /// Propagates feature-extraction errors (e.g. window still too short).
    pub fn classify_now(&self) -> Result<HarEstimate> {
        self.classify_current_window()
    }

    fn classify_current_window(&self) -> Result<HarEstimate> {
        let frames: Vec<Vec<f64>> = self.window.iter().cloned().collect();
        let features = self.extractor.extract(&frames)?;
        let (activity, confidence) = self.classifier.classify(&features);
        let cross_domain_warning = confidence < self.config.low_confidence_threshold;
        Ok(HarEstimate {
            activity,
            confidence,
            cross_domain_warning,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn cfg(window_len: usize, hop_len: usize) -> PipelineConfig {
        PipelineConfig {
            window_len,
            hop_len,
            low_confidence_threshold: 0.6,
            feature_config: HarConfig {
                sample_rate: 50.0,
                window_size: 16,
                hop_size: 4,
                min_window: 32,
            },
        }
    }

    #[test]
    fn no_estimate_until_window_full() {
        let mut p = HarPipeline::new(cfg(64, 16), HarClassifier::new_heuristic());
        for _ in 0..63 {
            assert!(p.push_frame(vec![2.0; 56]).unwrap().is_none());
        }
        // 64th frame fills window but hop counter already >= hop_len → emits.
        let est = p.push_frame(vec![2.0; 56]).unwrap();
        assert!(est.is_some());
    }

    #[test]
    fn sliding_window_caps_buffer() {
        let mut p = HarPipeline::new(cfg(32, 8), HarClassifier::new_heuristic());
        for _ in 0..100 {
            let _ = p.push_frame(vec![1.0; 56]).unwrap();
        }
        assert_eq!(p.buffered(), 32);
    }

    #[test]
    fn hop_controls_emit_cadence() {
        let mut p = HarPipeline::new(cfg(32, 8), HarClassifier::new_heuristic());
        let mut emits = 0;
        for _ in 0..64 {
            if p.push_frame(vec![2.0; 56]).unwrap().is_some() {
                emits += 1;
            }
        }
        // 64 frames, window 32, hop 8 → first emit at 32, then every 8 → ~5.
        assert!(emits >= 3, "expected several emits, got {emits}");
    }

    #[test]
    fn empty_room_classified_as_empty_high_confidence() {
        let mut p = HarPipeline::new(cfg(64, 16), HarClassifier::new_heuristic());
        let mut last = None;
        for _ in 0..80 {
            if let Some(e) = p.push_frame(vec![2.0; 56]).unwrap() {
                last = Some(e);
            }
        }
        let est = last.expect("should have emitted");
        assert_eq!(est.activity, Activity::Empty);
        assert!(!est.cross_domain_warning, "empty room should be confident");
    }

    #[test]
    fn low_confidence_raises_cross_domain_warning() {
        // Force a low-confidence regime via a high threshold.
        let mut config = cfg(64, 16);
        config.low_confidence_threshold = 0.99;
        let mut p = HarPipeline::new(config, HarClassifier::new_heuristic());
        let mut last = None;
        for t in 0..80 {
            // Ambiguous slow drift → heuristic returns low confidence.
            let v = 2.0 + 0.001 * (2.0 * PI * 0.3 * t as f64 / 50.0).sin();
            if let Some(e) = p.push_frame(vec![v; 56]).unwrap() {
                last = Some(e);
            }
        }
        let est = last.expect("should have emitted");
        assert!(
            est.cross_domain_warning,
            "confidence {} below 0.99 must warn",
            est.confidence
        );
    }

    #[test]
    fn classify_now_flushes_estimate() {
        let mut p = HarPipeline::new(cfg(48, 16), HarClassifier::new_heuristic());
        for _ in 0..48 {
            let _ = p.push_frame(vec![2.0; 56]).unwrap();
        }
        let est = p.classify_now().unwrap();
        assert_eq!(est.activity, Activity::Empty);
    }
}
