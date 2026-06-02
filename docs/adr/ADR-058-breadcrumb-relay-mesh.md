# ADR-058: Breadcrumb Relay Mesh for Deep Rubble Access

| Field | Value |
|-------|-------|
| Status | Proposed |
| Date | 2026-06-03 |
| Deciders | ruv |
| Depends on | ADR-029 (RuvSense Multistatic), ADR-032 (Multistatic Mesh Security Hardening) |
| Related | ADR-057 (Sub-GHz Multi-Band Front-End), ADR-059 (Coal-Mine TTE), Rescue Mode (`ruvsense/rescue_mode.rs`) |

## Context

In a collapsed structure, a single RF link cannot reach a sensing node placed deep inside the rubble *and* an operator standing at a safe distance outside. This is tempting to frame as a "range" problem to be solved by more sensing power — but that framing is wrong and we must be explicit about it.

### The Core Distinction: Sensing Range vs Communication Range

RuView separates two fundamentally different ranges, governed by different physics:

| | Sensing range | Communication range |
|---|---------------|----------------------|
| What it measures | Distance a node can *perceive* a human via RF perturbation | Distance a node's *result payload* can travel to the operator |
| Governing physics | RF propagation through bodies/rubble, link budget, integration gain | Digital packet transport over any hop medium |
| Bound | Short, local, hard physical limit (meters through rubble) | Effectively global — IP, multi-hop mesh, internet |
| Improved by | Lower frequency (ADR-057), processing gain (Rescue Mode) | More relay hops, store-and-forward |

The Istanbul-operates-a-Malatya-node scenario works **only** because of this separation: the *sensing* happens locally at the node in the rubble; the *communication* of that result travels over IP/mesh to an operator anywhere. Conflating the two leads to the false belief that a bigger antenna at the operator lets you "see" deeper into rubble. It does not. Range to the operator is a communication problem.

### Why Breadcrumbs

Rescuers physically advance into and around a collapse. As they go, they can drop cheap, battery-powered relay nodes — "breadcrumbs" — that self-form a multi-hop path back toward the command post. A sensing node placed near or inside a void then has a chain of short, survivable RF hops to ship its tiny payload out, even though no single hop spans the whole distance.

The payload is small by design: a coherence-gated detection result, a breathing rate, a node ID, a timestamp, an approximate location — bytes, not raw CSI streams. This matters because the links inside rubble are intermittent and low-bandwidth.

### Existing Foundation

ADR-029's multistatic mesh and ADR-032's mesh security hardening already define node-to-node coordination and a trust model. What is missing for the rubble case is **delay-tolerant, store-and-forward transport**: links here are not continuously connected, so the mesh must hold a message and forward it when the next hop becomes reachable, rather than dropping it.

## Decision

Extend the existing emergency mesh with **store-and-forward, delay-tolerant networking (DTN)** transport and **self-forming breadcrumb relays**, and codify the "sensing local, comms global" rule as an architectural invariant.

### 1. Store-and-Forward (Delay-Tolerant) Transport

Add a DTN-style bundle layer to the mesh:

- Each sensing result is wrapped as a **bundle** with a unique ID, priority class, TTL, and destination (the operator/command post).
- A node that cannot immediately forward a bundle **persists it** (NVS/flash) and retries opportunistically when a neighbor reappears (custody transfer).
- Bundles are de-duplicated by ID across multiple paths, so a result that finds two ways out is delivered once.

This is the well-understood DTN model (RFC 4838 Bundle Protocol family), adapted to small embedded relays.

### 2. Self-Forming Breadcrumb Relays

- Breadcrumb nodes are minimal: a radio (2.4 GHz mesh, and/or a sub-GHz link per ADR-057 for better diffraction around debris), a battery, flash, and the bundle layer. They do **no sensing** — they only relay.
- On power-up, a breadcrumb beacons, discovers neighbors, and inserts itself into the forwarding graph. As rescuers drop more, the chain extends automatically toward the interior.
- The graph is built bottom-up by proximity; there is no need for a pre-surveyed topology.

### 3. Message Prioritization — Alive-Signal First

Bundle priority classes, highest first:

1. **ALIVE** — a confirmed/likely-living detection (presence + breathing). Never dropped while TTL valid; preempts everything.
2. **VITALS** — breathing rate / vital updates for an already-reported subject.
3. **TELEMETRY** — node health, battery, link quality.
4. **BULK** — optional raw/diagnostic CSI snippets, only forwarded when bandwidth is spare.

Under congestion or battery pressure, low-priority classes are shed so the life-critical "someone is alive at node N" message gets out first.

### 4. Architectural Invariant

> **Sensing is local; communication is global.** A relay/mesh hop extends how far a *result* travels. It does **not** extend how far any single node can *sense*. The mesh moves bytes, not perception.

This invariant is enforced in code review and in the API contract: a node's reported detection is always attributed to *that node's* local sensing footprint, regardless of how many hops it took to arrive.

## Consequences

### Positive

- **Deep-rubble payloads get out.** A node near a void can deliver "alive + breathing" over a chain of short, survivable hops even when no single link reaches the operator.
- **Enables remote operation.** Directly supports the Istanbul-operates-Malatya-node model — the operator can be anywhere reachable over IP once the bundle exits the rubble onto a backhaul.
- **Robust to intermittency.** Store-and-forward tolerates the on/off connectivity that is normal inside a collapse; no continuous end-to-end path is required.
- **Life-first under stress.** Priority classes guarantee the ALIVE message survives congestion and battery shedding.
- **Builds on existing mesh + security.** Extends ADR-029/ADR-032 rather than introducing a parallel network stack.

### Negative

- **Node cost and logistics.** Breadcrumbs are cheap individually but must be carried, deployed, and recovered; a real collapse may need dozens.
- **Battery.** Relays must run for hours on battery in a hostile environment; sleep/duty-cycling vs. forwarding latency is a real tension (ALIVE bundles must still get out fast).
- **Intermittent-connectivity complexity.** Custody transfer, bundle persistence, de-duplication, and TTL expiry are non-trivial to implement and test correctly on embedded hardware.
- **Security surface.** More nodes and store-and-forward custody widen the attack/spoofing surface; ADR-032's hardening (authenticated bundles, replay protection) must extend to the DTN layer so a fake ALIVE cannot be injected.

### What the Mesh Does NOT Solve (Honest Limits)

- It does **not** extend the physical sensing reach of any single node. A breadcrumb cannot make a buried node "see" farther into rock — it only carries that node's local result outward.
- It does **not** create coverage where no node is physically present. If no sensing node is near a survivor, the mesh has nothing to forward.
- It does **not** overcome the link budget *of the sensing link itself*; that remains the domain of ADR-057 (band choice) and Rescue Mode (integration gain).

## References

- ADR-029: RuvSense Multistatic Sensing Mode (node coordination foundation)
- ADR-032: Multistatic Mesh Security Hardening (trust model extended to DTN bundles)
- ADR-057: Sub-GHz Multi-Band Front-End (sub-GHz hops diffract better around debris; produces the small payload carried here)
- ADR-059: Coal-Mine Architecture (the deep-mine analogue uses leaky-feeder/TTE backhaul in place of breadcrumbs)
- Rescue Mode (`ruvsense/rescue_mode.rs`) — generates the ALIVE/VITALS payloads prioritized by this mesh
- RFC 4838 / Bundle Protocol — delay-tolerant networking model
