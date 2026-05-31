//! Replay captured BFI frames through the real parser → presence + gait.
//!
//! Reads a file of one-hex-string-per-line **802.11 MAC frames** (the format
//! `v1/src/sensing/bfi_capture.py --dump` writes), parses each as a VHT
//! compressed beamforming report, then runs the production presence classifier
//! and gait profiler over the whole window. This is the bridge between a live
//! capture and the `wifi-densepose-bfi` crate: whatever the laptop/USB adapter
//! sniffs off the air gets validated by the exact same code the library ships.
//!
//! Usage:
//!   bfi-replay <dump.hex> [--window N] [--rate HZ]
//!
//! Each input line is hex for the 802.11 MAC frame starting at the frame-control
//! byte (radiotap already stripped by the capture tool). Lines that are not VHT
//! beamforming reports are skipped and counted.

use std::process::ExitCode;

use wifi_densepose_bfi::{
    extract_features, parse_vht_beamforming_report, BfiConfig, GaitProfiler, PresenceClassifier,
    PresenceState,
};

fn hex_to_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() || s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let path = match args.next() {
        Some(p) if !p.starts_with("--") => p,
        _ => {
            eprintln!("usage: bfi-replay <dump.hex> [--window N] [--rate HZ]");
            return ExitCode::from(2);
        }
    };

    let mut cfg = BfiConfig::default();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--window" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.window_frames = v;
                }
            }
            "--rate" => {
                if let Some(v) = args.next().and_then(|s| s.parse().ok()) {
                    cfg.sample_rate_hz = v;
                }
            }
            other => eprintln!("ignoring unknown flag: {other}"),
        }
    }

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let mut reports = Vec::new();
    let mut total = 0usize;
    let mut skipped = 0usize;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        total += 1;
        let Some(bytes) = hex_to_bytes(line) else {
            skipped += 1;
            continue;
        };
        match parse_vht_beamforming_report(&bytes) {
            Ok(report) => reports.push(report),
            Err(_) => skipped += 1,
        }
    }

    println!("== bfi-replay ==");
    println!("input lines      : {total}");
    println!("parsed BFI reports: {}", reports.len());
    println!("skipped (non-BFI): {skipped}");

    if reports.len() < 2 {
        println!("\nNot enough BFI reports to analyse (need >= 2).");
        println!("If this came from a live capture with 0 reports, the adapter");
        println!("likely can't surface VHT beamforming in monitor mode.");
        return ExitCode::from(1);
    }

    // Presence over the most recent window.
    let window: Vec<_> = reports
        .iter()
        .rev()
        .take(cfg.window_frames)
        .rev()
        .cloned()
        .collect();
    match extract_features(&window, &cfg) {
        Ok(feats) => {
            let res = PresenceClassifier::new(cfg).classify(&feats);
            let label = match res.state {
                PresenceState::Absent => "ABSENT",
                PresenceState::PresentStill => "PRESENT_STILL",
                PresenceState::Active => "ACTIVE",
            };
            println!("\npresence         : {label} (confidence {:.2})", res.confidence);
            println!("total variance   : {:.5}", feats.total_variance);
            println!("motion band power: {:.5}", feats.motion_band_power);
            println!("breathing power  : {:.5}", feats.breathing_band_power);
        }
        Err(e) => println!("\npresence: feature extraction failed: {e}"),
    }

    // Gait descriptor over all reports.
    match GaitProfiler::new().profile(&reports) {
        Ok(desc) => {
            let preview: Vec<String> = desc
                .values
                .iter()
                .take(6)
                .map(|v| format!("{v:.3}"))
                .collect();
            println!(
                "\ngait descriptor  : [{} ...] (len {})",
                preview.join(", "),
                desc.values.len()
            );
            println!("(enroll this with GaitRegistry to identify a person by walk)");
        }
        Err(e) => println!("\ngait: {e}"),
    }

    ExitCode::SUCCESS
}
