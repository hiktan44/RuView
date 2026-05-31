"""BFI (Beamforming Feedback Information) capture bridge for hardware-free WiFi sensing.

This module captures 802.11ac/ax **VHT Compressed Beamforming** action frames off
the air with ``tcpdump`` in monitor mode, parses the VHT MIMO Control header and
the per-subcarrier angle bitstream, and turns a temporal window of reports into a
presence decision (Absent / PresentStill / Active) — all WITHOUT firmware
modification, exactly the FR-1.2 "dual-mode" path described in the PRD.

Physics / honesty notes
-----------------------
* BFI is transmitted **unencrypted** (to minimise latency), so a passive monitor
  can read it without joining the network. This is why it works at all.
* You still need an adapter that supports **monitor mode** on the AP's channel.
  Many laptop built-in cards (incl. most Apple Wi-Fi cards on recent macOS) do
  *not* reliably surface VHT action frames; a cheap USB monitor-mode adapter
  (mt76 / Atheros) is the dependable path. ``--probe`` tells you what your card
  can actually do before you commit.
* Monitor mode + raw capture require **root** (``sudo``).

This is intentionally dependency-free (stdlib + ``tcpdump``). The heavy parsing /
angle decoding lives in the Rust ``wifi-densepose-bfi`` crate; this script mirrors
just enough of it to drive a live presence read and to dump raw frames (``--dump``)
that the Rust crate can consume verbatim.
"""

from __future__ import annotations

import argparse
import math
import platform
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field

# 802.11 action-frame constants for VHT Compressed Beamforming
CATEGORY_VHT = 0x15
ACTION_COMPRESSED_BEAMFORMING = 0x00

# Management frame, subtype Action (0xD0 in the first MAC header byte for
# type=management(00) subtype=action(1101)).
FRAME_CONTROL_ACTION = 0xD0


# --------------------------------------------------------------------------- #
# Capability probing (the #2 "can my Mac even do this?" test)
# --------------------------------------------------------------------------- #
@dataclass
class ProbeResult:
    platform: str
    wifi_iface: str | None
    has_tcpdump: bool
    monitor_capable: bool | None  # None = unknown without root
    notes: list[str] = field(default_factory=list)

    def human(self) -> str:
        lines = [
            f"platform        : {self.platform}",
            f"wifi interface  : {self.wifi_iface or '(not found)'}",
            f"tcpdump present : {self.has_tcpdump}",
            f"monitor capable : {self.monitor_capable if self.monitor_capable is not None else 'unknown (needs sudo to test)'}",
        ]
        for n in self.notes:
            lines.append(f"  - {n}")
        return "\n".join(lines)


def detect_wifi_iface() -> str | None:
    """Best-effort Wi-Fi interface detection per OS."""
    system = platform.system()
    if system == "Darwin":
        try:
            out = subprocess.run(
                ["networksetup", "-listallhardwareports"],
                capture_output=True, text=True, timeout=10,
            ).stdout
            lines = out.splitlines()
            for i, ln in enumerate(lines):
                if "Wi-Fi" in ln and i + 1 < len(lines):
                    dev = lines[i + 1].split(":")[-1].strip()
                    if dev:
                        return dev
        except Exception:
            return None
    elif system == "Linux":
        try:
            out = subprocess.run(["iw", "dev"], capture_output=True, text=True, timeout=10).stdout
            for ln in out.splitlines():
                ln = ln.strip()
                if ln.startswith("Interface "):
                    return ln.split()[1]
        except Exception:
            return None
    return None


def probe(iface: str | None) -> ProbeResult:
    system = platform.system()
    iface = iface or detect_wifi_iface()
    has_tcpdump = shutil.which("tcpdump") is not None
    notes: list[str] = []
    monitor_capable: bool | None = None

    if not has_tcpdump:
        notes.append("Install tcpdump (macOS ships it at /usr/sbin/tcpdump).")

    if system == "Darwin":
        notes.append(
            "macOS: Apple built-in cards rarely expose VHT beamforming action "
            "frames in monitor mode. If --listen sees 0 BFI frames, use a USB "
            "monitor-mode adapter (mt76 chipset, e.g. Alfa AWUS036ACM)."
        )
        notes.append(
            "macOS monitor capture: `sudo tcpdump -I -i <iface> -y IEEE802_11_RADIO`."
        )
    elif system == "Linux":
        notes.append(
            "Linux: put the card in monitor mode first, e.g. "
            "`sudo iw dev <iface> set type monitor && sudo ip link set <iface> up`, "
            "and lock the AP channel with `sudo iw dev <iface> set channel <N>`."
        )
    else:
        notes.append(f"Untested platform: {system}. BFI capture is best on Linux/macOS.")

    return ProbeResult(
        platform=system,
        wifi_iface=iface,
        has_tcpdump=has_tcpdump,
        monitor_capable=monitor_capable,
        notes=notes,
    )


