#!/usr/bin/env bash
#
# esp32-flash.sh — Flash the RuView ESP32-S3 CSI node firmware on macOS.
#
# Auto-detects the connected board's serial port, then flashes
# bootloader + partition table + app at the standard ESP-IDF offsets
# (0x0, 0x8000, 0x10000) for the default single-app partition layout that
# this firmware builds with.
#
# This script performs a real flash (it talks to hardware). If no board is
# connected it stops cleanly before doing anything destructive.
#
set -euo pipefail

# --- Resolve repo paths ----------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FW_DIR="${REPO_ROOT}/firmware/esp32-csi-node"
BUILD_DIR="${FW_DIR}/build"

BOOTLOADER="${BUILD_DIR}/bootloader/bootloader.bin"
PARTTABLE="${BUILD_DIR}/partition_table/partition-table.bin"
APP="${BUILD_DIR}/esp32-csi-node.bin"

# Default single-app (factory) layout offsets — matches the firmware README
# and the default partition table the build produces.
OFF_BOOTLOADER="0x0"
OFF_PARTTABLE="0x8000"
OFF_APP="0x10000"

PORT=""
BAUD="460800"
FLASH_SIZE="8MB"

usage() {
  cat <<'EOF'
esp32-flash.sh — Flash the ESP32-S3 CSI node firmware (macOS).

USAGE:
  scripts/esp32-flash.sh [--port <dev>] [--baud <rate>] [--help]

OPTIONS:
  --port <dev>   Serial device (e.g. /dev/cu.usbmodem1101). If omitted the
                 script auto-detects it. Required if multiple boards are found.
  --baud <rate>  Flash baud rate (default: 460800). Try 115200 if flashing
                 fails on a long/cheap cable.
  -h, --help     Show this help and exit.

WHAT IT FLASHES (default single-app layout):
  0x0       bootloader/bootloader.bin
  0x8000    partition_table/partition-table.bin
  0x10000   esp32-csi-node.bin

REQUIREMENTS:
  - Firmware already built (run scripts/esp32-build.sh first).
  - esptool installed:  python3 -m pip install esptool
  - A USB-C DATA cable from the Mac to the board's USB port.

PORT DETECTION (macOS):
  Scans for /dev/cu.usbmodem*, /dev/cu.usbserial*, /dev/cu.SLAB_USBtoUART*.
  The ESP32-S3-DevKitC-1 enumerates as a native USB device (usbmodem*) and
  usually needs no driver.
EOF
}

# --- Parse args ------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --port)
      [ $# -ge 2 ] || { echo "Error: --port requires a value" >&2; exit 2; }
      PORT="$2"; shift 2 ;;
    --baud)
      [ $# -ge 2 ] || { echo "Error: --baud requires a value" >&2; exit 2; }
      BAUD="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Error: unknown argument: $1" >&2; echo "Run with --help for usage." >&2; exit 2 ;;
  esac
done

# --- Validate inputs -------------------------------------------------------
if ! printf '%s' "${BAUD}" | grep -Eq '^[0-9]+$'; then
  echo "Error: --baud must be a positive integer (got: ${BAUD})" >&2
  exit 2
fi

# --- esptool availability --------------------------------------------------
PY="${PYTHON:-python3}"
if ! command -v "${PY}" >/dev/null 2>&1; then
  echo "Error: python3 not found. Install Python 3 (e.g. 'brew install python')." >&2
  exit 1
fi
if ! "${PY}" -m esptool version >/dev/null 2>&1; then
  cat >&2 <<EOF
Error: esptool is not installed for ${PY}.

Install it with:
    ${PY} -m pip install esptool

Then re-run this script.
EOF
  exit 1
fi

# --- Build artifacts present? ----------------------------------------------
missing=0
for f in "${BOOTLOADER}" "${PARTTABLE}" "${APP}"; do
  if [ ! -s "$f" ]; then
    echo "Error: missing firmware artifact: $f" >&2
    missing=1
  fi
done
if [ "${missing}" -ne 0 ]; then
  echo >&2
  echo "Build the firmware first:  scripts/esp32-build.sh" >&2
  exit 1
fi

# --- Serial port auto-detection (macOS) ------------------------------------
# Lists candidate /dev/cu.* devices, one per line, no trailing empties.
detect_ports() {
  local p
  for p in /dev/cu.usbmodem* /dev/cu.usbserial* /dev/cu.SLAB_USBtoUART*; do
    [ -e "$p" ] && printf '%s\n' "$p"
  done
}

if [ -z "${PORT}" ]; then
  # Read candidates into a list safely.
  ports=()
  while IFS= read -r line; do
    [ -n "$line" ] && ports+=("$line")
  done < <(detect_ports)

  case "${#ports[@]}" in
    0)
      cat >&2 <<'EOF'
Error: no ESP32 serial port found.

Checked: /dev/cu.usbmodem*  /dev/cu.usbserial*  /dev/cu.SLAB_USBtoUART*

Checklist:
  - Use a USB-C DATA cable (many charge-only cables look identical).
  - Plug into the board's USB port (the DevKitC-1 has two USB-C ports;
    try the other one if nothing appears).
  - List devices yourself:  ls /dev/cu.*
  - Then pass it explicitly:  scripts/esp32-flash.sh --port /dev/cu.usbmodemXXXX
EOF
      exit 1
      ;;
    1)
      PORT="${ports[0]}"
      echo "==> Auto-detected serial port: ${PORT}"
      ;;
    *)
      echo "Error: multiple serial ports detected — pick one with --port:" >&2
      for p in "${ports[@]}"; do echo "    ${p}" >&2; done
      echo >&2
      echo "Example:  scripts/esp32-flash.sh --port ${ports[0]}" >&2
      exit 1
      ;;
  esac
fi

# --- Sanity-check the chosen port ------------------------------------------
case "${PORT}" in
  /dev/cu.*|/dev/tty.*) : ;;
  *)
    echo "Error: --port should be a macOS device path like /dev/cu.usbmodemXXXX (got: ${PORT})" >&2
    exit 2 ;;
esac
if [ ! -e "${PORT}" ]; then
  echo "Error: serial device does not exist: ${PORT}" >&2
  echo "Is the board plugged in? List with: ls /dev/cu.*" >&2
  exit 1
fi

# --- Flash -----------------------------------------------------------------
echo "==> Flashing firmware"
echo "    Port:       ${PORT}"
echo "    Baud:       ${BAUD}"
echo "    Flash size: ${FLASH_SIZE}"
echo "    ${OFF_BOOTLOADER}  bootloader.bin"
echo "    ${OFF_PARTTABLE}  partition-table.bin"
echo "    ${OFF_APP}  esp32-csi-node.bin"
echo

"${PY}" -m esptool \
  --chip esp32s3 \
  --port "${PORT}" \
  --baud "${BAUD}" \
  write_flash \
  --flash_mode dio \
  --flash_size "${FLASH_SIZE}" \
  "${OFF_BOOTLOADER}" "${BOOTLOADER}" \
  "${OFF_PARTTABLE}" "${PARTTABLE}" \
  "${OFF_APP}" "${APP}"

echo
echo "==> Flash complete."
echo "Next: provision WiFi + aggregator IP with"
echo "    scripts/esp32-provision.sh --node 0 --ssid <SSID> --aggregator-ip <MAC_IP>"
