# Phase 2a — `umbam chain --qc` at full depth vs nf-core (NA11832_F, 78M pairs)

The first run of the complete resident chain on a real sample, diffed against the same
sample's nf-core/rnaseq 3.26.0 outputs from `runs/tier2a` (the real-scale golden). Spark,
20 threads, 6.67 GB / 76.1M-record STAR BAM, GENCODE v49. Script: `scripts/tier1_validate.sh`.

## Correctness — what matched, what didn't, and why
| output | vs nf-core | class |
|---|---|---|
| idxstats | **identical** | |
| dup-flag count (17,192,968) | **identical** | |
| Picard metrics (all 8 numeric columns) | **identical** (ours prints 6 dp, Picard 5) | format |
| RSeQC bam_stat | **identical** | |
| RSeQC infer_experiment | **identical** | |
| RSeQC pos.DupRate / seq.DupRate | **identical** | |
| junction_annotation (all counts) | **identical** (only the echoed BED path line differs) | format |
| flagstat | differs: duplicates lines | **our `flagstat` runs pre-markdup**; nf-core's runs post. Same records, 0x400 not yet set. Move flagstat after markdup or emit both. Fix. |
| genomecov (24,127,300 lines) | **identical** after `LC_ALL=C sort -k1,1 -k2,2n` (nf-core pipes bedtools through that sort; ours is in header order) | format |
| read_distribution | 10 of 10 tag counts identical; **6 `Total_bases` differ** (TSS/TES intervals, e.g. 26,850,235 vs 26,849,677) | interval-merge of the up/down-stream windows at chromosome edges or overlapping genes — a `process_gene_model` detail not exercised by chr22. Fix. |
| inner_distance | histogram shape same, **totals 22.5M vs 0.94M** | **`sample_size = 1,000,000`** pairs cap: RSeQC stops after 1M accepted pairs (counts skipped pairs? — 941,753 accepted suggests it counts attempts). Tier 0 had < 1M so never hit. Fix: honour the cap the way the source does. |
| dupRadar | 78,884 / 78,900 genes identical; **15 `allCountsMulti` and 6 `filteredCountsMulti` differ by ±1–5** | `countMultiMappingReads = TRUE` with `-p` in Rsubread: a multi-mapping *fragment* whose mates' alignments pair up differently across NH copies. Small (0.02% of genes). Investigate with `-R CORE` on one gene. |

Nothing here is a wrong-algorithm bug: one is pre/post-markdup ordering, one is a
missing sampling cap, one is an interval-merge edge case, and one is a multi-mapper
pairing subtlety in 21 genes. The Tier 0 fixture
did its job (chr22 caught all the semantics), and the full-depth sample caught the four
things chr22 structurally can't (sampling caps, chromosome-edge windows, multi-tid
multimappers, post-markdup stats).

## Timing — 17 min 10 s, 41 GB peak RSS
```
decode            45.8     (6.7 GB; 2× the 20M file's per-byte rate — page-cache cold)
sort               1.8
write_sorted      13.4
markdup            9.1
write_markdup     18.7
index              2.0
featurecounts      8.4
genomecov          3.9
--- chain subtotal ~103 s ---
qc_bam_stat       12.0
qc_seq_dup        35.9     (76M sequence-string hash inserts)
qc_pos_dup        31.2
qc_read_dist      32.8
qc_junction_ann   17.8
qc_infer_exp     771.9  <-- 
qc_junction_sat   22.5
qc_inner_dist    769.0  <--
qc_dupradar      125.4     (4 featureCounts passes; should be ~4 × 8 s)
qc_qualimap       31.0
```
**The chain is 103 s for what nf-core spends ~50 min of task-wall on** (STAR excluded).
Two QC stages are pathological: `infer_experiment` and `inner_distance` at ~770 s each
are doing a per-read BED lookup through something O(n) — each was 2.5 s on Tier 0 (1M
records, 1.6 MB BED) and is now 300× slower on 76M records against a 69 MB BED, i.e.
scaling with BED size × record count. Both need the per-tid sorted-interval structure the
other stages already use. dupRadar at 125 s is 4× the featureCounts cost and should be
one pass with four counters. With those three fixed the whole `--qc` chain should land
around 4–5 min per full-depth sample — versus ~58 min of single-threaded nf-core QC
task-wall.

