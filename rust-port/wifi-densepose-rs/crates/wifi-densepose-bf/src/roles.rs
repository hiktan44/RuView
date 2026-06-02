//! Sensing roles and identifiers from the 802.11bf measurement exchange.
//!
//! In 802.11bf a sensing session distinguishes *who manages the measurement*
//! (initiator/responder) from *who transmits or receives the sounding signal*
//! (transmitter/receiver). RuView maps these onto its TDM aggregator: the
//! aggregator behaves as the **initiator**, ESP32 nodes as **responders**.

use serde::{Deserialize, Serialize};

/// Sensing management role within a measurement session.
///
/// Mirrors the 802.11bf initiator/responder distinction. The **initiator**
/// sets up the session, schedules/triggers soundings, and collects reports; a
/// **responder** participates in soundings on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SensingRole {
    /// Manages the sensing procedure: setup, triggering, report collection.
    Initiator,
    /// Participates in soundings driven by an initiator.
    Responder,
}

impl SensingRole {
    /// Returns the complementary role (initiator ⇄ responder).
    pub fn counterpart(self) -> Self {
        match self {
            SensingRole::Initiator => SensingRole::Responder,
            SensingRole::Responder => SensingRole::Initiator,
        }
    }
}

/// Whether a device transmits the sounding PPDU in a given measurement.
///
/// Independent of [`SensingRole`]: an initiator may transmit *or* receive the
/// sounding depending on the configured sounding direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransmitterRole {
    /// This device transmits the sounding signal.
    Transmitter,
    /// This device does not transmit the sounding signal.
    NonTransmitter,
}

/// Whether a device receives and measures the sounding PPDU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReceiverRole {
    /// This device receives and measures the sounding signal.
    Receiver,
    /// This device does not measure the sounding signal.
    NonReceiver,
}

/// Identifier for a negotiated sensing measurement setup.
///
/// Corresponds to the 802.11bf *Measurement Setup ID* that ties together all
/// frames (trigger, sounding, report) belonging to one negotiated session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MeasurementSetupId(pub u32);

impl MeasurementSetupId {
    /// Wraps a raw setup identifier value.
    pub fn new(id: u32) -> Self {
        MeasurementSetupId(id)
    }

    /// Returns the raw identifier value.
    pub fn value(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for MeasurementSetupId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "setup#{}", self.0)
    }
}
