#!/usr/bin/env python3
"""Every place the docs count this project, checked against a fresh
measurement (handoff spec M5; the check-self-counts pattern). Each claim
is matched by a regex that must hit its stated count, so a claim that
moves out from under its pattern fails as loudly as a wrong number.

Measured sources: the exact rationals recomputed here with fractions;
the data files re-parsed; the transcription gate re-run; the frozen
constants read from the source that owns them. The cargo test totals are
measured by running the suite (and MUTATE=1) unless --fast is given, in
which case those rows SKIP; REQUIRE_ALL=1 makes a skip a failure.

Numbers this tool does NOT cover, deliberately: throughput and
diff-envelope figures (best-of-N timings and measured envelopes from
stamped runs; re-running them here would replace a recorded measurement
with a noisier one). Those live only in the reports beside their run
stamps.
"""
import re
import subprocess
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAST = "--fast" in sys.argv
REQUIRE_ALL = __import__("os").environ.get("REQUIRE_ALL") == "1"

failures = []
skips = []
passes = 0


def read(rel):
    return (ROOT / rel).read_text()


def claim(rel, pattern, expected_hits, note):
    """The pattern must appear exactly expected_hits times in the file."""
    global passes
    hits = len(re.findall(pattern, read(rel)))
    if hits == expected_hits:
        passes += 1
    else:
        failures.append(f"{rel}: pattern {pattern!r} hit {hits}x, expected {expected_hits} ({note})")


def measured(name, got, want):
    global passes
    if got == want:
        passes += 1
    else:
        failures.append(f"{name}: measured {got!r}, docs claim {want!r}")


# ---- The grid's own arithmetic, recomputed exactly. ----
fsc = Fraction(315_000_000, 88)
grid = 12 * fsc
measured("line residue", (341 * 8) % 12, 4)
measured("frame residue", (262 * 341 * 8) % 12, 4)
measured("short residue", (262 * 341 * 8 - 8) % 12, 8)
measured("broadcast line residue", 2730 % 12, 6)
measured("broadcast frame residue", (525 * 2730) % 12, 6)
measured("nes full rate", grid / 714_736, Fraction(29_531_250, 491_381))
measured("nes short rate", grid / 714_728, Fraction(8_437_500, 140_393))
measured("nes pair rate", 2 * grid / (714_736 + 714_728), Fraction(39_375_000, 655_171))
measured("broadcast field rate", 2 * grid / 1_433_250, Fraction(60_000, 1_001))
for rel, pat, n in [
    ("docs/m0-report.md", r"29,531,250 / 491,381", 1),
    ("docs/m0-report.md", r"8,437,500 / 140,393", 1),
    ("docs/m0-report.md", r"39,375,000 / 655,171", 1),
    ("docs/m0-report.md", r"60,000 / 1,001", 1),
    ("docs/m0-report.md", r"60\.09848", 2),
]:
    claim(rel, pat, n, "exact rate")
measured(
    "full-frame rate approx",
    f"{float(grid / 714_736):.5f}",
    "60.09848",
)

# ---- The transcription gate, re-run. ----
out = subprocess.run(
    [sys.executable, str(ROOT / "tools/diff-transcriptions.py")],
    capture_output=True,
    text=True,
)
m = re.search(r"(\d+) numeric values agree, 0 disagreements", out.stdout)
if not m or out.returncode != 0:
    failures.append("transcription gate did not run clean")
else:
    measured("gate values", m.group(1), "43")
claim("docs/m0-report.md", r"43 numeric values, 0\s+disagreements", 1, "gate count")
claim("README.md", r"two\s+independent transcriptions", 1, "gate prose")

# ---- The level table against its own docs. ----
levels = read("data/nes-levels.toml")
measured("attenuation factor", "0.816328" in levels, True)
claim("docs/m1-report.md", r"0\.816328", 0, "factor lives in M0's story")
claim("docs/m0-report.md", r"0\.816328", 2, "attenuation factor")

# ---- Pinned hashes, single source each, quoted where the docs say. ----
blargg_sha = re.search(r'SHA256="([0-9a-f]{64})"', read("tools/fetch-oracle.sh")).group(1)
claim("docs/m1-report.md", blargg_sha, 1, "blargg zip sha256")
smpte_sha = re.search(r'sha256 = "([0-9a-f]{64})"', read("data/broadcast-timing.toml")).group(1)
for rel in ["docs/m2-report.md", "data/yuv-matrix.toml", "NOTICE.md"]:
    hits = len(re.findall(smpte_sha, read(rel)))
    measured(f"st170 sha in {rel}", hits >= 1, True)

