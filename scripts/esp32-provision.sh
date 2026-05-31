#!/usr/bin/env bash
#
# esp32-provision.sh — Provision one ESP32-S3 CSI node on macOS.
#
# Thin wrapper around firmware/esp32-csi-node/provision.py that:
#   - auto-detects the serial port (like esp32-flash.sh),
#   - prompts for (or accepts) SSID / WiFi password / aggregator IP,
#   - configures a sensible node-id + TDM slot for a 3-node mesh via --node,
#   - NEVER prints the WiFi password to logs.
#
# The aggregator is the machine running the sensing-server. For LOCAL testing
# that is your Mac's LAN IP; for the remote deployment it is the server host.
# CSI is sent over UDP to <aggregator-ip>:<port> (port 5005 by default).
#
set -euo pipefail

# --- Resolve repo paths ----------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FW_DIR="${REPO_ROOT}/firmware/esp32-csi-node"
PROVISION_PY="${FW_DIR}/provision.py"

PY="${PYTHON:-python3}"

PORT=""
BAUD="460800"
SSID=""
PASSWORD=""
PASSWORD_SET=0
AGG_IP=""
AGG_PORT="5005"
NODE=""
TDM_TOTAL="3"
EDGE_TIER=""
DRY_RUN=0

usage() {
  cat <<'EOF'
esp32-provision.sh — Provision one ESP32-S3 CSI node (macOS).

USAGE:
  scripts/esp32-provision.sh --node <N> [options]

REQUIRED (prompted if omitted):
  --ssid <SSID>             WiFi network the node should join.
  --aggregator-ip <IP>      Host running the sensing-server (UDP target).
                            Your Mac's LAN IP for local testing.

NODE / MESH:
  --node <N>                Convenience: sets node-id=N, tdm-slot=N, and
                            tdm-total (default 3) for a 3-node TDM mesh.
                            Use 0, 1, 2 for the three boards.
  --tdm-total <T>           Total nodes in the mesh (default: 3).

OPTIONAL:
  --port <dev>              Serial device (auto-detected if omitted).
  --baud <rate>            Flash baud (default: 460800).
  --password <PW>          WiFi password. If omitted you are prompted
                            (input hidden). Pass '' explicitly for an open net.
  --aggregator-port <P>    UDP port (default: 5005 — what the server expects).
  --edge-tier <0|1|2>      0=raw CSI, 1=basic, 2=full. Default: firmware default.
  --dry-run                Build the NVS image but do NOT flash it.
  -h, --help               Show this help and exit.

EXAMPLES:
  # Board #1 of a 3-node mesh, local Mac aggregator at 192.168.1.50
  scripts/esp32-provision.sh --node 0 --ssid HomeWiFi --aggregator-ip 192.168.1.50

  # Board #2, password supplied non-interactively (CI/scripts)
  scripts/esp32-provision.sh --node 1 --ssid HomeWiFi --password 'secret' \
      --aggregator-ip 192.168.1.50

SECURITY:
  The WiFi password is never echoed or logged. It is passed to provision.py
  via argument; for stronger hygiene on shared machines, omit --password and
  let the script prompt for it (hidden input).
EOF
}

# --- Parse args ------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --port)            PORT="${2:?--port needs a value}"; shift 2 ;;
    --baud)            BAUD="${2:?--baud needs a value}"; shift 2 ;;
    --ssid)            SSID="${2:?--ssid needs a value}"; shift 2 ;;
    --password)        PASSWORD="${2-}"; PASSWORD_SET=1; shift 2 ;;
    --aggregator-ip)   AGG_IP="${2:?--aggregator-ip needs a value}"; shift 2 ;;
    --aggregator-port) AGG_PORT="${2:?--aggregator-port needs a value}"; shift 2 ;;
    --node)            NODE="${2:?--node needs a value}"; shift 2 ;;
    --tdm-total)       TDM_TOTAL="${2:?--tdm-total needs a value}"; shift 2 ;;
    --edge-tier)       EDGE_TIER="${2:?--edge-tier needs a value}"; shift 2 ;;
    --dry-run)         DRY_RUN=1; shift ;;
    -h|--help)         usage; exit 0 ;;
    *) echo "Error: unknown argument: $1" >&2; echo "Run with --help for usage." >&2; exit 2 ;;
  esac
done

# --- Preflight -------------------------------------------------------------
if [ ! -f "${PROVISION_PY}" ]; then
  echo "Error: provisioning script not found: ${PROVISION_PY}" >&2
  exit 1
fi
if ! command -v "${PY}" >/dev/null 2>&1; then
  echo "Error: python3 not found. Install Python 3 ('brew install python')." >&2
  exit 1
fi

# --- Validate numeric / range inputs ---------------------------------------
is_uint() { printf '%s' "$1" | grep -Eq '^[0-9]+$'; }

if [ -z "${NODE}" ]; then
  echo "Error: --node <N> is required (use 0, 1, or 2 for a 3-node mesh)." >&2
  exit 2
