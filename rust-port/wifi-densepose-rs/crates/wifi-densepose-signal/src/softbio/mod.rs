//! EXPERIMENTAL, opt-in soft-biometric (gait-derived "gender") estimator.
//!
//! # ⚠️ READ THIS FIRST — Honesty & Ethics Contract
//!
//! **This feature is EXPERIMENTAL, unreliable, opt-in only, and OFF by default.**
//! It must never be presented as a trustworthy classifier, and it must never be
//! used to make any real decision about any person.
//!
//! ## The science does not support reliable WiFi "gender" inference
//! - There is **no robust, reproducible cross-environment result** for inferring
//!   gender (or any protected soft-biometric) from WiFi CSI. Published numbers
//!   that look high are, almost without exception, **per-cohort overfitting**:
//!   the model memorizes the few specific people in one room on one day.
//! - WiFi "gender" classifiers ride entirely on **gait/body-scattering
//!   correlates** (stride cadence, body cross-section, multipath signature). These
//!   correlates vary *within* any gender group far more than they separate groups,
//!   and they **do not generalize** across subjects, rooms, hardware, or time
//!   (the same cross-domain degradation documented for CSI HAR — see
//!   [`crate::har`]). Change the room or the person and accuracy collapses to
//!   chance.
//! - In short: at best this is a **per-cluster nearest-centroid matcher** over a
//!   handful of enrolled examples. It is *not* measuring gender; it is matching
//!   whichever arbitrary clusters you enrolled.
//!
//! ## Inferring a protected attribute is ethically and legally fraught
//! - Gender is a **special-category / protected attribute**. Inferring it about
//!   **non-consenting** people who merely walk through a WiFi link raises serious
//!   issues under the **EU GDPR** (Art. 9 special-category data; lawful basis and
//!   explicit consent) and the **EU AI Act** (biometric categorization of
//!   sensitive attributes is restricted/likely prohibited in many contexts).
//! - Gender is not binary, and any A/B split imposed by a sensor is a crude,
//!   harmful abstraction. This module therefore **refuses to use the labels
//!   "male"/"female" in its core types** — see [`GenderGuess`].
//!
//! ## What this module actually does
//! - Provides a deterministic, dependency-free, gait-feature **nearest-centroid
//!   estimator** ([`SoftBioClassifier`]) that you must **explicitly enroll** with
//!   your own labeled walking windows. It learns *clusters you defined*, nothing
//!   more.
//! - Gates everything behind [`SoftBioConfig::enabled`], which **defaults to
//!   `false`**. With the gate off — or with no enrollment — it always returns
//!   [`GenderGuess::Unknown`] with confidence `0.0`. It never guesses by default.
//! - Every estimate carries a [`Reliability::Experimental`] marker and a
//!   non-empty [`SoftBioEstimate::disclaimer`] string so downstream code and UIs
//!   cannot present a result without the warning attached.
//!
//! If you are reading this to ship a "gender from WiFi" product: don't. This
//! exists for research reproducibility and to make the unreliability explicit.
//!
//! # Minimal usage
//! ```rust,no_run
//! use wifi_densepose_signal::softbio::{
//!     GaitFeatureExtractor, GenderGuess, SoftBioClassifier, SoftBioConfig,
//! };
//!
//! // OFF by default → always Unknown, confidence 0.
//! let off = SoftBioClassifier::new(SoftBioConfig::default());
//! let extractor = GaitFeatureExtractor::default_config();
//! let frame: Vec<f64> = vec![1.0; 56];
//! let window: Vec<Vec<f64>> = vec![frame; 64];
//! let gait = extractor.extract(&window).unwrap();
//! let est = off.classify(&gait).unwrap();
//! assert_eq!(est.guess, GenderGuess::Unknown);
//! assert!(!est.disclaimer.is_empty());
//! ```

pub mod classifier;
pub mod features;
pub mod types;

pub use classifier::{LabeledGaitWindow, SoftBioClassifier, SoftBioModel};
pub use features::{GaitConfig, GaitFeatureExtractor, GaitFeatures, N_GAIT_FEATURES};
pub use types::{GenderGuess, Reliability, SoftBioConfig, SoftBioEstimate, DISCLAIMER};
