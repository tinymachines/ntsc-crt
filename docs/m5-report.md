# M5 report: documentation and self-counts

Run stamp: 2026-09-01, sixth commit of this repository, the run of
`python3 tools/check-self-counts.py` recorded below. The ladder's last
milestone: nothing new is built here; what exists is counted, and the
counting is itself checked.

## The scanner

`tools/check-self-counts.py`: every load-bearing number in the docs
matched by a pattern that must hit its stated count, compared with a
fresh measurement. Measured sources: the exact rationals recomputed
with `fractions`; the transcription gate re-run; the data files
re-parsed; the frozen comparison constants read from the crate that
owns them; the workspace membership counted; the cargo suite and its
MUTATE=1 run executed and their totals parsed (`--fast` skips those two
rows; `REQUIRE_ALL=1` makes a skip a failure). Deliberately NOT
covered, stated in its header: throughput and diff-envelope figures,
which are stamped best-of-N measurements that re-running here would
replace with noisier ones.

**The scanner paid for itself on its first run**: the published bar
column in the M2 test and report carried 68.9 IRE for the yellow bar
where the derivation gives 68.97; the correct one-decimal value is
69.0, and M2's justified 1-IRE waveform tolerance had absorbed the
typo. Fixed in both places; the scanner now holds the column to the
derivation at 0.06 IRE. It also found NOTICE.md claiming "pinned by
hash" without quoting the hash; the hash is now quoted.

Final run: **56 claims verified, 0 failures, 0 skipped** (the slow
rows measured 56 tests green and 33 MUTATE reds, matching README and
the M4 report).

## Licences per data source

`NOTICE.md` is the per-source registry, one entry per data file and
vendored oracle with its terms and pinned hash; each data file also
carries its own provenance header. Recorded decision: a single
annotated registry satisfies the spec's "licence file per data source"
better than a directory of stubs would, because the registry is the one
place a reviewer already looks.

## Known divergences

Consolidated into `docs/divergences.md`: the blargg table with
magnitudes, the ST 170M simplifications, the nesdev differential-phase
non-model, and the internal deliberate choices, each pointing at the
report or module doc where it was measured or recorded.

## The spec, handed back

`docs/spec-v0_3-draft.md`: the three corrections that failed
confirmation (the full-frame rate, Rung D, the bandwidth clause), the
additions the implementation needed, v0.2's open questions all
resolved, and three new questions for the director, the first being
the hardware the real-recording gate waits on.

## The ladder, closed

M0 through M5 in six commits, one day: the grid contract with every
residue proven; the NES encoder held to the page's own waveform; Rung A
against blargg with every disagreement attributed; the RGB encoder
against the primary standard's own clauses; four separation rungs, one
of them refused by name and one corrected against the spec; the CRT
model honest about being one; the capture source earning its phase; and
this scan. Open at the end, all recorded: the real capture recording
(hardware), the arcade shell (the companion project), real-time
throughput (the named decimation and SIMD levers), and the v0.3
ratification.