fi
is_uint "${NODE}"      || { echo "Error: --node must be a non-negative integer (got: ${NODE})" >&2; exit 2; }
is_uint "${TDM_TOTAL}" || { echo "Error: --tdm-total must be a positive integer (got: ${TDM_TOTAL})" >&2; exit 2; }
is_uint "${BAUD}"      || { echo "Error: --baud must be an integer (got: ${BAUD})" >&2; exit 2; }
is_uint "${AGG_PORT}"  || { echo "Error: --aggregator-port must be an integer (got: ${AGG_PORT})" >&2; exit 2; }
if [ "${TDM_TOTAL}" -lt 1 ]; then echo "Error: --tdm-total must be >= 1" >&2; exit 2; fi
if [ "${NODE}" -ge "${TDM_TOTAL}" ]; then
  echo "Error: --node (${NODE}) must be less than --tdm-total (${TDM_TOTAL})." >&2
  exit 2
fi
if [ "${AGG_PORT}" -lt 1 ] || [ "${AGG_PORT}" -gt 65535 ]; then
  echo "Error: --aggregator-port out of range (1-65535): ${AGG_PORT}" >&2; exit 2
fi
if [ -n "${EDGE_TIER}" ]; then
  case "${EDGE_TIER}" in 0|1|2) : ;; *) echo "Error: --edge-tier must be 0, 1, or 2" >&2; exit 2 ;; esac
fi

# --- Prompt for missing SSID / aggregator IP -------------------------------
if [ -z "${SSID}" ]; then
  printf 'WiFi SSID: '
  IFS= read -r SSID
  [ -n "${SSID}" ] || { echo "Error: SSID cannot be empty." >&2; exit 2; }
fi

if [ -z "${AGG_IP}" ]; then
  printf 'Aggregator IP (host running the sensing-server, e.g. your Mac LAN IP): '
  IFS= read -r AGG_IP
  [ -n "${AGG_IP}" ] || { echo "Error: aggregator IP cannot be empty." >&2; exit 2; }
fi

# Light IPv4 shape check (provision.py stores it as a string regardless).
if ! printf '%s' "${AGG_IP}" | grep -Eq '^[0-9]{1,3}(\.[0-9]{1,3}){3}$'; then
  echo "Warning: '${AGG_IP}' does not look like an IPv4 address. Continuing anyway." >&2
fi

# --- Prompt for password (hidden) if not provided --------------------------
if [ "${PASSWORD_SET}" -eq 0 ]; then
  printf 'WiFi password (leave empty for an open network): '
  # -s hides input; restore newline afterwards.
  if read -rs PASSWORD 2>/dev/null; then
    echo
  else
    # Fallback for shells without read -s: read visibly (last resort).
    IFS= read -r PASSWORD
  fi
  PASSWORD_SET=1
fi

# --- Detect port if not given (macOS) --------------------------------------
detect_ports() {
  local p
  for p in /dev/cu.usbmodem* /dev/cu.usbserial* /dev/cu.SLAB_USBtoUART*; do
    [ -e "$p" ] && printf '%s\n' "$p"
  done
}
if [ -z "${PORT}" ]; then
  ports=()
  while IFS= read -r line; do [ -n "$line" ] && ports+=("$line"); done < <(detect_ports)
  case "${#ports[@]}" in
    0) echo "Error: no ESP32 serial port found. Plug in the board (USB-C data cable) or pass --port." >&2; exit 1 ;;
    1) PORT="${ports[0]}"; echo "==> Auto-detected serial port: ${PORT}" ;;
    *)
      echo "Error: multiple serial ports detected — pick one with --port:" >&2
      for p in "${ports[@]}"; do echo "    ${p}" >&2; done
      exit 1 ;;
  esac
fi
case "${PORT}" in
  /dev/cu.*|/dev/tty.*) : ;;
  *) echo "Error: --port should be a macOS device path like /dev/cu.usbmodemXXXX (got: ${PORT})" >&2; exit 2 ;;
esac
[ -e "${PORT}" ] || { echo "Error: serial device does not exist: ${PORT}" >&2; exit 1; }

# --- Map --node -> node-id / tdm-slot --------------------------------------
NODE_ID="${NODE}"
TDM_SLOT="${NODE}"

echo "==> Provisioning node ${NODE_ID} (TDM slot ${TDM_SLOT} of ${TDM_TOTAL})"
echo "    Port:           ${PORT}"
echo "    SSID:           ${SSID}"
echo "    WiFi password:  (hidden)"
echo "    Aggregator:     ${AGG_IP}:${AGG_PORT}"
[ -n "${EDGE_TIER}" ] && echo "    Edge tier:      ${EDGE_TIER}"
[ "${DRY_RUN}" -eq 1 ] && echo "    Mode:           dry-run (no flash)"
echo

# --- Assemble provision.py invocation --------------------------------------
# Build args as an array so values with spaces (SSID/password) stay intact.
args=(
  "${PROVISION_PY}"
  --port "${PORT}"
  --baud "${BAUD}"
  --ssid "${SSID}"
  --target-ip "${AGG_IP}"
  --target-port "${AGG_PORT}"
  --node-id "${NODE_ID}"
  --tdm-slot "${TDM_SLOT}"
  --tdm-total "${TDM_TOTAL}"
)
# Always pass --password when set (provision.py treats empty as "open net").
if [ "${PASSWORD_SET}" -eq 1 ]; then
  args+=( --password "${PASSWORD}" )
fi
[ -n "${EDGE_TIER}" ] && args+=( --edge-tier "${EDGE_TIER}" )
[ "${DRY_RUN}" -eq 1 ] && args+=( --dry-run )

# Run. provision.py masks the password in its own output; we never echo it.
exec "${PY}" "${args[@]}"
