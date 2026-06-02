# ADR-059: Coal-Mine Architecture — Through-The-Earth + Pre-Deployed Survivable Nodes

| Field | Value |
|-------|-------|
| Status | Proposed |
| Date | 2026-06-03 |
| Deciders | ruv |
| Depends on | ADR-029 (RuvSense Multistatic), ADR-032 (Mesh Security Hardening) |
| Related | ADR-057 (Sub-GHz Multi-Band Front-End), ADR-058 (Breadcrumb Relay Mesh), Rescue Mode (`ruvsense/rescue_mode.rs`) |

## Context

The open question for RuView is whether it can help in deep coal mines. The answer requires brutal honesty, because lives and a family's hope are at stake and a misleading claim here is unconscionable.

### The Hard Physics Limit (stated plainly)

**WiFi/CSI — and any radio at hundreds of MHz to GHz — cannot sense humans through hundreds of meters of rock. This is physically impossible.** RF at these frequencies is absorbed within meters of solid earth; it does not penetrate the overburden of a deep mine. There is no signal-processing trick, no antenna, no integration gain that recovers a human-perturbation signal that has been attenuated into nonexistence by hundreds of meters of strata. Any product claiming to "see survivors through the mountain" with WiFi-class RF is misleading its customers. RuView will not make that claim.

ADR-057 (sub-GHz) improves *rubble* penetration by meters, not *bedrock* penetration by hundreds of meters. ADR-058 (breadcrumb mesh) extends *communication*, not *sensing*. Neither changes this limit.

### What Real Mine Rescue Actually Uses

The physics that *does* reach the surface from deep underground is **not** GHz RF:

- **Through-The-Earth (TTE) magnetic induction**, operating in the **~300 Hz – 30 kHz** band (ULF/VLF). At these wavelengths the dominant coupling is the quasi-static **magnetic** near-field, which penetrates rock far better than propagating RF. Deployed systems (e.g. MagneLink MCS, Kutta-class TTE) carry **very low bandwidth** — short status/text messages, not imagery, not pose.
- **Leaky-feeder** radiating cable run along the galleries (radio coverage that follows the tunnel, not through rock).
- **Seismic/acoustic survivor detection** (geophones, trapped-miner location systems): a trapped person tapping on rock or pipework produces seismic energy that surface geophone arrays can localize. This is the established non-RF method for *detecting* a survivor through deep rock, complementary to anything RuView provides.

### Implication for RuView

RuView's value in a deep mine is therefore **not** through-rock sensing. It is local sensing at points where nodes physically are, plus a low-bandwidth uplink to carry tiny "alive" messages to the surface. That is an infrastructure and pre-deployment story, not a portable scanner you wave at a mountain.

## Decision

For deep mines, RuView is defined as a **pre-deployed sensing infrastructure with a low-bandwidth survivable uplink**, explicitly *not* a through-rock scanner.

### 1. Pre-Deployed, Collapse-Survivable Sensing Nodes

- Sensing nodes are **distributed through the galleries in advance** (during normal operation), not carried in during a rescue.
- Each node is **ruggedized and battery-backed** to survive a collapse and keep running on internal power: sealed enclosure, intrinsically-safe design for methane environments, multi-day battery.
- Each node senses only its **local** footprint — local presence and vitals (breathing) of anyone in its immediate gallery section, using the same RuvSense pipeline (and, per ADR-057, sub-GHz front-ends for the better local penetration around fallen debris within a gallery).
- A survivor near a surviving node is detected *locally*; the node then needs only to get a few bytes to the surface.

### 2. Low-Bandwidth Survivable Uplink to the Surface

Two complementary backhaul options, neither of which is GHz RF through rock:

