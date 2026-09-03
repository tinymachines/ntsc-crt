#!/usr/bin/env python3
"""Capture a composite waveform from a Rigol DS1000Z-series scope into
captures/, ready for the M4 real-recording gate.

    python3 tools/scope-capture.py <scope-ip> [name] [channel] [scale-V/div] [offset-V]

The instrument's address is an argument, never committed (the repository
rule about host-specific detail). The scope may belong to another
experiment (here it is the geiger rig's, armed and waiting): the whole
front-panel setup is saved (:SYSTem:SETup?) before a single command
changes anything and restored afterwards, and the capture channel
defaults to CH3 so the other project's probes are never disturbed. What it does, in order, printing each
step: identify the scope; configure CH1 (DC coupling, 200 mV/div,
-500 mV offset, so a 0..1.1 V composite signal sits on screen; probe
ratio left as the scope has it, because the recovery auto-levels and
absolute scale is cosmetic); set 12 Mpoint memory at 5 ms/div, which the
scope runs at 200 MSa/s: a 60 ms window, about 3.6 NES frames; free-run
briefly and STOP (no trigger level needed: any window of a live
composite signal contains whole frames); read the raw record in chunks
over SCPI; write captures/<name>.u8 beside captures/<name>.toml with the
sample rate the scope itself reports.

The scope input is 1 Mohm, so an unterminated NES sees roughly double
the 75-ohm table voltages and a soft edge or two from reflections;
auto-levelling normalizes the scale and the burst lock does not care. A
75-ohm feedthrough terminator at the BNC makes the levels textbook but
is not required.
"""
import socket
import sys
import time
from pathlib import Path

HOST = sys.argv[1] if len(sys.argv) > 1 else sys.exit(__doc__)
NAME = sys.argv[2] if len(sys.argv) > 2 else "real-nes"
CH = int(sys.argv[3]) if len(sys.argv) > 3 else 3
# Vertical window overrides, for signals that are not ~1.1 V: an
# unterminated NES into the 1 Mohm input measured 2.68 Vpp on the real
# bench, which the stock 200 mV/div window clips. 0.5 V/div with a
# -1.3 V offset frames it.
SCALE = float(sys.argv[4]) if len(sys.argv) > 4 else 0.2
OFFSET = float(sys.argv[5]) if len(sys.argv) > 5 else -0.5
PORT = 5555
CHUNK = 250_000  # max points per RAW BYTE read on the DS1000Z


class Scope:
    def __init__(self, host):
        self.s = socket.create_connection((host, PORT), timeout=5)
        self.s.settimeout(15)

    def cmd(self, c):
        self.s.sendall(c.encode() + b"\n")

    def ask(self, c):
        self.cmd(c)
        out = b""
        while not out.endswith(b"\n"):
            out += self.s.recv(4096)
        return out.decode().strip()

    def ask_block(self, c):
        """A #9-header TMC block: header, payload, trailing newline."""
        self.cmd(c)
        buf = b""
        while len(buf) < 11:
            buf += self.s.recv(4096)
        assert buf[0:1] == b"#", f"not a TMC block: {buf[:16]!r}"
        ndig = int(buf[1:2])
        length = int(buf[2 : 2 + ndig])
        need = 2 + ndig + length + 1
        while len(buf) < need:
            buf += self.s.recv(65536)
        return buf[2 + ndig : 2 + ndig + length]


def main():
    sc = Scope(HOST)
    idn = sc.ask("*IDN?")
    print(f"scope: {idn}")
    if "DS1" not in idn and "DHO" not in idn:
        print("warning: not a DS1000Z; the dialect below may not fit")

    print("saving the scope's current setup...")
    setup = sc.ask_block(":SYSTem:SETup?")
    print(f"  {len(setup)} bytes held; it will be restored")

    off = [f":CHANnel{c}:DISPlay OFF" for c in (1, 2, 3, 4) if c != CH]
    for c in [
        ":STOP",
        *off,
        f":CHANnel{CH}:DISPlay ON",
        f":CHANnel{CH}:PROBe 1",
        f":CHANnel{CH}:COUPling DC",
        f":CHANnel{CH}:BWLimit OFF",
        f":CHANnel{CH}:SCALe {SCALE}",
        f":CHANnel{CH}:OFFSet {OFFSET}",
        ":ACQuire:TYPE NORMal",
        ":TIMebase:MAIN:SCALe 0.005",
        ":TRIGger:SWEep AUTO",
    ]:
        sc.cmd(c)
        time.sleep(0.08)
    # The DS1054Z accepts a memory-depth set only while RUNning; issued
    # while stopped it stays AUTO and the raw readback below has no
    # definite record length. Found on the first real capture.
    sc.cmd(":RUN")
    time.sleep(0.5)
    sc.cmd(":ACQuire:MDEPth 12000000")
    time.sleep(0.5)
    got = sc.ask(":ACQuire:MDEPth?")
    assert got.strip() == "12000000", f"memory depth did not take: {got!r}"
    time.sleep(1.5)  # let the window fill at least once over
    sc.cmd(":STOP")
    time.sleep(0.3)

    srate = float(sc.ask(":ACQuire:SRATe?"))
    mdepth = int(float(sc.ask(":ACQuire:MDEPth?")))
    print(f"acquired: {mdepth} points at {srate:.0f} Sa/s ({mdepth / srate * 1e3:.1f} ms)")

    sc.cmd(f":WAVeform:SOURce CHANnel{CH}")
    sc.cmd(":WAVeform:MODE RAW")
    sc.cmd(":WAVeform:FORMat BYTE")
    data = bytearray()
    t0 = time.time()
    for start in range(1, mdepth + 1, CHUNK):
        stop = min(start + CHUNK - 1, mdepth)
        sc.cmd(f":WAVeform:STARt {start}")
        sc.cmd(f":WAVeform:STOP {stop}")
        data += sc.ask_block(":WAVeform:DATA?")
        print(f"\r  read {len(data)}/{mdepth}", end="", flush=True)
    print(f"\n  {time.time() - t0:.1f} s of transfer")
    assert len(data) == mdepth, f"short read: {len(data)} of {mdepth}"

    lo, hi = min(data), max(data)
    span = hi - lo
    print(f"sample range: {lo}..{hi} of 0..255 (span {span})")
    if span < 30:
        sys.exit("the record is nearly flat: is the NES connected and on?")
    if lo == 0 or hi == 255:
        print("warning: the record touches the ADC rail; nudge CHANnel1 SCALe/OFFSet")

    out = Path(__file__).resolve().parent.parent / "captures"
    out.mkdir(exist_ok=True)
    (out / f"{NAME}.u8").write_bytes(bytes(data))
    (out / f"{NAME}.toml").write_text(
        f'file = "{NAME}.u8"\nformat = "u8"\nrate_hz = {srate:.1f}\n'
        f'# captured {time.strftime("%Y-%m-%d %H:%M")} from {idn.split(",")[1] if "," in idn else idn}\n'
        f'# by tools/scope-capture.py: CH{CH} DC, {SCALE * 1000:.0f} mV/div, {OFFSET * 1000:.0f} mV offset, 12 Mpt, 5 ms/div\n'
    )
    print(f"wrote {out / (NAME + '.u8')} and its .toml")

    print("restoring the scope's setup...")
    sc.s.sendall(b":SYSTem:SETup " + f"#9{len(setup):09d}".encode() + setup + b"\n")
    time.sleep(2.0)
    sc.cmd(":RUN")  # back to the experiment it was armed for
    print("restored and running")


if __name__ == "__main__":
    main()
