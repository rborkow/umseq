# Card P2A-UMBAM-CPU — the control arm: one-pass, multi-threaded CPU BAM chain on `umem`

## Why this exists
Phase 1 profiling (`bench/PHASE1-bamchain-20M.md`, `bench/PHASE1-depth-scaling.md`) showed the
RNA-seq BAM chain is parallelism-bound, not bandwidth-bound: decode of a 4 GB BAM is ~2 s on 16
threads, but Picard MarkDuplicates is 279 s single-threaded, and sort/markdup/stats/counts/
coverage each re-decode the same BAM (5–6 passes). Any future GPU kernel must beat a *good*
multi-threaded CPU implementation of a one-pass design, not Picard. You are building that
control. It may also be the result on its own.

Read first: `docs/design-umem.md` (the buffer model you build on), `crates/umem/src/lib.rs`
(its API), `bench/PHASE1-bamchain-20M.md` (the numbers to beat), and the plan's Phase 2a rev 3
in `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md`.

## Deliverable
New workspace member `crates/umbam` — a library + `umbam` binary:

```
umbam chain --in unsorted.bam --gtf annot.gtf --out-dir D [--threads N]
```
producing, from **one** decode of the input into a `umem` buffer:
- `D/sorted.bam` — coordinate-sorted, BGZF, with `.bai`
- `D/markdup.bam` + `D/markdup.metrics.txt` — duplicates flagged (0x400), Picard-compatible
  metrics
- `D/flagstat.txt`, `D/idxstats.txt` — samtools-format
- `D/featureCounts.txt` — gene-level paired-end counts, featureCounts `-p --countReadPairs`
  format
- `D/genomecov.bg` — bedGraph coverage, `bedtools genomecov -bg -split` semantics
- `D/timing.tsv` — wall time per stage + peak RSS

## Architecture (fixed; don't redesign)
1. **Decode once.** Read the BGZF input with `noodles-bgzf` multi-threaded, parse records with
   `noodles-sam`/`noodles-bam` into a compact in-memory record table living in a
   `umem::Buf<Rw>` (`Allocation::Anon { huge: true, require_huge: false }` on Linux). Record
   layout: a `#[repr(C)]` fixed-size header per record (tid, pos, end, flag, mapq, mate tid/pos,
   tlen, name hash, offset+len into a bytes arena for the variable part) + one bytes arena.
   Both in `umem` buffers so a later GPU arm can consume the identical layout with no re-parse.
2. **Sort** = parallel radix/sort on (tid, pos, strand, name_hash) over the record index, not
   the bytes. `rayon`.
3. **Markdup** = Picard semantics: group by (library, tid, unclipped 5' start, strand, mate's
   unclipped 5' start, mate strand) for pairs; single-end and unpaired secondaries per Picard's
   rules; keep the highest base-quality-sum representative; flag the rest. Optical duplicates
   NOT implemented (record `--OPTICAL_DUPLICATE_PIXEL_DISTANCE 0` equivalence). Parallel over
   tid.
4. **Stats/flagstat/idxstats** = single parallel reduction over the sorted index.
5. **featureCounts** = interval tree per tid from the GTF (`noodles-gff`/`noodles-gtf`),
   paired-end fragment counting with featureCounts defaults (`-s 0`, overlap ≥1 bp, multi-
   overlap fragments not counted, multimappers (NH>1) not counted). Parallel over tid.
6. **genomecov** = per-tid sweep producing bedGraph runs with `-split` (CIGAR N gaps are not
   covered). Parallel over tid, concatenated in order.
7. **Write** sorted + markdup BAMs with `noodles-bgzf` multi-threaded writer.

## Correctness gate — byte-compat against Tier 0 goldens
Fixture: `~/uni-rnaseq-data/tier0/` (see `MANIFEST.tsv`): `chr22.unsorted.bam` (input),
`chr22.gtf`, and goldens produced by samtools 1.22.1 / picard 3.5.0 / featureCounts 2.1.1 /
bedtools 2.31.1. Tests in `crates/umbam/tests/tier0.rs` (gated on the fixture dir existing;
`#[ignore]` otherwise), each comparing **canonical text**:
- `samtools view sorted.bam` == `chr22.sorted.sam` — record order may legitimately differ for
  equal (tid,pos); compare after a stable secondary sort on (flag, name) applied to *both*.
- `samtools view markdup.bam` flags == `chr22.markdup.sam` flags, same secondary-sort rule.
  Report the count of records whose 0x400 differs; gate is **0**.
- `markdup.metrics.txt` `READ_PAIR_DUPLICATES`, `UNPAIRED_READ_DUPLICATES`, `PERCENT_DUPLICATION`
  equal to Picard's to 6 decimals.
- `flagstat.txt`, `idxstats.txt` byte-identical.
- `featureCounts.txt` count column identical for every gene (compare the 7th column joined on
  Geneid; ignore the header comment line).
- `genomecov.bg` byte-identical.
If a golden is *wrong* by your reading of the tool's documented semantics, do not "fix" the
comparison — write down the disagreement in `crates/umbam/COMPAT.md` and make the test fail
loudly. I decide.

## Performance target
`bench/PHASE1-bamchain-20M.md`: the 20M-pair BAM (4 GB, 53.9M alignments) costs ~450 s of
sequential tool time on the Spark. Target for `umbam chain` at 16–20 threads: **< 60 s**
end-to-end including both BAM writes. Do not optimize before the gate passes; do report
`timing.tsv` on Tier 0 in your summary.

## Hard requirements
- Rust, `cargo fmt`, `cargo clippy --all-targets -- -D warnings` clean, no dense one-liners.
- Deps: `noodles` (bam, sam, bgzf, gtf/gff, core), `rayon`, `clap`, `anyhow`, `thiserror`,
  `umem` (path). No `rust-htslib`.
- All large buffers through `umem`. `unsafe` only inside `umem`; if you need it elsewhere,
  stop and explain why in your summary.
- No `git commit`. Don't touch `docs/`, `bench/`, `scripts/`, `.hermes/`, `KANBAN.md`, or
  `crates/umem/` (if `umem` lacks something you need, describe the minimal API addition in
  your summary instead of editing it).
- Don't touch the DGX Spark. Develop and test on this Mac; I run the Spark benchmark.

Finish with: what's implemented; `cargo test -p umbam` output on Tier 0 (every gate, pass or
fail, with numbers); `timing.tsv` for Tier 0; any COMPAT.md disagreements; any `umem` API gaps.
