# ESP32-S3 CSI Node — macOS Setup Guide

Plug-and-play setup for the RuView WiFi-sensing nodes on a Mac (Mac mini or
laptop). This guide gets **raw CSI data flowing from your ESP32-S3 boards to
the sensing-server** — nothing more is promised. Pose/vital-sign *quality*
depends on placement, calibration, and tuning that are out of scope here; the
goal is a working pipeline you can build on.

Tested target hardware: **ESP32-S3-DevKitC-1 N16R8** (16 MB flash, 8 MB PSRAM,
dual USB-C, native USB — no soldering, usually no driver).

---

## TL;DR — one command per board

From the repo root, with Docker Desktop running and one board plugged in:

```bash
# Board #1
scripts/esp32-setup.sh --node 0 --ssid "YourWiFi" --aggregator-ip <MAC_LAN_IP>
```

`<MAC_LAN_IP>` is the LAN IP of the machine that will run the sensing-server
(for local testing, your Mac). Find it with:

```bash
ipconfig getifaddr en0   # Wi-Fi
ipconfig getifaddr en1   # Ethernet (Mac mini wired)
```

The script will: build the firmware (first time only) → detect the serial
port → flash → provision WiFi + aggregator IP → print how to verify CSI.

Repeat for `--node 1` and `--node 2`, plugging in one board at a time.

---

## Prerequisites

