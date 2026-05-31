#!/usr/bin/env bash
#
# run-notebook.sh -- one-command launcher for RuView notebook (laptop) WiFi
# RSSI presence sensing. Turns a stock macOS / Linux laptop into a coarse
# presence/motion sensing source for the RuView pipeline (no CSI hardware).
#
# Usage:
#   scripts/run-notebook.sh              # console mode (prints presence state)
#   scripts/run-notebook.sh --ws         # also broadcast to the web/mobile UI
#   scripts/run-notebook.sh --check      # just check if sensing is available
#   scripts/run-notebook.sh --ws --port 8765 --interval 0.5
#
# Any extra args are passed straight through to the Python entrypoint
# (see: python -m v1.src.sensing.notebook_runner --help).
#
# Windows: this is a bash script. On Windows run the module directly in
# PowerShell / cmd from the repo root:
#   python -m v1.src.sensing.notebook_runner --ws
# (Requires Python 3.9+ and, for --ws, the 'websockets' package.)

set -euo pipefail

# Resolve repo root (this script lives in <repo>/scripts).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

# Detect a Python interpreter.
PYTHON=""
for cand in python3 python; do
  if command -v "${cand}" >/dev/null 2>&1; then
    PYTHON="${cand}"
    break
  fi
done

if [ -z "${PYTHON}" ]; then
  echo "ERROR: no Python interpreter found (tried python3, python)." >&2
  echo "Install Python 3.9+ and retry." >&2
  exit 1
fi

echo "RuView notebook sensing"
echo "  repo   : ${REPO_ROOT}"
echo "  python : $(${PYTHON} --version 2>&1)"

# If WebSocket mode is requested, hint about the UI.
for arg in "$@"; do
  if [ "${arg}" = "--ws" ]; then
    echo "  UI hint: open the web UI or point the mobile app's WS URL at this"
    echo "           machine, e.g. ws://<this-laptop-ip>:8765"
    echo "           Find your IP with: ipconfig getifaddr en0 (macOS) or 'hostname -I' (Linux)"
    break
  fi
done

echo
# PYTHONPATH=repo root so 'v1.src...' imports resolve regardless of CWD.
exec env PYTHONPATH="${REPO_ROOT}:${PYTHONPATH:-}" \
  "${PYTHON}" -m v1.src.sensing.notebook_runner "$@"
