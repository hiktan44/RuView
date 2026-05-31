//! Hardware-free WiFi sensing via 802.11ac/ax Beamforming Feedback Information.
//!
//! # BFI vs CSI
//!
//! Most WiFi sensing relies on **Channel State Information (CSI)** — the
//! complex per-subcarrier channel estimate. Extracting CSI normally requires
//! special firmware (e.g. Atheros/Intel CSI tools, ESP32 CSI mode), which is a
//! deployment barrier.
//!
//! **Beamforming Feedback Information (BFI)** sidesteps this. In 802.11ac/ax,
//! when an access point sounds the channel, each station replies with a *VHT
//! Compressed Beamforming* action frame containing the beamforming matrix
//! **V** encoded as quantised Givens-rotation angles (φ and ψ). Crucially this
//! report is sent **in the clear** — it is not protected by link-layer
//! encryption — so any NIC in monitor mode within radio range can capture it.
//! The angles are a compressed, lower-dimensional view of the same channel,
//! and they vary as people and objects move through the environment. That is
//! enough to derive **presence** and a **gait** identity signature without any
//! firmware modification or raw CSI.
//!
//! This crate implements PRD requirement **FR-1.2 (Dual-Mode sensing)**: run
//! on CSI when it is available, and transparently fall back to firmware-free
//! BFI otherwise (see [`dual_mode`]).
//!
//! # Notebook / laptop sniffing workflow
//!
//! 1. Put a laptop/notebook Wi-Fi NIC into **monitor mode** on the target
//!    channel (e.g. `airmon-ng`, `iw dev … set type monitor`, or a USB
//!    monitor-capable adapter).
//! 2. Capture frames with libpcap / `tcpdump` / Wireshark and isolate the
//!    802.11 **VHT action** frames whose body begins with category `0x15` and
//!    action `0x00`.
//! 3. Strip the radiotap + 802.11 MAC header and feed the **action-frame body**
//!    bytes to [`frame::parse_vht_beamforming_report`].
//! 4. Buffer a window of [`BfiReport`]s, run [`features::extract_features`],
//!    then [`presence::PresenceClassifier`] and/or [`gait::GaitProfiler`].
//!
//! # Privacy note
//!
//! Because BFI is transmitted unencrypted, it can be captured passively by a
//! third party without joining the network. Treat captured BFI as sensitive:
//! it leaks occupancy and biometric (gait) information about people who never
//! consented to being sensed. Deployments must obtain consent and comply with
//! local surveillance and data-protection law.
//!
//! # End-to-end example
//!
//! ```ignore
//! use wifi_densepose_bfi::*;
//! let report = frame::parse_vht_beamforming_report(&action_frame_body)?;
//! let window: Vec<BfiReport> = capture_window();
//! let cfg = BfiConfig::default();
//! let feats = features::extract_features(&window, &cfg)?;
//! let presence = presence::PresenceClassifier::new(cfg).classify(&feats);
//! ```

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod dual_mode;
pub mod features;
pub mod frame;
pub mod gait;
pub mod presence;
pub mod types;

pub use dual_mode::{DualModeSelector, SensingMode};
pub use features::{extract_features, BfiFeatures};
// `BitReader` stays available as `frame::BitReader`; it is an implementation
// helper, so it is not re-exported at the crate root.
pub use frame::{parse_vht_beamforming_report, parse_vht_beamforming_report_with_subcarriers};
pub use gait::{GaitDescriptor, GaitProfiler, GaitRegistry, DEFAULT_GAIT_THRESHOLD, DESCRIPTOR_LEN};
pub use presence::{PresenceClassifier, PresenceResult, PresenceState};
pub use types::{
    Bandwidth, BfiConfig, BfiError, BfiReport, MimoControl, SubcarrierAngles,
};

#[cfg(test)]
mod end_to_end_tests {
    use super::*;
    use crate::frame::build_synthetic_frame;

    fn control() -> MimoControl {
        MimoControl {
            nc: 2,
            nr: 3,
            bandwidth: Bandwidth::Bw20,
            codebook_large: false,
            mu_feedback: false,
        }
    }

    /// Synthesize a VHT report, parse it, build a window, and run the whole
    /// presence + gait pipeline.
    #[test]
    fn synthesize_parse_window_features_presence_gait() {
        let ctrl = control();
        let subcarriers = 8;

        // Build a window where the codeword varies over time to mimic motion.
        let mut window = Vec::new();
        for t in 0..32u32 {
            let codeword = 4 + (t % 7); // small varying codeword
            let frame = build_synthetic_frame(&ctrl, subcarriers, codeword);
            let report =
                parse_vht_beamforming_report_with_subcarriers(&frame, subcarriers).unwrap();
            assert_eq!(report.subcarriers.len(), subcarriers);
            window.push(report);
        }

        let cfg = BfiConfig::default();
        let feats = extract_features(&window, &cfg).unwrap();
        assert!(feats.total_variance > 0.0);

        let presence = PresenceClassifier::new(cfg).classify(&feats);
        // A varying channel must register at least presence.
        assert_ne!(presence.state, PresenceState::Absent);

        let descriptor = GaitProfiler::new().profile(&window).unwrap();
        assert_eq!(descriptor.values.len(), DESCRIPTOR_LEN);
        assert!((descriptor.cosine_similarity(&descriptor) - 1.0).abs() < 1e-9);
    }
}
