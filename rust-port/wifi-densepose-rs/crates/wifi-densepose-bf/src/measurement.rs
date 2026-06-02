//! Sensing measurement reports and trigger-based scheduling (802.11bf-aligned).
//!
//! [`SensingMeasurementReport`] mirrors what an 802.11bf measurement-report
//! frame conveys: which setup it belongs to, the role/band/timestamp context,
//! the sounding type, and a CSI payload. [`MeasurementSchedule`] models the
//! trigger-based phase where an initiator polls responders at a negotiated
//! rate — the same shape as RuView's TDM aggregator triggering ESP32 nodes.

use crate::capability::SensingBand;
use crate::error::BfError;
use crate::roles::{MeasurementSetupId, SensingRole};
use serde::{Deserialize, Serialize};

/// How the sounding underlying a measurement was obtained.
///
/// 802.11bf distinguishes sounding mechanisms; sub-7 GHz CSI sensing can use a
/// dedicated sounding NDP or piggyback on regular traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SoundingType {
    /// Dedicated Null Data Packet sounding (trigger-based measurement).
    NdpSounding,
    /// Measurement derived from existing data-frame traffic (no dedicated NDP).
    TrafficDerived,
}

/// One subcarrier's channel measurement, in amplitude/phase form.
///
/// 802.11bf sub-7 GHz reports are CSI-based: per-subcarrier complex channel
/// estimates. We store amplitude and phase (radians) rather than re/im so that
/// vendor CSI (which often arrives as amplitude+phase) maps in directly; a
/// complex value is recoverable as `amplitude * (cos φ + i sin φ)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SubcarrierMeasurement {
    /// Channel amplitude (linear magnitude) on this subcarrier.
    pub amplitude: f32,
    /// Channel phase in radians on this subcarrier.
    pub phase: f32,
}

impl SubcarrierMeasurement {
    /// Builds a per-subcarrier measurement.
    pub fn new(amplitude: f32, phase: f32) -> Self {
        SubcarrierMeasurement { amplitude, phase }
    }

    /// Returns the real part of the complex channel estimate.
    pub fn re(self) -> f32 {
        self.amplitude * self.phase.cos()
    }

    /// Returns the imaginary part of the complex channel estimate.
    pub fn im(self) -> f32 {
        self.amplitude * self.phase.sin()
    }
}

/// CSI payload of a sensing measurement: one [`SubcarrierMeasurement`] per
/// subcarrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CsiPayload {
    /// Per-subcarrier measurements, ordered low to high subcarrier index.
    pub subcarriers: Vec<SubcarrierMeasurement>,
}

impl CsiPayload {
    /// Builds a CSI payload from per-subcarrier measurements.
    pub fn new(subcarriers: Vec<SubcarrierMeasurement>) -> Self {
        CsiPayload { subcarriers }
    }

    /// Number of subcarriers in the payload.
    pub fn len(&self) -> usize {
        self.subcarriers.len()
    }

    /// Returns `true` if the payload carries no subcarriers.
    pub fn is_empty(&self) -> bool {
        self.subcarriers.is_empty()
    }
}

/// A sensing measurement report, aligned to the 802.11bf report frame.
///
/// This is the type the rest of the RuView pipeline consumes. Vendor CSI is
/// wrapped into it today (see [`crate::ingest::from_vendor_csi`]); a real
/// 802.11bf report parser would produce the same type, letting the source be
/// swapped without touching downstream consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SensingMeasurementReport {
    /// The measurement setup this report belongs to.
    pub measurement_setup_id: MeasurementSetupId,
    /// Role of the reporting device in the session.
    pub role: SensingRole,
    /// Band the measurement was taken on.
    pub band: SensingBand,
    /// Capture timestamp in microseconds (device or TSF clock).
    pub timestamp_us: u64,
    /// How the sounding was obtained.
    pub sounding_type: SoundingType,
    /// Number of antennas (spatial streams) the payload represents.
    pub antenna_count: u8,
    /// The CSI payload.
    pub payload: CsiPayload,
}

