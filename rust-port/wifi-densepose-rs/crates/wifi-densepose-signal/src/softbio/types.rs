//! Public types for the experimental soft-biometric estimator.
//!
//! These types are deliberately conservative: neutral cluster labels (no
//! "male"/"female"), an always-present reliability marker, and a disclaimer
//! string that travels with every estimate. See the [module docs](crate::softbio)
//! for the full honesty/ethics contract.

use serde::{Deserialize, Serialize};

/// The standing disclaimer attached to **every** [`SoftBioEstimate`].
///
/// It is intentionally `&'static str` so it can never be dropped or replaced by
/// an empty value, and so UIs are forced to surface it alongside any guess.
pub const DISCLAIMER: &str = "EXPERIMENTAL / UNRELIABLE: WiFi-derived soft-biometric guess. \
There is no robust cross-environment WiFi gender result; this is per-cohort \
overfit nearest-cluster matching, NOT a measurement of gender. Class A/B are \
arbitrary enrolled clusters, not ground truth. Inferring a protected attribute \
about non-consenting people raises GDPR / EU-AI-Act concerns. Do not use for any \
real decision.";

/// Reliability marker carried by every estimate.
///
/// There is currently no non-experimental variant on purpose: there is no
/// validated, generalizable WiFi gender estimator to promote one to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Reliability {
    /// Research-only, unvalidated, must not drive real decisions. The only
    /// reliability level this module ever emits.
    Experimental,
}

impl Reliability {
    /// Stable lowercase label (for logs / UI badges).
    pub fn label(self) -> &'static str {
        match self {
            Reliability::Experimental => "experimental",
        }
    }
}

/// A deliberately neutral soft-biometric class guess.
///
/// **The labels are `A` / `B`, not `male` / `female`.** `A` and `B` are simply
/// whichever two clusters were enrolled by the operator; they carry no inherent
/// meaning and are **not** gender ground truth. [`GenderGuess::Unknown`] is the
/// default and the only value ever returned when the feature is disabled or the
/// classifier has not been enrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GenderGuess {
    /// No guess. Returned when the feature is disabled, the classifier is
    /// unenrolled, the window is unusable, or confidence is otherwise undefined.
    Unknown,
    /// Arbitrary enrolled cluster "A" (no inherent meaning, not gender truth).
    A,
    /// Arbitrary enrolled cluster "B" (no inherent meaning, not gender truth).
    B,
}

impl GenderGuess {
    /// All concrete (non-`Unknown`) enrolled cluster labels in stable order.
    pub const ENROLLED: [GenderGuess; 2] = [GenderGuess::A, GenderGuess::B];

    /// Stable cluster index for the two enrolled clusters. `Unknown` has no
    /// index and maps to `None`.
    pub fn index(self) -> Option<usize> {
        match self {
            GenderGuess::Unknown => None,
            GenderGuess::A => Some(0),
            GenderGuess::B => Some(1),
        }
    }

    /// Recover a cluster from its stable index. Out-of-range maps to `Unknown`.
    pub fn from_index(idx: usize) -> GenderGuess {
        match idx {
            0 => GenderGuess::A,
            1 => GenderGuess::B,
            _ => GenderGuess::Unknown,
        }
    }

    /// Stable lowercase label (used in logs and filenames). Neutral by design.
    pub fn label(self) -> &'static str {
        match self {
            GenderGuess::Unknown => "unknown",
            GenderGuess::A => "class_a",
            GenderGuess::B => "class_b",
        }
    }

    /// Parse a neutral label back into a [`GenderGuess`].
    ///
    /// Only neutral cluster labels are accepted. Gendered words like `"male"` /
    /// `"female"` are **rejected** (`None`) so this type cannot silently acquire
    /// gendered semantics.
    pub fn from_label(label: &str) -> Option<GenderGuess> {
        match label.trim().to_lowercase().as_str() {
            "unknown" | "none" => Some(GenderGuess::Unknown),
            "class_a" | "a" | "cluster_a" => Some(GenderGuess::A),
            "class_b" | "b" | "cluster_b" => Some(GenderGuess::B),
            _ => None,
        }
    }
}

/// Opt-in gate for the experimental soft-biometric estimator.
///
/// This is the **only** switch that lets the classifier emit anything other than
/// [`GenderGuess::Unknown`]. It **defaults to `enabled: false`** so the feature is
/// never active unless an operator explicitly and knowingly turns it on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SoftBioConfig {
    /// Master opt-in switch. **Defaults to `false`.** While `false`, every call
    /// to [`super::SoftBioClassifier::classify`] returns `Unknown` with
    /// confidence `0.0`, regardless of training state.
    pub enabled: bool,
}