# ---- Frozen comparison constants: owner is ntsc-oracle/src/lib.rs. ----
oracle = read("crates/ntsc-oracle/src/lib.rs")
for name, pat, doc_pat, where, hits in [
    ("rotation", r"FITTED_ROTATION_DEG: f64 = 120\.0", r"rotation 120\.0", "docs/m1-report.md", 2),
    ("scale", r"FITTED_SCALE: f64 = 0\.78799", r"0\.78799", "docs/m1-report.md", 2),
    ("offset", r"RESAMPLE_OFFSET: f64 = -11\.8", r"-11\.8", "docs/m1-report.md", 4),
    ("burst0", r"BURST0: i32 = 2", r"BURST0 2", "docs/m1-report.md", 1),
]:
    measured(f"frozen {name} in lib", bool(re.search(pat, oracle)), True)
    claim(where, doc_pat, hits, f"frozen {name} in report")

# ---- Filters: the generated files against the docs. ----
runga = read("data/filters/rung-a.toml")
gain = float(re.search(r"gain_at_subcarrier = ([0-9.]+)", runga).group(1))
measured("chroma gain rounds to docs", f"{gain:.4f}", "0.9069")
measured("chroma taps", len(re.search(r"taps = \[(.*?)\]", runga).group(1).split(",")), 51)
enc = read("data/filters/rgb-encoder.toml")
db13 = float(re.search(r"measured_db_at_1300000 = (-?[0-9.]+)", enc).group(1))
db36 = float(re.search(r"measured_db_at_3600000 = (-?[0-9.]+)", enc).group(1))
measured("encoder template 1.3 MHz", db13 > -2.0, True)
measured("encoder template 3.6 MHz", db36 < -20.0, True)

# ---- The published bar column against the matrix derivation. ----
import math

y_row = [0.299, 0.587, 0.114]  # cross-checked against data/yuv-matrix.toml
mx = read("data/yuv-matrix.toml")
measured("y row in matrix file", "y = [0.299, 0.587, 0.114]" in mx, True)
bars_rgb = [
    (0.75, 0.75, 0.75), (0.75, 0.75, 0.0), (0.0, 0.75, 0.75), (0.0, 0.75, 0.0),
    (0.75, 0.0, 0.75), (0.75, 0.0, 0.0), (0.0, 0.0, 0.75),
]
published = [76.9, 69.0, 56.1, 48.2, 36.1, 28.2, 15.4]
for (r, g, b), pub in zip(bars_rgb, published):
    derived = 0.925 * 100 * (y_row[0] * r + y_row[1] * g + y_row[2] * b) + 7.5
    if abs(derived - pub) > 0.06:
        failures.append(f"bar column: derived {derived:.2f} vs published {pub}")
    else:
        passes += 1
claim("docs/m2-report.md", r"76\.9 / 69\.0 / 56\.1 /\s*48\.2 /\s*36\.1 / 28\.2 / 15\.4", 1, "bar column")
claim("crates/ntsc-source-rgb/tests/bars.rs", r"\[76\.9, 69\.0, 56\.1, 48\.2, 36\.1, 28\.2, 15\.4\]", 1, "bar column in test")

# ---- Structure counts. ----
members = re.findall(r'"crates/([a-z0-9-]+)"', read("Cargo.toml"))
measured("crate count", len(members), 9)
claim("README.md", r"Nine crates", 1, "crate count in README")

# ---- The suite itself, measured by running it (slow rows). ----
def cargo_counts():
    env = dict(__import__("os").environ)
    env["PATH"] = str(Path.home() / ".rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin") + ":" + env["PATH"]
    r = subprocess.run(
        ["cargo", "test", "--workspace"], cwd=ROOT, capture_output=True, text=True, env=env
    )
    total = sum(int(m) for m in re.findall(r"test result: ok\. (\d+) passed", r.stdout))
    ok = r.returncode == 0
    m = subprocess.run(
        ["cargo", "test", "--workspace", "--no-fail-fast"],
        cwd=ROOT, capture_output=True, text=True, env={**env, "MUTATE": "1"},
    )
    reds = len(re.findall(r"^test .*FAILED", m.stdout, re.M))
    return ok, total, reds


if FAST:
    skips.append("cargo test totals (--fast)")
else:
    ok, total, reds = cargo_counts()
    measured("workspace green", ok, True)
    measured("test total", total, 56)
    measured("MUTATE reds", reds, 33)
claim("README.md", r"# 56 tests", 1, "test total in README")
claim("README.md", r"must go red: 33 tests", 1, "MUTATE count in README")
claim("docs/m4-report.md", r"56 tests green", 1, "test total in M4 report")
claim("docs/m4-report.md", r"reddens\s+33", 1, "MUTATE count in M4 report")

# ---- Verdict. ----
for s in skips:
    line = f"SKIP: {s}"
    if REQUIRE_ALL:
        failures.append(line)
    else:
        print(line)
for f in failures:
    print(f"FAIL: {f}")
print(f"{passes} claims verified, {len(failures)} failures, {len(skips)} skipped")
sys.exit(1 if failures else 0)
