#!/usr/bin/env bash
#
# esp32-setup.sh — Do everything for ONE ESP32-S3 CSI board on macOS.
#
#   build (if needed)  ->  detect port  ->  flash  ->  provision  ->
#   tell the user how to verify CSI is arriving at the sensing-server.
#
# Interactive by default; every input can be overridden with a flag so the
# whole flow runs unattended.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FW_DIR="${REPO_ROOT}/firmware/esp32-csi-node"
BUILD_DIR="${FW_DIR}/build"
APP="${BUILD_DIR}/esp32-csi-node.bin"

NODE=""
SSID=""
PASSWORD=""
PASSWORD_SET=0
AGG_IP=""
AGG_PORT="5005"
TDM_TOTAL="3"
PORT=""
BAUD="460800"
FORCE_BUILD=0
SKIP_BUILD=0

usage() {
  cat <<'EOF'
esp32-setup.sh — One command to prepare ONE ESP32-S3 CSI board (macOS).

USAGE:
  scripts/esp32-setup.sh --node <N> [options]

STEPS PERFORMED:
  1. Build firmware (only if build/esp32-csi-node.bin is missing, or --build).
  2. Auto-detect the serial port.
  3. Flash bootloader + partition table + app.
  4. Provision WiFi + aggregator IP + node-id/TDM slot.
  5. Print how to start the sensing-server and confirm CSI is flowing.

REQUIRED (prompted if omitted):
  --node <N>             0, 1, or 2 — which board in the 3-node mesh.
  --ssid <SSID>          WiFi network the node joins.
  --aggregator-ip <IP>   Host running the sensing-server (your Mac LAN IP
                         for local testing).

OPTIONAL:
  --password <PW>        WiFi password (prompted, hidden, if omitted).
  --aggregator-port <P>  UDP port (default: 5005).
  --tdm-total <T>        Total mesh nodes (default: 3).
  --port <dev>           Serial device (auto-detected if omitted).
  --baud <rate>          Flash baud (default: 460800).
  --build                Force a clean rebuild even if a binary exists.
  --skip-build           Never build; fail if no binary is present.
  -h, --help             Show this help and exit.

EXAMPLE (board #1, local Mac aggregator):
  scripts/esp32-setup.sh --node 0 --ssid HomeWiFi --aggregator-ip 192.168.1.50
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --node)            NODE="${2:?--node needs a value}"; shift 2 ;;
    --ssid)            SSID="${2:?--ssid needs a value}"; shift 2 ;;
    --password)        PASSWORD="${2-}"; PASSWORD_SET=1; shift 2 ;;
    --aggregator-ip)   AGG_IP="${2:?--aggregator-ip needs a value}"; shift 2 ;;
    --aggregator-port) AGG_PORT="${2:?--aggregator-port needs a value}"; shift 2 ;;
    --tdm-total)       TDM_TOTAL="${2:?--tdm-total needs a value}"; shift 2 ;;
    --port)            PORT="${2:?--port needs a value}"; shift 2 ;;
    --baud)            BAUD="${2:?--baud needs a value}"; shift 2 ;;
    --build)           FORCE_BUILD=1; shift ;;
    --skip-build)      SKIP_BUILD=1; shift ;;
    -h|--help)         usage; exit 0 ;;
    *) echo "Error: unknown argument: $1" >&2; echo "Run with --help for usage." >&2; exit 2 ;;
  esac
done

if [ "${FORCE_BUILD}" -eq 1 ] && [ "${SKIP_BUILD}" -eq 1 ]; then
  echo "Error: --build and --skip-build are mutually exclusive." >&2
  exit 2
fi
if [ -z "${NODE}" ]; then
  echo "Error: --node <N> is required (0, 1, or 2)." >&2
  exit 2
fi

# --- Step 1: build ---------------------------------------------------------
need_build=0
if [ "${FORCE_BUILD}" -eq 1 ]; then
  need_build=1
elif [ ! -s "${APP}" ]; then
  need_build=1
fi

if [ "${need_build}" -eq 1 ]; then
  if [ "${SKIP_BUILD}" -eq 1 ]; then
    echo "Error: no firmware binary at ${APP} and --skip-build was given." >&2
    echo "Run scripts/esp32-build.sh first, or drop --skip-build." >&2
    exit 1
  fi
  echo "==> [1/4] Building firmware (no binary found or --build requested)"
  build_args=()
  [ "${FORCE_BUILD}" -eq 1 ] && build_args+=( --clean )
  "${SCRIPT_DIR}/esp32-build.sh" "${build_args[@]}"
else
  echo "==> [1/4] Firmware already built: ${APP} (use --build to rebuild)"
fi

# --- Step 2 + 3: flash (auto-detects port) ---------------------------------
echo
echo "==> [2/4] Detecting board + [3/4] flashing"
flash_args=( --baud "${BAUD}" )
[ -n "${PORT}" ] && flash_args+=( --port "${PORT}" )
"${SCRIPT_DIR}/esp32-flash.sh" "${flash_args[@]}"

# --- Step 4: provision -----------------------------------------------------
echo
echo "==> [4/4] Provisioning node ${NODE}"
prov_args=( --node "${NODE}" --tdm-total "${TDM_TOTAL}" --baud "${BAUD}" --aggregator-port "${AGG_PORT}" )
[ -n "${PORT}" ]    && prov_args+=( --port "${PORT}" )
[ -n "${SSID}" ]    && prov_args+=( --ssid "${SSID}" )
[ -n "${AGG_IP}" ]  && prov_args+=( --aggregator-ip "${AGG_IP}" )
# Forward password only if explicitly provided; otherwise provision.sh prompts.
[ "${PASSWORD_SET}" -eq 1 ] && prov_args+=( --password "${PASSWORD}" )
"${SCRIPT_DIR}/esp32-provision.sh" "${prov_args[@]}"

# --- Done: verification guidance ------------------------------------------
DISP_IP="${AGG_IP:-<MAC_IP>}"
cat <<EOF

============================================================================
 Board #${NODE} is set up. To confirm CSI is flowing:

 1. On the aggregator host (${DISP_IP}), start the sensing-server with the
    ESP32 source on UDP ${AGG_PORT}:

      cd rust-port/wifi-densepose-rs
      CSI_SOURCE=esp32 cargo run -p wifi-densepose-sensing-server -- \\
        --http-port 3000 --source esp32

 2. Power-cycle the board. Within ~10 s the server log should show the
    source as 'esp32' and report non-zero CSI frames arriving from the node.

 3. Open the UI:  http://localhost:3000

 If no frames appear, see docs/esp32-setup.md -> Troubleshooting
 (firewall on UDP ${AGG_PORT}, wrong aggregator IP, WiFi join failure).
============================================================================
EOF
