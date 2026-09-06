# P2C roadmap — the STAR unified-memory/GPU hypothesis, end to end

Written 2026-09-06 for the seed lane (worktree `uni-rnaseq-seed`, branch `p2c-seed-triage`).
Supersedes the sequencing in `.hermes/plans/2026-09-06_092218-star-seed-search.md` §4 and
the `P2C-REPLAY-SCALE` backlog entry; keeps every gate in that plan, reorders them so the
cheap disqualifying measurements come first.

## The hypothesis, stated so it can be falsified

> On a unified-memory workstation, STAR's suffix-array seed search — 39% of STAR's CPU,
> ~25 CPU-min per 78M-pair sample, a latency-bound random walk over a 30 GB index — runs
> ≥2× faster on the GPU than on 20 Arm cores, reading the index in place with no staging,
> and that moves pipeline capacity by ≥15% (`bench/COST-CURVE.md`: 148 → ≤126 CPU-min/sample).

Three ways it fails, each with its own card: the replayed boundary is a minority of the 39%
(**SPLIT**); the GPU cannot beat the CPU on the access pattern (**PROBE**); the win doesn't
survive STAR's adaptive scheduler and index state (**INTEGRATE**).

## Ladder

```
              ┌── P2C-SEED-SPLIT (Terra, 1 Spark slot) ──────────────┐
  now ────────┤   inner vs direct vs short: counts + compared bytes    │
              └── P2C-SEED-GPU-PROBE (Terra, 1–2 Spark slots) ───────┘
                    CPU-20 vs GPU on real index, synthetic requests
                          │
        <1.5× ────────────┼──────────── ≥2×
          │               │                │
   P2C-NEGATIVE     1.5–2×: user      P2C-REPLAY-SCALE (existing plan §4.1, full rigor)
   (publish,        decides            PE + missing branches, full-index capture, real-data
    park lane)                         replay, CPU replay profile
                                           │
                                       P2C-SEED-CUDA (existing plan §4.2)
                                       real kernel on captured traces; ≥2× over all-core
                                       CPU replay incl. batching/materialization
                                           │
                                       P2C-STAR-INTEGRATE (below — new detail)
                                           │
                                       P2C-PIPELINE-RUN (below — new)
```

SPLIT and PROBE run in parallel (different files, different Spark slots, 2 workers).
SPLIT's answer changes PROBE's *boundary* (inner-only vs inner+direct) but not its
method; if SPLIT says "<70% inner", PROBE adds the direct-extension request type before its
final measurement.

## Cards

### P2C-SEED-SPLIT — `.hermes/cards/P2C-SEED-SPLIT.md` (written)
### P2C-SEED-GPU-PROBE — `.hermes/cards/P2C-SEED-GPU-PROBE.md` (written)

### P2C-NEGATIVE (orchestrator, no worker) — if PROBE < 1.5×
`experiments/star-seed/RESULTS-NEGATIVE.md`: the divergence data, achieved GB/s vs the T6
ceiling, per-warp imbalance, the 4K-page control, and the one-paragraph explanation of *why*
a latency-bound dependent walk with data-dependent compare lengths doesn't map to SIMT on
48 SMs. Fold into the memo as the honest bound on the UM thesis: "the BAM stages are the
GPU's; alignment is the CPU's." Park the lane; keep the replay tooling (it's a STAR test
harness with independent value).

### P2C-REPLAY-SCALE — existing plan §4.1, unchanged content, **now gated behind PROBE ≥2×**
One addition: the CPU replay profile at the end must report CPU-s for the replayed boundary
on the real 20M capture, so SPLIT's byte fractions get a CPU-time cross-check.

