//! Gait fingerprinting from a walking window of BFI reports.
//!
//! Walking modulates the beamforming angles in a person-specific way (stride
//! cadence, limb swing, torso sway). The [`GaitProfiler`] reduces a window to a
//! fixed-length, L2-normalised descriptor built from statistics of the
//! frame-to-frame angle deltas. [`GaitRegistry`] enrols named descriptors and
//! identifies a query descriptor by cosine similarity.
//!
//! This is the deterministic, ML-free scaffold for the "99.5% gait
//! recognition" capability: a robust descriptor plus a cosine match. No model
//! weights or external inference are involved.

use crate::types::{BfiError, BfiReport};
use serde::{Deserialize, Serialize};

/// Number of aggregated dimensions per statistic in the descriptor.
const AGG_DIMS: usize = 8;
/// Statistics captured per aggregated dimension: mean, std, spectral centroid.
const STATS_PER_DIM: usize = 3;
/// Total descriptor length.
pub const DESCRIPTOR_LEN: usize = AGG_DIMS * STATS_PER_DIM;
/// Default cosine-similarity acceptance threshold.
pub const DEFAULT_GAIT_THRESHOLD: f64 = 0.85;

/// A fixed-length L2-normalised gait descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaitDescriptor {
    /// Descriptor values (length [`DESCRIPTOR_LEN`]).
    pub values: Vec<f64>,
}

impl GaitDescriptor {
    /// Cosine similarity against another descriptor in `[-1, 1]`.
    pub fn cosine_similarity(&self, other: &GaitDescriptor) -> f64 {
        let mut dot = 0.0;
        let mut na = 0.0;
        let mut nb = 0.0;
        for (&a, &b) in self.values.iter().zip(other.values.iter()) {
            dot += a * b;
            na += a * a;
            nb += b * b;
        }
        let denom = (na.sqrt()) * (nb.sqrt());
        if denom <= f64::EPSILON {
            0.0
        } else {
            dot / denom
        }
    }
}

/// Builds gait descriptors from windows of walking reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct GaitProfiler;

impl GaitProfiler {
    /// Create a profiler.
    pub fn new() -> Self {
        Self
    }

    /// Produce a descriptor from a window of reports (≥3 frames required).
    pub fn profile(&self, window: &[BfiReport]) -> Result<GaitDescriptor, BfiError> {
        if window.len() < 3 {
            return Err(BfiError::WindowTooSmall {
                needed: 3,
                got: window.len(),
            });
        }
        let vectors: Vec<Vec<f64>> = window.iter().map(BfiReport::feature_vector).collect();
        let dims = vectors.iter().map(Vec::len).min().unwrap_or(0);
        if dims == 0 {
            return Err(BfiError::WindowTooSmall {
                needed: 3,
                got: window.len(),
            });
        }

        // Frame-to-frame deltas, then fold the (dims) channels into AGG_DIMS
        // buckets so the descriptor length is independent of geometry.
        let mut bucket_series: Vec<Vec<f64>> = vec![Vec::new(); AGG_DIMS];
        for pair in vectors.windows(2) {
            let prev = &pair[0];
            let cur = &pair[1];
            let mut bucket_sum = [0.0; AGG_DIMS];
            let mut bucket_cnt = [0usize; AGG_DIMS];
            for d in 0..dims {
                let delta = cur[d] - prev[d];
                let b = d % AGG_DIMS;
                bucket_sum[b] += delta.abs();
                bucket_cnt[b] += 1;
            }
            for b in 0..AGG_DIMS {
                let v = if bucket_cnt[b] == 0 {
                    0.0
                } else {
                    bucket_sum[b] / bucket_cnt[b] as f64
                };
                bucket_series[b].push(v);
            }
        }

        let mut values = Vec::with_capacity(DESCRIPTOR_LEN);
        for series in &bucket_series {
            values.push(mean(series));
            values.push(std_dev(series));
            values.push(spectral_centroid(series));
        }
        l2_normalize(&mut values);
        Ok(GaitDescriptor { values })
    }
}

/// A named registry of enrolled gait descriptors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GaitRegistry {
    entries: Vec<(String, GaitDescriptor)>,
    threshold: f64,
}