| Requirement | Why | Install |
|-------------|-----|---------|
| **Docker Desktop** | Cross-compiles the firmware in the ESP-IDF container (the only reliable build path). | <https://www.docker.com/products/docker-desktop/> — launch it and wait for the whale icon to settle. |
| **Python 3** | Runs `esptool` (flashing) and `provision.py` (NVS config). | Preinstalled on macOS, or `brew install python`. |
| **esptool** | Flashes the `.bin` files to the board. | `python3 -m pip install esptool` |
| **USB-C data cable** | Carries data, not just power. Many cables are charge-only. | Use the one that came with the board, or a known data cable. |
| **CP210x driver** | *Usually NOT needed.* The DevKitC-1 N16R8 uses **native USB** and enumerates as `usbmodem*`. Only boards with a Silicon Labs CP210x bridge need the [CP210x VCP driver](https://www.silabs.com/developers/usb-to-uart-bridge-vcp-drivers) (they show up as `cu.SLAB_USBtoUART`). |

Verify the toolchain before you start:

```bash
docker info >/dev/null && echo "Docker OK"
python3 -m esptool version
```

---

## Finding the serial port on macOS

When you plug a board in, macOS creates a device file under `/dev/`:

```bash
ls /dev/cu.*
```

You are looking for one of:

| Pattern | Typical board |
|---------|---------------|
| `/dev/cu.usbmodemXXXX` | ESP32-S3-DevKitC-1 (native USB) — **most likely** |
| `/dev/cu.usbserialXXXX` | Boards with a generic USB-UART bridge |
| `/dev/cu.SLAB_USBtoUART` | Boards with a Silicon Labs CP210x bridge |

All three scripts auto-detect this. You only need `--port` when **more than
one** board is connected at once (recommended: plug in one board at a time).

> Use the `cu.*` (call-up) device, not `tty.*`. The scripts default to `cu.*`.

---

## What each script does

| Script | Purpose |
|--------|---------|
| `scripts/esp32-build.sh` | Builds the firmware via Docker + ESP-IDF. Outputs three `.bin` files under `firmware/esp32-csi-node/build/`. |
| `scripts/esp32-flash.sh` | Auto-detects the port, flashes bootloader (`0x0`) + partition table (`0x8000`) + app (`0x10000`) with `--flash_size 8MB`. |
| `scripts/esp32-provision.sh` | Writes WiFi SSID/password + aggregator IP + node-id/TDM slot to NVS (no reflash needed). Password is never logged. |
| `scripts/esp32-setup.sh` | Runs build → flash → provision for one board, then prints verification steps. |

Each script supports `--help`.

---

## Step-by-step (all 3 boards, TDM mesh)

The three boards form a **3-node TDM mesh**. The `--node` flag is a
convenience that sets, for board `N`:

| Board | `--node` | node-id | TDM slot | TDM total |
|-------|----------|---------|----------|-----------|
| #1 | `0` | 0 | 0 | 3 |
| #2 | `1` | 1 | 1 | 3 |
| #3 | `2` | 2 | 2 | 3 |

Each node transmits in its own time slot so they don't collide on air.

### 1. Build once

```bash
scripts/esp32-build.sh
```

(The setup script does this automatically on the first board; running it
standalone first just gets the slow Docker pull out of the way.)

### 2. Set up each board

Plug in **one board at a time** and run:

```bash
# Board #1
scripts/esp32-setup.sh --node 0 --ssid "YourWiFi" --aggregator-ip 192.168.1.50

# Board #2 (unplug #1, plug in #2)
scripts/esp32-setup.sh --node 1 --ssid "YourWiFi" --aggregator-ip 192.168.1.50

# Board #3
scripts/esp32-setup.sh --node 2 --ssid "YourWiFi" --aggregator-ip 192.168.1.50
```

You'll be prompted for the WiFi password (hidden input). To run unattended,
pass `--password 'secret'` (note: it then appears in your shell history).

If you prefer the individual steps:

```bash
scripts/esp32-flash.sh                       # flash the connected board
scripts/esp32-provision.sh --node 0 \
    --ssid "YourWiFi" --aggregator-ip 192.168.1.50
```

---

## Pointing the nodes at the sensing-server

The ESP32 sends CSI over **UDP to `<aggregator-ip>:5005`**. The
sensing-server must be told to read from the ESP32 source.

### Local testing (server on your Mac)

```bash
cd rust-port/wifi-densepose-rs
CSI_SOURCE=esp32 cargo run -p wifi-densepose-sensing-server -- \
  --http-port 3000 --source esp32
```

Set `--aggregator-ip` (during provisioning) to **this Mac's LAN IP** so the
boards can reach it. `127.0.0.1` will NOT work — the ESP32 is a separate
device on the network.

### Remote server

If the sensing-server runs on the remote host (e.g. `144.76.180.37`), set
`--aggregator-ip 144.76.180.37` when provisioning, and make sure UDP 5005 is
reachable from your WiFi network to that host. The server there must also run
with `CSI_SOURCE=esp32` / `--source esp32`.

> To re-point already-flashed boards at a different aggregator, just re-run
> `scripts/esp32-provision.sh` with the new `--aggregator-ip`. No reflash.

---

## Confirming CSI is flowing

1. Start the sensing-server with `--source esp32` (above).
2. Power-cycle a provisioned board. Within ~10 seconds it should join WiFi
   and begin streaming.
3. In the **server log**, look for the source being reported as `esp32` and a
   **non-zero CSI frame count / rate** (~20 Hz per node). That confirms
   frames are arriving from the node over UDP 5005.
4. Open the UI at <http://localhost:3000>.

A quick independent sniff (optional), to prove UDP packets reach the Mac:

```bash
# Watch for UDP traffic on 5005 (Ctrl-C to stop). Requires sudo.
sudo tcpdump -i any -n udp port 5005
```

You should see packets sourced from each node's WiFi IP.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| **No serial port found** | Charge-only cable, or board on the wrong USB-C port. | Use a data cable; the DevKitC-1 has two USB-C ports — try the other. Then `ls /dev/cu.*`. |
| **Multiple ports detected** | More than one board (or other USB serial device) connected. | Plug in one board at a time, or pass `--port /dev/cu.usbmodemXXXX`. |
| **Permission denied on /dev/cu.*** | Another process holds the port (often a serial monitor). | Close any serial monitor / `screen` session. As a last resort, unplug/replug. |
| **`esptool` not installed** | Missing pip package. | `python3 -m pip install esptool` |
| **Docker not found / daemon unreachable** | Docker Desktop not installed or not started. | Install/launch Docker Desktop; wait until it reports running. |
| **Flash fails / timeouts** | Baud too high for a long cable, or board not in download mode. | Retry with `--baud 115200`. If it persists, hold **BOOT**, tap **RST**, release **BOOT**, then re-flash. |
| **`Wrong flash size` / boot loop** | Flashed with the wrong size. | These boards are 8 MB usable for this layout; the scripts already pass `--flash_size 8MB`. Re-flash with `scripts/esp32-flash.sh`. |
| **WiFi won't connect** | Wrong SSID/password, or 5 GHz-only network. | ESP32-S3 is 2.4 GHz only. Re-run `scripts/esp32-provision.sh` with correct credentials on a 2.4 GHz SSID. |
| **No CSI frames at server** | Firewall, wrong aggregator IP, or server not in esptool mode. | Allow inbound UDP 5005 on the Mac (System Settings → Network → Firewall, or temporarily disable). Confirm `--aggregator-ip` is the Mac's LAN IP, not `127.0.0.1`. Start the server with `--source esp32`. Verify with `tcpdump` above. |
| **Server shows source but zero frames** | Board powered but not transmitting yet. | Wait through the WiFi join; check the node's serial log at 115200 baud (`python3 -m serial.tools.miniterm /dev/cu.usbmodemXXXX 115200`). |

---

## See also

- `firmware/esp32-csi-node/README.md` — full firmware reference (tiers, wire
  protocols, NVS keys, WASM Tier 3, memory budget).
- `firmware/esp32-csi-node/provision.py` — the underlying provisioning tool
  (all NVS knobs: edge tier, presence/fall thresholds, vitals window, etc.).
