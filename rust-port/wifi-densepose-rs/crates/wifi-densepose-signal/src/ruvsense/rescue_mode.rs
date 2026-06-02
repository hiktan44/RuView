//! Rescue Mode -- Range/Depth Extension via Processing Gain (Kurtarma Modu)
//!
//! Rescue Mode trades **temporal resolution for SNR** by integrating many
//! consecutive CSI frames before running detection. This is standard radar
//! processing gain, applied honestly to WiFi sensing -- nothing exotic.
//!
//! # The honest physics
//!
//! Integrating `N` frames raises the signal above the noise floor:
//!
//! - **Coherent** integration (sum the complex CSI, phase-aligned) gives up to
//!   `10 * log10(N)` dB of SNR gain. The signal adds in amplitude (coherently,
//!   `N` times) while zero-mean noise adds in power (`sqrt(N)` in amplitude),
//!   so the SNR improves by `N` in power = `10*log10(N)` dB. This is the
//!   *theoretical upper bound* -- it is only reached when the target's phase is
//!   stable across the whole window (no motion, no LO drift). A faint breathing
//!   signature under rubble that is quasi-static is a good candidate.
//! - **Non-coherent** integration (sum the magnitudes / power, discard phase)
//!   gives roughly `5 * log10(N)` dB. It is robust to phase instability but
//!   only buys back ~half the dB because the noise no longer averages to zero.
//!
//! Slower data rate -> longer dwell -> more frames per decision -> weaker
//! (deeper / farther) targets cross the detection threshold. The price is that
//! pose/vital updates come out at `rate / N` -- e.g. integrating 100 frames at
//! 100 Hz yields a 1 Hz update. For survivor detection ("is anyone alive in
//! this void?") that trade is almost always worth it.
//!
//! # What this is NOT
//!
//! This boosts SNR within the link budget. It does **not** make 2.4/5 GHz WiFi
//! penetrate hundreds of metres of rock, concrete, or soil -- RF attenuation
//! through dense media is exponential and processing gain only adds a fixed dB
//! offset. Going truly *through* deep earth needs sub-GHz / through-the-earth
//! (TTE) magnetic-induction links (see ADRs on hardware tiers), which are out
//! of scope here. Rescue Mode helps with **faint or deep targets that are
//! still within RF reach** -- a person under collapsed drywall, furniture, or a
//! shallow rubble layer where the WiFi signal still arrives, just buried in
//! noise.

use num_complex::Complex32;

/// Integration strategy for accumulating frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationMode {
    /// Coherent (complex) integration: sum phase-aligned complex CSI.
    /// Upper-bound gain `10*log10(N)` dB; requires phase stability.
    Coherent,
    /// Non-coherent (magnitude/power) integration: sum magnitudes.
    /// Gain `~5*log10(N)` dB; robust to phase instability.
    NonCoherent,
}

impl IntegrationMode {
    /// Theoretical SNR-gain coefficient in dB per decade of frames.
    ///
    /// `gain_db = coeff * log10(N)` -> 10.0 for coherent, 5.0 for non-coherent.
    pub fn gain_coefficient_db(self) -> f32 {
        match self {
            IntegrationMode::Coherent => 10.0,
            IntegrationMode::NonCoherent => 5.0,
        }
    }
}

/// Configuration for Rescue Mode integration.
#[derive(Debug, Clone)]
pub struct RescueModeConfig {
    /// Number of frames to integrate per detection decision.
    ///
    /// 1 = no integration (0 dB gain). Larger = more gain, lower update rate.
    pub integration_window: usize,
    /// Coherent vs non-coherent integration.
    pub mode: IntegrationMode,
    /// Master enable flag. Disabled by default -- Rescue Mode slows pose/vital
    /// output, so it must be opted into explicitly.
    pub enabled: bool,
}

impl Default for RescueModeConfig {
    fn default() -> Self {
        Self {
            integration_window: 32,
            mode: IntegrationMode::NonCoherent,
            enabled: false,
        }
    }
}

