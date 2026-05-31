//! Core types, configuration and errors for BFI-based sensing.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced while parsing or analysing BFI data.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BfiError {
    /// The captured frame was too short to contain the expected fields.
    #[error("frame too short: need {needed} bytes, got {got}")]
    FrameTooShort {
        /// Number of bytes the parser required.
        needed: usize,
        /// Number of bytes actually present.
        got: usize,
    },

    /// The action frame is not a VHT/HE compressed beamforming report.
    #[error("not a compressed beamforming report (category={category:#x}, action={action:#x})")]
    NotBeamformingReport {
        /// The 802.11 action-frame category byte that was observed.
        category: u8,
        /// The action byte that was observed.
        action: u8,
    },

    /// MIMO control encoded an unsupported antenna geometry.
    #[error("unsupported MIMO geometry: Nr={nr}, Nc={nc}")]
    UnsupportedGeometry {
        /// Number of receive antennas (rows) decoded.
        nr: u8,
        /// Number of spatial streams (columns) decoded.
        nc: u8,
    },

    /// The angle bitstream ended before all angles were decoded.
    #[error("angle bitstream truncated: subcarrier {subcarrier}, decoded {decoded}/{expected}")]
    TruncatedAngles {
        /// Index of the subcarrier being decoded when the stream ended.
        subcarrier: usize,
        /// Number of angles decoded for that subcarrier before truncation.
        decoded: usize,
        /// Number of angles expected per subcarrier.
        expected: usize,
    },

    /// A temporal window did not contain enough frames to analyse.
    #[error("window too small: need {needed} frames, got {got}")]
    WindowTooSmall {
        /// Minimum number of frames required.
        needed: usize,
        /// Number of frames supplied.
        got: usize,
    },
}

/// Channel bandwidth advertised in the MIMO control field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bandwidth {
    /// 20 MHz channel.
    Bw20,
    /// 40 MHz channel.
    Bw40,
    /// 80 MHz channel.
    Bw80,
    /// 160 / 80+80 MHz channel.
    Bw160,
}

impl Bandwidth {
    /// Decode the 2-bit bandwidth subfield of VHT MIMO control.
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0 => Bandwidth::Bw20,
            1 => Bandwidth::Bw40,
            2 => Bandwidth::Bw80,
            _ => Bandwidth::Bw160,
        }
    }
}

/// Decoded VHT MIMO control field (the header preceding the angle bitstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimoControl {
    /// Number of columns (spatial streams) Nc.
    pub nc: u8,
    /// Number of rows (receive antennas) Nr.
    pub nr: u8,
    /// Channel bandwidth.
    pub bandwidth: Bandwidth,
    /// `true` for the larger codebook (ψ:7/φ:9), `false` for (ψ:5/φ:7).
    pub codebook_large: bool,
    /// `true` for MU-MIMO feedback, `false` for SU.
    pub mu_feedback: bool,
}

impl MimoControl {
    /// φ angle bit width given the codebook.
    pub fn phi_bits(&self) -> u32 {
        if self.codebook_large {
            9
        } else {
            7
        }
    }

    /// ψ angle bit width given the codebook.
    pub fn psi_bits(&self) -> u32 {
        if self.codebook_large {
            7
        } else {
            5
        }
    }

    /// Number of (φ + ψ) angles encoded per subcarrier for this geometry.
    ///
    /// Givens rotation count: for each column `i` in `1..=min(Nc, Nr-1)` there
    /// are `(Nr - i)` φ angles and `(Nr - i)` ψ angles.
    pub fn angles_per_subcarrier(&self) -> usize {
        let nc = self.nc as usize;
        let nr = self.nr as usize;
        if nr == 0 || nc == 0 {
            return 0;
        }
        let cols = nc.min(nr.saturating_sub(1));
        let mut total = 0usize;
        for i in 1..=cols {
            total += 2 * (nr - i);
        }
        total
    }
}

/// One subcarrier's worth of decoded beamforming angles (radians).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubcarrierAngles {
    /// φ angles in radians, in encoded order.
    pub phi: Vec<f64>,
    /// ψ angles in radians, in encoded order.
    pub psi: Vec<f64>,
}

impl SubcarrierAngles {
    /// Flatten φ then ψ into a single feature vector.
    pub fn as_vector(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(self.phi.len() + self.psi.len());
        v.extend_from_slice(&self.phi);
        v.extend_from_slice(&self.psi);
        v
    }
}

/// A fully decoded BFI report: MIMO control + per-subcarrier angles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BfiReport {
    /// MAC address (6 bytes) of the station that emitted the report, if known.
    pub source: Option<[u8; 6]>,
    /// Decoded MIMO control header.
    pub control: MimoControl,
    /// Decoded angles per subcarrier.
    pub subcarriers: Vec<SubcarrierAngles>,
}

impl BfiReport {
    /// Collapse the whole report into a single flat feature vector
    /// (subcarrier-major, φ then ψ). Used as the temporal sample.
    pub fn feature_vector(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for sc in &self.subcarriers {
            out.extend(sc.as_vector());
        }
        out
    }
}

/// Configuration for the BFI sensing pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BfiConfig {
    /// Sampling rate of BFI reports in Hz (router beamforming sounding rate).
    pub sample_rate_hz: f64,
    /// Number of frames per analysis window.
    pub window_frames: usize,
    /// Angle-variance threshold above which the room is considered occupied.
    pub presence_threshold: f64,
    /// Motion-band power threshold above which a target is considered active.
    pub motion_threshold: f64,
}

impl Default for BfiConfig {
    fn default() -> Self {
        // Commodity routers sound at ~10–30 Hz; 64 frames ≈ a few seconds.
        Self {
            sample_rate_hz: 20.0,
            window_frames: 64,
            presence_threshold: 0.015,
            motion_threshold: 0.05,
        }
    }
}
