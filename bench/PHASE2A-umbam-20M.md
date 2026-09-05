# Phase 2a — `umbam` CPU control arm on the Spark: 408 s → 55 s

Commit: (this one). `umbam chain` at 6/6 Tier 0 byte-compat gates (sorted BAM, flagstat,
idxstats, Picard markdup flags + metrics to 6 dp, featureCounts per-gene counts, bedtools
genomecov bedGraph). Run on the Tier 1 20M BAM: `NA12716_20M.Aligned.out.bam`, 4.0 GB,
53.9M alignments (38.8M primary, 15.0M secondary), 20 threads, THP-backed `umem` buffers.

## Correction
The first run (416 s, below) was accidentally given the StringTie *transcripts* GTF
(4,853 rows) instead of GENCODE v49 filtered (78,899 genes, 3.7M exons, 3.3 GB); its
featureCounts number is not comparable. Both runs in the "Perf pass" section use the
correct GTF (`data/ref/gencode.v49.filtered.gtf` on the Spark).

## Perf pass 4: 79 s → 55–58 s, outputs byte-identical — **target met**
featureCounts counting rewritten over the shared sort-and-pair name grouping (parallel,
no per-fragment allocations, per-tid sorted exons + prefix-max binary search). Three runs:
**57.6 / 54.9 / 54.9 s**. All five text outputs, the 8,918,475 dup flags, and a region
query through the direct BAI identical to v2.

| stage | v5 | v6 (median) |
|---|---|---|
| decode | 9.3 | 9.0 |
| sort | 1.2 | 1.2 |
| write sorted | 9.9 | 9.6 |
| markdup | 8.8 | 8.9 |
| write markdup | 11.9 | 12.9 |
| index | 1.6 | 1.7 |
| gtf_parse | 1.8 | 1.9 |
| featureCounts count | 28.6 | **5.6** |
| genomecov | 2.4 | 2.4 |
| **wall** | **79** | **55** |
| peak RSS | 29.3 GB | 29.2 GB |

**Versus the tool chain (samtools sort + index, Picard MarkDuplicates, samtools
flagstat/idxstats, featureCounts, bedtools genomecov): ~450 s sequential → 55 s, 8.2×**,
byte-compatible, from one decode. Nothing in this profile is memory-bandwidth-bound: the
55 s is ~22 s of BGZF compression (level 6, two files), 9 s of decode (2 s inflate + 7 s
record-table fill), 9 s markdup, and ~14 s of everything else. This is the bar a CUDA arm
has to clear on the same table.

## Perf pass 3: 104 s → 79–85 s, outputs byte-identical
Planned record-aligned BGZF blocks encoded+compressed in parallel (no burst structure),
BAI parallel per reference with dense bin vectors, GTF parsed in parallel by line chunk,
`gtf_parse` / `featurecounts_count` timing split. Level 6 kept (level 1 is +45% size on
Tier 0 for <2× write speed).

| stage | v4 | v5 a | v5 b |
|---|---|---|---|
| decode | 7.9 | 15.1 | 9.3 |
| write sorted | 16.4 | **9.7** | **9.9** |
| markdup | 8.9 | 9.1 | 8.8 |
| write markdup | 20.7 | **11.8** | **11.9** |
| index | 9.7 | **1.7** | **1.6** |
| gtf_parse | — | 2.1 | 1.8 |
| featureCounts count | — | **28.0** | **28.6** |
| genomecov | 5.7 | **2.3** | **2.4** |
| **total (wall)** | **104** | **85** | **79** |

Decode varies 8–15 s run to run (page cache / THP state; first run after a build is
slower). featureCounts is now the single biggest stage and it is the *counting*, not the
parse: `count_fragments` groups mates via `HashMap<String, _>` over 39M primaries and
builds two `HashSet<gene>` per fragment — the same pattern markdup had before the
sort-and-pair rewrite. That is the next target, then decode.

## Perf pass 2 (d4013f7): 162 s → 104–110 s, outputs byte-identical
Parallel Picard-exact markdup (sort-and-pair on 64-bit name hash + fragment-end tuples; the
old implementation kept as a `#[cfg(test)]` oracle with an equivalence test), BAI built from
in-memory virtual offsets with `ref_len` stored in `RecordHeader` at decode, block-reuse path
for `markdup.bam` (94.5% of blocks contain a patched flag on Tier 0 → full parallel
recompression). Two runs:

| stage | v2 | v4 run a | v4 run b |
|---|---|---|---|
| decode | 7.5 | 11.2 | 7.9 |
| sort | 1.2 | 1.2 | 1.3 |
| write sorted | 10.0 | 16.3 | 16.4 |
| markdup | 58.7 | **8.8** | **8.9** |
| write markdup | 17.9 | 21.5 | 20.7 |
| index | 27.6 | **10.0** | **9.7** |
| featureCounts | 32.0 | 32.8 | 32.6 |
| genomecov | 5.7 | 7.0 | 5.7 |
| **total (wall)** | **162** | **110** | **104** |

