"""
Notebook / laptop WiFi RSSI sensing (no special hardware required).

This module turns a stock laptop into a *coarse* RSSI-based presence/motion
sensor for the RuView pipeline. Stock laptop NICs do **not** expose raw CSI;
they only expose per-AP RSSI through the operating system's WiFi scan APIs.
Therefore this path provides:

    ABSENT  /  PRESENT_STILL  /  ACTIVE   +  a motion-band power value

It does **not** provide full DensePose, per-subcarrier CSI, or vital signs.
Those require CSI-capable hardware (ESP32 nodes or research NICs) and are a
separate, later phase.

Per-OS RSSI sources (all stdlib ``subprocess`` calls, no third-party deps):

    macOS    -> ``system_profiler SPAirPortDataType`` (CoreWLAN-backed)
    Linux    -> ``nmcli -t -f SSID,BSSID,SIGNAL dev wifi``  (fallback: ``iw``)
    Windows  -> ``netsh wlan show networks mode=bssid``

The parsers (``parse_macos_airport``, ``parse_nmcli``, ``parse_iw_scan``,
``parse_netsh_networks``) are deliberately separated from the subprocess
calls so they can be unit-tested against captured command output.

The collector aggregates the strongest visible APs into the existing
:class:`~v1.src.sensing.rssi_collector.WifiSample` ring buffer, so the
existing :class:`~v1.src.sensing.feature_extractor.RssiFeatureExtractor` and
:class:`~v1.src.sensing.classifier.PresenceClassifier` work unchanged.
"""

from __future__ import annotations

import logging
import platform
import re
import subprocess
import threading
import time
from dataclasses import dataclass
from typing import List, Optional, Sequence

from v1.src.sensing.rssi_collector import RingBuffer, WifiSample

logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Data type
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class WifiNetwork:
    """A single visible WiFi access point seen during a scan.

    Attributes
    ----------
    ssid : str
        Network name (may be empty for hidden networks).
    bssid : str
        AP MAC / BSSID, lower-cased and colon-separated when available.
    rssi_dbm : float
        Signal strength in dBm (negative; e.g. ``-55.0``).
    channel : int
        WiFi channel number, or ``0`` when unknown.
    """

    ssid: str
    bssid: str
    rssi_dbm: float
    channel: int = 0


# ---------------------------------------------------------------------------
# Signal helpers
# ---------------------------------------------------------------------------

def signal_pct_to_dbm(percent: float) -> float:
    """Convert a 0-100 signal-quality percentage to an approximate dBm value.

    Windows ``netsh`` and ``nmcli`` report a quality percentage rather than a
    raw dBm. The common linear mapping is ``dBm = (pct / 2) - 100``, which
    yields ``-100 dBm`` at 0 % and ``-50 dBm`` at 100 %. This is an estimate,
    not a calibrated measurement.

    Parameters
    ----------
    percent : float
        Signal quality, clamped to ``[0, 100]``.
    """
    pct = max(0.0, min(100.0, percent))
    return (pct / 2.0) - 100.0


def _norm_bssid(raw: str) -> str:
    """Normalise a BSSID string to lower-case colon-separated form."""
    return raw.strip().lower().replace("-", ":")


# ---------------------------------------------------------------------------
# Parsers (pure functions -- unit-testable, no subprocess)
# ---------------------------------------------------------------------------

