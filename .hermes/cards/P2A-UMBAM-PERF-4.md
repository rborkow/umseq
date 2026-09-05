# Card P2A-UMBAM-PERF-4 — featureCounts counting: 28 s → < 5 s

`crates/umbam` is at 79 s on the 20M BAM (Spark, 20 threads); featureCounts *counting* is
28 s of it, the largest stage (`bench/umbam-spark-20M-timing-v5.tsv`). `gtf_parse` is 2 s
and fine.

Read `count_fragments` (`lib.rs`, search for it) and then read `mark_duplicates` just above
it. `count_fragments` has the exact structure markdup had before its rewrite: a
`HashMap<String, _>` keyed by read name over 39M primary records (39M String allocations,
single-threaded), then two `HashSet<usize>` allocations per fragment for the gene votes.
`mark_duplicates` already solved the same mate-finding problem: it sorts
`(name_hash, index)` with `par_sort_unstable`, verifies names byte-equal within equal-hash
runs (re-sorting a run by actual name on collision), then walks runs with `chunk_by`.

## Do
1. **Factor the name-grouping out of `mark_duplicates`** into a helper that returns the
   sorted `(name_hash, index)` vector (or an iterator of runs) for primary mapped records
   and reuse it in `count_fragments`. Same semantics, one implementation, both callers.
   The markdup oracle-equivalence test and the markdup gate stay green.
2. **Rewrite `count_fragments`** over the runs, in parallel: `par_chunk_by`-style — split
   the sorted vector into runs (a serial pass to find run boundaries is fine, or use
   rayon's `par_chunk_by` if the rayon version has it), then process runs in parallel with
   per-thread `Vec<u64>` count accumulators reduced at the end (or `fold` + `reduce`).
   Within a run: find the first 0x40 and first 0x80 primary (mirror the current
   "multiple primary records for the same mate ⇒ invalid" and the `NH>1 ⇒ skip` rules
   exactly), compute each mate's gene hits into a small stack/`SmallVec`-style sorted
   `Vec<u32>` (a read overlaps a handful of genes at most — no `HashSet`), then
   intersection-else-union, count iff exactly one candidate.
3. `mate_gene_hits`: check it doesn't allocate per call beyond the result vec, and that the
   exon lookup is binary-search + short scan on a per-tid sorted array. If it's currently
   an interval-tree with boxed nodes, replace it.

Gate: `featurecounts_gate` byte-identical; all other gates + oracle + BAI tests green.
`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test -p umbam
--release -- --ignored && cargo test -p umbam --release`. `cargo fmt`; `cargo clippy -p
umbam --all-targets -- -D warnings`; `unsafe` only in `umem`; no `git commit`; don't
touch `docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/` or the Spark.

Finish with Tier 0 before/after `timing.tsv` (release, `--threads 12`) and the gate table.
