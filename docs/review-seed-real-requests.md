# Review 3: REAL-REQUESTS result and the integration decision — 2026-09-06 night

Reviewed `bench/PHASE2C-real-requests.md`, `bench/evidence/seed-real-requests-host2/`
(distributions, summary), `docs/STAR-INTEGRATE-DESIGN.md` (230 lines, read in full), and the
boundary-correction audit. Commit of record for the result: seed lane's `real-requests-host2`.

## The result, accepted

**6.60× over 20 cores on 999,914 real STAR inner requests, with every one of them matching
STAR's own recorded `(L_out, lo, hi, Nrep)`** — on both the CPU replay and the GPU. That last
clause is the one that matters: this is no longer "two implementations of the same algorithm
agree"; it is the GPU reproducing STAR's actual answers on STAR's actual queries over the
real 30 GB index. The oracle rung that REPLAY-SCALE was going to take a week to reach was
reached in one night by capturing at the boundary and comparing.

Representativeness held up: real requests do ~10 dependent gathers vs 37 synthetic (the
prefix index does its job), and the ratio fell 10.4× → 6.6×, not toward 2×. The 4K-page
control is again catastrophic (0.07×), overlap contention 4–5%. The validator bug was
diagnosed precisely — 0 requests with `N > S+1`; 1,317 with the fully-known-prefix
`L_in = N = S+1` pointer case — and fixed in the one place, without filtering.

The three-day arc: 39% of STAR is seeding (CPU accounting) → 99.8% of that is the inner
search (SPLIT) → the inner search runs 6.6× faster on the GPU in place, exactly (REAL-REQUESTS).
The hypothesis is true at the boundary. Odds it becomes a pipeline number: **~55%** (up from
~17% at the roadmap), and the remaining risk is entirely the scheduler.

## What it is worth (so the next card has a target)

Cost-model units (`bench/COST-CURVE.md`): STAR is 64.6 CPU-min/sample; seeding 39.03%;
inner ≈ 90% of that bracket (the bracket also holds `qualitySplit`, prefix lookup, and
`storeAligns`) → **~22.7 CPU-min/sample of inner search.** Pipeline after umbam: 148.5.

| scenario | saved | CPU-min/sample | capacity |
|---|---|---|---|
| all inner on GPU at 6.6× (ceiling) | 19.3 | 129 | **1.15×** |
| initial-start speculation only, 60% of inner *work* covered | 11.6 | 137 | 1.08× |
| same, with 25% wasted speculation + lookup overhead (net 4×) | 10.2 | 138 | 1.07× |
| net 4×, 85% work coverage (initial + grid positions) | 14.5 | 134 | 1.11× |

So the honest prize is **7–15% pipeline capacity**, set almost entirely by *work coverage of
the batching mechanism*, not by kernel speed. Going from 6.6× to 10× on the kernel is worth
~1 point; going from 60% to 85% coverage is worth ~4. That inverts where effort should go:
**the scheduler is the whole game now, and the kernel is done.**

Two things to keep straight in the memo: (1) this is CPU-minutes, i.e. throughput on a
saturated box — single-sample STAR wall moves from ~51 s to ~36 s at 100% coverage, which is
also nice but is not the cost-curve number; (2) 1.15× is on top of the 1.5× the CPU rewrite
already delivered, and it is the first number attributable specifically to unified memory
(the 4K control is the proof: this kernel does not exist without huge-page host memory read
through ATS).

## The design document

`STAR-INTEGRATE-DESIGN.md` is very good — the dependency trace (§"Exact dependency trace") is
the precise map of where `Lmapped += L` creates the chain, the memory table (§"Index copies")
gets the HugeTLB accounting right, and the three mechanisms are honestly costed. Three
points where I'd push:

