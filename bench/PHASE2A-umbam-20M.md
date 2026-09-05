# Phase 2a — `umbam` CPU control arm: first 20M-BAM run on the Spark

Commit: (this one). `umbam chain` at 6/6 Tier 0 byte-compat gates (sorted BAM, flagstat,
idxstats, Picard markdup flags + metrics to 6 dp, featureCounts per-gene counts, bedtools
genomecov bedGraph). Run on the Tier 1 20M BAM: `NA12716_20M.Aligned.out.bam`, 4.0 GB,
53.9M alignments (38.8M primary, 15.0M secondary), 20 threads, THP-backed `umem` buffers.

## Result: 416 s, 33.9 GB peak RSS — parity with the tool chain, not yet a win
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