- **Through-The-Earth (TTE) magnetic-induction uplink (~300 Hz – 30 kHz).** A node (or a gallery aggregator) drives a large loop antenna; a surface receiver picks up the magnetic near-field. Bandwidth is tiny — enough for **"node N: alive, breathing rate B, timestamp T"**, *not* pose, *not* CSI. This is the link of last resort when galleries are severed.
- **Leaky-feeder / wired mesh backhaul** along the galleries where the infrastructure survives the event. This carries more bandwidth than TTE but only where the cable/tunnel path is intact. ADR-058's store-and-forward DTN model applies directly here: galleries reconnect intermittently, and bundles are forwarded opportunistically toward the portal.

The node software treats "which uplink is available" the same way ADR-057's front-end treats "which band is available": pick the best surviving path, ship the highest-priority (ALIVE) bundle first (ADR-058 priority classes), accept that TTE is status-only.

### 3. Seismic/Acoustic as the Complementary Non-RF Method

RuView explicitly defers to / integrates with **seismic-acoustic survivor detection** (surface geophone arrays locating a trapped miner's tapping). This is the proven physics for *detecting* a survivor through deep rock independent of any pre-placed node. Where RuView provides "a node near them confirms breathing," geophones provide "we can localize them even with no node nearby." They are complementary, not competing, and we will present them that way.

### 4. Capability Honesty in the Product

The capability/witness model (ADR-028 lineage) must, for the mine profile, report:
- Sensing: **local presence + breathing only, at node locations.**
- Uplink (TTE): **status messages only — no pose, no imaging.**
- Coverage: **only where nodes were pre-deployed and survived.**
- Through-rock sensing: **NOT SUPPORTED — physically impossible.**

## Consequences

### Positive

- **Honest, deliverable value.** Pre-deployed local sensing + a status uplink is genuinely achievable and genuinely useful: "node 14 in the west gallery reports a living, breathing person" is actionable rescue intelligence.
- **Matches real mine-rescue physics.** TTE magnetic induction and leaky-feeder/geophone methods are what mine rescue actually uses; RuView slots in alongside them instead of contradicting them.
- **Reuses RuView pipeline + mesh.** Local sensing is the existing RuvSense path; the uplink reuses ADR-058's prioritized store-and-forward bundles.

### Negative

- **Infrastructure model, not a portable scanner.** This requires pre-installation, maintenance, and survivability engineering of nodes throughout a mine — a major operational and cost commitment by the operator.
- **Very low TTE bandwidth.** TTE carries status only. Families and operators must understand the uplink says "alive," not "here is their posture/condition in detail."
- **Significant hardware and standards work.** Intrinsically-safe (methane-rated) enclosures, TTE loop-antenna driver electronics, and the low-frequency uplink modem are specialized hardware with regulatory/safety certification burdens far beyond commodity ESP32 work.
- **Coverage gaps are real.** A survivor with no surviving node nearby is invisible to RuView; only geophone/seismic methods can find them. We must not imply blanket coverage.

### Hard Limit — Stated for the Record

> RuView **cannot** sense humans through hundreds of meters of rock. No RF method at WiFi/sub-GHz frequencies can. For deep mines, RuView's role is pre-deployed local sensing plus a low-bandwidth TTE/leaky-feeder uplink, complemented by seismic-acoustic detection. We will state this limit to mine operators and to families directly and without softening it. Overpromising here is not a marketing risk — it is a betrayal of people waiting for news.

## References

- ADR-029: RuvSense Multistatic Sensing Mode (local sensing pipeline)
- ADR-032: Mesh Security Hardening (authenticated ALIVE bundles over the uplink)
- ADR-057: Sub-GHz Multi-Band Front-End (better *local* penetration around debris within a gallery — not through bedrock)
- ADR-058: Breadcrumb Relay Mesh (store-and-forward DTN model reused for intermittent gallery backhaul)
- Rescue Mode (`ruvsense/rescue_mode.rs`) — generates the local ALIVE/breathing payloads the uplink carries
- Through-The-Earth communications: MagneLink MCS, Kutta-class TTE systems (~300 Hz – 30 kHz magnetic induction)
- Leaky-feeder mine radio; surface geophone trapped-miner location systems (seismic-acoustic, non-RF)