# --------------------------------------------------------------------------- #
# Minimal radiotap + 802.11 + VHT beamforming parsing
# --------------------------------------------------------------------------- #
class BitReader:
    """MSB-first bit reader over a byte string (mirrors the Rust BitReader)."""

    def __init__(self, data: bytes) -> None:
        self._data = data
        self._bitpos = 0

    def remaining_bits(self) -> int:
        return len(self._data) * 8 - self._bitpos

    def read(self, nbits: int) -> int | None:
        if nbits > self.remaining_bits():
            return None
        val = 0
        for _ in range(nbits):
            byte = self._data[self._bitpos // 8]
            bit = (byte >> (7 - (self._bitpos % 8))) & 1
            val = (val << 1) | bit
            self._bitpos += 1
        return val


def _radiotap_len(buf: bytes) -> int | None:
    """Return the radiotap header length (it_len, little-endian u16 at offset 2)."""
    if len(buf) < 4:
        return None
    return buf[2] | (buf[3] << 8)


@dataclass
class BfiFrame:
    source: bytes  # 6-byte TA
    nc: int
    nr: int
    bandwidth: int
    feature: list[float]  # flattened angle feature vector (radians)


def parse_action_frame(mac_body: bytes) -> BfiFrame | None:
    """Parse a management Action frame body; return a BfiFrame if it is a VHT
    compressed beamforming report, else None.

    `mac_body` must start at the 802.11 MAC header.
    """
    # 802.11 mgmt header: FC(2) Dur(2) A1(6) A2(6) A3(6) SeqCtl(2) = 24 bytes
    if len(mac_body) < 24 + 5:
        return None
    fc0 = mac_body[0]
    if fc0 != FRAME_CONTROL_ACTION:
        return None
    ta = mac_body[10:16]  # A2 = transmitter address
    body = mac_body[24:]
    category = body[0]
    action = body[1]
    if category != CATEGORY_VHT or action != ACTION_COMPRESSED_BEAMFORMING:
        return None

    # VHT MIMO Control: 3 bytes, little-endian 24-bit word.
    if len(body) < 2 + 3:
        return None
    mc = body[2] | (body[3] << 8) | (body[4] << 16)
    nc = (mc & 0x7) + 1
    nr = ((mc >> 3) & 0x7) + 1
    bandwidth = (mc >> 6) & 0x3
    # grouping = (mc >> 8) & 0x3
    codebook = (mc >> 10) & 0x1
    # feedback_type = (mc >> 11) & 0x1

    phi_bits = 9 if codebook else 7
    psi_bits = 7 if codebook else 5

    # angles per subcarrier for this geometry
    cols = min(nc, max(nr - 1, 0))
    angles = 0
    for i in range(1, cols + 1):
        angles += 2 * (nr - i)
    if angles == 0:
        return None

    # Skip Nc average-SNR bytes, then decode the angle bitstream.
    snr_off = 5 + nc
    stream = body[snr_off:]
    reader = BitReader(stream)

    feat: list[float] = []
    # decode as many full subcarriers as the stream allows
    bits_per_sc = cols * 0  # compute precisely below
    # per subcarrier: for each col i: (nr-i) phi (phi_bits) + (nr-i) psi (psi_bits)
    per_sc_bits = 0
    for i in range(1, cols + 1):
        per_sc_bits += (nr - i) * phi_bits + (nr - i) * psi_bits
    if per_sc_bits == 0:
        return None
    n_sc = reader.remaining_bits() // per_sc_bits
    n_sc = min(n_sc, 256)  # safety cap

    two_pow = lambda k: float(1 << k)
    for _ in range(n_sc):
        for i in range(1, cols + 1):
            count = nr - i
            for _ in range(count):
                k = reader.read(phi_bits)
                if k is None:
                    return _finish(ta, nc, nr, bandwidth, feat)
                phi = k * math.pi / two_pow(phi_bits - 1) + math.pi / two_pow(phi_bits)
                feat.append(phi)
            for _ in range(count):
                k = reader.read(psi_bits)
                if k is None:
                    return _finish(ta, nc, nr, bandwidth, feat)
                psi = k * math.pi / two_pow(psi_bits + 1) + math.pi / two_pow(psi_bits + 2)
                feat.append(psi)
    return _finish(ta, nc, nr, bandwidth, feat)


def _finish(ta, nc, nr, bw, feat) -> BfiFrame | None:
    if not feat:
        return None
    return BfiFrame(source=ta, nc=nc, nr=nr, bandwidth=bw, feature=feat)


# --------------------------------------------------------------------------- #
# Presence from a temporal window (mirrors wifi-densepose-bfi presence.rs)
# --------------------------------------------------------------------------- #
def classify_window(features: list[list[float]], sample_rate_hz: float,
                    presence_thr: float, motion_thr: float) -> tuple[str, float, float]:
    """Return (state, total_variance, motion_band_power)."""
    if len(features) < 2:
        return ("ABSENT", 0.0, 0.0)
    # align to shortest feature length
    dim = min(len(f) for f in features)
    if dim == 0:
        return ("ABSENT", 0.0, 0.0)
    cols = [[f[d] for f in features] for d in range(dim)]
    # per-dim variance -> total variance
    total_var = 0.0
    for c in cols:
        m = sum(c) / len(c)
        total_var += sum((x - m) ** 2 for x in c) / len(c)
    total_var /= dim
    # motion band power via Goertzel-ish: energy of first differences
    motion = 0.0
    for c in cols:
        diffs = [c[i + 1] - c[i] for i in range(len(c) - 1)]
        motion += sum(d * d for d in diffs) / max(len(diffs), 1)
    motion /= dim
    motion *= sample_rate_hz / 20.0  # normalise to reference rate

    if total_var < presence_thr:
        state = "ABSENT"
        conf = 0.6
    elif motion < motion_thr:
        state = "PRESENT_STILL"
        conf = 0.7
    else:
        state = "ACTIVE"
        conf = min(0.95, 0.6 + motion)
    return (state, total_var, motion)


# --------------------------------------------------------------------------- #
# Live capture
# --------------------------------------------------------------------------- #
def tcpdump_cmd(iface: str) -> list[str]:
    """tcpdump capturing ALL 802.11 frames with radiotap in monitor mode.

    macOS tcpdump rejects 802.11 BPF primitives (``type mgt subtype action``) on
    the radiotap link type, so we capture everything and filter to VHT
    beamforming action frames in Python (see ``parse_action_frame``).

    -I monitor mode, -y radiotap link type, -e link headers, -x hex, no name res.
    """
    return [
        "tcpdump", "-I", "-i", iface,
        "-y", "IEEE802_11_RADIO",
        "-e", "-x", "-nn", "-l", "-s", "0",
    ]


def _hex_block_to_bytes(lines: list[str]) -> bytes:
    """Parse tcpdump -x hex dump lines (offset: hhhh hhhh ...) into bytes."""
    out = bytearray()
    for ln in lines:
        ln = ln.strip()
        if ":" not in ln:
            continue
        hexpart = ln.split(":", 1)[1].strip()
        for tok in hexpart.split():
            if len(tok) == 4 and all(c in "0123456789abcdefABCDEF" for c in tok):
                out.append(int(tok[0:2], 16))
                out.append(int(tok[2:4], 16))
            elif len(tok) == 2 and all(c in "0123456789abcdefABCDEF" for c in tok):
                out.append(int(tok, 16))
    return bytes(out)


def listen(iface: str, seconds: int, window: int, sample_rate_hz: float,
           presence_thr: float, motion_thr: float, dump_path: str | None) -> int:
    cmd = tcpdump_cmd(iface)
    print(f"  Capturing on {iface} (monitor mode) for {seconds}s ...", flush=True)
    print(f"  cmd: {' '.join(cmd)}", flush=True)
    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    except FileNotFoundError:
        print("  ERROR: tcpdump not found.", file=sys.stderr)
        return 2

    dump = open(dump_path, "w") if dump_path else None
    window_feats: list[list[float]] = []
    total_frames = 0
    bfi_frames = 0
    block: list[str] = []

    # Make stdout non-blocking so the deadline fires even when NO frames arrive
    # (the previous blocking `for line in proc.stdout` hung forever on a silent
    # interface). We poll with select() and stop at the deadline regardless.
    import select

    start = time.time()
    deadline = start + seconds
    last_report = start
    stdout_fd = proc.stdout  # type: ignore[union-attr]

    def flush_block() -> None:
        nonlocal total_frames, bfi_frames, block
        if not block:
            return
        total_frames += 1
        pkt = _hex_block_to_bytes(block)
        fr = _handle_packet(pkt)
        if fr is not None:
            bfi_frames += 1
            window_feats.append(fr.feature)
            if dump:
                dump.write(pkt.hex() + "\n")
        block = []

    try:
        while True:
            now = time.time()
            if now >= deadline:
                break
            # wait up to 0.5s for a line; loop continues to check the deadline
            ready, _, _ = select.select([stdout_fd], [], [], 0.5)
            if ready:
                raw = stdout_fd.readline()
                if raw == "":  # tcpdump exited
                    break
                line = raw.rstrip("\n")
                # tcpdump groups: a header line (no leading whitespace) then hex lines
                if line and not line[0].isspace() and ":" in line and "0x0000" not in line:
                    flush_block()
                block.append(line)

            if len(window_feats) >= window:
                state, var, motion = classify_window(
                    window_feats[-window:], sample_rate_hz, presence_thr, motion_thr)
                print(f"  {time.strftime('%H:%M:%S')}  {state:<13} "
                      f"var={var:.4f} motion={motion:.4f} bfi={bfi_frames} total={total_frames}",
                      flush=True)
                window_feats[:] = window_feats[-window:]
            elif now - last_report >= 2.0:
                print(f"  ... {total_frames} frames, {bfi_frames} BFI so far "
                      f"({int(deadline - now)}s left)", flush=True)
                last_report = now
        flush_block()
    except KeyboardInterrupt:
        print("\n  (interrupted)", flush=True)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except Exception:
            proc.kill()
        if dump:
            dump.close()

    err = proc.stderr.read() if proc.stderr else ""
    print("\n  === Summary ===", flush=True)
    print(f"  802.11 frames captured (all types): {total_frames}")
    print(f"  VHT BFI reports parsed            : {bfi_frames}")
    if dump_path:
        print(f"  raw BFI frames written to         : {dump_path}")
    if bfi_frames == 0:
        print("\n  No BFI frames seen. Most likely causes:")
        print("   - This card can't surface VHT beamforming in monitor mode")
        print("     (very common on Apple built-in Wi-Fi). Try a USB mt76 adapter.")
        print("   - Wrong channel: lock the monitor iface to the AP's channel.")
        print("   - The network/clients aren't using MU-MIMO beamforming right now.")
        if err.strip():
            print(f"\n  tcpdump stderr: {err.strip()[:400]}")
    return 0 if bfi_frames > 0 else 1


def _handle_packet(pkt: bytes) -> BfiFrame | None:
    rt_len = _radiotap_len(pkt)
    if rt_len is None or rt_len <= 0 or rt_len >= len(pkt):
        return None
    mac = pkt[rt_len:]
    return parse_action_frame(mac)


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #
def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(
        prog="bfi_capture",
        description="Capture 802.11ac/ax BFI frames and derive WiFi presence (FR-1.2).",
    )
    p.add_argument("--iface", help="Wi-Fi interface (auto-detected if omitted)")
    p.add_argument("--probe", action="store_true",
                   help="Report capture capability and exit (no root needed)")
    p.add_argument("--listen", action="store_true",
                   help="Capture live (needs sudo / monitor mode)")
    p.add_argument("--seconds", type=int, default=20, help="Capture duration (default 20)")
    p.add_argument("--window", type=int, default=32, help="Frames per presence window")
    p.add_argument("--rate", type=float, default=20.0, help="Assumed BFI rate Hz")
    p.add_argument("--presence-threshold", type=float, default=0.015)
    p.add_argument("--motion-threshold", type=float, default=0.05)
    p.add_argument("--dump", help="Write raw captured BFI frames (hex/line) to this file")
    args = p.parse_args(argv)

    if args.probe or not args.listen:
        res = probe(args.iface)
        print(res.human())
        if not args.listen:
            print("\nRun a live capture with:")
            ifc = res.wifi_iface or "en1"
            print(f"  sudo python3 -m v1.src.sensing.bfi_capture --listen --iface {ifc} --seconds 30")
            return 0

    iface = args.iface or detect_wifi_iface()
    if not iface:
        print("ERROR: could not detect a Wi-Fi interface; pass --iface.", file=sys.stderr)
        return 2
    return listen(
        iface, args.seconds, args.window, args.rate,
        args.presence_threshold, args.motion_threshold, args.dump,
    )


if __name__ == "__main__":
    raise SystemExit(main())
