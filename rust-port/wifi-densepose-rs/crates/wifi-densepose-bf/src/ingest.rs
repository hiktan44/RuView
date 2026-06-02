//! Adapter from today's vendor CSI to the 802.11bf-aligned schema.
//!
//! # The gap, stated honestly
//!
//! RuView's ESP32-S3 nodes emit **vendor CSI** — amplitude+phase vectors per
//! subcarrier, with a node id and a timestamp. This is **not** an 802.11bf
//! measurement report: there is no negotiated measurement setup on the wire, no
//! standardized report frame, and the ESP32 is not a certified 802.11bf sensing
//! radio. The CSI is a vendor extension, not standardized sensing feedback.
//!
//! [`from_vendor_csi`] is a **forward-compatible wrapper**: it dresses vendor
//! CSI in the [`SensingMeasurementReport`] schema so the rest of the pipeline
//! can speak the bf data model now. When real 802.11bf hardware appears, a
//! genuine bf-report parser replaces this adapter and emits the *same*
//! [`SensingMeasurementReport`] type — so downstream code is untouched. The
//! source is swapped, the pipeline is not.

use crate::capability::SensingBand;
use crate::error::BfError;
use crate::measurement::{
    CsiPayload, SensingMeasurementReport, SoundingType, SubcarrierMeasurement,
};
use crate::roles::{MeasurementSetupId, SensingRole};

/// A vendor CSI frame as produced by RuView's ESP32-S3 nodes today.
///
/// Deliberately minimal and vendor-shaped: parallel amplitude/phase vectors,
/// an explicit subcarrier count, a node identifier, and a capture timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct VendorCsiFrame {
    /// Originating node identifier (becomes the responder setup id).
    pub node_id: u32,
    /// Per-subcarrier amplitudes (linear magnitude).
    pub amplitudes: Vec<f32>,
    /// Per-subcarrier phases in radians; must match `amplitudes` in length.
    pub phases: Vec<f32>,
    /// Declared subcarrier count; must match the vector lengths.
    pub subcarrier_count: u16,
    /// Capture timestamp in microseconds.
    pub timestamp_us: u64,
    /// Number of antennas the frame represents.
    pub antenna_count: u8,
}

/// Wraps a vendor CSI frame into an 802.11bf-aligned measurement report.
///
/// The node is treated as a sensing **responder** on the **sub-7 GHz**
/// CSI-based band, with [`SoundingType::TrafficDerived`] (vendor CSI is not a
/// dedicated bf NDP sounding). The resulting report is validated before return.
///
/// # Errors
///
/// Returns [`BfError::Ingest`] if `amplitudes` and `phases` differ in length or
/// disagree with `subcarrier_count`, and [`BfError::InvalidReport`] (via
/// [`SensingMeasurementReport::new`]) if the resulting report is malformed.
pub fn from_vendor_csi(frame: &VendorCsiFrame) -> Result<SensingMeasurementReport, BfError> {
    if frame.amplitudes.len() != frame.phases.len() {
        return Err(BfError::Ingest(format!(
            "amplitude/phase length mismatch: {} vs {}",
            frame.amplitudes.len(),
            frame.phases.len()
        )));
    }
    if frame.amplitudes.len() != frame.subcarrier_count as usize {
        return Err(BfError::Ingest(format!(
            "vector length {} disagrees with subcarrier_count {}",
            frame.amplitudes.len(),
            frame.subcarrier_count
        )));
    }

    let subcarriers: Vec<SubcarrierMeasurement> = frame
        .amplitudes
        .iter()
        .zip(frame.phases.iter())
        .map(|(&a, &p)| SubcarrierMeasurement::new(a, p))
        .collect();

    SensingMeasurementReport::new(
        MeasurementSetupId::new(frame.node_id),
        SensingRole::Responder,
        SensingBand::Sub7GHz,
        frame.timestamp_us,
        SoundingType::TrafficDerived,
        frame.antenna_count,
        CsiPayload::new(subcarriers),
    )
}
