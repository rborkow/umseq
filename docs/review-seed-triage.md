# Review: P2C-SEED-TRIAGE (the STAR seed-search lane) — 2026-09-06

Reviewed: `uni-rnaseq-seed/experiments/star-seed/{RESULTS.md,RESULTS-20M.md,CONTRACT.md,
experiment.py}`, `evidence/spark-run20m/ablation/raw.tsv`, the plan
`.hermes/plans/2026-09-06_092218-star-seed-search.md`, and STAR's `ReadAlign_mapOneRead.cpp`
on the Spark. One additional measurement taken (below), under the lane's lock protocol,
using the lane's own binaries.

## Verdict

**The lane's evidence is sound and its discipline is excellent; its decision rule is
measuring the wrong quantity.** The wall-clock ablation says seeding is ~10% of STAR and
"below gate". The CPU-time accounting says seeding is **~39% of STAR's CPU-seconds**, which
on a throughput-bound box is the number that moves the cost curve. Re-read with the right
metric, seed search clears the lane's own 10% gate by a wide margin and is the first stage
where a GPU result would put a number on the unified-memory thesis.

## Why the ablation under-reads (the missing measurement)

The runner records `/usr/bin/time -f "%e %M %x"` — wall, RSS, exit — no CPU time. I reran
the three private binaries once each on the same 20M input (`Sparky:/tmp/seed-cpu-*`):

| variant | wall | user CPU-s | sys | cores busy | Δ user vs previous |
|---|---|---|---|---|---|
| `OFF_BEFORE_SEEDING` | 30.8 s | 41.7 | 11.8 | 1.4 | — (≈ `zcat` of both files: 40.6 CPU-s) |
| `OFF_AFTER_SEEDING` | 39.0 s | 307.8 | 15.2 | 7.9 | **+266 CPU-s = seeding** |
| stock | 49.9 s | 674.8 | 12.9 | 13.5 | +367 CPU-s = stitch/select/output |

Wall numbers match the lane's (33/37/51 s medians) — this is the same experiment, plus one
column.

The `before` variant is an **input-bound floor**: 1.4 cores busy, wall set by single-reader
gzip decompression (30 s to inflate 20M pairs). Seeding's 266 CPU-seconds then land on 20
otherwise-idle threads and largely *overlap the input wait*, so wall grows only 8 s. The
"after − before = 10% of wall" number is a measurement of how well seeding hides behind
`zcat`, not of how much work seeding is. The lane's own RESULTS-20M.md flagged exactly this
("before-seeding variant still spends 24–25 s in this phase … investigate read supply") — it
was one column short of the answer.

CPU split of stock STAR at 20M pairs: input 6%, **seeding 39%**, stitching + multimapper
selection + output 54%. This agrees with the lane's `perf` (maxMappableLength2strands 30–42%
inclusive, compareSeqToGenome 25–34% self) — the profile was right all along.

## What the corrected number means

The cost model (`bench/COST-CURVE.md`) is CPU-minute-driven: samples/day = cores × util ÷
CPU-min/sample. STAR is 65 CPU-min of the 148 per sample after umbam. 39% of that is ~25
CPU-min of seed search per sample.

- **If seeding moved to the (currently idle) GPU entirely:** 148 → ~123 CPU-min/sample,
  **1.2× throughput** — the first GPU line on the cost curve that isn't a rounding error.
- **At the lane's hypothetical "2× seed acceleration" on CPU:** 148 → 135, 1.09× — the
  lane's gate was framed as CPU acceleration; a GPU offload is a different, better question.
- **Single-process wall (what the lane measured):** stock is CPU-bound at 13.5 cores;
  removing seeding's 266 CPU-s gives ~409/13.5 ≈ 30 s → ~1.6× on STAR wall; at 2× seed
  speed ~1.25×. Both above the 10% gate.

## Is a GPU suffix-array search plausible on GB10? (odds, not a result)

The workload: per read, a handful of dependent binary-search chains over the 25 GB packed
SA + 3.3 GB genome, each chain ~35 dependent random 8-byte gathers with a variable-length
`compareSeqToGenome` at each step. That is memory-latency-bound with no reuse — precisely
the shape Phase 0 measured: T6 showed GPU random-gather latency ≈ CPU (137 ns) but ~2.6 G
dependent lookups/s sustained with THP, vs ~20 cores × ~7 M/s. Back-of-envelope: 20M pairs ×
~10 searches × ~35 gathers ≈ 14 G gathers ≈ 5–6 s GPU vs 266 CPU-s / 20 = 13 s CPU. So
**2–3× on the seed stage is plausible, not promised**; divergence across reads (variable
`compareSeqToGenome` lengths, unique-hit early exits) is the thing that erodes it, and the
lane's risk list (§5, items 1–3) is the right list. This is a 30 GB resident index read in
place with zero staging — the one stage in the pipeline where unified memory is *necessary*
rather than convenient (a PCIe box cannot stage 28 GB per batch).

Odds of a ≥2× seed-stage result on GB10: **~45%**. Odds that it moves the cost curve by ≥15%
if it lands: ~80% (the CPU-minute arithmetic above). Combined ≈ 35% for "GPU thesis gets a
cost number". Higher than anything else on the board.

## Specific comments on the lane

1. **Keep everything.** CONTRACT.md's ten items and the test matrix are the right shape;
   "unknown is a review blocker, not permission to invent behavior" is the right rule; the
   pinned hashes, private source copies, lock protocol, and refusal to relax `summarize` are
   exactly how this should be run.
2. **Fix the runner:** `%e %U %S %M %x`, and record `Log.final.out`'s own started/finished
   mapping timestamps (already done post hoc). `summarize()` should compute the CPU-time
   fraction alongside the wall fraction and report both; the gate should be stated on CPU-s.
3. **Drop the "larger input" line of inquiry.** 5M → 20M moved the wall estimate 1.03 → 1.05
   because the input floor scales with input too. It will never converge on the CPU share.
4. **The gzip floor is its own finding:** STAR's single reader thread inflating gz input caps
   the `before` variant at 30 s for 20M pairs → ~2 min for 78M. In nf-core STAR ran at 5.2×
   efficiency on 12 threads; part of that gap is this reader. Not our problem to fix, but it
   bounds any STAR wall claim and should be in the report.
5. **Decision:** promote from *narrow* to **go — P2C-SEED-REPLAY** (CPU oracle + trace
   replay first, per the lane's own sequence). The kernel card stays conditional on replay
   correctness. Budget: this is Astra-grade semantic work (the `compareSeqToGenome` direction
   cases, packed SA, `GstrandBit`, sentinels); Terra for the tooling.

## What I did not verify

- Only one repeat of the CPU-time measurement (the lane's three-repeat wall spread was
  ±3%; CPU-s should be tighter, but it is n=1).
- I did not read `CONTRACT.md` line-by-line against the STAR source; I checked the
  `OFF_*` switch placement in `ReadAlign_mapOneRead.cpp` and it brackets `qualitySplit` +
  the seed loop + `maxMappableLength2strands`/`storeAligns`, as the lane says.
- `perf` self/inclusive were not re-derived.