1. **It is right that (a) initial-start speculation is the first experiment**, and right
   that its coverage is unknown. But the capture already taken can answer the coverage
   question *without a new run*: 999,914 requests with `L_in` recorded. `L_in = 14`
   (prefix length) with `Lmapped = 0` is an initial start; `L_in = 0` (83,823 requests) is
   an absent-prefix case. What's missing is the per-request `Lmapped`/chain position. Add it
   to the attribution sidecar (it's `Lmapped` at `mapOneRead:68`, one integer) and re-run
   the *counters-only* build — a 6-minute STAR run under lock, no SSIR, no probe. Then
   "fraction of inner gathers that occur at `Lmapped=0`" is measured, not estimated. Do
   this before writing scheduler code.

2. **(b) continuations should be dropped from consideration**, not deferred. The document's
   own numbers kill it: 64k suspended `ReadAlign` states at 64 KB/read = 3.8 GiB of PC arrays
   alone, plus pinned chunk buffers, plus RNG-order parity. Speculation (a) + a measured grid
   (c) covers the same work at a fraction of the state. Say so and stop carrying it.

3. **The double-index question should be settled by measurement, not design.** Accept
   duplication for the experiment (57 GiB general is fine on an otherwise idle box); the
   `Genome_genomeLoad` ownership patch is a shipping concern and the design correctly says
   "does not justify itself before a positive result."

## Recommendation: authorize a bounded integration experiment

Two cards, sequential, both in the private STAR copy behind a compile flag:

**P2C-CHAIN-POSITION (Terra, tooling, one counters-only Spark run).** Add `Lmapped`,
`istart`, `iDir`, `ip`, and the reverse-suppression flag to the per-request attribution of the
counters-only build. Report the joint distribution: gathers and compared bytes by
`(Lmapped == 0, istart, iDir)`. Output: **measured initial-start work coverage** and the
adaptive-tail share. If initial-start coverage < 50% of inner gathers, add grid positions
(the design's (c), stock prefix narrowing, offsets 0/20/40/… per split edge) to the same
counters build as *hypothetical* candidates and count what they would have hit. One number
decides the mechanism: work coverage of {initial} vs {initial + grid20}.

**P2C-STAR-INTEGRATE-1 (Astra design freeze → Terra implementation; Astra reviews the
consumption-order code once).** Exactly the design's §"Implementation on-ramp": duplicate
G/SA `umem` context (accept the memory), synchronous batch bridge via the existing
`umgpu_seed_probe` transport hardened to a C ABI, CPU prefix preparation, bounded
initial-start window per worker chunk (64k target, aggregated across workers by a
coordinator), exact-tuple lookup at the original inner call site, stock CPU function as
fallback and oracle, `storeAligns` untouched at its original site. **Gates, in order:**
(i) SAM records, order, all tags, SJ.out.tab, non-timing Log.final.out identical to stock on
the 20M input — this is the same parity checker SPLIT already runs; (ii) hit/miss/waste
counts reported; (iii) STAR CPU-s (`%U+%S`) vs stock, three repeats — **target ≥ 12%
reduction** (that's ~60% coverage at net 4×; anything above 8% is a memo-grade positive,
below 5% with parity intact is an honest "the scheduler ate it"). Wall and RSS reported but
not gated. No allocator ownership patch, no continuations, no grid in v1 unless
CHAIN-POSITION says initial-start coverage is < 50%.

Then **P2C-PIPELINE-RUN**: nf-core on the 6 Tier 2A samples with the integrated STAR and
umbam — the number for the chart.

Budget: CHAIN-POSITION is a day; INTEGRATE-1 is 2–4 days at the lane's rigor with one review
per card; PIPELINE-RUN is 4 h of Spark. End-to-end odds of a ≥ 8% pipeline result: ~55%.

## Housekeeping

- `bench/PHASE2C-real-requests.md` and the design doc should be committed in main now; the
  main tree has uncommitted `umgpu` glue from the probe work (`git status`) that belongs with
  them.
- The held cards (PE-HOST-FAULTS, OWNED-SESSION, FULL-INDEX-CONTRACT) stay held. REPLAY-SCALE
  is now mostly moot: its payload (real capture + oracle) exists; its remaining rigor (PE
  edge matrix, capacity profiles) is shipping work, not decision work. Mark it superseded.
- Update `bench/COST-CURVE.md` with a fourth line — "umbam + GPU seed search (projected,
  1.07–1.15×)" — dashed, labelled projected, so the chart shows what INTEGRATE-1 is for.
