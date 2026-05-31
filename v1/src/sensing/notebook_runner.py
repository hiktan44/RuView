"""
Notebook RSSI sensing entrypoint.

Runs continuous notebook (laptop) WiFi RSSI scanning and reports the presence
state on a configurable interval. Optionally broadcasts the *same* JSON frame
format used by :mod:`v1.src.sensing.ws_server` so the existing mobile/web UI
(``ui/mobile/src/services/ws.service.ts``) can consume it unchanged.

Run it directly::

    # console only (prints ABSENT / PRESENT_STILL / ACTIVE)
    python -m v1.src.sensing.notebook_runner

    # also broadcast to the web/mobile UI over WebSocket
    python -m v1.src.sensing.notebook_runner --ws --host 0.0.0.0 --port 8765

What it can sense
-----------------
Coarse RSSI-based presence/motion only (ABSENT / PRESENT_STILL / ACTIVE) plus a
motion-band power value. It does NOT do pose estimation or vital signs -- those
need CSI hardware (ESP32 / research NIC).
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import platform
import signal
import sys
import time
from typing import Optional

from v1.src.sensing.classifier import PresenceClassifier, SensingResult
from v1.src.sensing.feature_extractor import RssiFeatureExtractor, RssiFeatures
from v1.src.sensing.notebook_scan import NotebookWifiCollector, ScanError

logger = logging.getLogger(__name__)

DEFAULT_HOST = "0.0.0.0"
DEFAULT_PORT = 8765


def _build_ws_message(
    features: RssiFeatures,
    result: SensingResult,
    n_aps: int,
) -> str:
    """Build a ``sensing_update`` frame matching the existing WS contract.

    The structure mirrors :meth:`ws_server.SensingWebSocketServer._build_message`
    so ``ws.service.ts`` parses it with no changes. The notebook path has no
    per-subcarrier amplitude, so ``nodes[].amplitude`` is empty and
    ``signal_field`` is omitted (the UI tolerates its absence).
    """
    msg = {
        "type": "sensing_update",
        "timestamp": time.time(),
        "source": "notebook_wifi",
        "nodes": [
            {
                "node_id": 1,
                "rssi_dbm": features.mean,
                "position": [2.0, 0.0, 1.5],
                "amplitude": [],
                "subcarrier_count": 0,
                "visible_aps": n_aps,
            }
        ],
        "features": {
            "mean_rssi": features.mean,
            "variance": features.variance,
            "std": features.std,
            "motion_band_power": features.motion_band_power,
            "breathing_band_power": features.breathing_band_power,
            "dominant_freq_hz": features.dominant_freq_hz,
            "change_points": features.n_change_points,
            "spectral_power": features.total_spectral_power,
            "range": features.range,
            "iqr": features.iqr,
            "skewness": features.skewness,
            "kurtosis": features.kurtosis,
        },
        "classification": {
            "motion_level": result.motion_level.value,
            "presence": result.presence_detected,
            "confidence": round(result.confidence, 3),
        },
    }
    return json.dumps(msg)


class NotebookSensingApp:
    """Drives the notebook collector + feature extractor + classifier loop."""

    def __init__(
        self,
        interval: float = 1.0,
        window_seconds: float = 10.0,
        scan_rate_hz: float = 1.0,
        top_k: int = 5,
    ) -> None:
        self.interval = interval
        self.collector = NotebookWifiCollector(
            sample_rate_hz=scan_rate_hz, top_k=top_k
        )
        self.extractor = RssiFeatureExtractor(window_seconds=window_seconds)
        self.classifier = PresenceClassifier()
        self._clients: set = set()
        self._running = False

    # -- pipeline ------------------------------------------------------------

    def _tick(self) -> Optional[tuple[RssiFeatures, SensingResult, int]]:
        """Run one extract+classify cycle; returns None until enough samples."""
        window = self.extractor.window_seconds
        rate = self.collector.sample_rate_hz
        n_needed = max(4, int(window * rate))
        samples = self.collector.get_samples(n=n_needed)
        if len(samples) < 4:
            return None
        features = self.extractor.extract(samples)
        result = self.classifier.classify(features)
        return features, result, len(self.collector.last_networks)

    # -- console mode --------------------------------------------------------

    def run_console(self) -> None:
        """Continuous console reporting (no WebSocket)."""
        self._start_collector_or_exit()
        print(f"\n  Notebook RSSI sensing on {platform.system()}")
        print(f"  Scan rate: {self.collector.sample_rate_hz:.2f} Hz | "
              f"Window: {self.extractor.window_seconds:.0f}s | "
              f"Report: every {self.interval:.1f}s")
        print("  (RSSI presence only -- no pose/vitals; that needs CSI hardware)")
        print("  Press Ctrl+C to stop\n")
        self._running = True
        try:
            while self._running:
                out = self._tick()
                if out is None:
                    print("  warming up (collecting WiFi scans) ...")
                else:
                    features, result, n_aps = out
                    print(
                        f"  {time.strftime('%H:%M:%S')}  "
                        f"{result.motion_level.value.upper():<14} "
                        f"conf={result.confidence:5.0%}  "
                        f"motion_power={features.motion_band_power:.4f}  "
                        f"var={features.variance:.3f}  "
                        f"APs={n_aps}"
                    )
                time.sleep(self.interval)
        except KeyboardInterrupt:
            pass
        finally:
            self.collector.stop()
            print("\n  Stopped.")

    # -- websocket mode ------------------------------------------------------

    async def run_ws(self, host: str, port: int) -> None:
        """Continuous reporting that also broadcasts ``sensing_update`` frames."""
        try:
            import websockets
        except ImportError:
            print("ERROR: 'websockets' package not found. Install with:")
            print("    pip install websockets")
            sys.exit(1)

        self._start_collector_or_exit()
        self._running = True
        print(f"\n  Notebook RSSI sensing WebSocket on ws://{host}:{port}")
        print(f"  Source: notebook_wifi | Scan: {self.collector.sample_rate_hz:.2f} Hz")
        print("  Point the mobile/web UI at this host:port.")
        print("  Press Ctrl+C to stop\n")

        async with websockets.serve(self._ws_handler, host, port):
            await self._broadcast_loop()

    async def _ws_handler(self, websocket) -> None:
        self._clients.add(websocket)
        logger.info("UI client connected: %s", websocket.remote_address)
        try:
            async for _ in websocket:
                pass
        finally:
            self._clients.discard(websocket)
            logger.info("UI client disconnected: %s", websocket.remote_address)

    async def _broadcast_loop(self) -> None:
        while self._running:
            out = self._tick()
            if out is not None:
                features, result, n_aps = out
                message = _build_ws_message(features, result, n_aps)
                await self._broadcast(message)
                logger.info(
                    "%s conf=%.0f%% motion_power=%.4f APs=%d",
                    result.motion_level.value, result.confidence * 100,
                    features.motion_band_power, n_aps,
                )
            await asyncio.sleep(self.interval)

    async def _broadcast(self, message: str) -> None:
        if not self._clients:
            return
        dead = set()
        for ws in self._clients:
            try:
                await ws.send(message)
            except Exception:
                dead.add(ws)
        self._clients -= dead

    # -- helpers -------------------------------------------------------------

    def _start_collector_or_exit(self) -> None:
        try:
            self.collector.start()
        except ScanError as exc:
            print("ERROR: cannot start notebook WiFi scanning.\n")
            print(f"  {exc}\n")
            sys.exit(2)


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="notebook-sensing",
        description=(
            "Notebook (laptop) WiFi RSSI presence sensing for RuView. "
            "Coarse presence/motion only -- no CSI/pose/vitals."
        ),
    )
    parser.add_argument(
        "--interval", type=float, default=1.0,
        help="Seconds between presence reports / broadcasts (default 1.0).",
    )
    parser.add_argument(
        "--scan-rate", type=float, default=1.0,
        help="WiFi scan rate in Hz (default 1.0; OS scans are slow).",
    )
    parser.add_argument(
        "--window", type=float, default=10.0,
        help="Feature window length in seconds (default 10).",
    )
    parser.add_argument(
        "--top-k", type=int, default=5,
        help="Number of strongest APs to average per scan (default 5).",
    )
    parser.add_argument(
        "--ws", action="store_true",
        help="Broadcast sensing_update frames over WebSocket for the UI.",
    )
    parser.add_argument("--host", default=DEFAULT_HOST, help="WS bind host.")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help="WS port.")
    parser.add_argument(
        "--check", action="store_true",
        help="Run a single scan, print whether sensing is available, and exit.",
    )
    return parser


def main(argv: Optional[list] = None) -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    )
    args = build_arg_parser().parse_args(argv)

    if args.check:
        available, reason = NotebookWifiCollector.is_available()
        print(f"platform : {platform.system()}")
        print(f"available: {available}")
        print(f"reason   : {reason}")
        return 0 if available else 2

    app = NotebookSensingApp(
        interval=args.interval,
        window_seconds=args.window,
        scan_rate_hz=args.scan_rate,
        top_k=args.top_k,
    )

    if args.ws:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        def _shutdown(_sig, _frame):
            app._running = False
            app.collector.stop()
            loop.stop()

        signal.signal(signal.SIGINT, _shutdown)
        try:
            loop.run_until_complete(app.run_ws(args.host, args.port))
        except (KeyboardInterrupt, RuntimeError):
            pass
        finally:
            app.collector.stop()
            loop.close()
    else:
        app.run_console()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
