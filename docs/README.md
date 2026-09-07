# Documentation index

Read in this order if you're new.

## The argument
1. `../bench/COST-CURVE.md` — the cost model, its calibration, and the four figures.
2. `../.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md` — the plan (rev 3) and why the
   CPU control arm came before any kernel.
3. `review-seed-real-requests.md` — where the unified-memory thesis stands after the seed-search result,
   and what the prize is worth in cost-model units.

## Phase 0/1 — memory model and baselines
- `../bench/RESULTS-2026-09-04.md`, `../bench/RESULTS-T6-summary.md` — the PCIe hypothesis retested
  on GB10: THP host memory read in place by the GPU at ~145 GB/s; `cudaHostRegister` 15× slower;
  `cudaMalloc` a 300× cliff; dependent-gather latency ≈ CPU.
- `../bench/PHASE1-bamchain-20M.md`, `../bench/PHASE1-depth-scaling.md` — nf-core stage costs.
- `../bench/PHASE2-tier2a-throughput.md` — six full-depth samples through nf-core on the Spark:
  the measured samples/day that calibrates the cost model.
- `env-spark.md`, `env-mac.md` — toolchains.

## Phase 2A — the CPU control arm (`umbam`)
- `design-umem.md`, `review-umem-astra.md`, `review-umem-impl-astra.md` — the shared-memory
  ownership model and its two adversarial reviews.
- `../bench/PHASE2A-umbam-20M.md` — 408 → 55 s on a 20M BAM, byte-identical at every step.
- `../bench/PHASE2A-tier1-validation.md` — full-depth (76M-record) validation runs 1–6 against
  nf-core outputs, with every residual named.
- `../crates/umbam/COMPAT.md` — documented divergences (bedtools D, Picard multi-library SE,
  Qualimap ambiguous split, dupRadar multimapper ±1).
- `tool-src/` — verbatim RSeQC/dupRadar source used as the specification.

## Phase 2B — GPU on the resident table
- `design-phase2b.md` — the pattern ("output order is the contract"), the markdup and
  dup-histogram results, and the nvCOMP negative result.

## Phase 2C — STAR seed search
- `review-seed-triage.md` — why wall-clock ablation under-read the seed share (39% of STAR CPU).
- `review-seed-replay.md` — why to measure before hardening; the SPLIT and PROBE cards.
- `../bench/PHASE2C-seed-probe.md` — 10× over 20 cores on synthetic requests; 4K-page control.
- `../bench/PHASE2C-real-requests.md` — 6.6× on 1M real STAR requests, all results identical.
- `../bench/PHASE2C-chain-position.md` — 76% of inner work is at initial starts (batchable).
- `STAR-INTEGRATE-DESIGN.md` — the batching mechanism, memory accounting, and C ABI.

## How work was done
- `../KANBAN.md` — the board.
- `../.hermes/cards/` — one brief per worker dispatch; the `.log` files (untracked) are the
  worker transcripts. Cards say what was asked; the commit says what landed; the bench doc
  says what was measured. Where they disagree, the bench doc wins.
