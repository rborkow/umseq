# Card P2A-UMQC-6 — inner_distance (resolved), then dupRadar, Qualimap

Seven of ten QC outputs are byte-identical. `inner_distance` blocked on two things, both
now resolved from the source — no more inference needed:

## inner_distance — exact semantics
1. **Bins.** The card's `0..1000 step 10` were the *library* defaults; the CLI
   (`inner_distance.py main()`) passes its own: **`low_bound = -250, up_bound = 250,
   step = 5, sample_size = 1_000_000, q_cut = 30`**. nf-core passed no overrides. So the
   window left bounds are `range(-250, 250, 5)`.
2. **Histogram binning is off-by-one via bx-python.** Each accepted pair's distance `d`
   is inserted as `Interval(d - 1, d)`; each bin's count is `len(find(st, st + step))`.
   With bx's strict overlap rule (`query_end > iv.start && query_start < iv.end`) that
   is: `st + step > d - 1 && st < d` ⇔ **`st < d ≤ st + step`**. Bins are
   **(st, st+step]** — open on the left, closed on the right. `d == st` belongs to the
   *previous* bin. This is why your 0–10 total was 38,258 vs 34,255.
3. **Filters and distance**, per `mRNA_inner_distance` in
   `docs/tool-src/rseqc-5.x-relevant.py` (re-read it; summary): iterate records in file
   (coordinate) order; skip qcfail, duplicate, secondary, unmapped, not-paired,
   mate-unmapped, `mapq < 30`; skip if `mpos < pos`; skip if `mpos == pos && is_read1`
   (note: `pair_num` is **not** incremented for skips). Then `pair_num += 1` and stop
   once `pair_num >= sample_size` (checked at loop top — irrelevant on Tier 0). Different
   chromosomes → written as `NA`, **not** added to `ranges`. Otherwise
   `read1_end = pos + qlen + Σ(N-intron sizes)` where `qlen` is pysam's *aligned query
   length* (query length consumed by M/I/S/=/X — check: pysam `qlen` = `query_alignment_
   length`, which **excludes** soft clips; verify against the golden and say which). If
   `mpos >= read1_end` → `d = mpos − read1_end`, then the same-transcript branch (exon
   bitset intersection over `[read1_end, mpos)`; `size == d` → `dist=mRNA` value `d`;
   `0 < size < d` → `dist=mRNA` value `size`? — read lines ~93–113 exactly for which
   value is inserted in each branch); else (`mpos < read1_end`) → overlap branch,
   `d = −len({exon positions in (mpos, read1_end]})` when both mates share a transcript,
   else genomic `mpos − read1_end` (negative). Every branch except `sameChrom=No` does
   `ranges.add_interval(Interval(d - 1, d))`.
4. Output `inner_distance_freq.txt`: one line per window `"{st}\t{st+step}\t{count}"`.
   Gate: byte-identical to `qc/rseqc/chr22.inner_distance_freq.txt`.

## Then dupRadar and Qualimap — as specified in `.hermes/cards/P2A-UMQC-5.md`
(read that card's two sections; unchanged).

Rules unchanged: source beats card silently; stop only on source/golden disagreement or
~2 attempts per output. Gates in `tests/tier0_qc.rs`; `cargo fmt`; strict clippy; `unsafe`
only in `umem`; no commits; don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/` or
the Spark. `source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo
test -p umbam --release -- --ignored && cargo test -p umbam --release`.

Finish with the ten-row gate table, Tier 0 `timing.tsv` with every `qc_*` row, and
COMPAT.md additions (replace the inner_distance blocker note with the resolved semantics).
