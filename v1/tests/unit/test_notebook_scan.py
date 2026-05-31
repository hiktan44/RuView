"""
Unit tests for the notebook (laptop) WiFi scan-output parsers.

These tests feed captured ``netsh`` / ``nmcli`` / ``iw`` / macOS
``system_profiler`` output strings to the pure parser functions and assert the
resulting :class:`WifiNetwork` lists. No subprocess / hardware is touched, so
they run anywhere.
"""

from __future__ import annotations

import pytest

from v1.src.sensing.notebook_scan import (
    NotebookWifiCollector,
    WifiNetwork,
    aggregate_rssi,
    parse_iw_scan,
    parse_macos_airport,
    parse_netsh_networks,
    parse_nmcli,
    signal_pct_to_dbm,
)


# ---------------------------------------------------------------------------
# Sample command outputs (captured / representative)
# ---------------------------------------------------------------------------

NETSH_SAMPLE = """
Interface name : Wi-Fi
There are 2 networks currently visible.

SSID 1 : HomeNet
    Network type            : Infrastructure
    Authentication          : WPA2-Personal
    Encryption              : CCMP
    BSSID 1                 : aa:bb:cc:dd:ee:ff
         Signal             : 86%
         Radio type         : 802.11ac
         Channel            : 36
    BSSID 2                 : aa:bb:cc:dd:ee:00
         Signal             : 50%
         Radio type         : 802.11n
         Channel            : 6

SSID 2 : CafeWiFi
    Network type            : Infrastructure
    BSSID 1                 : 11:22:33:44:55:66
         Signal             : 30%
         Channel            : 11
"""

NMCLI_SAMPLE = (
    r"HomeNet:AA\:BB\:CC\:DD\:EE\:FF:86" "\n"
    r"CafeWiFi:11\:22\:33\:44\:55\:66:30" "\n"
    r":DE\:AD\:BE\:EF\:00\:01:10" "\n"   # hidden SSID (empty name)
)

IW_SAMPLE = """
BSS aa:bb:cc:dd:ee:ff(on wlan0)
    signal: -55.00 dBm
    SSID: HomeNet
    DS Parameter set: channel 36
BSS 11:22:33:44:55:66(on wlan0)
    signal: -78.00 dBm
    SSID: CafeWiFi
    DS Parameter set: channel 11
"""

MACOS_SAMPLE = """
Wi-Fi:

      Software Versions:
          CoreWLAN: 16.0
      Interfaces:
        en0:
          Card Type: Wi-Fi
          Current Network Information:
            HomeNet:
              PHY Mode: 802.11ax
              Channel: 36 (5GHz, 80MHz)
              Signal / Noise: -52 dBm / -90 dBm
          Other Local Wi-Fi Networks:
            CafeWiFi:
              PHY Mode: 802.11n
              Channel: 11 (2GHz, 20MHz)
              Signal / Noise: -75 dBm / -92 dBm
            Library5G:
              PHY Mode: 802.11ac
              Channel: 149 (5GHz, 80MHz)
              Signal / Noise: -68 dBm / -91 dBm
"""


# ---------------------------------------------------------------------------
# signal_pct_to_dbm
# ---------------------------------------------------------------------------

def test_signal_pct_to_dbm_endpoints():
    assert signal_pct_to_dbm(0) == pytest.approx(-100.0)
    assert signal_pct_to_dbm(100) == pytest.approx(-50.0)
    assert signal_pct_to_dbm(86) == pytest.approx(-57.0)


def test_signal_pct_to_dbm_clamps():
    assert signal_pct_to_dbm(-20) == pytest.approx(-100.0)
    assert signal_pct_to_dbm(150) == pytest.approx(-50.0)


# ---------------------------------------------------------------------------
# netsh (Windows)
# ---------------------------------------------------------------------------

def test_parse_netsh_networks_counts_and_fields():
    nets = parse_netsh_networks(NETSH_SAMPLE)
    # 2 BSSIDs under HomeNet + 1 under CafeWiFi = 3
    assert len(nets) == 3

    home = [n for n in nets if n.ssid == "HomeNet"]
    assert len(home) == 2
    strongest = max(home, key=lambda n: n.rssi_dbm)
    assert strongest.bssid == "aa:bb:cc:dd:ee:ff"
    assert strongest.channel == 36
    assert strongest.rssi_dbm == pytest.approx(signal_pct_to_dbm(86))

    cafe = [n for n in nets if n.ssid == "CafeWiFi"][0]
    assert cafe.bssid == "11:22:33:44:55:66"
    assert cafe.channel == 11