impl Default for SoftBioConfig {
    fn default() -> Self {
        // OFF by default. This default is load-bearing for the ethics contract.
        Self { enabled: false }
    }
}

impl SoftBioConfig {
    /// Construct a config with the feature explicitly enabled.
    ///
    /// Naming this `enabled()` (rather than allowing `enabled: true` to be set
    /// implicitly) forces the opt-in to be intentional and greppable in callers.
    pub fn enabled() -> Self {
        Self { enabled: true }
    }
}

/// The result of an experimental soft-biometric classification.
///
/// Always carries a [`Reliability::Experimental`] marker and a non-empty
/// [`SoftBioEstimate::disclaimer`]; both are present even for `Unknown` results
/// so that no downstream consumer can render a guess without the warning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoftBioEstimate {
    /// The neutral cluster guess (or `Unknown`).
    pub guess: GenderGuess,
    /// Confidence in `[0,1]`. `0.0` for `Unknown`. Intentionally reported
    /// honestly — small enrolled cohorts give low, untrustworthy confidence.
    pub confidence: f64,
    /// Always [`Reliability::Experimental`].
    pub reliability: Reliability,
    /// Always-present, non-empty disclaimer (see [`DISCLAIMER`]).
    pub disclaimer: &'static str,
}

impl SoftBioEstimate {
    /// The canonical "no guess" estimate: `Unknown`, confidence `0.0`, with the
    /// experimental marker and disclaimer attached. Used whenever the feature is
    /// disabled, unenrolled, or the input is unusable.
    pub fn unknown() -> Self {
        Self {
            guess: GenderGuess::Unknown,
            confidence: 0.0,
            reliability: Reliability::Experimental,
            disclaimer: DISCLAIMER,
        }
    }

    /// Build an estimate for an enrolled cluster, clamping confidence to `[0,1]`
    /// and always attaching the experimental marker and disclaimer.
    pub fn guessed(guess: GenderGuess, confidence: f64) -> Self {
        match guess {
            GenderGuess::Unknown => Self::unknown(),
            concrete => Self {
                guess: concrete,
                confidence: confidence.clamp(0.0, 1.0),
                reliability: Reliability::Experimental,
                disclaimer: DISCLAIMER,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_to_disabled() {
        assert!(!SoftBioConfig::default().enabled);
        assert!(SoftBioConfig::enabled().enabled);
    }

    #[test]
    fn unknown_estimate_is_honest() {
        let e = SoftBioEstimate::unknown();
        assert_eq!(e.guess, GenderGuess::Unknown);
        assert_eq!(e.confidence, 0.0);
        assert_eq!(e.reliability, Reliability::Experimental);
        assert!(!e.disclaimer.is_empty());
    }

    #[test]
    fn guessed_clamps_confidence_and_keeps_disclaimer() {
        let e = SoftBioEstimate::guessed(GenderGuess::A, 5.0);
        assert_eq!(e.guess, GenderGuess::A);
        assert_eq!(e.confidence, 1.0);
        assert!(!e.disclaimer.is_empty());
        assert_eq!(e.reliability, Reliability::Experimental);
    }

    #[test]
    fn guessed_unknown_collapses_to_unknown() {
        let e = SoftBioEstimate::guessed(GenderGuess::Unknown, 0.9);
        assert_eq!(e.guess, GenderGuess::Unknown);
        assert_eq!(e.confidence, 0.0);
    }

    #[test]
    fn cluster_index_roundtrip() {
        for g in GenderGuess::ENROLLED {
            let idx = g.index().expect("enrolled clusters have indices");
            assert_eq!(GenderGuess::from_index(idx), g);
        }
        assert_eq!(GenderGuess::Unknown.index(), None);
        assert_eq!(GenderGuess::from_index(99), GenderGuess::Unknown);
    }

    #[test]
    fn labels_are_neutral_and_reject_gendered_words() {
        assert_eq!(GenderGuess::A.label(), "class_a");
        assert_eq!(GenderGuess::B.label(), "class_b");
        assert_eq!(GenderGuess::from_label("A"), Some(GenderGuess::A));
        // Gendered words are explicitly rejected.
        assert_eq!(GenderGuess::from_label("male"), None);
        assert_eq!(GenderGuess::from_label("female"), None);
    }

    #[test]
    fn disclaimer_constant_is_non_empty() {
        assert!(!DISCLAIMER.is_empty());
        assert!(DISCLAIMER.to_lowercase().contains("experimental"));
    }
}
