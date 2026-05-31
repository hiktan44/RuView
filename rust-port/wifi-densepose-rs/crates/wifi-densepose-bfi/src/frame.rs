//! Parser for 802.11 VHT Compressed Beamforming action frames.
//!
//! A beamforming feedback (BFI) report is carried in a *VHT Compressed
//! Beamforming* action frame. Unlike CSI, this report is transmitted in the
//! clear by the beamformee (the station) to the beamformer (the AP), so any
//! monitor-mode NIC within range can capture it. This module turns the raw
//! action-frame body into a [`BfiReport`] of dequantised Givens-rotation
//! angles.
//!
//! Action frame layout (body, after the 802.11 MAC header):
//! ```text
//! +----------+--------+------------------+-----------+------------------+
//! | Category | Action | VHT MIMO Control | Avg SNR   | Compressed       |
//! | (1 byte) | (1 b)  | (3 bytes)        | (Nc bytes)| beamforming angles|
//! +----------+--------+------------------+-----------+------------------+
//! ```

use crate::types::{Bandwidth, BfiError, BfiReport, MimoControl, SubcarrierAngles};
use std::f64::consts::PI;

/// 802.11 action-frame category for VHT.
pub const CATEGORY_VHT: u8 = 0x15;
/// VHT action value for a Compressed Beamforming report.
pub const ACTION_COMPRESSED_BEAMFORMING: u8 = 0x00;

/// MSB-first bit reader over a byte slice.
///
/// Beamforming angles are packed most-significant-bit first within each byte,
/// continuing across byte boundaries. This reader yields fixed-width unsigned
/// fields in that order.
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Create a reader positioned at the first bit of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    /// Total number of bits available in the underlying slice.
    pub fn total_bits(&self) -> usize {
        self.bytes.len() * 8
    }

    /// Number of bits not yet consumed.
    pub fn remaining_bits(&self) -> usize {
        self.total_bits().saturating_sub(self.bit_pos)
    }

    /// Read `n` bits (`0 <= n <= 32`) MSB-first, advancing the cursor.
    ///
    /// Returns `None` if fewer than `n` bits remain.
    pub fn read_bits(&mut self, n: u32) -> Option<u32> {
        debug_assert!(n <= 32, "read_bits supports at most 32 bits");
        if n == 0 {
            return Some(0);
        }
        if self.remaining_bits() < n as usize {
            return None;
        }
        let mut value: u32 = 0;
        for _ in 0..n {
            let byte = self.bytes[self.bit_pos / 8];
            let bit_index = 7 - (self.bit_pos % 8);
            let bit = (byte >> bit_index) & 1;
            value = (value << 1) | u32::from(bit);
            self.bit_pos += 1;
        }
        Some(value)
    }
}

/// Decode a φ (phi) angle from its quantised codeword.
///
/// `φ = k·(π / 2^(bφ-1)) + π / 2^bφ`
fn dequantize_phi(k: u32, phi_bits: u32) -> f64 {
    let k = f64::from(k);
    let denom_step = (1u64 << (phi_bits - 1)) as f64;
    let denom_off = (1u64 << phi_bits) as f64;
    k * (PI / denom_step) + PI / denom_off
}

/// Decode a ψ (psi) angle from its quantised codeword.
///
/// `ψ = k·(π / 2^(bψ+1)) + π / 2^(bψ+2)`
fn dequantize_psi(k: u32, psi_bits: u32) -> f64 {
    let k = f64::from(k);
    let denom_step = (1u64 << (psi_bits + 1)) as f64;
    let denom_off = (1u64 << (psi_bits + 2)) as f64;
    k * (PI / denom_step) + PI / denom_off
}

/// Number of grouped subcarriers (Ng = 1) reported for each bandwidth.
///
/// These are the canonical VHT compressed-beamforming scidx group counts.
pub fn subcarriers_for_bandwidth(bw: Bandwidth) -> usize {
    match bw {
        Bandwidth::Bw20 => 52,
        Bandwidth::Bw40 => 108,
        Bandwidth::Bw80 => 234,
        Bandwidth::Bw160 => 468,
    }
}

