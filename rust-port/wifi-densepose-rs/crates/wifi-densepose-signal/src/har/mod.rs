//! Human Activity Recognition (HAR) from single-antenna ESP32-S3 CSI.
//!
//! This module implements **coarse** activity recognition (walking, sitting,
//! falling, …) from a temporal window of CSI amplitude. It is grounded in what
//! actually works on this hardware class — not aspirational claims.
//!
//! # What the literature supports (be honest)
//! - **Strohmayer & Kampel (2024), arXiv:2401.01388** show that an
//!   amplitude-spectrogram + CNN reaches **~86–92% in-domain accuracy** on
//!   exactly this single-antenna ESP32-S3 CSI hardware class. We reuse the same
//!   amplitude-spectrogram representation (via [`crate::spectrogram`]).
//! - **Widar3.0 (Zheng et al., 2019)** introduced the cross-domain Body-coordinate
//!   Velocity Profile (BVP). We borrow a *simplified* velocity-profile idea in
//!   the feature set, but we do **not** claim Widar's multi-link, multi-antenna
//!   cross-domain performance on a single ESP32 antenna.
//! - **CSI-Bench (2025)** and the broader literature consistently report that
//!   single-antenna CSI HAR **degrades sharply cross-subject and cross-room**
//!   without domain adaptation.
//!
//! ## Honesty contract
//! Therefore this module:
//! - Targets **in-domain** deployments (same room, calibrated per site).
//! - Emits a [`HarEstimate::cross_domain_warning`] flag whenever confidence is
//!   low, so the UI can tell the operator the estimate may not generalize.
//! - Ships a rule-based heuristic that works with **zero training**, plus a
//!   trainable softmax model ([`HarClassifier::train`]) for per-site tuning.
//!
//! # Relationship to other modules
//! - This is **coarse activity**, complementary to (not a replacement for) the
//!   fine-grained DTW gesture classifier in [`crate::ruvsense::gesture`].
//! - It reuses [`crate::spectrogram`] for the time-frequency representation.
//!
//! # Minimal usage
//! ```rust,no_run
//! use wifi_densepose_signal::har::{HarPipeline, HarClassifier, PipelineConfig};
//!
//! // Zero-training pipeline (rule-based heuristic):
//! let mut pipeline = HarPipeline::default_heuristic();
//!
//! // Feed CSI amplitude frames (one Vec<f64> of per-subcarrier amplitudes each):
//! let frame: Vec<f64> = vec![1.0; 56];
//! if let Some(estimate) = pipeline.push_frame(frame).unwrap() {
//!     println!(
//!         "activity={:?} confidence={:.2} cross_domain_warning={}",
//!         estimate.activity, estimate.confidence, estimate.cross_domain_warning
//!     );
//! }
//! ```

pub mod classifier;
pub mod features;
pub mod pipeline;
pub mod taxonomy;

pub use classifier::{HarClassifier, HarModel, LabeledWindow};
pub use features::{HarConfig, HarFeatureExtractor, HarFeatures, N_HAR_FEATURES};
pub use pipeline::{HarEstimate, HarPipeline, PipelineConfig};
pub use taxonomy::{Activity, N_ACTIVITIES};
