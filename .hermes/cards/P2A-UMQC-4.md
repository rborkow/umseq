# Card P2A-UMQC-4 — read_distribution: the bx-python query semantics, measured

You stopped `read_distribution` with all totals right but tag assignments off (CDS 868,060
vs golden 865,627) and correctly suspected bx-python's zero-length query. I measured it in
the container:

```
Intersecter with one Interval(100, 200); t.find(st, end) returns:
(99,99)→0  (100,100)→0  (150,150)→1  (199,199)→1  (200,200)→0  (201,201)→0
(100,101)→1  (199,200)→1  (200,201)→0
```

So `foundone(chrom, ranges, mid, mid)` is a hit iff **`start < mid < end`** — strictly
greater than start, strictly less than end (for zero-length queries; the general rule is
`query_end > start && query_start < end`, which with `st == end == mid` reduces to strict
on both sides). A block midpoint that lands exactly on an interval's `start` is **not**
inside it. Encode exactly this in your interval lookup for every `foundone` call in the
classifier.

Also from the source: `chrom = chrom.upper()` on the read side before lookup, and
`process_gene_model` builds its `ranges` keyed by the BED chrom — check whether it uppercases
too (`build_bitsets` / `process_gene_model` in `docs/tool-src/rseqc-5.x-relevant.py`); if
only the read side is uppercased, a lowercase BED chrom would never match — Tier 0's are
`chr22` both sides so it's moot here, but mirror it.

`mid = exn[1] + int((exn[2] - exn[1]) / 2)` — Python's `int()` truncates toward zero;
lengths are positive so this is floor division. Your `fetch_exon` mirror already advances
on soft clips (the same buggy helper as pos.DupRate).

Then continue in order: `junction_saturation`, `infer_experiment`, `inner_distance`,
dupRadar, Qualimap. The rule stands — source beats card silently; stop only when source
and golden disagree with each other, or after ~2 attempts on one output. If `infer_experiment`
still mismatches after applying the same `foundone` strictness (it uses the same helper
pattern: check `configure_experiment`), report the specific counts.

Gates in `crates/umbam/tests/tier0_qc.rs`, goldens in `~/uni-rnaseq-data/tier0/qc/`.
`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test -p
umbam --release -- --ignored && cargo test -p umbam --release`; `cargo fmt`; strict clippy;
`unsafe` only in `umem`; no commits; don't touch `docs/ bench/ .hermes/ KANBAN.md
crates/umem/` or the Spark. Add the `junction_annotation.log` gate you said matches.

Finish with the per-output gate table, Tier 0 `timing.tsv`, COMPAT.md additions.
