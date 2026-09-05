# Phase 1 — BAM-chain standalone profile, 20M-pair Tier 1 BAM

**Date:** 2026-09-05. **Input:** identical `NA12716_20M.Aligned.out.bam` (3.98 GB unsorted,
53.9M alignments incl. 15.0M secondary, 74 bp PE) produced by STAR on the Spark; copied to the
Mac. Native tools, no containers: samtools 1.24 (Spark) / 1.22.1 (Mac), picard 3.5.0,
featureCounts 2.1.1, bedtools 2.31.1. Script: `scripts/bamchain_bench.sh`.
Raw TSVs: `bench/bamchain-spark-20M.tsv`, `bench/bamchain-mac-20M.tsv`.

## Wall time per stage

| stage | threads | Spark (20c) | Mac M4 Pro (14c) | Mac/Spark | note |
|---|---|---|---|---|---|
| samtools sort | 16 / 12 | 22.6 s | 62.2 s | 2.75× | Mac peak RSS 12.4 GB, `-m 1G` × 12 thr; heavy sys time (42 s) |
| samtools sort | 1 | 190.4 s | 184.2 s | 0.97× | single-core equal |
| samtools index | 16 / 12 | 2.8 s | 2.1 s | — | |
| **samtools view -c (decode only)** | 16 / 12 | **2.1 s** | **1.9 s** | — | inflate + parse of 2.7 GB BGZF |
| samtools view -c (decode only) | 1 | 8.7 s | 8.7 s | 1.0× | |
| samtools stats | 1 | 24.3 s | 21.3 s | 0.88× | |
| samtools flagstat | 16 / 12 | 1.7 s | 2.0 s | — | |
| **picard MarkDuplicates** | 1 | **279 s** | **292 s** | 1.05× | single-threaded Java; 7 GB RSS |
| samtools markdup (nsort→fixmate→sort→markdup) | 16 | 71.1 s | (errored, see below) | — | 808 s user time: parallel but wasteful |
| featureCounts | 16 / 12 | 14.9 s | 29.1 s | 1.95× | Mac: 64 s sys time |
| bedtools genomecov | 1 | 73.4 s | 81.5 s | 1.11× | |

## What this says

1. **Decompression is not the bottleneck.** Multi-threaded BGZF inflate of a 2.7 GB BAM takes
   ~2 s on both boxes — ~1.3 GB/s compressed, ~4 GB/s decompressed. The old PoC's "CPU
   decompression starves the GPU" story doesn't reproduce here with modern htslib. **This
   weakens the Phase 1.5 thesis** (GPU inflate as the unlock). It's still cheap to measure
   (nvCOMP is a library call), but it should not be the gate.

2. **The wall is single-threaded tools, dominated by Picard MarkDuplicates.** 279 s at 1 core
   on a box that can sort the same file in 23 s. Per nf-core Tier 1 trace, Picard + Qualimap +
   dupRadar + 5× RSeQC ≈ 22 min of the 28 min per-sample time, all single-threaded. That's
   parallelism starvation, not memory bandwidth — and it's the same on both boxes because
   single-core speed is the same (Cortex-X925 ≈ M4 P-core).

3. **`samtools markdup` already gives 4× over Picard** on the Spark (71 s vs 279 s) with
   16 threads, and the nf-core pipeline supports `--skip_markduplicates` + samtools alternatives.
   That's a **zero-engineering** win worth putting on the cost curve immediately: it changes
   per-sample time before any GPU work exists.

4. **The GPU opportunity is the whole chain as one pass, not any single tool.** sort + markdup
   + stats + counts + coverage each re-read and re-decode the BAM (5–6 full passes,
   ~15 GB of decompressed traffic). An in-place pipeline that decodes once into unified memory
   and runs sort → markdup → count → coverage as GPU kernels over the same resident records
   is where bandwidth pays. Target for that chain on the Spark: ~5 s (bandwidth-bound at
   ~200 GB/s over ~15 GB of record traffic plus radix sort) vs. ~450 s today for the same
   outputs sequentially. That's the Phase 2a hypothesis, now with a concrete baseline.

5. **Single-thread performance is identical across boxes; multi-thread favors the Spark**
   by core count (20 vs 14) and by the Mac's higher `sys` overhead on I/O-heavy stages
   (sort: 42 s sys on Mac vs 8.5 s on Spark; featureCounts 64 s vs 3 s). Worth a look at Mac
   I/O settings (APFS + page cache) before attributing it to hardware.

## Amdahl, first cut (per-sample, 20M pairs, Spark, nf-core star_salmon, index amortized)

| component | today | after samtools markdup swap | after GPU BAM chain (target) |
|---|---|---|---|
| STAR | 5.8 min | 5.8 | 5.8 |
| salmon quant | 3.9 | 3.9 | 3.9 |
| markdup | 4.8 | 1.2 | ~0.1 |
| sort/index/stats/flagstat/featureCounts/genomecov | ~3 | ~3 | ~0.1 |
| Qualimap / dupRadar / RSeQC (Java/Python, 1 core) | ~14 | ~14 | ~14 (unchanged unless reimplemented) |
| trim/FastQC/other | ~4 | ~4 | ~4 |
| **serial per-sample** | **~36** | **~32** | **~28** |

The uncomfortable row is Qualimap/dupRadar/RSeQC: 14 min of R/Python/Java QC that isn't a
kernel-shaped problem. Two options for the plan: (a) they overlap across samples and become
free at ≥4 samples in flight, so they don't bound throughput, only latency; (b) most of what
they compute (coverage profiles, junction stats, duplication rate, gene-body coverage) is
derivable from the same single GPU pass. Recommend (a) for the cost curve now, (b) as a
Phase 3 stretch.

**Amdahl gate verdict (BAM chain):** GPU-addressable share of *serial* time is ~8 min of 36
(22%) → capped at 1.3× per-sample latency. But for *throughput* with QC overlapped, the chain
is ~8 of ~14 min of CPU-heavy work per sample (57%) → up to 2.3×. Passes on throughput, fails
on latency. That's the right framing for a fixed asset. **Proceed to 2a; demote 1.5 to a
measurement, not a gate.**

## Mac-specific findings

- `samtools markdup` pipeline errored on the Mac (samtools 1.22.1 vs 1.24; investigate — likely
  `fixmate -m` piping). Not blocking; re-run after upgrade.
- Mac nf-core end-to-end fails at STAR (exit 137 at 20 GB container cap): sparse-3 index +
  `--twopassMode Basic` + GTF junction insertion exceeds what a 24 GB box can give a VM.
  Native (non-container) STAR would likely fit. **The M4 Pro is a kernel-dev box; the Ultra is
  the pipeline box.** Noted in plan.

## Full-depth (78M pairs) Spark pipeline

Completed. Trace in `bench/trace-spark-tier1-full.txt` (see summary below when extracted).
