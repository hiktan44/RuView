//! Coarse Human Activity Recognition (HAR) taxonomy.
//!
//! These are the **coarse** activity classes that single-antenna ESP32-S3 CSI
//! can support in-domain (Strohmayer & Kampel 2024, arXiv:2401.01388). They are
//! deliberately *not* fine-grained gestures — fine gestures are handled by the
//! DTW-based [`crate::ruvsense::gesture`] classifier, which is complementary.

use serde::{Deserialize, Serialize};

/// Number of activity classes (used to size fixed arrays in the classifier).
pub const N_ACTIVITIES: usize = 7;

/// Coarse human activities recognizable from single-antenna CSI amplitude.
///
/// Ordering is stable and matches [`Activity::index`] / [`Activity::from_index`]
/// so that serialized model weights stay valid across loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Activity {
    /// No human present / no motion. Low broadband variance, low motion-band
    /// power. The "background" class that anchors confidence.
    Empty,

    /// A person walking through the link. Sustained, broadband Doppler energy
    /// with a moving spectral centroid — the most separable active class.
    Walking,

    /// A person seated and largely still. Slightly elevated variance over
    /// `Empty` (micro-motion, breathing) but no sustained Doppler.
    Sitting,

    /// A person standing and largely still. Like `Sitting` but typically a
    /// different static multipath signature; both are "present, low motion".
    Standing,

    /// A fall: a sudden high-energy transient followed by stillness. The
    /// **highest-value** class for disaster / elderly-care use. Detected by a
    /// sharp energy spike then a drop to near-`Empty` activity.
    Falling,

    /// An arm wave / large upper-body gesture. Moderate, bursty motion-band
    /// energy that is localized in time, unlike the sustained energy of
    /// `Walking`.
    Waving,

    /// Anything that does not match the above with confidence — an explicit
    /// "unknown / out-of-vocabulary" sink so the pipeline never has to guess.
    Other,
}

impl Activity {
    /// All activities in canonical (index) order.
    pub const ALL: [Activity; N_ACTIVITIES] = [
        Activity::Empty,
        Activity::Walking,
        Activity::Sitting,
        Activity::Standing,
        Activity::Falling,
        Activity::Waving,
        Activity::Other,
    ];

    /// Stable class index in `[0, N_ACTIVITIES)`.
    pub fn index(self) -> usize {
        match self {
            Activity::Empty => 0,
            Activity::Walking => 1,
            Activity::Sitting => 2,
            Activity::Standing => 3,
            Activity::Falling => 4,
            Activity::Waving => 5,
            Activity::Other => 6,
        }
    }

    /// Recover an activity from its stable index. Out-of-range maps to `Other`.
    pub fn from_index(idx: usize) -> Activity {
        Activity::ALL.get(idx).copied().unwrap_or(Activity::Other)
    }

    /// Human-readable lowercase label (stable; used in logs and filenames).
    pub fn label(self) -> &'static str {
        match self {
            Activity::Empty => "empty",
            Activity::Walking => "walking",
            Activity::Sitting => "sitting",
            Activity::Standing => "standing",
            Activity::Falling => "falling",
            Activity::Waving => "waving",
            Activity::Other => "other",
        }
    }

    /// Parse a label (case-insensitive) back into an [`Activity`].
    ///
    /// Recognizes a few common synonyms so labeled recordings from different
    /// sources map cleanly (e.g. `"fall"` → `Falling`).
    pub fn from_label(label: &str) -> Option<Activity> {
        match label.trim().to_lowercase().as_str() {
            "empty" | "absent" | "none" => Some(Activity::Empty),
            "walking" | "walk" | "moving" => Some(Activity::Walking),
            "sitting" | "sit" | "seated" => Some(Activity::Sitting),
            "standing" | "stand" => Some(Activity::Standing),
            "falling" | "fall" | "fell" => Some(Activity::Falling),
            "waving" | "wave" | "gesture" => Some(Activity::Waving),
            "other" | "unknown" => Some(Activity::Other),
            _ => None,
        }
    }

    /// Whether this activity represents a human being present in the link.
    pub fn is_present(self) -> bool {
        !matches!(self, Activity::Empty)
    }

    /// Whether this is the safety-critical fall class.
    pub fn is_fall(self) -> bool {
        matches!(self, Activity::Falling)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrip() {
        for a in Activity::ALL {
            assert_eq!(Activity::from_index(a.index()), a);
        }
    }

    #[test]
    fn index_is_dense_and_ordered() {
        for (i, a) in Activity::ALL.iter().enumerate() {
            assert_eq!(a.index(), i);
        }
    }

    #[test]
    fn label_roundtrip() {
        for a in Activity::ALL {
            assert_eq!(Activity::from_label(a.label()), Some(a));
        }
    }

    #[test]
    fn label_synonyms() {
        assert_eq!(Activity::from_label("fall"), Some(Activity::Falling));
        assert_eq!(Activity::from_label("WALK"), Some(Activity::Walking));
        assert_eq!(Activity::from_label("absent"), Some(Activity::Empty));
        assert_eq!(Activity::from_label("nonsense"), None);
    }

    #[test]
    fn presence_and_fall_flags() {
        assert!(!Activity::Empty.is_present());
        assert!(Activity::Walking.is_present());
        assert!(Activity::Falling.is_fall());
        assert!(!Activity::Walking.is_fall());
    }

    #[test]
    fn out_of_range_index_is_other() {
        assert_eq!(Activity::from_index(999), Activity::Other);
    }
}