/// Theoretical processing-gain SNR boost for integrating `n_frames` frames.
///
/// Returns the **theoretical upper bound** in dB:
///
/// - Coherent:     `10 * log10(N)`
/// - Non-coherent: `5  * log10(N)`
///
/// A window of 1 frame returns exactly `0.0` dB (no gain). Empty (0 frames)
/// also returns `0.0` dB. Real-world gain will be at or below this bound
/// because of residual phase noise, target motion, and clutter.
pub fn integration_gain_db(n_frames: usize, mode: IntegrationMode) -> f32 {
    if n_frames <= 1 {
        return 0.0;
    }
    mode.gain_coefficient_db() * (n_frames as f32).log10()
}

/// Result of integrating one window of frames.
#[derive(Debug, Clone)]
pub struct IntegratedFrame {
    /// Per-subcarrier integrated magnitude (the detection-ready frame).
    ///
    /// For coherent mode this is `|sum(complex)| / N`; for non-coherent it is
    /// `sum(|complex|) / N` (mean magnitude). Either way it is a single
    /// canonical-width frame the downstream pipeline can consume.
    pub magnitude: Vec<f32>,
    /// Number of frames actually integrated.
    pub frames_integrated: usize,
    /// Integration mode used.
    pub mode: IntegrationMode,
    /// Theoretical (upper-bound) SNR gain in dB from this integration.
    pub snr_gain_db: f32,
}

/// Sliding-window CSI integrator for Rescue Mode.
///
/// Accumulates complex CSI frames and, once a full window is collected,
/// emits an [`IntegratedFrame`] with an honest theoretical `snr_gain_db`.
#[derive(Debug, Clone)]
pub struct RescueIntegrator {
    config: RescueModeConfig,
    /// Ring of recent complex frames (one Vec per frame, each subcarrier-wide).
    window: Vec<Vec<Complex32>>,
    /// Expected subcarrier width (set on first frame).
    width: Option<usize>,
}

impl RescueIntegrator {
    /// Create a new integrator with the given config.
    pub fn new(config: RescueModeConfig) -> Self {
        Self {
            config,
            window: Vec::new(),
            width: None,
        }
    }

    /// Create with default config (disabled, window 32, non-coherent).
    pub fn with_defaults() -> Self {
        Self::new(RescueModeConfig::default())
    }

    /// Return a reference to the active configuration.
    pub fn config(&self) -> &RescueModeConfig {
        &self.config
    }

    /// Number of frames currently buffered in the window.
    pub fn buffered(&self) -> usize {
        self.window.len()
    }

    /// Clear all buffered frames.
    pub fn clear(&mut self) {
        self.window.clear();
        self.width = None;
    }

    /// Push one complex CSI frame into the window.
    ///
    /// Returns `Some(IntegratedFrame)` once a full `integration_window` of
    /// frames has been collected (consuming the window), otherwise `None`.
    /// Empty frames and width mismatches are ignored gracefully (return `None`).
    ///
    /// When the config is disabled, each frame is emitted immediately with
    /// `snr_gain_db = 0.0` (passthrough, no temporal-resolution penalty).
    pub fn push(&mut self, frame: &[Complex32]) -> Option<IntegratedFrame> {
        if frame.is_empty() {
            return None;
        }

        // Passthrough when disabled or window <= 1: no integration, no gain.
        if !self.config.enabled || self.config.integration_window <= 1 {
            let magnitude = frame.iter().map(|c| c.norm()).collect();
            return Some(IntegratedFrame {
                magnitude,
                frames_integrated: 1,
                mode: self.config.mode,
                snr_gain_db: 0.0,
            });
        }

        // Lock width on first frame; drop mismatched frames.
        match self.width {
            None => self.width = Some(frame.len()),
            Some(w) if w != frame.len() => return None,
            _ => {}
        }

        self.window.push(frame.to_vec());

        if self.window.len() >= self.config.integration_window {
            Some(self.flush_internal())
        } else {
            None
        }
    }