def parse_macos_airport(output: str) -> List[WifiNetwork]:
    """Parse ``system_profiler SPAirPortDataType`` output into networks.

    ``system_profiler`` indents each AP name and lists its properties below.
    We extract the network name plus ``Signal / Noise`` (in dBm) and
    ``Channel``. Output format varies slightly across macOS versions, so the
    parser is tolerant of missing fields.

    Example block::

        Other Local Wi-Fi Networks:
            MyNet:
              PHY Mode: 802.11ax
              Channel: 36 (5GHz, 80MHz)
              Signal / Noise: -57 dBm / -90 dBm
    """
    networks: List[WifiNetwork] = []
    current_name: Optional[str] = None
    current_rssi: Optional[float] = None
    current_channel = 0

    def _flush() -> None:
        nonlocal current_name, current_rssi, current_channel
        if current_name is not None and current_rssi is not None:
            networks.append(
                WifiNetwork(
                    ssid=current_name,
                    bssid="",  # system_profiler does not expose BSSID
                    rssi_dbm=current_rssi,
                    channel=current_channel,
                )
            )
        current_name = None
        current_rssi = None
        current_channel = 0

    # A network header is a line ending in ":" that is *not* a known property.
    _PROPERTY_PREFIXES = (
        "PHY Mode", "Channel", "Country Code", "Network Type",
        "Security", "Signal / Noise", "BSSID", "MCS Index",
    )

    for raw_line in output.splitlines():
        line = raw_line.rstrip()
        stripped = line.strip()
        if not stripped:
            continue

        if stripped.startswith("Signal / Noise"):
            m = re.search(r"(-?\d+)\s*dBm", stripped)
            if m:
                current_rssi = float(m.group(1))
            continue

        if stripped.startswith("Channel:"):
            m = re.search(r"Channel:\s*(\d+)", stripped)
            if m:
                current_channel = int(m.group(1))
            continue

        # Skip other known property lines
        if any(stripped.startswith(p) for p in _PROPERTY_PREFIXES):
            continue

        # A line ending in ":" at this point is a new network name.
        if stripped.endswith(":"):
            name = stripped[:-1].strip()
            # Section headers we never want as network names.
            if name.endswith("Networks") or name in (
                "Software Versions", "Interfaces", "Current Network Information",
            ):
                _flush()
                continue
            _flush()
            current_name = name

    _flush()
    return networks


def parse_nmcli(output: str) -> List[WifiNetwork]:
    """Parse ``nmcli -t -f SSID,BSSID,SIGNAL dev wifi`` output.

    The ``-t`` (terse) mode emits colon-separated fields. Because BSSIDs also
    contain colons, ``nmcli`` escapes them as ``\\:`` -- so we split on
    unescaped colons.

    Example line::

        MyNet:AA\\:BB\\:CC\\:DD\\:EE\\:FF:72
    """
    networks: List[WifiNetwork] = []
    for raw_line in output.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        # Split on ':' that is NOT preceded by a backslash.
        fields = re.split(r"(?<!\\):", line)
        if len(fields) < 3:
            continue
        ssid = fields[0].replace("\\:", ":").strip()
        bssid = fields[1].replace("\\:", ":").strip()
        signal_raw = fields[2].strip()
        try:
            signal_pct = float(signal_raw)
        except ValueError:
            continue
        networks.append(
            WifiNetwork(
                ssid=ssid,
                bssid=_norm_bssid(bssid),
                rssi_dbm=signal_pct_to_dbm(signal_pct),
                channel=0,
            )
        )
    return networks


def parse_iw_scan(output: str) -> List[WifiNetwork]:
    """Parse ``iw dev <iface> scan`` output (Linux fallback for nmcli).

    ``iw`` reports real dBm signal levels, so no percentage conversion is
    needed. Each AP block begins with a ``BSS <mac>`` line.
    """
    networks: List[WifiNetwork] = []
    bssid: Optional[str] = None
    ssid = ""
    rssi: Optional[float] = None
    channel = 0

    def _flush() -> None:
        nonlocal bssid, ssid, rssi, channel
        if bssid is not None and rssi is not None:
            networks.append(
                WifiNetwork(
                    ssid=ssid,
                    bssid=_norm_bssid(bssid),
                    rssi_dbm=rssi,
                    channel=channel,
                )
            )
        bssid = None
        ssid = ""
        rssi = None
        channel = 0

    for raw_line in output.splitlines():
        line = raw_line.strip()
        m_bss = re.match(r"BSS\s+([0-9a-fA-F:]{17})", line)
        if m_bss:
            _flush()
            bssid = m_bss.group(1)
            continue
        if line.startswith("signal:"):
            m = re.search(r"(-?\d+(?:\.\d+)?)\s*dBm", line)
            if m:
                rssi = float(m.group(1))
            continue
        if line.startswith("SSID:"):
            ssid = line[len("SSID:"):].strip()
            continue
        m_freq = re.search(r"DS Parameter set: channel\s*(\d+)", line)
        if m_freq:
            channel = int(m_freq.group(1))

    _flush()
    return networks