def test_parse_netsh_empty():
    assert parse_netsh_networks("") == []


# ---------------------------------------------------------------------------
# nmcli (Linux)
# ---------------------------------------------------------------------------

def test_parse_nmcli_handles_escaped_bssid_colons():
    nets = parse_nmcli(NMCLI_SAMPLE)
    assert len(nets) == 3

    home = nets[0]
    assert home.ssid == "HomeNet"
    assert home.bssid == "aa:bb:cc:dd:ee:ff"
    assert home.rssi_dbm == pytest.approx(signal_pct_to_dbm(86))

    hidden = nets[2]
    assert hidden.ssid == ""
    assert hidden.bssid == "de:ad:be:ef:00:01"


def test_parse_nmcli_skips_malformed_lines():
    out = "JustOneField\nGood:AA\\:BB\\:CC\\:DD\\:EE\\:FF:42\n"
    nets = parse_nmcli(out)
    assert len(nets) == 1
    assert nets[0].ssid == "Good"


# ---------------------------------------------------------------------------
# iw (Linux fallback)
# ---------------------------------------------------------------------------

def test_parse_iw_scan_uses_real_dbm():
    nets = parse_iw_scan(IW_SAMPLE)
    assert len(nets) == 2
    home = [n for n in nets if n.ssid == "HomeNet"][0]
    assert home.bssid == "aa:bb:cc:dd:ee:ff"
    assert home.rssi_dbm == pytest.approx(-55.0)
    assert home.channel == 36


# ---------------------------------------------------------------------------
# macOS system_profiler
# ---------------------------------------------------------------------------

def test_parse_macos_airport_extracts_networks():
    nets = parse_macos_airport(MACOS_SAMPLE)
    names = {n.ssid for n in nets}
    assert {"HomeNet", "CafeWiFi", "Library5G"}.issubset(names)

    home = [n for n in nets if n.ssid == "HomeNet"][0]
    assert home.rssi_dbm == pytest.approx(-52.0)
    assert home.channel == 36
    # system_profiler does not expose BSSID
    assert home.bssid == ""


def test_parse_macos_airport_empty():
    assert parse_macos_airport("") == []


# ---------------------------------------------------------------------------
# aggregate_rssi
# ---------------------------------------------------------------------------

def test_aggregate_rssi_averages_top_k():
    nets = [
        WifiNetwork("a", "", -40.0),
        WifiNetwork("b", "", -50.0),
        WifiNetwork("c", "", -90.0),
    ]
    # top_k=2 -> mean(-40, -50) = -45
    assert aggregate_rssi(nets, top_k=2) == pytest.approx(-45.0)


def test_aggregate_rssi_empty_returns_floor():
    assert aggregate_rssi([], top_k=5) == pytest.approx(-100.0)


# ---------------------------------------------------------------------------
# Collector wiring (no real scan -- uses parsed sample to build a WifiSample)
# ---------------------------------------------------------------------------

def test_notebook_collector_read_sample_builds_wifisample(monkeypatch):
    from v1.src.sensing import notebook_scan as ns

    parsed = parse_netsh_networks(NETSH_SAMPLE)
    monkeypatch.setattr(ns, "scan_for_platform", lambda system=None: parsed)

    collector = NotebookWifiCollector(system="Windows", top_k=3)
    sample = collector.collect_once()

    assert sample.rssi_dbm == pytest.approx(
        aggregate_rssi(parsed, top_k=3)
    )
    assert "notebook-scan" in sample.interface
    assert collector.last_networks == parsed


def test_notebook_collector_is_available(monkeypatch):
    from v1.src.sensing import notebook_scan as ns

    monkeypatch.setattr(
        ns, "scan_for_platform",
        lambda system=None: parse_nmcli(NMCLI_SAMPLE),
    )
    available, reason = NotebookWifiCollector.is_available(system="Linux")
    assert available is True
    assert "networks visible" in reason