    /// Force-integrate whatever is currently buffered (partial window).
    ///
    /// Useful at end-of-stream. Returns `None` if nothing is buffered. The
    /// reported gain honestly reflects the *actual* frame count, not the
    /// configured window.
    pub fn flush(&mut self) -> Option<IntegratedFrame> {
        if self.window.is_empty() {
            return None;
        }
        Some(self.flush_internal())
    }

    fn flush_internal(&mut self) -> IntegratedFrame {
        let n = self.window.len();
        let width = self.width.unwrap_or(0);
        let mode = self.config.mode;

        let magnitude = match mode {
            IntegrationMode::Coherent => {
                // Sum complex (phase-aligned add), then take magnitude / N.
                let mut acc = vec![Complex32::new(0.0, 0.0); width];
                for f in &self.window {
                    for (a, c) in acc.iter_mut().zip(f.iter()) {
                        *a += *c;
                    }
                }
                acc.iter().map(|c| c.norm() / n as f32).collect()
            }
            IntegrationMode::NonCoherent => {
                // Sum magnitudes (discard phase), then mean.
                let mut acc = vec![0.0_f32; width];
                for f in &self.window {
                    for (a, c) in acc.iter_mut().zip(f.iter()) {
                        *a += c.norm();
                    }
                }
                acc.iter().map(|m| m / n as f32).collect()
            }
        };

        let snr_gain_db = integration_gain_db(n, mode);
        self.clear();

        IntegratedFrame {
            magnitude,
            frames_integrated: n,
            mode,
            snr_gain_db,
        }
    }
}

/// Outcome of a detectability assessment after applying processing gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectabilityVerdict {
    /// Target was already above threshold before integration.
    AlreadyDetectable,
    /// Target crossed the threshold thanks to integration gain.
    RecoveredByGain,
    /// Target is still below threshold even after integration.
    StillUndetectable,
}

