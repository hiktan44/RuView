//! IEEE 802.11bf (WLAN Sensing) **aligned** data model for RuView.
//!
//! # What 802.11bf standardizes
//!
//! IEEE Std 802.11bf-2025 (approved September 2025) is the first amendment to
//! 802.11 that defines **WLAN Sensing**: using the same radios that carry data
//! frames to also estimate features of the environment (presence, motion,
//! gesture, proximity, range, velocity). It standardizes the *measurement
//! exchange* rather than any particular sensing algorithm. The key abstractions
//! it introduces, which this crate mirrors, are:
//!
//! - **Sensing procedure phases**: a session moves through
//!   *setup* → *measurement* → *report* → *termination*. Setup negotiates the
//!   parameters; measurement performs the sounding; report carries the result;
//!   termination tears the session down.
//! - **Roles**: a sensing **initiator** starts and manages a measurement, and
//!   one or more **responders** participate. A device can also be the
//!   transmitter or receiver of the sounding PPDU independently of who
//!   initiated. See [`roles`].
//! - **Bands / measurement types**: sub-7 GHz sensing is **CSI-based**
//!   (per-subcarrier channel estimates), whereas the 60 GHz DMG/EDMG band is
//!   **beam-based** (beam-refinement / sector-sweep measurements). See
//!   [`capability::SensingBand`].
//! - **Trigger-based measurement**: the initiator can poll/trigger responders
//!   on a schedule so that soundings happen at a negotiated rate — directly
//!   analogous to RuView's TDM aggregator triggering ESP32 nodes. See
//!   [`measurement::MeasurementSchedule`].
//! - **Capability negotiation**: before sensing, devices advertise what they
//!   support (bands, antennas, rate, roles, trigger support) and agree on an
//!   intersection. See [`capability`].
//!
//! # Honest scope: ALIGNED, not certified
//!
//! **This crate is a software data model and parser, not a certified 802.11bf
//! implementation.** RuView runs on ESP32-S3 hardware, which exposes *vendor*
//! CSI — it is not, and cannot be, a certified 802.11bf sensing radio. There is
//! no real bf measurement-report frame on the wire here.
//!
//! The design goal is to be **802.11bf-ALIGNED**: we model the standard's
//! sensing measurement abstractions so the rest of the pipeline speaks the bf
//! data model *today* (fed by vendor CSI via [`ingest::from_vendor_csi`]), and
//! when genuine 802.11bf hardware appears we **swap the source, not the
//! pipeline** — a real bf report parser produces the same
//! [`measurement::SensingMeasurementReport`] type the pipeline already consumes.
//!
//! # References
//!
//! - IEEE Std 802.11bf-2025, *Enhancements for Wireless Local Area Network
//!   (WLAN) Sensing* (approved September 2025).
//! - R. Du et al., "An Overview of IEEE 802.11bf: WLAN Sensing,"
//!   arXiv:2207.04859, 2022.
//! - C. Chen et al., "Wi-Fi Sensing Based on IEEE 802.11bf," IEEE
//!   Communications Magazine, 2023.
//!
//! These references describe the *standard*; the gaps between them and this
//! vendor-CSI-backed model are documented honestly in [`ingest`].

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod capability;
pub mod error;
pub mod ingest;
pub mod measurement;
pub mod roles;

pub use capability::{NegotiatedSession, SensingBand, SensingCapability};
pub use error::BfError;
pub use ingest::{from_vendor_csi, VendorCsiFrame};
pub use measurement::{
    CsiPayload, MeasurementSchedule, SensingMeasurementReport, SoundingType, SubcarrierMeasurement,
};
pub use roles::{MeasurementSetupId, ReceiverRole, SensingRole, TransmitterRole};

/// Result alias for fallible operations in this crate.
pub type Result<T> = core::result::Result<T, BfError>;
