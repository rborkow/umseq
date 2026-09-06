# P2C-SEED-SPLIT — Terra (tooling) — real-index request-path split, counters only

Workdir `/Users/rborkows/projects/uni-rnaseq-seed`, branch `p2c-seed-triage`. Read
`experiments/star-seed/RESULTS-CPU-20M.md` and `/Users/rborkows/projects/uni-rnaseq/docs/review-seed-replay.md`
(commit `69c3bca`) first. Then `replay/capture_hooks.hpp`, `replay/capture_impl.hpp`,
`replay/run_capture.py`, and `evidence/read-capture-run1/execution.json` → `cases/*/coverage`.

## Question this card answers

Seeding is 39.03% of STAR's CPU (CPU-ACCOUNTING, 3 repeats). That bracket contains three
paths in `ReadAlign_maxMappableLength2strands.cpp:74–96`: the short-prefix shortcut (no
comparison), the **direct unique-hit extension** (one `compareSeqToGenome`, lines 80–89), and
the **inner `maxMappableLength` search** (lines 90–96), which is the only path the replay
covers. **What fraction of real-index seed work — by request count and by compared bytes —
goes through the inner search?** Until this is measured, we do not know whether replaying the
inner boundary addresses 90% or 20% of the 39%.

## What already exists

The capture build's hooks already count, per read: `inners`, `direct_calls[4]`,
`direct_bytes[4]`, `short_direct_inner`, `absent_reductions`, `Nmarked`, `bad_upper`, and the
per-direction compared-byte totals for the inner path. On the tiny fixtures every `direct_*`
is zero (tiny genomes, `genomeSAindexNbases` small) — which is exactly why the tiny results
cannot answer this question.

## Deliverable

1. A **counters-only mode** for the capture build: same hooks, same counters, **no SSIR
   records written** (no per-call trace), summary JSON at exit. Compile-time or runtime flag —
   your choice, but it must be impossible to confuse a counters-only run with a trace
   capture in the evidence (distinct status string, distinct output filename). Counters must
   be per-thread accumulated and summed at exit, no locks in the hot path (the whole point
   is to run at 20 threads on 20M pairs without perturbing the profile).
2. Add to the counters (if not already present): inner requests' `N` histogram (or at least
   sum/max), inner-request iteration count (`while (i1+1<i2)` loop trips) sum/max, and
   `findMultRange` compared bytes. These are the GPU divergence inputs for the probe card.
3. Runner script (same discipline as `run_cpu20m_v2.sh`: lock, provenance, exact counts,
   evidence dir `evidence/seed-split-run1/`): **stock warmup, then one counters-only run on
   `ERR188140_20M` × full index × 20 threads**, plus one run on the 5M subset. GNU time
   five-column, `Log.final.out` read counts, and stock-parity check of the capture binary's
   alignments against the retained stock outputs (as READ-CAPTURE did) so the counters-only
   binary is shown not to change mapping.
4. **Report `RESULTS-SEED-SPLIT.md`**: for each run — outer requests; short / direct / inner
   counts and fractions; compared bytes on direct vs inner (and inner's split between the
   endpoint comparisons, binary-search loop, and `findMultRange`); per-direction breakdown;
   inner `N` and loop-trip distributions; and the *CPU-s* of the counters-only run vs stock
   (to bound hook overhead). State the boundary decision explicitly using this rule:
   - inner ≥ 70% of compared bytes → **inner-only boundary is sufficient**; REPLAY-SCALE and
     the GPU probe use it as designed.
   - inner < 70% → **boundary must widen to inner + direct extension**. Direct extension is a
     pure function of `(s, S, N, Lind, iSA1, dirR)` over immutable index state, so it is
     replayable with the same contract; record what the trace format needs to add.

## Rules

Standard library Python; TDD with real RED/GREEN recorded; tests for the counters-only
mode's exit summary parsing and for refusing to treat a counters-only run as a trace. No
edits outside `replay/` capture files, the new runner, tests, and the new RESULTS file. No
commits. Orchestrator runs the Spark slot (lock protocol unchanged; one full-index STAR at a
time). Never invent a number; if hook overhead exceeds 5% CPU-s vs stock, report it and do
not "correct" for it.

Finish with: exact changed paths, test counts, the Spark commands to run, and the decision
rule restated so the orchestrator can apply it mechanically to the JSON.