/// Parse the 3-byte (24-bit) VHT MIMO Control field into a [`MimoControl`].
///
/// The three bytes are interpreted as a little-endian 24-bit word, matching the
/// over-the-air byte order. Bit layout (LSB first):
/// `Nc index(3) | Nr index(3) | bandwidth(2) | grouping(2) | codebook(1) |
/// feedback type(2) | remaining feedback segments(3) | first feedback(8)`.
pub fn parse_mimo_control(bytes: &[u8]) -> Result<MimoControl, BfiError> {
    if bytes.len() < 3 {
        return Err(BfiError::FrameTooShort {
            needed: 3,
            got: bytes.len(),
        });
    }
    let word = u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16);
    let nc = ((word & 0b111) as u8) + 1;
    let nr = (((word >> 3) & 0b111) as u8) + 1;
    let bandwidth = Bandwidth::from_bits(((word >> 6) & 0b11) as u8);
    // grouping at bits 8..10 (Ng), codebook info at bit 10, feedback type at 11.
    let codebook_large = ((word >> 10) & 0b1) != 0;
    let mu_feedback = ((word >> 11) & 0b1) != 0;

    if nr == 0 || nc == 0 || nc > nr {
        return Err(BfiError::UnsupportedGeometry { nr, nc });
    }
    Ok(MimoControl {
        nc,
        nr,
        bandwidth,
        codebook_large,
        mu_feedback,
    })
}

/// Decode the per-subcarrier angle bitstream.
///
/// For each subcarrier, angles are emitted in this documented order: for each
/// column `i` in `1..=min(Nc, Nr-1)`, `(Nr-i)` φ angles (each `phi_bits` wide)
/// followed by `(Nr-i)` ψ angles (each `psi_bits` wide).
#[allow(clippy::similar_names)] // phi_bits / psi_bits mirror the φ / ψ spec names
fn decode_angles(
    reader: &mut BitReader,
    control: MimoControl,
    subcarriers: usize,
) -> Result<Vec<SubcarrierAngles>, BfiError> {
    let phi_bits = control.phi_bits();
    let psi_bits = control.psi_bits();
    let nc = control.nc as usize;
    let nr = control.nr as usize;
    let cols = nc.min(nr.saturating_sub(1));
    let expected = control.angles_per_subcarrier();

    let mut out = Vec::with_capacity(subcarriers);
    for sc in 0..subcarriers {
        let mut phi = Vec::new();
        let mut psi = Vec::new();
        let mut decoded = 0usize;
        for i in 1..=cols {
            let count = nr - i;
            for _ in 0..count {
                let k = reader.read_bits(phi_bits).ok_or(BfiError::TruncatedAngles {
                    subcarrier: sc,
                    decoded,
                    expected,
                })?;
                phi.push(dequantize_phi(k, phi_bits));
                decoded += 1;
            }
            for _ in 0..count {
                let k = reader.read_bits(psi_bits).ok_or(BfiError::TruncatedAngles {
                    subcarrier: sc,
                    decoded,
                    expected,
                })?;
                psi.push(dequantize_psi(k, psi_bits));
                decoded += 1;
            }
        }
        out.push(SubcarrierAngles { phi, psi });
    }
    Ok(out)
}

/// Parse a VHT Compressed Beamforming action-frame body into a [`BfiReport`].
///
/// The subcarrier count is inferred from how many angle bits remain after the
/// header and average-SNR bytes: `subcarriers = available_bits / bits_per_sc`.
/// Use [`parse_vht_beamforming_report_with_subcarriers`] when the count is
/// known from the channel bandwidth and grouping.
pub fn parse_vht_beamforming_report(bytes: &[u8]) -> Result<BfiReport, BfiError> {
    let (control, snr_end) = parse_header(bytes)?;
    let angle_bytes = &bytes[snr_end..];
    let bits_per_sc = bits_per_subcarrier(control);
    let subcarriers = if bits_per_sc == 0 {
        0
    } else {
        (angle_bytes.len() * 8) / bits_per_sc as usize
    };
    finish_parse(bytes, control, snr_end, subcarriers)
}

/// Parse a report when the number of subcarriers is already known.
pub fn parse_vht_beamforming_report_with_subcarriers(
    bytes: &[u8],
    subcarriers: usize,
) -> Result<BfiReport, BfiError> {
    let (control, snr_end) = parse_header(bytes)?;
    finish_parse(bytes, control, snr_end, subcarriers)
}

