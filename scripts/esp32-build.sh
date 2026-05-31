#!/usr/bin/env bash
#
# esp32-build.sh — Build the RuView ESP32-S3 CSI node firmware on macOS.
#
# Uses the Docker + ESP-IDF method documented in
# firmware/esp32-csi-node/README.md (the only reliable build path):
#
#     docker run espressif/idf:v5.2  ->  idf.py set-target esp32s3 && idf.py build
#
# Produces three .bin files under firmware/esp32-csi-node/build/.
#
# This script never touches hardware. It only builds.
#
set -euo pipefail

# --- Resolve repo paths (script lives in <repo>/scripts) -------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FW_DIR="${REPO_ROOT}/firmware/esp32-csi-node"
BUILD_DIR="${FW_DIR}/build"

IDF_IMAGE="espressif/idf:v5.2"
CLEAN=0

usage() {
  cat <<'EOF'
esp32-build.sh — Build the ESP32-S3 CSI node firmware (Docker + ESP-IDF).

USAGE:
  scripts/esp32-build.sh [--clean] [--image <docker-image>] [--help]

OPTIONS:
  --clean            Remove build/ and sdkconfig before building (fresh build).
  --image <image>    ESP-IDF Docker image to use (default: espressif/idf:v5.2).
  -h, --help         Show this help and exit.

OUTPUT (on success):
  firmware/esp32-csi-node/build/bootloader/bootloader.bin
  firmware/esp32-csi-node/build/partition_table/partition-table.bin
  firmware/esp32-csi-node/build/esp32-csi-node.bin

REQUIREMENTS:
  - Docker Desktop running (https://www.docker.com/products/docker-desktop/)

NOTES:
  - First run pulls the ~2 GB ESP-IDF image and may take several minutes.
  - The build itself takes ~2-4 minutes on a Mac mini.
EOF
}

# --- Parse args ------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --clean) CLEAN=1; shift ;;
    --image)
      [ $# -ge 2 ] || { echo "Error: --image requires a value" >&2; exit 2; }
      IDF_IMAGE="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Error: unknown argument: $1" >&2; echo "Run with --help for usage." >&2; exit 2 ;;
  esac
done

# --- Preflight: firmware dir + docker --------------------------------------
if [ ! -f "${FW_DIR}/CMakeLists.txt" ]; then
  echo "Error: firmware project not found at:" >&2
  echo "  ${FW_DIR}" >&2
  echo "Are you running this from inside the RuView repo?" >&2
  exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
  cat >&2 <<EOF
Error: 'docker' was not found on your PATH.

The firmware MUST be cross-compiled inside the ESP-IDF Docker image
(${IDF_IMAGE}). Plain idf.py on the host is not supported here.

Fix on macOS:
  1. Install Docker Desktop:  https://www.docker.com/products/docker-desktop/
  2. Launch Docker Desktop and wait until the whale icon is steady.
  3. Re-run:  scripts/esp32-build.sh
EOF
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  cat >&2 <<'EOF'
Error: Docker is installed but the daemon is not reachable.

On macOS, open Docker Desktop and wait until it reports "Docker Desktop is
running", then re-run this script.
EOF
  exit 1
fi

# --- Build -----------------------------------------------------------------
echo "==> Building ESP32-S3 CSI node firmware"
echo "    Project: ${FW_DIR}"
echo "    Image:   ${IDF_IMAGE}"
[ "${CLEAN}" -eq 1 ] && echo "    Mode:    clean (build/ + sdkconfig removed first)"
echo

if [ "${CLEAN}" -eq 1 ]; then
  IDF_CMD="rm -rf build sdkconfig && idf.py set-target esp32s3 && idf.py build"
else
  IDF_CMD="idf.py set-target esp32s3 && idf.py build"
fi

# MSYS_NO_PATHCONV is harmless on macOS and keeps the command identical to the
# README so behaviour matches across platforms.
MSYS_NO_PATHCONV=1 docker run --rm \
  -v "${FW_DIR}:/project" -w /project \
  "${IDF_IMAGE}" bash -c "${IDF_CMD}"

# --- Verify outputs --------------------------------------------------------
BOOTLOADER="${BUILD_DIR}/bootloader/bootloader.bin"
PARTTABLE="${BUILD_DIR}/partition_table/partition-table.bin"
APP="${BUILD_DIR}/esp32-csi-node.bin"

missing=0
for f in "${BOOTLOADER}" "${PARTTABLE}" "${APP}"; do
  if [ ! -s "$f" ]; then
    echo "Error: expected build artifact missing or empty: $f" >&2
    missing=1
  fi
done
[ "${missing}" -eq 0 ] || { echo "Build did not produce all expected binaries." >&2; exit 1; }

echo
echo "==> Build complete. Artifacts:"
echo "    bootloader:      ${BOOTLOADER}"
echo "    partition table: ${PARTTABLE}"
echo "    application:     ${APP}"
echo
echo "Next: flash a connected board with"
echo "    scripts/esp32-flash.sh"