### P2C-SEED-CUDA — existing plan §4.2, unchanged, with PROBE's kernel as the starting point
The probe kernel becomes the v0; the card's job is correctness on real captured traces
(the lane's oracle), the batching/materialization overhead, and the ≥2× gate *including*
those overheads. Astra reviews the packed-SA/sentinel/direction handling once.

### P2C-STAR-INTEGRATE — new, Astra design + Terra implementation; separate authorization
The plan's §4.3 names the risk (adaptive scheduling, index state) but not the mechanism.
Design first, one document, answering:
1. **Where the batch boundary goes.** `mapOneRead` issues inner requests one at a time,
   each dependent on the last (`Lmapped += L`). Batching across reads requires STAR's
   per-thread `ReadAlign` to *yield* mid-read. Options: (a) coroutine-style — each thread
   collects requests for a chunk of reads up to the first dependency, submits, resumes;
   (b) speculative — issue all `Nstart` starting positions per read up front (they're
   independent of each other; only the `Lmapped` advance depends), accept some wasted
   searches; (c) two-phase — GPU pre-computes `maxMappableLength` for every `(read, S, dirR)`
   on a fixed grid, CPU scheduler consumes from the table and falls back to CPU search for
   off-grid requests. Astra picks with evidence from SPLIT's request-shape data.
2. **Index state.** v1: `--twopassMode None`, no `--sjdbInsertSave`, sparse=1. The GPU sees
   the on-disk arrays through `umem`; STAR's `genomeLoad` copy is *also* in memory → 60 GB.
   Either STAR loads via `umem` (a `Genome_genomeLoad.cpp` patch in the private copy — the
   lane already has a hook there) or the pipeline accepts 2× index footprint on a 121 GB box.
3. **The C ABI**: `int umseed_search_batch(const Request*, Result*, size_t n, void* ctx)`;
   fallback counts for rejected requests; per-batch timing exported to `Log.final.out`.
4. **Equivalence policy**: SAM records/order/tags identical to stock on the 20M input, with
   the plan's documented exceptions (header `@PG`, timing lines). Not "mapping rate agrees."
Then implement in the private STAR copy behind a compile flag; gate on equivalence + STAR
CPU-s reduction ≥ 25% (the SPLIT-adjusted share of the 39%).

### P2C-PIPELINE-RUN — new, orchestrator; the number for the memo
The only result that goes on the cost curve. `runs/tier2c/`: nf-core/rnaseq on the same 6
Tier 2A samples with (a) `--star_bin` → the integrated STAR, (b) umbam replacing the BAM
chain (already possible via `--skip_*` + a post-step). Trace → CPU-min/sample → the model.
Compare to Tier 2A's 223 and COST-CURVE's projected 148/126. Three numbers, one chart.

## Budget and odds

| card | worker | Spark slots | odds it's reached | odds it passes given reached |
|---|---|---|---|---|
| SPLIT | Terra | 1 | — | n/a (a measurement) |
| PROBE | Terra + orchestrator | 2 | — | ~40% ≥2×, ~25% 1.5–2×, ~35% <1.5× |
| REPLAY-SCALE | Terra, Luna review | 2–3 | 40–65% | ~85% |
| SEED-CUDA | Terra, Astra review | 2 | ~40% | ~70% (batching overhead is the killer) |
| STAR-INTEGRATE | Astra + Terra | 2–3 | ~28% | ~60% (the scheduler) |
| PIPELINE-RUN | orchestrator | 1 (4 h) | ~17% | — |

End-to-end: **~17% that the UM/GPU hypothesis gets a positive cost-curve number**, ~35%
that it gets a clean negative from PROBE within two days, the rest is intermediate results
that still bound the thesis. Every branch produces a memo-grade statement.

## Coordination
Two workers total across both sessions. The main session is idle on workers (BAM/QC queue
exhausted); the seed lane may use both slots for SPLIT + PROBE. Spark slots via the shared
lock as before; PROBE's index load is ~30 GB + STAR's own if SPLIT runs concurrently — run
SPLIT's STAR first, PROBE's loads after, or accept ~65 GB combined (fits in 89 GB general;
`earlyoom` at ~110 GB RSS is per-process).