/// Estimate whether a target crosses the detection threshold after gain.
///
/// Simple, honest link-budget model in the dB domain:
///
///   `effective_snr_db = baseline_snr_db + integration_gain_db(N, mode)`
///
/// and the target is "detectable" when `effective_snr_db >= threshold_db`.
///
/// # Assumptions (documented, not hidden)
///
/// - `baseline_snr_db` is the single-frame SNR of the target return at the
///   detector input (negative for sub-noise-floor targets).
/// - `threshold_db` is the minimum SNR the detector needs (often ~6-13 dB
///   depending on desired Pd/Pfa). Caller supplies it.
/// - The gain is the *theoretical upper bound*; a conservative caller should
///   subtract an implementation-loss margin before calling. This function does
///   not invent margin it cannot justify.
pub fn assess_detectability(
    baseline_snr_db: f32,
    threshold_db: f32,
    n_frames: usize,
    mode: IntegrationMode,
) -> DetectabilityVerdict {
    if baseline_snr_db >= threshold_db {
        return DetectabilityVerdict::AlreadyDetectable;
    }
    let effective = baseline_snr_db + integration_gain_db(n_frames, mode);
    if effective >= threshold_db {
        DetectabilityVerdict::RecoveredByGain
    } else {
        DetectabilityVerdict::StillUndetectable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f32 = 1e-3;

    fn const_complex_frame(width: usize, re: f32, im: f32) -> Vec<Complex32> {
        vec![Complex32::new(re, im); width]
    }

    #[test]
    fn default_config_is_disabled_and_sane() {
        let cfg = RescueModeConfig::default();
        assert!(!cfg.enabled, "Rescue Mode must default to disabled");
        assert!(cfg.integration_window >= 1, "window must be >= 1");
        assert_eq!(cfg.integration_window, 32);
        assert_eq!(cfg.mode, IntegrationMode::NonCoherent);
    }

    #[test]
    fn coherent_gain_matches_10log10n() {
        for &n in &[2usize, 4, 10, 100, 1000] {
            let g = integration_gain_db(n, IntegrationMode::Coherent);
            let expected = 10.0 * (n as f32).log10();
            assert!((g - expected).abs() < TOL, "N={n}: got {g}, want {expected}");
        }
        // Sanity: 100 coherent frames -> 20 dB.
        assert!((integration_gain_db(100, IntegrationMode::Coherent) - 20.0).abs() < TOL);
    }

    #[test]
    fn non_coherent_gain_matches_5log10n() {
        for &n in &[2usize, 4, 10, 100, 1000] {
            let g = integration_gain_db(n, IntegrationMode::NonCoherent);
            let expected = 5.0 * (n as f32).log10();
            assert!((g - expected).abs() < TOL, "N={n}: got {g}, want {expected}");
        }
        // Sanity: 100 non-coherent frames -> 10 dB (half of coherent).
        assert!((integration_gain_db(100, IntegrationMode::NonCoherent) - 10.0).abs() < TOL);
    }

    #[test]
    fn single_frame_window_gives_zero_gain() {
        assert_eq!(integration_gain_db(1, IntegrationMode::Coherent), 0.0);
        assert_eq!(integration_gain_db(1, IntegrationMode::NonCoherent), 0.0);
    }

    #[test]
    fn empty_window_gives_zero_gain() {
        assert_eq!(integration_gain_db(0, IntegrationMode::Coherent), 0.0);
        assert_eq!(integration_gain_db(0, IntegrationMode::NonCoherent), 0.0);
    }

    #[test]
    fn integrator_emits_after_full_window_with_correct_gain() {
        let cfg = RescueModeConfig {
            integration_window: 16,
            mode: IntegrationMode::Coherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        let mut out = None;
        for _ in 0..16 {
            out = integ.push(&const_complex_frame(8, 1.0, 0.0));
        }
        let frame = out.expect("should emit after 16 frames");
        assert_eq!(frame.frames_integrated, 16);
        // 16 identical-phase frames -> coherent magnitude == 1.0 (mean of sum).
        for m in &frame.magnitude {
            assert!((m - 1.0).abs() < TOL, "coherent sum should preserve unit magnitude, got {m}");
        }
        let expected_gain = 10.0 * (16f32).log10();
        assert!((frame.snr_gain_db - expected_gain).abs() < TOL);
        assert_eq!(integ.buffered(), 0, "window should reset after emit");
    }

    #[test]
    fn coherent_integration_of_identical_phase_frames_yields_10log10n_gain() {
        // Directly exercise the gain formula via the integrator's report.
        let cfg = RescueModeConfig {
            integration_window: 64,
            mode: IntegrationMode::Coherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        let mut frame = None;
        for _ in 0..64 {
            frame = integ.push(&const_complex_frame(4, 0.5, 0.5));
        }
        let f = frame.unwrap();
        assert!((f.snr_gain_db - 10.0 * 64f32.log10()).abs() < TOL);
        // 10*log10(64) ~= 18.06 dB.
        assert!((f.snr_gain_db - 18.0618).abs() < 0.01);
    }

    #[test]
    fn non_coherent_integration_yields_5log10n_gain() {
        let cfg = RescueModeConfig {
            integration_window: 64,
            mode: IntegrationMode::NonCoherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        let mut frame = None;
        for _ in 0..64 {
            frame = integ.push(&const_complex_frame(4, 3.0, 4.0)); // |c| = 5
        }
        let f = frame.unwrap();
        // Mean magnitude preserved at 5.0.
        for m in &f.magnitude {
            assert!((m - 5.0).abs() < TOL, "non-coherent mean magnitude should be 5.0, got {m}");
        }
        assert!((f.snr_gain_db - 5.0 * 64f32.log10()).abs() < TOL);
    }

    #[test]
    fn weak_target_below_threshold_becomes_detectable() {
        // Baseline -8 dB, detector needs 10 dB. Coherent N=200 -> +23 dB.
        let verdict = assess_detectability(-8.0, 10.0, 200, IntegrationMode::Coherent);
        assert_eq!(verdict, DetectabilityVerdict::RecoveredByGain);
    }

    #[test]
    fn far_too_weak_target_still_undetectable_no_overpromise() {
        // Baseline -60 dB is hopeless: coherent N=100 only adds 20 dB -> -40 dB.
        let verdict = assess_detectability(-60.0, 10.0, 100, IntegrationMode::Coherent);
        assert_eq!(verdict, DetectabilityVerdict::StillUndetectable);
    }

    #[test]
    fn already_detectable_target_reported_as_such() {
        let verdict = assess_detectability(15.0, 10.0, 100, IntegrationMode::Coherent);
        assert_eq!(verdict, DetectabilityVerdict::AlreadyDetectable);
    }

    #[test]
    fn non_coherent_recovers_less_than_coherent() {
        // -8 dB baseline, 10 dB threshold, N=16.
        // Coherent: +12.04 dB -> effective +4.04 -> still below -> undetectable.
        // (demonstrates honest difference between modes)
        let coh = assess_detectability(-8.0, 10.0, 16, IntegrationMode::Coherent);
        let non = assess_detectability(-8.0, 10.0, 16, IntegrationMode::NonCoherent);
        assert_eq!(coh, DetectabilityVerdict::StillUndetectable);
        assert_eq!(non, DetectabilityVerdict::StillUndetectable);
        // But with a bigger window coherent recovers while non-coherent does not.
        let coh_big = assess_detectability(-8.0, 10.0, 100, IntegrationMode::Coherent);
        let non_big = assess_detectability(-8.0, 10.0, 100, IntegrationMode::NonCoherent);
        assert_eq!(coh_big, DetectabilityVerdict::RecoveredByGain); // -8 + 20 = 12
        assert_eq!(non_big, DetectabilityVerdict::StillUndetectable); // -8 + 10 = 2
    }

    #[test]
    fn empty_frame_handled_gracefully() {
        let cfg = RescueModeConfig {
            integration_window: 8,
            mode: IntegrationMode::Coherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        assert!(integ.push(&[]).is_none());
        assert_eq!(integ.buffered(), 0);
    }

    #[test]
    fn width_mismatch_dropped() {
        let cfg = RescueModeConfig {
            integration_window: 4,
            mode: IntegrationMode::Coherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        assert!(integ.push(&const_complex_frame(8, 1.0, 0.0)).is_none());
        // Wrong width: dropped, buffer unchanged.
        assert!(integ.push(&const_complex_frame(4, 1.0, 0.0)).is_none());
        assert_eq!(integ.buffered(), 1);
    }

    #[test]
    fn disabled_config_passes_through_with_zero_gain() {
        let mut integ = RescueIntegrator::with_defaults(); // disabled
        let out = integ.push(&const_complex_frame(4, 3.0, 4.0)).unwrap();
        assert_eq!(out.frames_integrated, 1);
        assert_eq!(out.snr_gain_db, 0.0);
        for m in &out.magnitude {
            assert!((m - 5.0).abs() < TOL);
        }
    }

    #[test]
    fn flush_partial_window_reports_actual_frame_count() {
        let cfg = RescueModeConfig {
            integration_window: 100,
            mode: IntegrationMode::NonCoherent,
            enabled: true,
        };
        let mut integ = RescueIntegrator::new(cfg);
        for _ in 0..10 {
            assert!(integ.push(&const_complex_frame(4, 1.0, 0.0)).is_none());
        }
        let f = integ.flush().expect("partial flush should emit");
        assert_eq!(f.frames_integrated, 10);
        assert!((f.snr_gain_db - 5.0 * 10f32.log10()).abs() < TOL);
        assert_eq!(integ.buffered(), 0);
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut integ = RescueIntegrator::with_defaults();
        assert!(integ.flush().is_none());
    }

    #[test]
    fn gain_coefficient_values() {
        assert_eq!(IntegrationMode::Coherent.gain_coefficient_db(), 10.0);
        assert_eq!(IntegrationMode::NonCoherent.gain_coefficient_db(), 5.0);
    }
}
