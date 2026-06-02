# ADR-057: Sub-GHz Multi-Band Front-End for Rubble Penetration

| Field | Value |
|-------|-------|
| Status | Proposed |
| Date | 2026-06-03 |
| Deciders | ruv |
| Depends on | ADR-014 (SOTA Signal Processing), ADR-029 (RuvSense Multistatic), ADR-042 (Coherent Human Channel Imaging) |
| Related | ADR-058 (Breadcrumb Relay Mesh), ADR-059 (Coal-Mine TTE), Rescue Mode (`ruvsense/rescue_mode.rs`) |

## Context

RuView's sensing currently operates on the 2.4 / 5 / 6 GHz WiFi ISM bands. These bands are excellent for room-scale pose estimation but are a poor choice for sensing **through rubble** in an earthquake/collapse scenario. The physics is unambiguous and we will not pretend otherwise.

### Why High-Band WiFi Fails Through Rubble

Two independent effects make 2.4–6 GHz penetrate dense, wet, rebar-laced concrete debris poorly:

1. **Material attenuation rises with frequency.** Loss through reinforced concrete is roughly 10–30 dB at 2.4 GHz per wall, climbing further at 5/6 GHz. Through a meter-plus of fragmented, moisture-laden rubble the cumulative loss easily exceeds the link budget — the signal is simply gone before it reaches a buried survivor and returns.
2. **Short wavelength couples poorly to large obstructions.** At 2.4 GHz, λ ≈ 12.5 cm; at 5 GHz, λ ≈ 6 cm. Wavelengths this short do not diffract around meter-scale rubble blocks — they scatter and are absorbed. Lower frequencies, with longer wavelengths, **diffract** around obstacles and creep through voids.

| Band | λ | Rubble/concrete penetration | Spatial resolution | Sensing capability through rubble |
|------|-----|----------------------------|--------------------|-----------------------------------|
| 6 GHz | 5.0 cm | Moderate→Poor | Highest | Surface / very thin debris only |
| 5 GHz | 6.0 cm | Poor | High | Surface / thin debris only |
| 2.4 GHz | 12.5 cm | Fair (1 wall) | Good | Through 1–2 walls; not deep rubble |
| 915 MHz (ISM) | 32.8 cm | Good | Coarse | Alive/no + breathing through rubble |
| 868 MHz (ISM, EU) | 34.5 cm | Good | Coarse | Alive/no + breathing through rubble |
| 433 MHz (ISM) | 69.2 cm | Very good | Very coarse | Alive/no + gross motion through deeper rubble |

The honest summary: **frequency trades against penetration and resolution.** You cannot have deep penetration *and* fine pose resolution from one band. Sub-GHz buys you a life-detection answer ("someone is alive and breathing under here") where 2.4 GHz buys you nothing; full DensePose-grade pose still requires the high band on a clear or near-surface link.

### Why This Belongs in the Existing Pipeline

`ruvsense/multiband.rs` already fuses CSI frames from multiple center frequencies into a single `MultiBandCsiFrame` annotated with per-channel center frequency and cross-channel coherence (today: channels 1/6/11 from 2.4 GHz hopping). Its data model is **frequency-agnostic by construction** — it carries center frequencies as explicit metadata rather than assuming a band. This makes it the natural host for sub-GHz channels: a 915 MHz or 433 MHz CSI/RSSI row is just another entry in the frequency list, weighted by its own coherence.

The Rescue Mode processing-gain work (`ruvsense/rescue_mode.rs`, added in parallel) is where sub-GHz pays off most: long coherent integration over a stationary buried subject pulls a faint breathing Doppler out of the noise. Sub-GHz's better penetration plus Rescue Mode's integration gain is the combination that turns "no signal" into "weak but real vital sign."

## Decision

Define a **frequency-agnostic RF front-end abstraction** so the signal pipeline can ingest sub-GHz CSI/RSSI alongside 2.4/5/6 GHz, without the downstream fusion, coherence, and Rescue Mode code needing to know which physical radio produced a frame.

### 1. Front-End Trait

Introduce a `SensingFrontEnd` abstraction in `wifi-densepose-hardware` that yields band-tagged measurements:

```rust
/// A frequency-agnostic source of channel measurements.
/// Implementors: WiFiCsiFrontEnd (2.4/5/6 GHz), SubGhzFrontEnd (433/868/915 MHz).
pub trait SensingFrontEnd {
    /// Center frequency of this front-end's current channel, in MHz.
    fn center_freq_mhz(&self) -> u32;

    /// Native measurement richness. Sub-GHz transceivers typically
    /// expose RSSI + coarse phase, not full per-subcarrier CSI.
    fn measurement_kind(&self) -> MeasurementKind; // FullCsi | RssiOnly | CoarsePhase

    /// Pull the next band-tagged measurement.
    fn next_measurement(&mut self) -> Result<BandTaggedMeasurement, FrontEndError>;
}
```

