# Card P2A-UMQC-PERF — parallelize the QC sweeps: ~460 s → ~90 s at full depth

`crates/umbam/src/qc.rs` (1,444 lines) is correct — every output byte-identical to nf-core
at 78M pairs except a documented ±1 dupRadar residual — and **contains zero rayon**. Every
stage is a serial sweep over 76M records. Full-depth timings, 20 threads
(`bench/PHASE2A-tier1-validation.md`, run 2):

```
qc_bam_stat              12.8
qc_seq_duplication       42.1
qc_pos_duplication       31.6
qc_read_distribution     33.4
qc_junction_annotation   18.2
qc_infer_experiment     102.6
qc_junction_saturation   21.3
qc_inner_distance       102.4
qc_dupradar              61.9
qc_qualimap              32.3
                        ~460 s   target ≤ 90 s
```

The chain stages above them (markdup 9 s, featureCounts 5 s, genomecov 4 s on the same
records) show what the same table does when swept in parallel per tid. Do the same here.
**Every Tier 0 QC gate must stay byte-identical** — these are exact-count outputs, so
parallel reductions must be order-independent (sums, histograms, sets) or must preserve
the tool's order where it matters (noted below).

## Shared fixes first
A. **BED model lookups**: `BedModel` keys `HashMap<String, Vec<Interval>>` by chromosome
   name and functions like `contains`, `overlapping_genes`, `transcript_names_at`,
   `cigar_exons` take `&str` chroms and allocate. Resolve chromosome → `tid` **once** when
   the model is loaded (map BED chrom names, uppercased per the source, to the BAM
   header's tid; unknown chroms get no tid), store per-tid `Vec<Interval>` sorted by
   start with a prefix-max-end array, and look up by `tid` with binary search. Return
   small `Vec`s or iterate without collecting into `HashSet<String>`. Keep the
   *semantics* (strict `start < mid < end`, uppercase normalization) — only the
   data structure changes.
B. **Per-record derived data** (`cigar_exons`, `cigar_introns`, unclipped ends,
   `bam_sequence_hash`) is recomputed in several stages. Compute once into a
   `Vec<...>` (or reuse what the chain already has: `RecordHeader.ref_len`, the name
   grouping, the fragment ends from markdup) and share across stages.

## Per-stage
1. `bam_stat`, `pos_duplication`, `seq_duplication`: `par_iter` over records with
   `fold`/`reduce` into per-thread counters/`HashMap`s, merged at the end. For the two
   histograms the key sets are large (tens of millions): per-thread `HashMap<u64, u32>`
   keyed by hash with collision verification, merged by summing. Order-independent.
2. `read_distribution`, `infer_experiment`: records grouped by tid (contiguous in sorted
   order) → `par_iter` over tid ranges, each producing its group counters; sum. For
   `infer_experiment` the **first 200,000 usable reads in coordinate order** is an
   order-dependent cap — keep a serial prefix scan to find the cut index, then
   parallelize *within* that prefix; or note that with 76M records the cap is hit inside
   chr1 and handle it as "serial until cap" (it's cheap: 200k reads).
3. `junction_annotation`, `junction_saturation`: junction extraction `par_iter` per tid
   into per-tid `HashMap<(start, end), count>`; classification per junction is
   independent. Saturation's seeded shuffle over the event list stays serial after the
   parallel extraction (it's 20 subsamples over a few million events — fine).
4. `inner_distance`: **order-dependent** (`pair_num` cap at 1,000,000 accepted pairs in
   coordinate order). Serial prefix scan to find the cut, then parallel over the
   accepted pairs for the distance computation and histogram (`fold`/`reduce`).
5. `dupradar`: it's four featureCounts variants — reuse `count_fragments`' parallel
   name-run machinery with four accumulators; if it already does, profile why it's 62 s
   when featureCounts is 5 s (probably the BED/HI lookups from A/B).
6. `qualimap`: same per-tid pattern as `read_distribution`.

## Rules
Gates: `source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test
-p umbam --release -- --ignored && cargo test -p umbam --release` — all green after every
stage. `cargo fmt`; strict clippy; `unsafe` only in `umem`; no commits; don't touch
`docs/ bench/ .hermes/ KANBAN.md crates/umem/` or the Spark. Add a timing row split only
if useful.

Finish with Tier 0 `timing.tsv` before/after per `qc_*` row (release, `--threads 12`) and
the gate table. I run the full-depth benchmark.