/// Total number of angle bits encoded per subcarrier for this geometry.
#[allow(clippy::similar_names)] // phi_count / psi_count mirror the φ / ψ spec names
fn bits_per_subcarrier(control: MimoControl) -> u32 {
    let nc = control.nc as usize;
    let nr = control.nr as usize;
    let cols = nc.min(nr.saturating_sub(1));
    let mut phi_count = 0u32;
    let mut psi_count = 0u32;
    for i in 1..=cols {
        let count = (nr - i) as u32;
        phi_count += count;
        psi_count += count;
    }
    phi_count * control.phi_bits() + psi_count * control.psi_bits()
}

/// Validate category/action and decode the MIMO control + average SNR section.
/// Returns the control field and the byte offset where the angle stream starts.
fn parse_header(bytes: &[u8]) -> Result<(MimoControl, usize), BfiError> {
    if bytes.len() < 2 {
        return Err(BfiError::FrameTooShort {
            needed: 2,
            got: bytes.len(),
        });
    }
    let category = bytes[0];
    let action = bytes[1];
    if category != CATEGORY_VHT || action != ACTION_COMPRESSED_BEAMFORMING {
        return Err(BfiError::NotBeamformingReport { category, action });
    }
    let control = parse_mimo_control(&bytes[2..])?;
    // 2 (cat/action) + 3 (MIMO control) + Nc (average SNR) bytes consumed.
    let snr_end = 2 + 3 + control.nc as usize;
    if bytes.len() < snr_end {
        return Err(BfiError::FrameTooShort {
            needed: snr_end,
            got: bytes.len(),
        });
    }
    Ok((control, snr_end))
}

/// Decode the angle stream given a validated header and subcarrier count.
fn finish_parse(
    bytes: &[u8],
    control: MimoControl,
    snr_end: usize,
    subcarriers: usize,
) -> Result<BfiReport, BfiError> {
    let mut reader = BitReader::new(&bytes[snr_end..]);
    let decoded = decode_angles(&mut reader, control, subcarriers)?;
    Ok(BfiReport {
        source: None,
        control,
        subcarriers: decoded,
    })
}

/// Build a synthetic VHT compressed beamforming frame for tests.
///
/// `angle_codewords` is a flat list of per-subcarrier codewords already laid
/// out in the documented φ-then-ψ order; each is packed at the appropriate
/// bit width derived from `control`.
#[doc(hidden)]
#[allow(clippy::similar_names)] // φ / ψ widths intentionally share a stem
pub fn build_synthetic_frame(
    control: &MimoControl,
    subcarriers: usize,
    codeword: u32,
) -> Vec<u8> {
    let mut frame = vec![CATEGORY_VHT, ACTION_COMPRESSED_BEAMFORMING];
    // Encode MIMO control as the inverse of parse_mimo_control.
    let bw_bits = match control.bandwidth {
        Bandwidth::Bw20 => 0u32,
        Bandwidth::Bw40 => 1,
        Bandwidth::Bw80 => 2,
        Bandwidth::Bw160 => 3,
    };
    let mut word: u32 = 0;
    word |= u32::from(control.nc - 1) & 0b111;
    word |= (u32::from(control.nr - 1) & 0b111) << 3;
    word |= (bw_bits & 0b11) << 6;
    word |= u32::from(control.codebook_large) << 10;
    word |= u32::from(control.mu_feedback) << 11;
    frame.push((word & 0xff) as u8);
    frame.push(((word >> 8) & 0xff) as u8);
    frame.push(((word >> 16) & 0xff) as u8);
    // Average SNR: Nc bytes.
    frame.extend(std::iter::repeat_n(0u8, control.nc as usize));

    // Pack the angle bitstream MSB-first.
    let phi_bits = control.phi_bits();
    let psi_bits = control.psi_bits();
    let nc = control.nc as usize;
    let nr = control.nr as usize;
    let cols = nc.min(nr.saturating_sub(1));
    let mut writer = BitWriter::new();
    for _ in 0..subcarriers {
        for i in 1..=cols {
            let count = nr - i;
            for _ in 0..count {
                writer.write_bits(codeword & mask(phi_bits), phi_bits);
            }
            for _ in 0..count {
                writer.write_bits(codeword & mask(psi_bits), psi_bits);
            }
        }
    }
    frame.extend_from_slice(&writer.finish());
    frame
}

