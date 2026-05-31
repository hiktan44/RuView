//! Dual-mode sensing selection (PRD FR-1.2).
//!
//! The system can sense from full CSI (when hardware/firmware provides it) or
//! fall back to firmware-free BFI captured passively. [`DualModeSelector`]
//! implements the policy: prefer CSI when available, otherwise BFI, and
//! auto-switch to BFI when CSI frames stop arriving for a configurable
//! staleness window. The logic is pure (no I/O) and serde-serialisable so it
//! can live in shared configuration/state.

use serde::{Deserialize, Serialize};

/// The active sensing modality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SensingMode {
    /// Full Channel State Information (requires CSI-capable firmware).
    Csi,
    /// Beamforming Feedback Information (firmware-free, passive capture).
    Bfi,
}

/// Selects between CSI and BFI sensing based on availability and freshness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DualModeSelector {
    csi_available: bool,
    bfi_available: bool,
    /// Milliseconds without a CSI frame before falling back to BFI.
    csi_timeout_ms: u64,
    /// Milliseconds since the last CSI frame was observed.
    ms_since_csi: u64,
    mode: SensingMode,
}

impl DualModeSelector {
    /// Create a selector with the given availability and CSI fallback timeout.
    pub fn new(csi_available: bool, bfi_available: bool, csi_timeout_ms: u64) -> Self {
        let mut s = Self {
            csi_available,
            bfi_available,
            csi_timeout_ms,
            ms_since_csi: 0,
            mode: SensingMode::Bfi,
        };
        s.mode = s.resolve();
        s
    }

    /// Resolve the preferred mode from current availability/freshness.
    fn resolve(&self) -> SensingMode {
        let csi_fresh = self.ms_since_csi <= self.csi_timeout_ms;
        if self.csi_available && csi_fresh {
            SensingMode::Csi
        } else if self.bfi_available {
            SensingMode::Bfi
        } else if self.csi_available {
            // CSI stale but no BFI to fall back to: stay on CSI.
            SensingMode::Csi
        } else {
            // Nothing available; default to BFI as the firmware-free baseline.
            SensingMode::Bfi
        }
    }

    /// The currently selected sensing mode.
    pub fn mode(&self) -> SensingMode {
        self.mode
    }

    /// Update CSI availability and recompute the mode.
    pub fn set_csi_available(&mut self, available: bool) {
        self.csi_available = available;
        if available {
            self.ms_since_csi = 0;
        }
        self.mode = self.resolve();
    }

    /// Update BFI availability and recompute the mode.
    pub fn set_bfi_available(&mut self, available: bool) {
        self.bfi_available = available;
        self.mode = self.resolve();
    }

    /// Record that a fresh CSI frame just arrived, resetting the staleness clock.
    pub fn note_csi_frame(&mut self) {
        self.ms_since_csi = 0;
        self.csi_available = true;
        self.mode = self.resolve();
    }

    /// Advance the staleness clock by `delta_ms`, applying the fallback policy.
    ///
    /// Returns the (possibly changed) active mode.
    pub fn tick(&mut self, delta_ms: u64) -> SensingMode {
        self.ms_since_csi = self.ms_since_csi.saturating_add(delta_ms);
        self.mode = self.resolve();
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_csi_when_available_and_fresh() {
        let s = DualModeSelector::new(true, true, 500);
        assert_eq!(s.mode(), SensingMode::Csi);
    }

    #[test]
    fn falls_back_to_bfi_without_csi() {
        let s = DualModeSelector::new(false, true, 500);
        assert_eq!(s.mode(), SensingMode::Bfi);
    }

    #[test]
    fn auto_switches_to_bfi_when_csi_goes_stale() {
        let mut s = DualModeSelector::new(true, true, 500);
        assert_eq!(s.mode(), SensingMode::Csi);
        assert_eq!(s.tick(300), SensingMode::Csi);
        assert_eq!(s.tick(300), SensingMode::Bfi); // 600 ms > 500 ms timeout
    }

    #[test]
    fn recovers_to_csi_when_frame_arrives() {
        let mut s = DualModeSelector::new(true, true, 500);
        s.tick(1000);
        assert_eq!(s.mode(), SensingMode::Bfi);
        s.note_csi_frame();
        assert_eq!(s.mode(), SensingMode::Csi);
    }

    #[test]
    fn serde_roundtrip() {
        let s = DualModeSelector::new(true, false, 250);
        let json = serde_json::to_string(&s).unwrap();
        let back: DualModeSelector = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