## Run 2 (after card P2A-UMQC-8): 5 min 40 s, every gated output identical except dupRadar-Multi
| output | run 2 |
|---|---|
| flagstat | **identical** (now post-markdup) |
| read_distribution | **identical** (TSS/TES window merge fixed) |
| inner_distance_freq | **identical** (1M-pair sampling cap mirrored) |
| everything identical in run 1 | still identical |
| dupRadar | 78,883 / 78,900 genes identical; **17 genes ±1 in `allCountsMulti`/`filteredCountsMulti` only** (the non-Multi columns are all identical). Rsubread `countMultiMappingReads=TRUE` tie-breaking among NH>1 alignment pairs; HI-based pairing closed 4 of the 21. Left as a documented COMPAT residual — it's 0.02% of genes in the multimapper-inclusive columns of a QC plot. |

Timing (20 threads, same BAM):
```
chain (decode→genomecov)     ~77 s   (decode 13.4 warm vs 45.8 cold in run 1)
qc_bam_stat                  12.8
qc_seq_duplication           42.1    (still slow: 76M seq hashes)
qc_pos_duplication           31.6
qc_read_distribution         33.4
qc_junction_annotation       18.2
qc_infer_experiment         102.6    (was 772)
qc_junction_saturation       21.3
qc_inner_distance           102.4    (was 769)
qc_dupradar                  61.9    (was 125)
qc_qualimap                  32.3
--- --qc subtotal           ~460 s ---
wall                         340 s   peak RSS 40 GB
```
**5.7 min wall for the full post-alignment chain + all QC on a 78M-pair sample**, versus
~58 min of single-threaded nf-core task-wall for the QC alone (Tier 2A). The QC stages are
now each 10–100 s and mostly single-threaded sweeps; the next pass is parallelizing them
over tid like the chain stages (they share the resident table) — plausible target ~90 s
for all of `--qc`.

## Run 3 (after P2A-UMQC-PERF): 4 min 30 s; every gated output identical (dupRadar-Multi residual unchanged)
```
qc_bam_stat               0.9   (was 12.8)
qc_seq_duplication       35.9   (42.1)
qc_pos_duplication       25.1   (31.6)
qc_read_distribution     13.3   (33.4)
qc_junction_annotation   18.0   (18.2)
qc_infer_experiment      75.7   (102.6)  <-- see below
qc_junction_saturation   22.8   (21.3)
qc_inner_distance        75.6   (102.4)  <-- see below
qc_dupradar              60.7   (61.9)
qc_qualimap              12.6   (32.3)
wall                     270 s   (340)
```
**Timing attribution bug found by perf, not by the numbers.** `infer_experiment` and
`inner_distance` are ~1 s each on Tier 0 and ~70 s at both 20M *and* 78M — a fixed cost,
not a per-record one. `perf` on the 20M run shows 56% of all samples in `zlib_rs::deflate`
and `infer_experiment`/`inner_distance` themselves at 0.02–0.05%. The `Timing` struct
computes `infer_experiment: infer_started.elapsed()` and `inner_distance:
inner_started.elapsed()` *at the end of `qc::write`* — after dupRadar and Qualimap have
run — so each row absorbs everything that follows it. The "70 s" is dupRadar (61 s) +
Qualimap + file writes, counted three times. Real cost of those two stages is ~1 s.
Corrected `--qc` breakdown: ~180 s of which dupRadar 61, seq-dup 36, pos-dup 25,
saturation 23, junctions 18, read_dist 13, qualimap 13. dupRadar's four featureCounts
passes and the two hash-histogram stages (which the GPU card is replacing) are the
remaining targets.

## Run 4 (after P2A-DUPRADAR-PERF + P2B-MARKDUP/DUPHIST on CPU path): every gated output identical except the known dupRadar-Multi residual