def parse_netsh_networks(output: str) -> List[WifiNetwork]:
    """Parse ``netsh wlan show networks mode=bssid`` output.

    ``netsh`` groups one or more BSSIDs under each SSID and reports a
    ``Signal`` percentage per BSSID. We emit one :class:`WifiNetwork` per
    BSSID. Output is localisation-dependent; this parser targets the English
    field labels (``SSID``, ``BSSID``, ``Signal``, ``Channel``).

    Example::

        SSID 1 : MyNet
            Network type            : Infrastructure
            BSSID 1                 : aa:bb:cc:dd:ee:ff
                 Signal             : 86%
                 Channel            : 36
    """
    networks: List[WifiNetwork] = []
    current_ssid = ""
    pending_bssid: Optional[str] = None
    pending_signal: Optional[float] = None
    pending_channel = 0

    def _flush() -> None:
        nonlocal pending_bssid, pending_signal, pending_channel
        if pending_bssid is not None and pending_signal is not None:
            networks.append(
                WifiNetwork(
                    ssid=current_ssid,
                    bssid=_norm_bssid(pending_bssid),
                    rssi_dbm=signal_pct_to_dbm(pending_signal),
                    channel=pending_channel,
                )
            )
        pending_bssid = None
        pending_signal = None
        pending_channel = 0

    for raw_line in output.splitlines():
        line = raw_line.strip()
        if not line:
            continue

        # New SSID block: "SSID 12 : NetworkName"
        m_ssid = re.match(r"SSID\s+\d+\s*:\s*(.*)$", line)
        if m_ssid:
            _flush()
            current_ssid = m_ssid.group(1).strip()
            continue

        # New BSSID: "BSSID 1 : aa:bb:cc:dd:ee:ff"
        m_bssid = re.match(r"BSSID\s+\d+\s*:\s*([0-9a-fA-F:\-]{17})", line)
        if m_bssid:
            _flush()
            pending_bssid = m_bssid.group(1)
            continue

        # "Signal : 86%"
        m_sig = re.match(r"Signal\s*:\s*(\d+)\s*%", line)
        if m_sig:
            pending_signal = float(m_sig.group(1))
            continue

        # "Channel : 36"
        m_ch = re.match(r"Channel\s*:\s*(\d+)", line)
        if m_ch:
            pending_channel = int(m_ch.group(1))
            continue

    _flush()
    return networks


# ---------------------------------------------------------------------------
# Aggregation: many APs -> one RSSI scalar
# ---------------------------------------------------------------------------

def aggregate_rssi(
    networks: Sequence[WifiNetwork],
    top_k: int = 5,
) -> float:
    """Combine the strongest visible APs into a single RSSI scalar (dBm).

    Motion in the environment perturbs the multipath received from *many*
    nearby APs. Averaging the strongest ``top_k`` BSSIDs yields a more
    motion-sensitive scalar than tracking a single link, while staying robust
    to APs dropping in/out of a scan.

    Returns ``-100.0`` (effective noise floor) when no networks are visible.
    """
    if not networks:
        return -100.0
    ranked = sorted(networks, key=lambda n: n.rssi_dbm, reverse=True)
    chosen = ranked[: max(1, top_k)]
    return sum(n.rssi_dbm for n in chosen) / len(chosen)


# ---------------------------------------------------------------------------
# Scan command runners (thin subprocess wrappers around the parsers)
# ---------------------------------------------------------------------------

class ScanError(RuntimeError):
    """Raised when a platform WiFi scan cannot be performed.

    The message is intentionally actionable -- it tells the user what to
    install or which permission to grant.
    """


def _run(cmd: List[str], timeout: float = 8.0) -> str:
    """Run a fixed-argument command and return stdout.

    ``cmd`` is always a list of literal strings (never a shell string), so no
    shell interpolation of untrusted input is possible. ``shell=True`` is
    never used.
    """
    try:
        proc = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError as exc:
        raise ScanError(f"'{cmd[0]}' not found: {exc}") from exc
    except subprocess.TimeoutExpired as exc:
        raise ScanError(f"'{cmd[0]}' timed out after {timeout}s") from exc
    return proc.stdout


