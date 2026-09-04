#!/usr/bin/env python3
"""Figures for the composite deep-dive, straight from scope captures.

    python3 tools/composite-figures.py <outdir> <loaded.u8> <loaded.toml> [unloaded.u8 unloaded.toml]

Every capture is a DS1054Z byte record; the .toml written beside it by
tools/scope-capture.py names the window (V/div and offset), and the
scope's own convention turns bytes into volts:

    V = (byte - yorigin - yreference) * yincrement
    yincrement = V/div / 25,  yreference = 127,  yorigin = offset / yincrement

(read back from the instrument at the capture window on 2026-09-04:
200 mV/div, -528 mV offset gave yincrement 8 mV, yorigin -66, yref 127).
Nothing here is typed from a datasheet: the levels drawn against the
waveform come from data/nes-levels.toml, the line period from the grid
contract's subcarrier (315/88 MHz, 227.5 cycles per line), and every
number printed is measured from the record.

Writes: scanline.png, burst.png, overlay.png (with an unloaded
record), histogram.png, and figures.json with the measurements.
"""
import json
import re
import sys
from pathlib import Path

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

F_SC = 315e6 / 88
LINE_S = 227.5 / F_SC  # 63.5556 us
RATE = 125e6


def load(u8_path, toml_path):
    text = Path(toml_path).read_text()
    m = re.search(r"(\d+) mV/div, (-?\d+) mV offset", text)
    vdiv, offs = int(m.group(1)) / 1000, int(m.group(2)) / 1000
    yinc = vdiv / 25
    yor = offs / yinc
    raw = np.fromfile(u8_path, dtype=np.uint8).astype(np.float64)
    return (raw - yor - 127) * yinc, vdiv, offs


def sync_edges(v):
    """Horizontal sync falling edges. The threshold sits a little above
    the record's floor (the sync tips): floor plus 15 percent of the
    whole swing, which is below blanking on every record seen and above
    nothing else that is low for long. A fall counts only if the signal
    stays below the threshold for the next 3 us (a horizontal sync is
    4.7 us low; a chroma trough or an equalizing pulse is not), and
    falls closer than most of a line to the last kept one are dropped."""
    lo, hi = np.percentile(v, 0.5), np.percentile(v, 99.9)
    thr = lo + 0.15 * (hi - lo)
    below = v < thr
    falls = np.flatnonzero(~below[:-1] & below[1:])
    hold = int(3e-6 * RATE)
    keep = []
    for f in falls:
        if f + hold >= len(v):
            break
        if np.mean(below[f + 1 : f + 1 + hold]) < 0.9:
            continue
        if keep and f - keep[-1] < 0.9 * LINE_S * RATE:
            continue
        keep.append(f)
    return np.array(keep)


def line_levels(v, edges):
    """Sync tip, blanking and burst amplitude read from where they sit
    on every line (medians over all lines found): the tip over
    1.0..4.0 us after the edge, blanking on the back porch after the
    burst over 8.3..8.9 us, the burst's swing as the 1st..99th
    percentile over 5.5..8.0 us."""
    def win(t0, t1):
        return np.concatenate([v[e + int(t0 * RATE) : e + int(t1 * RATE)] for e in edges[1:-1]])
    tip = float(np.median(win(1.0e-6, 4.0e-6)))
    blank = float(np.median(win(8.3e-6, 8.9e-6)))
    per_line = [v[e + int(5.5e-6 * RATE) : e + int(8.0e-6 * RATE)] for e in edges[1:-1]]
    pp = [np.percentile(b, 99) - np.percentile(b, 1) for b in per_line]
    return tip, blank, float(np.median(pp))