Region queries through the direct BAI match the samtools-built index (md5 of
`chr22:20000000-20100000` and `chr1:150000000-150100000` identical). The sorted write
regressed 10 → 16 s between v2 and v3 — the same commit added virtual-offset capture per
record; worth a look. Remaining: writes 37 s, featureCounts 33 s, index 10 s.

## Perf pass 1 (420064e): 408 s → 162 s, outputs byte-identical
Same input, same GTF, same box, 20 threads. flagstat/idxstats/featureCounts/genomecov
byte-identical between builds; markdup flags identical (8,918,475 records × 0x400), metrics
identical (4,447,593 pair dups, 22.96%).

| stage | v1 (3272754) | v2 | ceiling |
|---|---|---|---|
| decode | 57.7 | **7.5** | 3–5 |
| sort | 1.3 | 1.2 | — |
| write sorted | 58.3 | **10.0** | ~10 (BGZF level 6) |
| markdup compute | (149.7 incl. write) | **58.7** | <10 |
| write markdup | | **17.9** | ~1 with block reuse |
| index | 27.4 | 27.6 | <1 |
| featureCounts | 51.7 | **32.0** | ~10 |
| genomecov | 61.5 | **5.7** | ~5 |
| **total** | **408** | **162** | **<60** |
| peak RSS | 33.9 GB | 28.9 GB | |

What changed: the arena now holds BAM record bodies verbatim (no per-record object
construction on decode, straight copy on write); multithreaded BGZF on both writes; markdup
patches flags in place; genomecov per-tid. Remaining fat is markdup compute (by-name
`HashMap<String, _>` pair grouping over 54M records), index (re-scans our own output),
and the markdup write (full recompression instead of block reuse) — ~104 s of 162.

## First run (historical, wrong GTF for featureCounts): 416 s, 33.9 GB peak RSS
| stage | s | tool-chain equivalent (Phase 1) |
|---|---|---|
| decode → resident table+arena | 57.9 | (samtools sort reads: ~30 s) |
| sort (index permutation) | 1.3 | samtools sort 16t: ~60 s |
| write sorted.bam | 57.2 | (part of sort) |
| markdup + write markdup.bam | 147.7 | Picard MarkDuplicates: 279 s |
| index ×2 | 27.4 | samtools index: ~15 s |
| featureCounts | 25.5 | featureCounts: ~20 s |
| genomecov | 98.8 | bedtools genomecov: ~90 s |
| **total** | **416** | **~450** (sequential tools) |

Duplicate rate 23.0% (4.45M pair dups); counts and bedGraph not yet re-checked against the
tools at this scale (Tier 0 gates are the correctness evidence; a Tier 1 golden comparison
is the next validation step).

## Reading
The resident model works exactly as designed where it's used: sort is 1.3 s over 54M records
because it permutes an index over a 48-byte `Pod` table in THP memory. Everything else is
slow for reasons that have nothing to do with memory bandwidth or compute:

- **Decode 58 s** is parsing, not inflating. Phase 1 measured 2 s to inflate this file on 16
  threads. The remaining 56 s is one-record-at-a-time construction through noodles' SAM
  object model (String allocations per record for name/CIGAR/seq/qual/aux), then re-encoding
  to the arena. The BAM binary record layout *is already* the arena layout we want.
- **Writes 57 + ~120 s** re-encode from the object model and BGZF-compress on the writer's
  thread count. The markdup BAM differs from the sorted BAM only in the 0x400 bit of 4.45M×2
  records; it should be a patch of the first write's compressed blocks, not a second write.
- **Index 27 s** re-reads our own output from disk. The BAI is a function of the sorted
  table + the virtual offsets produced during the write; both are in hand.
- **genomecov 99 s** is an event-sorted sweep that isn't parallel across tids.

Per-stage ceilings from Phase 1 hardware numbers (≥160 GB/s streaming, 2 s inflate, 20
cores): decode 3–5 s, writes ~15 s each (compression-bound), index <1 s, genomecov <5 s.
Target after the performance pass: **< 60 s** end-to-end — 7× over the tool chain — with
byte-identical gates.

## What this says about the GPU question
Nothing in this profile is GPU-shaped either. A CUDA arm built on the same resident table
would inherit the same parse/write floor and would have to beat a 20-thread Rayon markdup +
sweep that hasn't been optimized yet. The control arm has to be fast *first* or the GPU
comparison is meaningless. Sequencing stands: perf pass on `umbam` → Tier 1 golden check →
then 2b.