def scan_macos() -> List[WifiNetwork]:
    """Scan visible WiFi networks on macOS via ``system_profiler``."""
    out = _run(["system_profiler", "SPAirPortDataType"], timeout=12.0)
    nets = parse_macos_airport(out)
    if not nets:
        raise ScanError(
            "No WiFi networks parsed from system_profiler. On macOS Sonoma+ "
            "WiFi scanning requires Location Services to be enabled for the "
            "app running this process (System Settings > Privacy & Security > "
            "Location Services). Also ensure WiFi is turned on."
        )
    return nets


def scan_linux() -> List[WifiNetwork]:
    """Scan visible WiFi networks on Linux via ``nmcli`` (fallback ``iw``)."""
    try:
        out = _run(["nmcli", "-t", "-f", "SSID,BSSID,SIGNAL", "dev", "wifi"])
        nets = parse_nmcli(out)
        if nets:
            return nets
    except ScanError as exc:
        logger.debug("nmcli unavailable (%s); trying iw", exc)

    # Fallback: iw scan needs an interface name and usually root/CAP_NET_ADMIN.
    iface = _detect_linux_iface()
    if iface is None:
        raise ScanError(
            "No WiFi scan available. Install NetworkManager ('nmcli') -- "
            "'sudo apt install network-manager' -- or 'iw' "
            "('sudo apt install iw'). 'iw dev <iface> scan' also typically "
            "requires sudo/CAP_NET_ADMIN."
        )
    out = _run(["iw", "dev", iface, "scan"], timeout=12.0)
    nets = parse_iw_scan(out)
    if not nets:
        raise ScanError(
            f"'iw dev {iface} scan' returned no networks. It usually needs "
            f"root privileges: try 'sudo iw dev {iface} scan'."
        )
    return nets


def scan_windows() -> List[WifiNetwork]:
    """Scan visible WiFi networks on Windows via ``netsh``."""
    out = _run(["netsh", "wlan", "show", "networks", "mode=bssid"], timeout=12.0)
    nets = parse_netsh_networks(out)
    if not nets:
        raise ScanError(
            "No WiFi networks parsed from netsh. Ensure the WLAN AutoConfig "
            "service is running and a WiFi adapter is enabled. On non-English "
            "Windows the field labels differ and may not parse."
        )
    return nets


def _detect_linux_iface() -> Optional[str]:
    """Best-effort detection of a wireless interface name on Linux."""
    try:
        out = _run(["iw", "dev"], timeout=4.0)
    except ScanError:
        return None
    m = re.search(r"Interface\s+(\S+)", out)
    return m.group(1) if m else None


def scan_for_platform(system: Optional[str] = None) -> List[WifiNetwork]:
    """Run the correct scan command for the current (or given) OS.

    Raises :class:`ScanError` with an actionable message on unsupported
    platforms or missing tools.
    """
    system = system or platform.system()
    if system == "Darwin":
        return scan_macos()
    if system == "Linux":
        return scan_linux()
    if system == "Windows":
        return scan_windows()
    raise ScanError(
        f"Unsupported platform '{system}'. Notebook RSSI sensing supports "
        f"macOS, Linux, and Windows."
    )


# ---------------------------------------------------------------------------
# Notebook collector -- feeds the existing WifiSample pipeline
# ---------------------------------------------------------------------------

