# Card P2A-UMBAM-PERF-2b — parallel Picard-exact markdup + direct BAI + block reuse

You are working on `crates/umbam`, a one-pass resident BAM chain (decode → `umem` record
table + arena of native BAM bodies → sort permutation → markdup / stats / featureCounts /
genomecov, with two BGZF writes). It is correct — six byte-compat gates vs samtools /
Picard 3.5.0 / Subread / bedtools on a chr22 fixture — and 162 s on a 4 GB / 53.9M-record
BAM at 20 threads. The remaining ~100 s is three things, all correctness-sensitive, which is
why this is your card and not a plumbing card:

```
markdup         58.7 s   pair grouping via HashMap<String, Vec<usize>>, single-threaded
index           27.6 s   re-reads our own BAM from disk to build the BAI
write_markdup   17.9 s   full recompression of a file that differs from sorted.bam in 8.9M flag bits
```

A previous attempt at a sort-based markdup changed 624 Tier 0 flags (singleton / secondary
edge case) and was reverted. **The current `mark_duplicates` in `crates/umbam/src/lib.rs`
is the oracle.** It is slow but Picard-exact on the fixture; read it first and preserve its
semantics precisely:

- Secondary/supplementary (0x900) and unmapped (0x4) are never examined, never flagged.
- A *pair* = a name with a primary 0x40 record AND a primary 0x80 record, both with 0x1 set
  and 0x8 clear. Note the `find` picks the **first** such record per name in table order;
  with only primaries in play there's at most one of each, but keep the rule.
- Every examined primary that is not in a pair is *unpaired* — this includes mate-unmapped
  reads (0x8), single-end reads, and a read whose mate exists but is secondary-only.
- Pair key = the two `FragmentEnd`s ordered (`fragment_end()` = tid, unclipped 5′, strand).
  Best = highest score (sum of both reads' base qualities ≥ 15), tie → lowest stream rank
  of either read (`sorted_rank` min over the two). Everything else in the group is a dup.
- Unpaired key = one FragmentEnd; **if any pair has that FragmentEnd, every unpaired read
  there is a duplicate** (pairs always win); else best by score then rank.
- Metrics: counts as in `DupMetrics`.

## Do
1. **Parallel, allocation-free markdup with identical output.** Suggested design (yours to
   change if you see better, but justify it):
   - `RecordHeader.name_hash` is already a 64-bit FNV-1a of the read name, filled at decode.
   - Build `Vec<(name_hash, is_read2, record_index)>` for examined primaries; `par_sort`.
     Adjacent entries with equal hash form a name group; verify the names byte-equal (via
     the arena) before pairing, and treat a hash collision by falling back to grouping by
     actual name bytes for that run. Pair = the first 0x40 and first 0x80 in the group (in
     record-index order, to match the oracle's `find`) meeting the 0x1/!0x8 rule.
   - Pairs: build `(key_lo, key_hi, u64::MAX - score, min_rank, a, b)`; `par_sort`; walk
     runs. First of each run is best.
   - Unpaired: build `(end, u64::MAX - score, rank, idx)`; `par_sort`; walk runs; consult a
     sorted `Vec<FragmentEnd>` of pair ends (binary search) for the pairs-win rule.
   - Output the same `MarkdupResult`.
   Keep the old implementation as `mark_duplicates_reference` behind `#[cfg(test)]` and add
   a test that runs both on Tier 0 and asserts identical duplicate sets and metrics —
   *that* test is your correctness evidence, in addition to the golden gate. Delete the
   reference only if you're confident; I'd keep it.
2. **BAI from memory.** The sorted write knows each record's virtual offset. Compute
   `reg2bin(pos, end)` per record (`end` = pos + CIGAR reference length; unmapped-with-tid
   records use pos+1), accumulate chunks per (tid, bin) with adjacent-chunk merging, and the
   16 kb linear index (min voffset per window). Write BAI v1 (magic `BAI\1`, n_ref, per ref:
   n_bin, {bin, n_chunk, chunks}, n_intv, ioffsets; optional pseudo-bin 37450 with mapped/
   unmapped counts — samtools writes it and `idxstats` reads it, so include it). `noodles-
   csi`/`noodles-bam` index types can write it if you prefer. Gate: the existing region-query
   test and `idxstats` (already gated) on both BAMs.
3. **Block reuse for `markdup.bam`.** Retain the sorted write's compressed blocks (or their
   file offsets and re-read); recompress only blocks whose uncompressed span contains at
   least one patched flag; byte-copy the rest. Report the recompressed fraction. If > 80%,
   say so and just parallelize the full recompression harder.

## Rules
- All Tier 0 gates green after each step: `source ~/miniconda3/etc/profile.d/conda.sh &&
  conda activate rnaseq && cargo test -p umbam --release -- --ignored`.
- `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean; `unsafe` only in
  `crates/umem` (if you need an API there, describe it — don't edit it).
- No `git commit`; don't touch `docs/ bench/ scripts/ .hermes/ KANBAN.md`; don't touch the
  DGX Spark (I benchmark there).

Finish with: Tier 0 `timing.tsv` before/after (release, `--threads 12`), the gate table
including your new reference-equivalence test, the recompressed-block fraction, and any
place you believe the oracle itself is wrong relative to Picard (with a concrete case).