Same 17 genes, same ±1–2 in `allCountsMulti`/`filteredCountsMulti` only, all non-Multi
columns identical — the name-grouping rewrite did not move it in either direction.
Stage times at 76M records (20 threads, CPU path):

| stage | run 3 | run 4 |
|---|---|---|
| qc_dupradar | **61.1 s** | **7.3 s** |
| qc_seq_duplication | 36.3 | 35.3 (GPU path: ~2 s) |
| qc_pos_duplication | 25.6 | 25.5 (GPU path: ~1.7 s) |
| qc_junction_saturation | 22.6 | 22.0 |
| qc_junction_annotation | 18.0 | 17.9 |
| qc_read_distribution | 13.4 | 13.6 |
| qc_qualimap | 12.9 | 11.2 |
| markdup | 9.1 | 9.6 (GPU path: 2.8–5 s) |
| write_sorted + write_markdup | 32.4 | 32.2 |
| **wall** | **4 min 30 s** | **~3 min 40 s** |

Found while diffing: `dupsPerIdMulti` was `u64` subtraction, so the two genes where our
`allCountsMulti` is one *below* `filteredCountsMulti` printed `18446744073709551615`.
Now signed (prints `-1`, as R would). Still a residual, no longer a landmine.

Remaining CPU-side cost (excluding BGZF, which stays on the CPU — see nvCOMP result):
seq/pos-dup 61 s → ~4 s on the GPU already; junction_saturation + junction_annotation +
read_distribution + qualimap ≈ 65 s are the next per-tid sweeps if (a) continues.

## Runs 5–6 (after P2A-UMQC-PERF-2 + BED subtraction fix): 2 min 55 s, every gated output identical except the known dupRadar-Multi residual

| stage (76M records, 20 thr) | run 4 | run 6 |
|---|---|---|
| qc_junction_annotation | 17.9 s | **1.1 s** |
| qc_junction_saturation | 22.0 s | **0.4 s** |
| qc_read_distribution (was mis-attributed, see below) | 13.6 s | **1.4 s** |
| qc_bed_parse (new row) | — | 2.4 s |
| qc_seq_duplication / qc_pos_duplication (CPU) | 35 / 26 s | 37 / 28 s (GPU: ~2 / ~1.7 s) |
| qc_dupradar | 7.3 s | 7.5 s |
| qc_qualimap | 11.2 s | 11.9 s |
| **wall** | **3 min 40 s** | **2 min 55 s** |

Two findings. (1) RSeQC's "insertion-ordered" junction table isn't inherently serial:
first-encounter order is min coordinate index per junction, recovered exactly by a parallel
extraction + sort + run walk; both junction outputs share the one pass. (2) The
`read_distribution` row was timing the BED12 parse. The parse itself was 12 s on the
full GENCODE BED because `subtract()` scanned each cut list from index 0 for every
interval — O(|a|·|b|) per chromosome; a `partition_point` start turns it into 1.9 s.
Terra's per-tid conversion of the interval maps was correct but cosmetic — the profiler
(0.5% in the sweep) is what found the real cost. Lesson repeated twice now: **time the
stage, not the region between two `Instant::now()` calls that happen to bracket it.**

`--qc` chain, CPU only, 76M records: 4:30 → 2:55 today. Remaining big rows are the two
duplication histograms (65 s CPU, ~4 s on the GPU) and BGZF (32 s, stays on CPU).

## Next
1. Fix the `Timing` attribution (`elapsed()` captured at each stage's end, not at return).
2. dupRadar 61 s → one pass, four accumulators (it's 4 × featureCounts' 5 s + overhead).
3. Parallelize the `qc_*` sweeps (they're the last single-threaded code in the chain).
2. dupRadar-Multi residual: COMPAT note; revisit only if someone needs those columns exact.
3. Then: this is the CPU control arm, done. Phase 2b/2c (CUDA) starts from here.

## Run 1 notes (historical)
1. Fix the four correctness items above (all small, all now have a real-scale golden).
2. Fix the three pathological QC stages.
3. Re-run this script; every row should read IDENTICAL except the two format rows.