impl SensingMeasurementReport {
    /// Builds and validates a measurement report.
    ///
    /// # Errors
    ///
    /// Returns [`BfError::InvalidReport`] if the payload is empty, the antenna
    /// count is zero, or any sample is non-finite (NaN/inf).
    pub fn new(
        measurement_setup_id: MeasurementSetupId,
        role: SensingRole,
        band: SensingBand,
        timestamp_us: u64,
        sounding_type: SoundingType,
        antenna_count: u8,
        payload: CsiPayload,
    ) -> Result<Self, BfError> {
        let report = SensingMeasurementReport {
            measurement_setup_id,
            role,
            band,
            timestamp_us,
            sounding_type,
            antenna_count,
            payload,
        };
        report.validate()?;
        Ok(report)
    }

    /// Validates the report against the bf-aligned schema invariants.
    ///
    /// # Errors
    ///
    /// Returns [`BfError::InvalidReport`] on empty payload, zero antennas, or
    /// non-finite amplitude/phase values.
    pub fn validate(&self) -> Result<(), BfError> {
        if self.payload.is_empty() {
            return Err(BfError::InvalidReport("empty CSI payload".to_string()));
        }
        if self.antenna_count == 0 {
            return Err(BfError::InvalidReport("antenna_count is zero".to_string()));
        }
        for (i, sc) in self.payload.subcarriers.iter().enumerate() {
            if !sc.amplitude.is_finite() || !sc.phase.is_finite() {
                return Err(BfError::InvalidReport(format!(
                    "non-finite sample at subcarrier {}",
                    i
                )));
            }
            if sc.amplitude < 0.0 {
                return Err(BfError::InvalidReport(format!(
                    "negative amplitude at subcarrier {}",
                    i
                )));
            }
        }
        Ok(())
    }

    /// Number of subcarriers in the report's payload.
    pub fn subcarrier_count(&self) -> usize {
        self.payload.len()
    }
}

/// A trigger-based measurement schedule: an initiator polling responders.
///
/// Mirrors the 802.11bf trigger-based measurement phase and RuView's TDM
/// aggregator, where one initiator triggers a set of responders at a fixed
/// rate. Trigger-based scheduling requires the negotiated session to support
/// it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasurementSchedule {
    /// Setup the schedule operates under.
    pub measurement_setup_id: MeasurementSetupId,
    /// Sounding rate in Hz (measurements per second).
    pub rate_hz: f32,
    /// Responder setup IDs the initiator triggers, in trigger order.
    pub responders: Vec<MeasurementSetupId>,
}

impl MeasurementSchedule {
    /// Builds a trigger-based schedule.
    ///
    /// # Errors
    ///
    /// Returns [`BfError::InvalidSchedule`] if the rate is not strictly
    /// positive and finite, if `max_rate_hz` is exceeded, or if there are no
    /// responders to trigger.
    pub fn new(
        measurement_setup_id: MeasurementSetupId,
        rate_hz: f32,
        responders: Vec<MeasurementSetupId>,
        max_rate_hz: f32,
    ) -> Result<Self, BfError> {
        if !(rate_hz.is_finite() && rate_hz > 0.0) {
            return Err(BfError::InvalidSchedule(
                "rate_hz must be finite and positive".to_string(),
            ));
        }
        if rate_hz > max_rate_hz {
            return Err(BfError::InvalidSchedule(format!(
                "rate_hz {} exceeds negotiated max {}",
                rate_hz, max_rate_hz
            )));
        }
        if responders.is_empty() {
            return Err(BfError::InvalidSchedule(
                "schedule has no responders".to_string(),
            ));
        }
        Ok(MeasurementSchedule {
            measurement_setup_id,
            rate_hz,
            responders,
        })
    }

    /// Nominal inter-measurement interval in microseconds.
    pub fn interval_us(&self) -> u64 {
        (1_000_000.0 / self.rate_hz) as u64
    }

    /// Number of responders the initiator triggers each cycle.
    pub fn responder_count(&self) -> usize {
        self.responders.len()
    }
}
