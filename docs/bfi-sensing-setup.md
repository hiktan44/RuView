# Hardware-Free WiFi Sensing via BFI (Beamforming Feedback Information)

RuView can detect **presence** and recognize a person's **gait** from ordinary
802.11ac/ax routers **without any firmware modification** — by passively sniffing
the unencrypted Beamforming Feedback (BFI) frames that stations send to the AP.
This implements PRD requirement **FR-1.2 (dual-mode sensing)**.

This guide explains what hardware works, why laptop built-in cards usually don't,
and the exact commands to run once you have a monitor-mode adapter.

---

## What BFI sensing can and cannot do

| Capability | BFI (this guide) | CSI (ESP32 / research NIC) |
|------------|------------------|----------------------------|
| Presence (Absent / Still / Active) | ✅ yes | ✅ yes |
| Gait identity (who is walking) | ✅ yes | ✅ yes |
| Full body pose (DensePose) | ❌ no | ✅ yes |
| Breathing / heart rate | ❌ no | ✅ yes |
| Firmware change required | **No** | Yes (ESP32 firmware) |
| Hardware cost | ~$25 USB adapter | ~$5 ESP32-S3 |

BFI is **unencrypted** (sent before the data link is secured, to minimise
latency), so a passive monitor can read it without joining the network. That is
the whole reason hardware-free sensing is possible.

---

## Why your laptop's built-in Wi-Fi usually won't work

BFI capture needs the adapter to deliver raw 802.11 **management/action frames**
in **monitor mode**, locked to the AP's channel. Most built-in cards fail this:

- **Apple Wi-Fi (incl. Mac mini / MacBook, Broadcom/Apple silicon):** monitor
  mode often returns **0 packets** even with `sudo tcpdump -I`. The driver keeps
  the card "associated" and does not surface VHT beamforming action frames. This
  is a hardware/driver limit, not a software one — confirmed on this repo's test
  Mac mini (`0 packets captured by filter`).
- **Many Intel laptop cards (Windows/Linux):** monitor mode is restricted or
  channel-locking is unreliable.

**Verdict:** for dependable BFI capture, use a USB monitor-mode adapter.

### Recommended adapters (monitor mode + VHT, well-supported)

| Adapter | Chipset | Notes |
|---------|---------|-------|
| **Alfa AWUS036ACM** | MediaTek **mt76** (mt7612u) | Best pick. Excellent Linux monitor mode, 2.4 + 5 GHz, VHT. ~$25 |
| Alfa AWUS036ACH | Realtek rtl8812au | Works with the `aircrack-ng/rtl8812au` driver; 5 GHz VHT |
| Panda / generic mt7612u | mt76 | Cheaper mt76 alternatives |

Prefer **mt76** — it is in-tree in the Linux kernel and "just works" in monitor
mode without out-of-tree drivers.

---

## Quick capability check (no adapter needed)

```bash
cd /path/to/RuView
PYTHONPATH=. python3 -m v1.src.sensing.bfi_capture --probe
```

This prints your platform, Wi-Fi interface, whether `tcpdump` is present, and
per-OS guidance. It does **not** need root.

---

## Live capture — Linux (recommended)

With an mt76 USB adapter plugged in (say it enumerates as `wlan1`):

```bash
# 1) Put the adapter into monitor mode
sudo ip link set wlan1 down
sudo iw dev wlan1 set type monitor
sudo ip link set wlan1 up

# 2) Lock it to the AP's channel (find the AP channel first, e.g. 48 on 5 GHz)
sudo iw dev wlan1 set channel 48

# 3) Capture BFI + classify presence (move around to trigger beamforming)
cd /path/to/RuView
sudo PYTHONPATH=. python3 -m v1.src.sensing.bfi_capture \
    --listen --iface wlan1 --seconds 30 --dump /tmp/bfi.hex
```

To find the AP channel:
```bash
sudo iw dev wlan1 scan | grep -E 'SSID|freq|primary channel' | head
# or, while still associated on another iface:
iw dev wlan0 link
```

## Live capture — macOS (only if your card supports it)

```bash
cd /path/to/RuView
sudo PYTHONPATH=. python3 -m v1.src.sensing.bfi_capture \
    --listen --iface en1 --seconds 30 --dump /tmp/bfi.hex
```

macOS removed the `airport` utility, so channel locking is unreliable; if you get
`0 packets captured`, your card cannot do this — use a USB adapter on Linux.

---

## Validate captured frames through the Rust crate

The capture tool writes raw 802.11 MAC frames (one hex string per line) to the
`--dump` file. Feed them to the production parser in `wifi-densepose-bfi`:

```bash
cd rust-port/wifi-densepose-rs
cargo build -p wifi-densepose-bfi --bin bfi-replay
./target/debug/bfi-replay /tmp/bfi.hex --window 32 --rate 20
```

Output:
```
== bfi-replay ==
input lines      : 412
parsed BFI reports: 388
skipped (non-BFI): 24

presence         : ACTIVE (confidence 0.78)
total variance   : 0.04213
motion band power: 0.18120
breathing power  : 0.00910

gait descriptor  : [0.123, -0.041, 0.337, ...] (len 64)
```

`bfi-replay` runs the **exact same** parser, presence classifier and gait
profiler the library ships, so whatever the adapter sniffs is validated by
production code.

---

## How it feeds the live system

- The capture tool can be extended to broadcast presence over the same WebSocket
  frame format the UI consumes (see `notebook_runner.py --ws` for the pattern).
- The `DualModeSelector` in `wifi-densepose-bfi` auto-switches between CSI (when
  an ESP32 is streaming on UDP :5005) and BFI (when only beamforming feedback is
  available), so a deployment can use whichever signal is present.

## Privacy note

Captured BFI is unencrypted and identifies people by gait with high accuracy.
Treat it as sensitive: process in memory, do not persist raw identities, and only
deploy where you are authorised to sense. See the privacy guidance in the
`wifi-densepose-bfi` crate docs.
