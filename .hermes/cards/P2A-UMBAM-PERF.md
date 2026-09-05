# Card P2A-UMBAM-PERF — make the CPU control arm fast: 416 s → < 60 s on the 20M BAM

`crates/umbam` is correct (6/6 Tier 0 byte-compat gates — run them with
`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test -p umbam
--release -- --ignored`; they must stay green after every change). It is slow. Profile on the
Spark, 20 threads, 4 GB / 53.9M-record BAM (`bench/PHASE2A-umbam-20M.md`,
`bench/umbam-spark-20M-timing.tsv`):

```
decode          57.9 s   (raw BGZF inflate of this file measured at 2 s on 16 threads)
sort             1.3 s   (fine — this is the design working)
write_sorted    57.2 s
write_markdup  147.7 s   (markdup compute + full second BAM write)
index           27.4 s   (re-reads our own output from disk, twice)
featurecounts   25.5 s
genomecov       98.8 s
peak RSS        33.9 GB
```

Nothing here is memory- or compute-bound; it's serial parsing and serial re-encoding. Fix
that. Work in this order, measuring on **Tier 0 on this Mac** after each step (its timing.tsv
scales roughly 50× to the 20M file) and keeping all six gates green. I run the Spark
benchmark; don't touch the Spark.

## 1. Decode: parallel BGZF-block parse straight into the table + arena (target 3–5 s @ 20M)
Stop constructing `noodles::sam::alignment::RecordBuf` per record. The BAM on-disk record
body (everything after the 4-byte `block_size`: refID, pos, l_read_name, mapq, bin, n_cigar,
flag, l_seq, next_refID, next_pos, tlen, read_name, cigar[], seq[], qual[], aux[]) **is** the
payload we want in the arena verbatim — writers can copy it back out unchanged. Decoder:
1. Read the whole BGZF file into a `umem` buffer (or mmap it via `Allocation::File`).
2. Split at BGZF block boundaries (each block header carries `BSIZE`) — a serial scan of
   headers only, ~200k blocks.
3. Inflate blocks in parallel with rayon into a second `umem` buffer (`flate2` with the
   `zlib-rs` backend, or `libdeflater` — check what `noodles-bgzf` already pulls in and
   prefer that; do not add a C dependency without saying so in the summary). Record each
   block's uncompressed offset — you need it for step 3's virtual offsets anyway.
4. Parse the record stream in parallel: records straddle block boundaries, so first do a
   serial pass that only reads `block_size` fields to produce record start offsets
   (53.9M × one u32 read — a few hundred ms), then fill `RecordHeader[i]` in parallel from
   the record body at each offset. `name_hash` becomes unnecessary once sort ties break on
   index; drop it or keep it, your call.
The arena is then just the inflated stream (payload `Span` = offset+len into it): zero copy.
Keep the *existing* decoder behind a `--decoder noodles` flag for A/B on Tier 0 until the
gates pass with the new one; then delete it.

## 2. Writes: encode records in parallel, compress in parallel, patch instead of rewrite
- `sorted.bam`: for each record in sorted order, output = `block_size` + payload bytes
  copied from the arena. Chunk the sorted index into ~64 KB-uncompressed groups, have rayon
  produce each group's BGZF block (compress with level 1–2 — samtools default is 6 but the
  gate compares *records*, not bytes; note the level in the summary), then write blocks in
  order. Record each block's file offset and each record's virtual offset as you go: that's
  the BAI input.
- `markdup.bam` differs from `sorted.bam` only in the flag field of duplicate records. Do
  **not** re-encode: after markdup, patch the flag u16 in the arena copy (or in a per-record
  flag override array consulted during encode) and run the same parallel encode. Better: if
  you already produce sorted blocks, only the blocks containing a patched record need
  recompression; unchanged blocks are byte-copied. Report which you did.
- Both BAMs keep the standard 28-byte BGZF EOF block.

## 3. Index: build the BAI from the in-memory sorted table + virtual offsets (target < 1 s)
`noodles-csi`/`noodles-bam` has an indexer you can feed (tid, start, end, virtual offsets)
without re-reading. Bins are `reg2bin(start, end)` per the SAM spec; linear index is per
16 kb window. Compare to `samtools index` output on Tier 0 with `samtools idxstats` + a
region query (`samtools view sorted.bam chr22:20000000-20100000 | md5sum` against the
golden BAM's same query) — add that as a test.

## 4. genomecov: per-tid parallel sweep (target < 5 s @ 20M)
Group sorted records by tid (they're contiguous after sort), sweep each tid on its own
rayon task producing its bedGraph runs into a per-tid `Vec<u8>`, concatenate in header
order. Peak memory is one difference array per *in-flight* tid; cap concurrency if the
largest tids (chr1/2 at 250 Mbp × 4 B = 1 GB each) push RSS too high — or use the event
sweep you already have, just per tid.

## 5. markdup: measure it separately
`write_markdup` conflates markdup compute with the second write. Time them separately in
`timing.tsv` (`markdup` and `write_markdup`). If markdup compute alone is > 10 s at 20M,
parallelize the pair-grouping by tid (pairs keyed on the lower fragment end's tid; the
`HashMap<String, Vec<usize>>` by read name is the likely hot spot — key by name hash +
verify, or group mates via `next_pos`/`next_ref` instead of by name).

## Constraints
- All gates byte-identical throughout. If a change forces a semantic choice (e.g. block
  compression level), document it in `COMPAT.md`.
- `umem` buffers for every large allocation; `unsafe` stays inside `umem`. If you need a
  `umem` API (e.g. `Buf<Ro>` from a file mapping, sub-slicing a `Buf` into disjoint `&mut`
  regions for parallel fill — `as_pod_mut_slice` + `par_chunks_mut` already covers this),
  describe the minimal addition in your summary rather than editing `crates/umem`.
- `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean; readable named
  helpers; no `git commit`; don't touch `docs/ bench/ scripts/ .hermes/ KANBAN.md`.

Finish with: Tier 0 `timing.tsv` before/after per stage (release, `--threads 12`), the
six-gate table, the compression level and patch strategy you chose, and any `umem` gaps.