impl GaitRegistry {
    /// Create a registry with the default acceptance threshold.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            threshold: DEFAULT_GAIT_THRESHOLD,
        }
    }

    /// Create a registry with a custom cosine-similarity threshold.
    pub fn with_threshold(threshold: f64) -> Self {
        Self {
            entries: Vec::new(),
            threshold,
        }
    }

    /// Current acceptance threshold.
    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    /// Number of enrolled identities.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no identities are enrolled.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Enrol a descriptor under `name` (replaces an existing entry of that name).
    pub fn enroll(&mut self, name: impl Into<String>, descriptor: GaitDescriptor) {
        let name = name.into();
        if let Some(slot) = self.entries.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = descriptor;
        } else {
            self.entries.push((name, descriptor));
        }
    }

    /// Identify `descriptor`, returning the best match above threshold.
    pub fn identify(&self, descriptor: &GaitDescriptor) -> Option<(String, f64)> {
        let mut best: Option<(String, f64)> = None;
        for (name, enrolled) in &self.entries {
            let sim = enrolled.cosine_similarity(descriptor);
            let better = match &best {
                Some((_, b)) => sim > *b,
                None => true,
            };
            if better {
                best = Some((name.clone(), sim));
            }
        }
        best.filter(|(_, sim)| *sim >= self.threshold)
    }
}

/// Arithmetic mean of a slice (0 for empty).
fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

/// Population standard deviation of a slice.
fn std_dev(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let var = xs.iter().map(|&x| (x - m) * (x - m)).sum::<f64>() / xs.len() as f64;
    var.sqrt()
}

/// Spectral centroid of a series via a magnitude-weighted bin index.
///
/// Computed against a small DFT; returns 0 when the series is flat.
fn spectral_centroid(xs: &[f64]) -> f64 {
    let n = xs.len();
    if n < 2 {
        return 0.0;
    }
    let m = mean(xs);
    let mut weighted = 0.0;
    let mut total = 0.0;
    for k in 1..=(n / 2) {
        let omega = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
        let mut re = 0.0;
        let mut im = 0.0;
        for (t, &x) in xs.iter().enumerate() {
            let centered = x - m;
            let angle = omega * t as f64;
            re += centered * angle.cos();
            im -= centered * angle.sin();
        }
        let mag = (re * re + im * im).sqrt();
        weighted += k as f64 * mag;
        total += mag;
    }
    if total <= f64::EPSILON {
        0.0
    } else {
        weighted / total
    }
}

/// L2-normalise a vector in place (no-op for a zero vector).
fn l2_normalize(v: &mut [f64]) {
    let norm = v.iter().map(|&x| x * x).sum::<f64>().sqrt();
    if norm > f64::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Bandwidth, MimoControl, SubcarrierAngles};

    fn walking_window(seed: f64) -> Vec<BfiReport> {
        (0..32)
            .map(|t| {
                let phase = 2.0 * std::f64::consts::PI * seed * (t as f64) / 20.0;
                BfiReport {
                    source: None,
                    control: MimoControl {
                        nc: 1,
                        nr: 2,
                        bandwidth: Bandwidth::Bw20,
                        codebook_large: false,
                        mu_feedback: false,
                    },
                    subcarriers: vec![SubcarrierAngles {
                        phi: vec![phase.sin(), (phase * 1.3).cos(), (phase * 0.5).sin()],
                        psi: vec![(phase * 2.0).sin()],
                    }],
                }
            })
            .collect()
    }

    #[test]
    fn descriptor_has_fixed_length_and_is_normalized() {
        let d = GaitProfiler::new().profile(&walking_window(1.0)).unwrap();
        assert_eq!(d.values.len(), DESCRIPTOR_LEN);
        let norm: f64 = d.values.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-9 || norm < 1e-9);
    }

    #[test]
    fn same_gait_self_similarity_is_one() {
        let d = GaitProfiler::new().profile(&walking_window(1.0)).unwrap();
        assert!((d.cosine_similarity(&d) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn registry_identifies_enrolled_gait() {
        let prof = GaitProfiler::new();
        let mut reg = GaitRegistry::with_threshold(0.5);
        let alice = prof.profile(&walking_window(1.0)).unwrap();
        reg.enroll("alice", alice.clone());
        let (name, sim) = reg.identify(&alice).unwrap();
        assert_eq!(name, "alice");
        assert!(sim > 0.99);
    }

    #[test]
    fn unenrolled_below_threshold_returns_none() {
        let prof = GaitProfiler::new();
        let mut reg = GaitRegistry::with_threshold(0.999_999);
        reg.enroll("alice", prof.profile(&walking_window(1.0)).unwrap());
        let other = prof.profile(&walking_window(3.7)).unwrap();
        assert!(reg.identify(&other).is_none());
    }
}
