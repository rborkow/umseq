# Phase 1 — depth scaling, Spark, nf-core star_salmon, Tier 1 NA12716

20M pairs vs full 78.6M pairs (3.93×). Salmon index prebuilt for both. Traces:
`bench/trace-spark-tier1-20M.txt`, `bench/trace-spark-tier1-full.txt`.

| | 20M | 78M | ratio |
|---|---|---|---|
| wall (index amortized) | ~28 min | **105 min** | 3.7× |
| sum task realtime | 52 min | 160 min | 3.1× |

## Per-process scaling (realtime seconds)

| process | 20M | 78M | ratio | 78M peak RSS | %cpu | threads |
|---|---|---|---|---|---|---|
| QUALIMAP_RNASEQ | 548 | **1997** | 3.6 | 17.8 GB | 104% | 6 |
| PICARD_MARKDUPLICATES | 289 | **1086** | 3.8 | 26.7 GB | 118% | 6 |
| STAR_ALIGN | 348 | 941 | 2.7 | 38.6 GB | **746%** | 16 |
| DUPRADAR | 195 | 884 | 4.5 | 0.8 | 101% | 1 |
| SALMON_QUANT | 235 | 528 | 2.2 | 16.3 | **865%** | 16 |
| RSEQC_READDUPLICATION | 125 | 468 | 3.7 | 11.9 | 101% | 6 |
| RSEQC_READDISTRIBUTION | 109 | 317 | 2.9 | 1.7 | 101% | 6 |
| TRIMGALORE | 80 | 306 | 3.8 | 0.1 | 203% | 8 |
| RSEQC ×3 more, STRINGTIE, GENOMECOV, FASTQC, SORT | ~500 | ~1600 | ~3.3 | | ~100% | |

## Reading

- **Everything scales linearly with depth; nothing is amortized.** No fixed-cost stage hides
  the problem at higher depth. Per-sample cost at production depth is ~4× the 20M number.
- **The multi-threaded stages (STAR, salmon) scale *sub*-linearly** (2.7×, 2.2×) — their
  fixed index-load cost is a bigger share at 20M. At 78M STAR is 15.7 min at 47% efficiency
  and salmon 8.8 min at 54%.
- **The single-threaded stages scale super-linearly or linear at 1 core:** Qualimap 33 min,
  Picard 18 min, dupRadar 15 min, RSeQC read-duplication 8 min. **Qualimap alone is a third of
  the wall clock** at production depth, and it's one Java thread.
- Critical path at 78M: trim (5) → STAR (16) → sort → **Picard (18) → Qualimap (33)** with
  RSeQC/dupRadar in parallel ≈ 75 min of an unavoidable serial chain; the box is ~5% busy
  during the last 50 min.

## Updated Amdahl (78M, per sample, serial)

| component | min | share | class |
|---|---|---|---|
| Qualimap + dupRadar + RSeQC ×5 | 62 | 39% | single-thread QC, Java/Python/R |
| Picard MarkDup | 18 | 11% | single-thread, GPU-shaped |
| STAR | 16 | 10% | multi-thread, 47% eff |
| salmon | 9 | 6% | multi-thread |
| sort/index/stats/flagstat/featureCounts/genomecov/stringtie | 15 | 9% | mixed, GPU-shaped |
| trim / FastQC / lint / misc | 12 | 8% | |
| tximport/SE/MultiQC/etc | ~28 | 17% | small tasks, R |
| **total task time** | **160** | | |

GPU-shaped (Picard + sort/stats/counts/coverage): **33 min of 160 = 21%** serial. Same
conclusion as the 20M profile, now at production depth: **the QC block is the throughput
governor**, not alignment and not bandwidth. Two levers, in order of return per effort:

1. **Replace/parallelize the single-thread QC.** Qualimap → `samtools stats` + coverage from
   the GPU pass; Picard → samtools markdup (4×) then GPU; dupRadar and RSeQC modules are
   ~all derivable from a single sorted-BAM pass. This is where the box's other 19 cores go.
2. **Overlap across samples.** Even with no code, 6 samples in flight fills the box during
   the QC tail. Tier 2A run (next) measures this directly.

## Cost curve, first real row (Spark, CPU-only, nf-core defaults, 78M pairs)

- Serial: 105 min/sample → **~14 samples/day** at 100% utilization if run one at a time.
- Overlapped (est. from CPU-minutes: 160 cpu-min / 20 cores ≈ 8 min/sample of box time, but
  memory-bound by STAR's 38 GB + Picard 27 GB + Qualimap 18 GB → ~2 samples in the heavy
  stages at once): **~25–35 samples/day**. Tier 2A measures this.
- At ~30/day, break-even vs $12/sample: Spark ~12 days, Mac Studio ~25 days of continuous use.
