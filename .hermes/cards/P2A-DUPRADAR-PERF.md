# Card P2A-DUPRADAR-PERF — dupRadar 61 s → ~8 s: reuse the parallel name grouping

`qc::subread_counts_all` (`crates/umbam/src/qc.rs`) is the last big serial stage in
`--qc`: 61 s at 76M records while `featureCounts` — the same work, once instead of four
times — is 5 s. Read it. The cost is structural:

1. It sorts 76M record indices with `sort_unstable_by` comparing **name bytes fetched
   from the arena** for every comparison — ~2 billion random arena reads on one thread.
   `lib.rs` already has the fast, parallel, collision-safe way to group by name: the
   `(name_hash, index)` `par_sort` + verified run walk shared by `mark_duplicates` and
   `count_fragments` (look for `next_name_run` / the name-grouping helper). Use it.
   Runs must contain **all** records with `flag & 0x804 == 0` (secondaries included —
   `primaryOnly = FALSE`), so build the grouping over that filter, not markdup's
   examined set; the helper may need a filter parameter — add one rather than
   duplicating it.
2. The run walk is serial. Runs are independent: `par_iter` over runs with per-thread
   `[SubreadCounts; 4]` accumulators (`fold` + `reduce`), exactly as `count_fragments`
   does for its single accumulator.
3. Inside a run, `cache_gene_hits` uses a `HashMap<usize, Vec<usize>>` per run — fine,
   runs are tiny — but `subread_pairs` re-parses `HI` aux for each of four modes. Parse
   `HI` and `NH` once per record in the run into a small `Vec`, then derive the four
   mate lists from it.

Everything else (the pairing rule, `one_fragment_gene`, the mode filters, the `n`
totals) is correct and gated — **`dupradar_matrix_integer_gate` on Tier 0 must stay
byte-identical**, and I re-run the full-depth comparison (its known ±1 residual on 17
genes in the `*Multi` columns must not change in either direction; if it does, say so —
it would mean the HI pairing changed).

Target: `qc_dupradar` ≤ 8 s at 76M records (it is ~4× featureCounts' work).

Rules: all Tier 0 gates green (`source ~/miniconda3/etc/profile.d/conda.sh && conda
activate rnaseq && cargo test -p umbam --release -- --ignored && cargo test -p umbam
--release`); `cargo fmt`; strict clippy; `unsafe` only in `umem`; no commits; don't touch
`docs/ bench/ .hermes/ KANBAN.md crates/umem/ crates/umgpu/` or the Spark. **Another
worker is editing the BGZF writer in `lib.rs` concurrently — confine your `lib.rs`
changes to the name-grouping helper's signature if you must touch it at all, and say
exactly what you changed there.**

Finish with Tier 0 `timing.tsv` before/after (`qc_dupradar` row) and the gate table.
