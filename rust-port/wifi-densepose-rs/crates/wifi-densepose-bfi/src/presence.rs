//! Three-state presence classification from BFI window features.
//!
//! Mirrors the v1 Python rule logic:
//! * angle variance below `presence_threshold` → [`PresenceState::Absent`]
//! * variance above threshold but motion-band power below `motion_threshold`
//!   → [`PresenceState::PresentStill`]
//! * motion-band power at/above `motion_threshold` → [`PresenceState::Active`]

use crate::features::BfiFeatures;
use crate::types::BfiConfig;
use serde::{Deserialize, Serialize};

/// Coarse occupancy state derived from a single analysis window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PresenceState {
    /// No occupant detected (channel essentially static).
    Absent,
    /// An occupant is present but not moving (e.g. seated, sleeping).
    PresentStill,
    /// An occupant is actively moving (walking, gesturing).
    Active,
}

/// The classifier verdict plus a `[0, 1]` confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresenceResult {
    /// Classified state.
    pub state: PresenceState,
    /// Heuristic confidence in `[0, 1]`.
    pub confidence: f64,
}

/// Threshold-based presence classifier.
#[derive(Debug, Clone, Copy)]
pub struct PresenceClassifier {
    config: BfiConfig,
}

impl PresenceClassifier {
    /// Build a classifier from a [`BfiConfig`].
    pub fn new(config: BfiConfig) -> Self {
        Self { config }
    }

    /// Classify a window's [`BfiFeatures`] into a [`PresenceResult`].
    pub fn classify(&self, features: &BfiFeatures) -> PresenceResult {
        let presence_t = self.config.presence_threshold;
        let motion_t = self.config.motion_threshold;
        let variance = features.total_variance;
        let motion = features.motion_band_power;

        if variance < presence_t {
            // Confidence scales with how far below threshold we are.
            let conf = clamp01(1.0 - variance / presence_t.max(f64::EPSILON));
            return PresenceResult {
                state: PresenceState::Absent,
                confidence: conf,
            };
        }
        if motion < motion_t {
            // Present but still: confident when variance clears presence but
            // motion is well below the motion threshold.
            let above_presence = clamp01((variance - presence_t) / presence_t.max(f64::EPSILON));
            let below_motion = clamp01(1.0 - motion / motion_t.max(f64::EPSILON));
            return PresenceResult {
                state: PresenceState::PresentStill,
                confidence: clamp01(0.5 * above_presence + 0.5 * below_motion),
            };
        }
        let conf = clamp01(0.5 + 0.5 * (motion - motion_t) / motion_t.max(f64::EPSILON));
        PresenceResult {
            state: PresenceState::Active,
            confidence: conf,
        }
    }
}

/// Clamp a value into the closed unit interval.
fn clamp01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feats(total_variance: f64, motion: f64) -> BfiFeatures {
        BfiFeatures {
            frames: 32,
            per_dim_variance: vec![total_variance],
            total_variance,
            motion_band_power: motion,
            breathing_band_power: 0.0,
        }
    }

    #[test]
    fn absent_when_variance_low() {
        let c = PresenceClassifier::new(BfiConfig::default());
        let r = c.classify(&feats(0.0, 0.0));
        assert_eq!(r.state, PresenceState::Absent);
        assert!(r.confidence > 0.9);
    }

    #[test]
    fn present_still_when_variance_high_motion_low() {
        let cfg = BfiConfig::default();
        let c = PresenceClassifier::new(cfg);
        let r = c.classify(&feats(cfg.presence_threshold * 3.0, 0.0));
        assert_eq!(r.state, PresenceState::PresentStill);
    }

    #[test]
    fn active_when_motion_high() {
        let cfg = BfiConfig::default();
        let c = PresenceClassifier::new(cfg);
        let r = c.classify(&feats(cfg.presence_threshold * 5.0, cfg.motion_threshold * 2.0));
        assert_eq!(r.state, PresenceState::Active);
        assert!(r.confidence >= 0.5);
    }
}