`BandTaggedMeasurement` carries the center frequency, the measurement kind, and the payload. `multiband.rs` fusion already keys on center frequency, so a sub-GHz RSSI-only row participates in fusion as a low-resolution, high-penetration channel.

### 2. Honest Capability Reporting

Each front-end reports what it can *actually* deliver at its band, surfaced through the existing capability/witness model (ADR-028 lineage). A 433 MHz front-end reports `presence + breathing`, never `pose`. The UI and API must render this distinction so an operator never reads a sub-GHz "alive" hit as a localized skeleton.

### 3. Band-Aware Fusion Weights

Extend `multiband.rs` so fusion weights are band-aware: sub-GHz channels dominate the **detection / vital** decision (they are the only channels that survive the rubble), while high-band channels — when present — contribute the **localization / pose** refinement. The cross-channel coherence score already computed in the module gates whether a high-band channel is even usable; under deep rubble it will correctly read as incoherent and be down-weighted to zero for pose.

### 4. Rescue Mode Coupling

`rescue_mode.rs` integration: when operating on a sub-GHz front-end, Rescue Mode extends its coherent integration window (the buried subject is stationary, the only motion is respiration at ~0.2–0.5 Hz), trading latency for processing gain. The front-end abstraction lets Rescue Mode request "longest-penetration band available" without hard-coding a radio.

## Consequences

### Positive

- **A life-detection answer where there was none.** Sub-GHz gives "alive / breathing" through rubble that 2.4 GHz physically cannot reach.
- **Reuses existing fusion.** `multiband.rs` is already frequency-keyed; sub-GHz slots in as additional channels rather than a parallel pipeline.
- **Composes with Rescue Mode.** Better penetration × longer integration is the right multiplier for buried, stationary survivors.
- **Honest by design.** Per-band capability reporting prevents over-reading a coarse sub-GHz hit as a precise pose.

### Negative

- **Extra hardware.** Sub-GHz needs dedicated transceivers (SX1276/SX1262 LoRa-class, or CC1101-class sub-GHz radios) and antennas. This is a hardware-dependent direction, not a firmware-only upgrade. ESP32 main MCUs can host these over SPI, but they are added BOM.
- **Reduced spatial resolution on the low band.** Long wavelength means coarse localization — sub-GHz answers "is someone alive here," not "their left arm is at (x, y, z)."
- **Regulatory band fragmentation.** Sub-GHz ISM allocations are *regional*: 902–928 MHz (FCC, Americas), 863–870 MHz (ETSI, EU), 433 MHz (ITU Region 1 ISM). Duty-cycle and power limits differ per region (e.g., EU 868 MHz duty-cycle caps). The front-end must be region-configured and must not assume a global band plan.
- **Lower native richness.** Many sub-GHz radios provide RSSI + coarse phase, not per-subcarrier CSI. Fusion and Rescue Mode must handle `RssiOnly` measurements gracefully (this is why `MeasurementKind` is explicit).

### Neutral

- This ADR is **Proposed** and hardware-dependent. The trait/abstraction can land first (with a stub sub-GHz front-end and simulated data) so the software path is ready before radios are sourced.

## Honest Physics Statement

Sub-GHz improves penetration; it does not create resolution. No band choice lets WiFi-class RF reconstruct a full pose through deep rubble. The realistic deliverable through rubble is **presence + respiration**, and only when combined with the Rescue Mode integration gain. We will say exactly this to rescue teams — nothing more.

## References

- ADR-014: SOTA Signal Processing
- ADR-029: RuvSense Multistatic Sensing Mode
- ADR-042: Coherent Human Channel Imaging (multi-band fusion, penetration vs resolution table)
- ADR-058: Breadcrumb Relay Mesh for Deep Rubble Access (carries the small sub-GHz result payload out)
- ADR-059: Coal-Mine Architecture — physics limits of RF through rock
- `ruvsense/multiband.rs` — frequency-keyed multi-band fusion (host for sub-GHz channels)
- `ruvsense/rescue_mode.rs` — coherent integration / processing gain for buried survivors
- FCC 47 CFR Part 15.247 (902–928 MHz); ETSI EN 300 220 (863–870 MHz); ITU Region 1 433 MHz ISM