class NotebookWifiCollector:
    """Cross-platform notebook RSSI collector (scan-based, no CSI).

    Periodically runs the OS WiFi scan, aggregates the strongest visible APs
    into a single RSSI value, and appends it to a :class:`RingBuffer` of
    :class:`WifiSample` objects -- the same structure the rest of the sensing
    pipeline already consumes.

    Conforms to the informal collector protocol used elsewhere
    (``start`` / ``stop`` / ``get_samples`` / ``sample_rate_hz``) so it can be
    dropped into the existing feature-extractor + classifier flow and the
    WebSocket server.

    Parameters
    ----------
    sample_rate_hz : float
        Target scan rate. OS scans are slow (often 1-4 s each), so values
        above ~1 Hz are usually not achievable; the loop self-paces.
    buffer_seconds : int
        Ring-buffer history length in seconds.
    top_k : int
        Number of strongest APs to average per scan.
    system : str, optional
        Force a platform ("Darwin"/"Linux"/"Windows"); auto-detected if None.
    """

    def __init__(
        self,
        sample_rate_hz: float = 1.0,
        buffer_seconds: int = 120,
        top_k: int = 5,
        system: Optional[str] = None,
    ) -> None:
        self._rate = max(0.05, sample_rate_hz)
        self._buffer = RingBuffer(max_size=max(8, int(self._rate * buffer_seconds)))
        self._top_k = top_k
        self._system = system or platform.system()
        self._running = False
        self._thread: Optional[threading.Thread] = None
        self._last_networks: List[WifiNetwork] = []

    # -- public API ----------------------------------------------------------

    @property
    def sample_rate_hz(self) -> float:
        return self._rate

    @property
    def last_networks(self) -> List[WifiNetwork]:
        """The networks seen in the most recent scan (read-only snapshot)."""
        return list(self._last_networks)

    @classmethod
    def is_available(cls, system: Optional[str] = None) -> tuple[bool, str]:
        """Check whether a notebook scan can run, without starting a thread.

        Returns ``(available, reason)``. Performs one real scan attempt so the
        returned reason is the actual, actionable error when unavailable.
        """
        try:
            nets = scan_for_platform(system)
            return True, f"ok ({len(nets)} networks visible)"
        except ScanError as exc:
            return False, str(exc)

    def collect_once(self) -> WifiSample:
        """Run a single scan now and return the aggregated sample (blocking).

        Raises :class:`ScanError` if the platform scan fails.
        """
        return self._read_sample()

    def start(self) -> None:
        """Start the background scanning thread.

        Validates the platform scan first so configuration errors surface
        immediately rather than silently in the thread.
        """
        if self._running:
            return
        available, reason = self.is_available(self._system)
        if not available:
            raise ScanError(reason)
        self._running = True
        self._thread = threading.Thread(
            target=self._scan_loop, daemon=True, name="notebook-rssi-collector"
        )
        self._thread.start()
        logger.info(
            "NotebookWifiCollector started on %s at %.2f Hz (top_k=%d)",
            self._system, self._rate, self._top_k,
        )

    def stop(self) -> None:
        """Stop the background scanning thread."""
        self._running = False
        if self._thread is not None:
            self._thread.join(timeout=3.0)
            self._thread = None
        logger.info("NotebookWifiCollector stopped")

    def get_samples(self, n: Optional[int] = None) -> List[WifiSample]:
        if n is not None:
            return self._buffer.get_last_n(n)
        return self._buffer.get_all()

    # -- internals -----------------------------------------------------------

    def _read_sample(self) -> WifiSample:
        networks = scan_for_platform(self._system)
        self._last_networks = networks
        rssi = aggregate_rssi(networks, top_k=self._top_k)
        return WifiSample(
            timestamp=time.time(),
            rssi_dbm=rssi,
            noise_dbm=-95.0,
            link_quality=max(0.0, min(1.0, (rssi + 100.0) / 60.0)),
            tx_bytes=0,
            rx_bytes=0,
            retry_count=0,
            interface=f"notebook-scan ({len(networks)} APs)",
        )

    def _scan_loop(self) -> None:
        interval = 1.0 / self._rate
        while self._running:
            t0 = time.monotonic()
            try:
                sample = self._read_sample()
                self._buffer.append(sample)
            except ScanError as exc:
                logger.warning("Notebook scan failed: %s", exc)
            except Exception:  # pragma: no cover - defensive
                logger.exception("Unexpected error in notebook scan loop")
            elapsed = time.monotonic() - t0
            sleep_time = max(0.0, interval - elapsed)
            if sleep_time > 0:
                time.sleep(sleep_time)
