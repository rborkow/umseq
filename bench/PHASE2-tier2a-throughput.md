# Tier 2A — 6 × 78M-pair samples, nf-core/rnaseq 3.26.0, DGX Spark, overlapped

Run: `runs/tier2a/` on the Spark, 2026-09-05 10:21 → 13:49 EDT. 20 cores, 121 GB, 32 GiB
HugeTLB reserved (so ~89 GB general). `spark.config` caps STAR at 40 GB / 12 cores, salmon
8 cores, everything else per nf-core defaults; Nextflow allowed to overlap samples.
Samples: 6 Geuvadis LCLs (NA11832, NA12399, NA12751, NA12775, NA12814, NA12815), 78M pairs
each. 214 tasks, all COMPLETED.

## Headline
| | |
|---|---|
| wall for 6 samples | **208 min (3.47 h)** |
| amortized | **34.7 min/sample → 41.5 samples/day** |
| serial single sample (Tier 1 full, same box) | 105 min |
| overlap speedup | 3.0× |
| mean CPU parallelism over the run | 6.5 cores of 20 (32%) |
| marginal power cost at ~150 W | ~$0.02/sample |

At $12/sample on AWS Batch, the Spark pays for itself (~$4k) after ~330 samples — **8 days**
at this throughput. The comparison is unfair in the Spark's favour on one axis (no
storage/egress/orchestration cost counted) and unfair against it on another (this is an
un-tuned nf-core run using the stock tools; see below).

## Where the time goes (task-wall, sum over 6 samples)
| process | n | task-wall | cpu | eff |
|---|---|---|---|---|
| QUALIMAP_RNASEQ | 6 | 128 min | 136 cpu-min | 1.1× |
| STAR_ALIGN | 6 | 75 | 388 | 5.2× |
| PICARD_MARKDUPLICATES | 6 | 64 | 78 | 1.2× |
| SALMON_QUANT | 12 | 49 | 308 | 6.3× |
| DUPRADAR | 6 | 45 | 46 | 1.0× |
| RSEQC_READDUPLICATION | 6 | 28 | 28 | 1.0× |
| RSEQC_READDISTRIBUTION | 6 | 20 | 21 | 1.0× |
| TRIMGALORE | 6 | 19 | 40 | 2.1× |
| RSEQC_JUNCTIONSATURATION | 6 | 18 | 18 | 1.0× |
| RSEQC_BAMSTAT | 6 | 17 | 17 | 1.0× |
| STRINGTIE | 6 | 16 | 27 | 1.7× |
| BEDTOOLS_GENOMECOV | 6 | 15 | 16 | 1.1× |
| RSEQC_JUNCTIONANNOTATION | 6 | 14 | 15 | 1.0× |
| FASTQC | 6 | 13 | 26 | 2.0× |

Sum of task-wall 10.1 h; sum of CPU 22.5 cpu-h. **Single-threaded QC (Qualimap, Picard,
dupRadar, RSeQC ×5, bedtools) is 350 task-min = 58% of all task-wall**, at ≤1.2× efficiency.
That is 58 min of one-core work per sample. STAR + salmon together are 124 task-min at
5–6× and are what actually consumes the CPU budget.

## Reading
1. **The cost-curve result stands** and is mostly about having 20 cores and 120 GB resident
   with no per-container memory billing. 41.5 samples/day from a desk-side box.
2. **Utilization is 32%** because the single-threaded QC tail serialises per sample and
   Nextflow's default overlap can't fill 20 cores with 1-core tasks fast enough. More
   concurrency (more samples in flight) would push this up until STAR memory (40 GB each)
   caps it at 2 concurrent aligners. Expected ceiling on this box with stock tools: ~60–70
   samples/day.
3. **The QC chain is the target.** `umbam` (Phase 2a control arm) replaces Picard + bedtools
   + the BAM-consuming half of the stats with one resident pass; at its current 416 s on a
   20M BAM it is already at parity with the tool chain and has 5–6× headroom in parse/write
   (see `bench/PHASE2A-umbam-20M.md`). Qualimap, dupRadar, and RSeQC are the remaining
   single-threaded 260 task-min and are all one-pass BAM readers — the same resident table
   serves them.
4. Nothing here is GPU-shaped. The GPU question (2b/2c) is about whether a resident decode
   feeding kernels beats a resident decode feeding Rayon; this run says the prize for either
   is the 58% QC share, not STAR.

Traces: `bench/trace-spark-tier2a.txt` (Nextflow trace), `runs/tier2a/{report,timeline}.html`
on the Spark.
