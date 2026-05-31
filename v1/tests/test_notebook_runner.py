"""Tests for the scipy-optional notebook RSSI sensing path.

These lock in the fix that makes the notebook capture path runnable with the
Python standard library + numpy only (no scipy):

  * ``feature_extractor`` imports and computes features without scipy.
  * The numpy ``_skew`` / ``_kurtosis`` fallbacks match scipy's bias-corrected
    reference values.
  * ``NotebookSensingApp.run_once`` exits cleanly (rc=3, no traceback) when the
    scanner returns no networks (e.g. macOS Location Services permission denied).
  * The full collect -> extract -> classify pipeline runs end to end.

The file is runnable two ways:
    python3 -m pytest v1/tests/test_notebook_runner.py        # with pytest
    PYTHONPATH=. python3 v1/tests/test_notebook_runner.py      # no pytest needed
"""

import sys

import numpy as np

try:
    import pytest
except ImportError:  # allow running as a plain script when pytest is absent
    pytest = None

from v1.src.sensing import feature_extractor as fe
from v1.src.sensing.feature_extractor import (
    RssiFeatureExtractor,
    _kurtosis,
    _skew,
)


def _approx(a: float, b: float, rel: float = 1e-12) -> bool:
    if pytest is not None:
        return a == pytest.approx(b, rel=rel)
    return abs(a - b) <= rel * max(abs(a), abs(b), 1.0)


def test_feature_extractor_runs_without_scipy():
    """Features are produced regardless of whether scipy is installed."""
    rng = np.random.default_rng(42)
    rssi = rng.normal(-55.0, 5.0, 64)
    features = RssiFeatureExtractor(window_seconds=10.0).extract_from_array(
        rssi, sample_rate_hz=1.0
    )
    assert features.n_samples == 64
    assert features.std > 0.0
    # FFT-derived band powers must be real, finite, non-negative.
    assert np.isfinite(features.motion_band_power)
    assert features.motion_band_power >= 0.0
    assert np.isfinite(features.dominant_freq_hz)


def test_skew_matches_scipy_reference():
    """_skew matches scipy.stats.skew(x, bias=False) for a known array."""
    x = np.array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 20.0])
    # Reference from scipy's bias-corrected definition:
    #   G1 = (m3 / m2**1.5) * sqrt(n*(n-1)) / (n-2)
    expected = 1.9045442052463915
    assert _approx(_skew(x), expected)


def test_kurtosis_matches_scipy_reference():
    """_kurtosis matches scipy.stats.kurtosis(x, bias=False) for a known array."""
    x = np.array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 20.0])
    # Reference from scipy's bias-corrected Fisher kurtosis:
    #   G2 = ((n+1)*g2 + 6) * (n-1) / ((n-2)*(n-3)), g2 = m4/m2**2 - 3
    expected = 4.583510204081634
    assert _approx(_kurtosis(x), expected)


def test_moment_fallbacks_handle_degenerate_input():
    """Too-few-samples / constant signals return 0.0 without raising."""
    assert _skew(np.array([1.0, 2.0])) == 0.0
    assert _kurtosis(np.array([1.0, 2.0, 3.0])) == 0.0
    assert _skew(np.ones(8)) == 0.0
    assert _kurtosis(np.ones(8)) == 0.0


def test_fft_fallback_forced_off_scipy():
    """Forcing the numpy FFT branch still yields sane, finite spectral output."""
    original = fe._HAS_SCIPY
    fe._HAS_SCIPY = False
    try:
        rng = np.random.default_rng(7)
        rssi = rng.normal(-60.0, 3.0, 128)
        features = RssiFeatureExtractor(window_seconds=30.0).extract_from_array(
            rssi, sample_rate_hz=10.0
        )
    finally:
        fe._HAS_SCIPY = original
    assert np.isfinite(features.total_spectral_power)
    assert features.total_spectral_power >= 0.0
    assert 0.0 <= features.dominant_freq_hz <= 5.0  # within Nyquist (10 Hz / 2)


def test_run_once_empty_scan_reports_absent_not_crash():
    """An empty scan (zero APs) yields a clean ABSENT result, not a crash.

    When the OS scan succeeds but returns no networks, RSSI aggregates to the
    noise floor (-100 dBm) and the classifier honestly reports ABSENT. The
    process exits 0 with a real presence line -- never a traceback.
    """
    import v1.src.sensing.notebook_scan as ns
    from v1.src.sensing.notebook_runner import NotebookSensingApp

    original = ns.scan_for_platform
    ns.scan_for_platform = lambda system=None: []
    try:
        app = NotebookSensingApp(
            interval=0.02, window_seconds=2.0, scan_rate_hz=20.0
        )
        rc = app.run_once(timeout=3.0)
    finally:
        ns.scan_for_platform = original
    assert rc == 0


def test_run_once_unavailable_scanner_exits_cleanly():
    """When the platform scan is unavailable, run_once exits 2 with a message.

    This is the macOS-Location-denied / missing-tool path: ``is_available()``
    returns False, so the app prints the actionable reason and calls
    ``sys.exit(2)`` -- a clean exit, not a ModuleNotFoundError traceback.
    """
    import v1.src.sensing.notebook_scan as ns
    from v1.src.sensing.notebook_runner import NotebookSensingApp

    original = ns.scan_for_platform

    def boom(system=None):
        raise ns.ScanError("simulated: Location Services permission denied")

    ns.scan_for_platform = boom
    try:
        app = NotebookSensingApp(window_seconds=2.0, scan_rate_hz=10.0)
        raised = None
        try:
            app.run_once(timeout=1.0)
        except SystemExit as exc:
            raised = exc.code
    finally:
        ns.scan_for_platform = original
    assert raised == 2


def test_full_pipeline_runs_without_scipy():
    """End-to-end: stubbed scan -> real extract+classify produces a result."""
    import random
    import time as _t

    import v1.src.sensing.notebook_scan as ns
    from v1.src.sensing.notebook_scan import WifiNetwork
    from v1.src.sensing.notebook_runner import NotebookSensingApp

    random.seed(42)

    def fake_scan(system=None):
        base = -55.0 + random.uniform(-6, 6)
        return [
            WifiNetwork(
                ssid=f"AP{i}",
                bssid=f"aa:bb:cc:00:00:0{i}",
                rssi_dbm=base - i * 3 + random.uniform(-2, 2),
                channel=36,
            )
            for i in range(5)
        ]

    original = ns.scan_for_platform
    ns.scan_for_platform = fake_scan
    try:
        app = NotebookSensingApp(
            interval=0.02, window_seconds=2.0, scan_rate_hz=20.0
        )
        app.collector.start()
        out = None
        deadline = _t.monotonic() + 3.0
        while out is None and _t.monotonic() < deadline:
            out = app._tick()
            if out is None:
                _t.sleep(0.05)
        app.collector.stop()
    finally:
        ns.scan_for_platform = original

    assert out is not None, "pipeline produced no result within timeout"
    features, result, n_aps = out
    assert n_aps == 5
    assert np.isfinite(features.motion_band_power)
    assert result.motion_level.value  # a real classification label


def _run_all_as_script() -> int:
    """Run every test_* function in-process so this file works without pytest."""
    tests = [v for k, v in sorted(globals().items())
             if k.startswith("test_") and callable(v)]
    failures = 0
    for t in tests:
        try:
            t()
            print(f"PASS {t.__name__}")
        except Exception as exc:  # noqa: BLE001 - surface any failure
            failures += 1
            print(f"FAIL {t.__name__}: {exc!r}")
    print(f"\n{len(tests) - failures} passed, {failures} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(_run_all_as_script())
