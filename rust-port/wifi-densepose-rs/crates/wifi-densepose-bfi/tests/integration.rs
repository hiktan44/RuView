//! Integration tests for the BFI sensing pipeline.
//!
//! Builds synthetic [`BfiReport`] sequences directly (without the byte-level
//! frame parser) to exercise the higher-level presence and gait logic against
//! controlled, physically-motivated signals.

use wifi_densepose_bfi::{
    extract_features, BfiConfig, BfiReport, Bandwidth, GaitProfiler, GaitRegistry, MimoControl,
    PresenceClassifier, PresenceState, SubcarrierAngles,
};

const DIMS: usize = 6;

fn control() -> MimoControl {
    MimoControl {
        nc: 2,
        nr: 3,
        bandwidth: Bandwidth::Bw20,
        codebook_large: false,
        mu_feedback: false,
    }
}

/// Build a report whose flattened feature vector equals `values`.
fn report_from(values: [f64; DIMS]) -> BfiReport {
    BfiReport {
        source: None,
        control: control(),
        subcarriers: vec![
            SubcarrierAngles {
                phi: vec![values[0], values[1], values[2]],
                psi: vec![values[3], values[4], values[5]],
            },
        ],
    }
}

/// Empty room: a static channel with only tiny numerical jitter.
fn empty_window() -> Vec<BfiReport> {
    (0..48)
        .map(|t| {
            let eps = 1e-6 * (t as f64).sin();
            report_from([0.4 + eps, 0.5, 0.6, 0.3, 0.2, 0.1])
        })
        .collect()
}

/// Window length used for the synthetic captures (matches the DFT length).
const WIN: usize = 64;

/// Still person: a breathing-band oscillation, no gross motion.
///
/// The tone is placed at exactly DFT bin k=1 (`sample_rate / WIN` Hz ≈
/// 0.31 Hz), which sits squarely inside the breathing band (0.1–0.5 Hz). The
/// amplitude is chosen so total variance clears `presence_threshold` while the
/// motion-band power stays below `motion_threshold`.
fn still_window(cfg: &BfiConfig) -> Vec<BfiReport> {
    let f_breath = cfg.sample_rate_hz / WIN as f64; // bin k=1
    (0..WIN)
        .map(|t| {
            let phase = 2.0 * std::f64::consts::PI * f_breath * (t as f64) / cfg.sample_rate_hz;
            let m = 0.2 * phase.sin();
            report_from([0.4 + m, 0.5 + m, 0.6, 0.3, 0.2, 0.1])
        })
        .collect()
}

/// Walking person: strong motion-band oscillation, parameterised by a bin index.
///
/// `cadence_bin` selects the DFT bin (`cadence_bin · sample_rate / WIN` Hz);
/// bins ≥ 2 land in the motion band (0.5–4 Hz).
fn walking_window(cfg: &BfiConfig, cadence_bin: f64, sway: f64) -> Vec<BfiReport> {
    let cadence_hz = cadence_bin * cfg.sample_rate_hz / WIN as f64;
    (0..WIN)
        .map(|t| {
            let phase = 2.0 * std::f64::consts::PI * cadence_hz * (t as f64) / cfg.sample_rate_hz;
            let a = sway * phase.sin();
            let b = sway * (phase * 2.0).cos();
            report_from([0.4 + a, 0.5 + b, 0.6 + a, 0.3 - b, 0.2 + a, 0.1])
        })
        .collect()
}

#[test]
fn classifier_distinguishes_three_states() {
    let cfg = BfiConfig::default();
    let clf = PresenceClassifier::new(cfg);

    let empty = extract_features(&empty_window(), &cfg).unwrap();
    assert_eq!(clf.classify(&empty).state, PresenceState::Absent);

    let still = extract_features(&still_window(&cfg), &cfg).unwrap();
    assert_eq!(clf.classify(&still).state, PresenceState::PresentStill);

    // Cadence bin 5 -> ~1.56 Hz, well inside the motion band.
    let walking = extract_features(&walking_window(&cfg, 5.0, 0.8), &cfg).unwrap();
    assert_eq!(clf.classify(&walking).state, PresenceState::Active);
}

#[test]
fn gait_registry_distinguishes_two_people() {
    let cfg = BfiConfig::default();
    let profiler = GaitProfiler::new();

    // Two distinct gaits: different cadence and sway amplitude.
    let alice_walk = walking_window(&cfg, 1.6, 0.5);
    let bob_walk = walking_window(&cfg, 2.6, 0.25);

    let alice = profiler.profile(&alice_walk).unwrap();
    let bob = profiler.profile(&bob_walk).unwrap();

    let mut registry = GaitRegistry::with_threshold(0.7);
    registry.enroll("alice", alice.clone());
    registry.enroll("bob", bob.clone());
    assert_eq!(registry.len(), 2);

    // Re-profile a fresh capture of Alice's gait; should match Alice.
    let alice_again = profiler.profile(&walking_window(&cfg, 1.6, 0.5)).unwrap();
    let (name, sim) = registry
        .identify(&alice_again)
        .expect("alice should be identified");
    assert_eq!(name, "alice");
    assert!(sim >= 0.7, "similarity {sim} below threshold");

    // Bob's descriptor must be more similar to Bob than to Alice.
    let bob_to_bob = bob.cosine_similarity(&bob);
    let bob_to_alice = bob.cosine_similarity(&alice);
    assert!(bob_to_bob > bob_to_alice);
}