/// Bit mask of the lowest `n` bits.
fn mask(n: u32) -> u32 {
    if n >= 32 {
        u32::MAX
    } else {
        (1u32 << n) - 1
    }
}

/// MSB-first bit writer used only to construct synthetic test frames.
#[doc(hidden)]
struct BitWriter {
    bytes: Vec<u8>,
    cur: u8,
    nbits: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            cur: 0,
            nbits: 0,
        }
    }

    fn write_bits(&mut self, value: u32, n: u32) {
        for shift in (0..n).rev() {
            let bit = ((value >> shift) & 1) as u8;
            self.cur = (self.cur << 1) | bit;
            self.nbits += 1;
            if self.nbits == 8 {
                self.bytes.push(self.cur);
                self.cur = 0;
                self.nbits = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.cur <<= 8 - self.nbits;
            self.bytes.push(self.cur);
        }
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    fn su_control() -> MimoControl {
        MimoControl {
            nc: 2,
            nr: 3,
            bandwidth: Bandwidth::Bw20,
            codebook_large: false,
            mu_feedback: false,
        }
    }

    #[test]
    fn bitreader_reads_msb_first() {
        let mut r = BitReader::new(&[0b1010_0000]);
        assert_eq!(r.read_bits(1), Some(1));
        assert_eq!(r.read_bits(1), Some(0));
        assert_eq!(r.read_bits(2), Some(0b10));
        assert_eq!(r.remaining_bits(), 4);
    }

    #[test]
    fn bitreader_runs_out() {
        let mut r = BitReader::new(&[0xff]);
        assert_eq!(r.read_bits(8), Some(0xff));
        assert_eq!(r.read_bits(1), None);
    }

    #[test]
    fn dequantize_within_range() {
        // φ ∈ (0, π), ψ ∈ (0, π/2) roughly for valid codewords.
        let phi = dequantize_phi(0, 7);
        assert!(phi > 0.0 && phi < PI);
        let psi = dequantize_psi(0, 5);
        assert!(psi > 0.0 && psi < PI / 2.0);
    }

    #[test]
    fn mimo_control_roundtrips() {
        let ctrl = su_control();
        let frame = build_synthetic_frame(&ctrl, 1, 3);
        let parsed = parse_mimo_control(&frame[2..]).unwrap();
        assert_eq!(parsed, ctrl);
    }

    #[test]
    fn rejects_wrong_category() {
        let err = parse_vht_beamforming_report(&[0x04, 0x00, 0, 0, 0]).unwrap_err();
        assert!(matches!(err, BfiError::NotBeamformingReport { .. }));
    }

    #[test]
    fn parses_synthetic_frame_with_known_subcarriers() {
        let ctrl = su_control();
        let frame = build_synthetic_frame(&ctrl, 4, 5);
        let report = parse_vht_beamforming_report_with_subcarriers(&frame, 4).unwrap();
        assert_eq!(report.subcarriers.len(), 4);
        // angles_per_subcarrier for Nc=2,Nr=3: cols=2 -> 2*(2)+2*(1)=6 angles
        assert_eq!(report.control.angles_per_subcarrier(), 6);
        let sc = &report.subcarriers[0];
        assert_eq!(sc.phi.len() + sc.psi.len(), 6);
        // First phi codeword 5 dequantized.
        assert_relative_eq!(sc.phi[0], dequantize_phi(5, 7), epsilon = 1e-12);
    }

    #[test]
    fn infers_subcarrier_count() {
        let ctrl = su_control();
        let frame = build_synthetic_frame(&ctrl, 4, 1);
        let report = parse_vht_beamforming_report(&frame).unwrap();
        // Inference may round; should recover at least the encoded subcarriers.
        assert!(report.subcarriers.len() >= 4);
    }

    #[test]
    fn truncated_stream_errors() {
        let ctrl = su_control();
        let mut frame = build_synthetic_frame(&ctrl, 4, 1);
        frame.truncate(frame.len() - 2);
        let err = parse_vht_beamforming_report_with_subcarriers(&frame, 4).unwrap_err();
        assert!(matches!(err, BfiError::TruncatedAngles { .. }));
    }
}