def levels_table(path):
    # data/nes-levels.toml, [levels]: sync_volts, burst_low_volts,
    # burst_high_volts, low_volts[4], high_volts[4]; blanking is the $1D
    # row, low_volts[1], the 0 IRE reference.
    text = Path(path).read_text()
    out = {}
    for key, name in (("sync_volts", "sync"), ("burst_low_volts", "burst_low"), ("burst_high_volts", "burst_high")):
        m = re.search(rf"^{key}\s*=\s*([0-9.]+)", text, re.M)
        if m:
            out[name] = float(m.group(1))
    lows = re.search(r"^low_volts\s*=\s*\[([^\]]+)\]", text, re.M)
    highs = re.search(r"^high_volts\s*=\s*\[([^\]]+)\]", text, re.M)
    if lows:
        out["low"] = [float(x) for x in lows.group(1).split(",")]
        out["blank"] = out["low"][1]
    if highs:
        out["high"] = [float(x) for x in highs.group(1).split(",")]
    return out


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    v, vdiv, offs = load(sys.argv[2], sys.argv[3])
    edges = sync_edges(v)
    tip, blank, burst_pp = line_levels(v, edges)
    periods = np.diff(edges) / RATE
    meas = {
        "loaded": {
            "window": f"{int(vdiv*1000)} mV/div, {int(offs*1000)} mV offset",
            "sync_tip_v": round(tip, 4),
            "blanking_v": round(blank, 4),
            "sync_to_blank_v": round(blank - tip, 4),
            "burst_pp_v": round(burst_pp, 4),
            "burst_over_sync": round(burst_pp / (blank - tip), 3),
            "peak_v": round(float(np.percentile(v, 99.9)), 4),
            "lines_found": int(len(edges)),
            "line_period_us_median": round(float(np.median(periods) * 1e6), 4),
            "line_period_nominal_us": round(LINE_S * 1e6, 4),
            "line_period_nes_2728_of_2730_us": round(LINE_S * 2728 / 2730 * 1e6, 4),
        }
    }
    table = levels_table(Path(__file__).resolve().parent.parent / "data" / "nes-levels.toml")
    meas["table"] = table

    # A middle line: from one sync fall to the next.
    k = len(edges) // 2
    a, b = edges[k], edges[k + 1]
    t = (np.arange(a - 200, b + 200) - a) / RATE * 1e6
    seg = v[a - 200 : b + 200]
    fig, ax = plt.subplots(figsize=(12, 4), dpi=150)
    ax.plot(t, seg, lw=0.6, color="#1f3b73")
    ax.axhline(tip, color="#999", lw=0.6, ls="--")
    ax.axhline(blank, color="#999", lw=0.6, ls="--")
    ax.text(t[-1], tip, " sync tip", va="center", fontsize=8, color="#666")
    ax.text(t[-1], blank, " blanking", va="center", fontsize=8, color="#666")
    ax.set_xlabel("microseconds from the sync edge")
    ax.set_ylabel("volts into 75 ohms")
    ax.set_title(f"One NES scanline, {int(vdiv*1000)} mV/div, 125 MS/s")
    fig.tight_layout()
    fig.savefig(out / "scanline.png")
    plt.close(fig)

    # The burst: 4.7 us of sync, then the back porch carries the burst.
    a2 = a + int(4.0e-6 * RATE)
    b2 = a + int(10.5e-6 * RATE)
    t = (np.arange(a2, b2) - a) / RATE * 1e6
    seg = v[a2:b2]
    fig, ax = plt.subplots(figsize=(12, 4), dpi=150)
    ax.plot(t, seg, lw=0.8, color="#1f3b73", marker=".", ms=2)
    ax.axhline(blank, color="#999", lw=0.6, ls="--")
    ax.set_xlabel("microseconds from the sync edge")
    ax.set_ylabel("volts into 75 ohms")
    ax.set_title("The colour burst: 3.579545 MHz on the back porch, every sample shown")
    fig.tight_layout()
    fig.savefig(out / "burst.png")
    plt.close(fig)
    meas["table"]["burst_over_sync"] = round((table["burst_high"] - table["burst_low"]) / (table["blank"] - table["sync"]), 3)

    # Level histogram against the table.
    fig, ax = plt.subplots(figsize=(12, 4), dpi=150)
    ax.hist(v, bins=400, color="#1f3b73", log=True)
    for name, val in (("sync tip", tip), ("blanking", blank)):
        ax.axvline(val, color="#c33", lw=0.8)
        ax.text(val, ax.get_ylim()[1], f" {name} {val:.3f} V", rotation=90, va="top", fontsize=7, color="#c33")
    if "sync" in table and "blank" in table:
        for name, val in (("table sync", table["sync"]), ("table blank", table["blank"])):
            ax.axvline(val, color="#393", lw=0.8, ls=":")
            ax.text(val, ax.get_ylim()[0] * 3, f" {name} {val:.3f} V", rotation=90, va="bottom", fontsize=7, color="#393")
    ax.set_xlabel("volts into 75 ohms")
    ax.set_ylabel("samples (log)")
    ax.set_title("Where the samples sit: the measured levels beside the transcribed table")
    fig.tight_layout()
    fig.savefig(out / "histogram.png")
    plt.close(fig)

    if len(sys.argv) > 5:
        u, uvdiv, uoffs = load(sys.argv[4], sys.argv[5])
        uedges = sync_edges(u)
        utip, ublank, uburst = line_levels(u, uedges)
        uk = len(uedges) // 2
        ua = uedges[uk]
        meas["unloaded"] = {
            "window": f"{int(uvdiv*1000)} mV/div, {int(uoffs*1000)} mV offset",
            "sync_tip_v": round(utip, 4),
            "blanking_v": round(ublank, 4),
            "sync_to_blank_v": round(ublank - utip, 4),
            "burst_pp_v": round(uburst, 4),
            "burst_over_sync": round(uburst / (ublank - utip), 3),
            "peak_v": round(float(np.percentile(u, 99.9)), 4),
        }
        n = int(12e-6 * RATE)
        t = (np.arange(-200, n) ) / RATE * 1e6
        fig, ax = plt.subplots(figsize=(12, 4), dpi=150)
        ax.plot(t, u[ua - 200 : ua + n], lw=0.6, color="#c33", label=f"unterminated, into the 1 Mohm probe ({meas['unloaded']['window']})")
        ax.plot(t, v[a - 200 : a + n], lw=0.6, color="#1f3b73", label=f"terminated, into 75 ohms ({meas['loaded']['window']})")
        ax.set_xlabel("microseconds from the sync edge")
        ax.set_ylabel("volts")
        ax.set_title("One console, two loads: sync, burst and the start of a line (different frames on screen)")
        ax.legend(fontsize=8)
        fig.tight_layout()
        fig.savefig(out / "overlay.png")
        plt.close(fig)
        meas["ratio_unloaded_over_loaded"] = {
            "sync_to_blank": round(meas["unloaded"]["sync_to_blank_v"] / meas["loaded"]["sync_to_blank_v"], 3),
            "burst_pp": round(uburst / burst_pp, 3),
        }

    (out / "figures.json").write_text(json.dumps(meas, indent=2))
    print(json.dumps(meas, indent=2))


if __name__ == "__main__":
    main()
