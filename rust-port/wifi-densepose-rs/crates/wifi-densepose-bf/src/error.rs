//! Error type for the 802.11bf-aligned data model.

use thiserror::Error;

/// Errors produced when building, negotiating, or validating 802.11bf-aligned
/// sensing data structures.
///
/// Modeled on the failure modes of the standard's measurement exchange:
/// incompatible capabilities (no overlap to negotiate), malformed measurement
/// reports, and invalid scheduling parameters.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BfError {
    /// Two peers advertised capabilities with no usable intersection
    /// (e.g. disjoint bands or no common role), so no session can be set up.
    #[error("capability negotiation failed: {0}")]
    NegotiationFailed(String),

    /// A measurement report failed validation (e.g. empty payload, mismatched
    /// subcarrier count, or non-finite sample values).
    #[error("invalid measurement report: {0}")]
    InvalidReport(String),

    /// A measurement schedule had invalid parameters (e.g. zero rate, or a rate
    /// exceeding the negotiated maximum).
    #[error("invalid measurement schedule: {0}")]
    InvalidSchedule(String),

    /// Vendor CSI ingestion could not be mapped to the bf-aligned schema
    /// (e.g. amplitude/phase length mismatch).
    #[error("vendor CSI ingest failed: {0}")]
    Ingest(String),
}
