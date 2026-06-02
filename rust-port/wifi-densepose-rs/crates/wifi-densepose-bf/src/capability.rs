//! Sensing capability advertisement and negotiation (802.11bf-aligned).
//!
//! Before sensing, 802.11bf peers exchange capability elements and agree on a
//! common set of parameters. This module models that exchange:
//! [`SensingCapability`] is what one device advertises, and
//! [`SensingCapability::negotiate`] intersects two advertisements into a
//! [`NegotiatedSession`] that both peers can support.

use crate::error::BfError;
use crate::roles::SensingRole;
use serde::{Deserialize, Serialize};

/// Frequency band / measurement family supported for sensing.
///
/// 802.11bf splits sensing by band: sub-7 GHz uses **CSI-based** measurements
/// (per-subcarrier channel estimates), while the 60 GHz DMG/EDMG band uses
/// **beam-based** measurements (beam refinement / sector sweep).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensingBand {
    /// Sub-7 GHz, CSI-based sensing (the band RuView's ESP32 hardware uses).
    Sub7GHz,
    /// 60 GHz DMG/EDMG, beam-based sensing.
    Dmg60GHz,
}

/// What a single device advertises it can do as a sensing participant.
///
/// This mirrors the 802.11bf sensing capability negotiation. Values are
/// per-device maxima/supports; negotiation takes the conservative intersection
/// of two such advertisements.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensingCapability {
    /// Bands this device can sense on (must be non-empty to be useful).
    pub supported_bands: Vec<SensingBand>,
    /// Number of antennas available for sensing.
    pub antenna_count: u8,
    /// Subcarrier resolution: number of subcarriers reported per measurement
    /// (for CSI-based bands). Beam-based bands may report a small value.
    pub subcarrier_resolution: u16,
    /// Maximum sustained measurement rate this device supports, in Hz.
    pub max_measurement_rate_hz: f32,
    /// Whether this device supports trigger-based (initiator-polled)
    /// measurement scheduling.
    pub trigger_based_supported: bool,
    /// Sensing management roles this device can take.
    pub roles_supported: Vec<SensingRole>,
}

impl SensingCapability {
    /// Builds a capability advertisement.
    pub fn new(
        supported_bands: Vec<SensingBand>,
        antenna_count: u8,
        subcarrier_resolution: u16,
        max_measurement_rate_hz: f32,
        trigger_based_supported: bool,
        roles_supported: Vec<SensingRole>,
    ) -> Self {
        SensingCapability {
            supported_bands,
            antenna_count,
            subcarrier_resolution,
            max_measurement_rate_hz,
            trigger_based_supported,
            roles_supported,
        }
    }

    /// Negotiates a session by intersecting two capability advertisements.
    ///
    /// Mirrors 802.11bf setup: the agreed session uses the **common** bands,
    /// the **minimum** of antenna counts / subcarrier resolution / measurement
    /// rate, trigger support only if **both** support it, and the **common**
    /// roles. The two peers must additionally be able to take *complementary*
    /// roles (one initiator, one responder).
    ///
    /// # Errors
    ///
    /// Returns [`BfError::NegotiationFailed`] if there is no common band, no
    /// common role, or the peers cannot form an initiator/responder pair.
    pub fn negotiate(
        local: &SensingCapability,
        remote: &SensingCapability,
    ) -> Result<NegotiatedSession, BfError> {
        let bands: Vec<SensingBand> = local
            .supported_bands
            .iter()
            .copied()
            .filter(|b| remote.supported_bands.contains(b))
            .collect();
        if bands.is_empty() {
            return Err(BfError::NegotiationFailed(
                "no common sensing band".to_string(),
            ));
        }

        let roles: Vec<SensingRole> = local
            .roles_supported
            .iter()
            .copied()
            .filter(|r| remote.roles_supported.contains(r))
            .collect();
        if roles.is_empty() {
            return Err(BfError::NegotiationFailed(
                "no common sensing role".to_string(),
            ));
        }

        // A working session needs one initiator and one responder. Confirm the
        // pair can cover both ends across the two peers.
        let pair_ok = (local.roles_supported.contains(&SensingRole::Initiator)
            && remote.roles_supported.contains(&SensingRole::Responder))
            || (local.roles_supported.contains(&SensingRole::Responder)
                && remote.roles_supported.contains(&SensingRole::Initiator));
        if !pair_ok {
            return Err(BfError::NegotiationFailed(
                "peers cannot form an initiator/responder pair".to_string(),
            ));
        }

        Ok(NegotiatedSession {
            bands,
            antenna_count: local.antenna_count.min(remote.antenna_count),
            subcarrier_resolution: local
                .subcarrier_resolution
                .min(remote.subcarrier_resolution),
            measurement_rate_hz: local
                .max_measurement_rate_hz
                .min(remote.max_measurement_rate_hz),
            trigger_based: local.trigger_based_supported && remote.trigger_based_supported,
            roles,
        })
    }
}

/// The agreed parameters after intersecting two [`SensingCapability`]s.
///
/// This is the 802.11bf-aligned analogue of a completed measurement setup: the
/// parameters every subsequent sounding and report in the session must honor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NegotiatedSession {
    /// Bands both peers support.
    pub bands: Vec<SensingBand>,
    /// Antenna count both peers can sustain (the minimum of the two).
    pub antenna_count: u8,
    /// Subcarrier resolution both peers can sustain (the minimum of the two).
    pub subcarrier_resolution: u16,
    /// Agreed measurement rate in Hz (the minimum of the two maxima).
    pub measurement_rate_hz: f32,
    /// Whether trigger-based scheduling is available (both peers support it).
    pub trigger_based: bool,
    /// Roles both peers can take.
    pub roles: Vec<SensingRole>,
}

impl NegotiatedSession {
    /// Returns `true` if the session can carry CSI-based (sub-7 GHz) sensing.
    pub fn supports_csi(&self) -> bool {
        self.bands.contains(&SensingBand::Sub7GHz)
    }
}
