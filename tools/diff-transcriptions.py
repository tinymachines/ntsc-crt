#!/usr/bin/env python3
"""The M0 transcription gate, re-runnable: compare every numeric field of
two nes-levels transcriptions and exit 1 on any disagreement.

Usage: tools/diff-transcriptions.py [A.toml B.toml]
Defaults to data/nes-levels.toml against data/nes-levels-transcription-b.toml.

Parses only the flat TOML subset these files use (this box's python3
predates tomllib); an unparseable line is a loud failure, not a skip.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TABLES = ("levels", "attenuation", "phase")


def load(path):
    tables, cur = {}, None
    for n, raw in enumerate(open(path), 1):
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        m = re.match(r"\[(.+)\]$", line)
        if m:
            cur = tables.setdefault(m.group(1), {})
            continue
        if cur is None or "=" not in line:
            sys.exit(f"{path}:{n}: cannot parse: {line}")
        k, v = [s.strip() for s in line.split("=", 1)]
        if v.startswith("["):
            cur[k] = [float(x) for x in re.findall(r"-?\d+(?:\.\d+)?", v)]
        elif v.startswith('"'):
            cur[k] = v.strip('"')
        else:
            try:
                cur[k] = float(v)
            except ValueError:
                sys.exit(f"{path}:{n}: not a number: {line}")
    return tables


def main():
    a_path = sys.argv[1] if len(sys.argv) > 2 else ROOT / "data/nes-levels.toml"
    b_path = (
        sys.argv[2]
        if len(sys.argv) > 2
        else ROOT / "data/nes-levels-transcription-b.toml"
    )
    a, b = load(a_path), load(b_path)
    diffs = agreed = 0
    for table in TABLES:
        for k in sorted(set(a.get(table, {})) | set(b.get(table, {}))):
            va, vb = a[table].get(k), b[table].get(k)
            la = va if isinstance(va, list) else [va]
            lb = vb if isinstance(vb, list) else [vb]
            if (
                va is None
                or vb is None
                or len(la) != len(lb)
                or any(x != y for x, y in zip(la, lb))
            ):
                diffs += 1
                print(f"DIFF {table}.{k}: A={va} B={vb}")
            else:
                agreed += len(la)
    print(f"{agreed} numeric values agree, {diffs} disagreements")
    if diffs:
        sys.exit("GATE FAILED")
    print("GATE CLEAN")


if __name__ == "__main__":
    main()
