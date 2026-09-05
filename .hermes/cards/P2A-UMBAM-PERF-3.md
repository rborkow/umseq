# Card P2A-UMBAM-PERF-3 — writes and featureCounts: 104 s → < 60 s

`crates/umbam` on the 20M BAM, Spark, 20 threads (`bench/umbam-spark-20M-timing-v4.tsv`):

```
decode           7.9
sort             1.3
write_sorted    16.4   <-- A
markdup          8.9
write_markdup   20.7   <-- A
index            9.7   <-- C
featurecounts   32.6   <-- B
genomecov        5.7
total          104     target < 60
```

Same rules: all Tier 0 gates + oracle equivalence + BAI tests green after each step
(`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test -p umbam
--release -- --ignored && cargo test -p umbam --release`); `cargo fmt`; `cargo clippy -p
umbam --all-targets -- -D warnings`; `unsafe` only in `umem`; no `git commit`; don't touch
`docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/`; don't touch the Spark.

## A. Writes: 37 s → ~12 s total
Read `BlockWriter` (`lib.rs` ~281–348). The problem is structural: one thread appends record
bytes into 64 KB blocks; every `batch_size` (= threads × 8 = 160) blocks it stops, compresses
the batch in parallel, writes it, and only then resumes appending. So compression runs in
bursts and the appender idles during each burst; the writer thread's file I/O also serialises
behind compression. For a 2.8 GB output that's ~42k blocks in ~270 bursts.

Fix as a pipeline with no burst structure:
1. **Chunk the sorted index up front** into blocks: walk `resident.order`, accumulating
   record lengths (`header.len + 4`) until the next record would exceed `OUTPUT_BLOCK_SIZE`;
   emit `(start_rank, end_rank)` per block. Records must never be split across blocks —
   samtools does split them, and the BAI virtual-offset logic already handles either, but
   not splitting makes every block independently encodable. This walk is a few hundred ms.
2. **Encode + compress each block in parallel** with `par_iter` over the block ranges:
   each task copies its records' bodies (4-byte `block_size` + body from the arena, with
   the flag patch applied for the markdup write) into a local buffer and compresses it.
   Output is `Vec<Vec<u8>>` of compressed blocks. Peak memory = the whole compressed file
   (~2.8 GB) — acceptable at 29 GB RSS on a 120 GB box; if you'd rather bound it, process
   in windows of ~2000 blocks with the writer draining each window on a separate thread.
3. **Write sequentially**; compute `blocks[]` offsets and every record's virtual offset
   from the block table (block file offset ≪ 16 | in-block offset) — no per-record
   bookkeeping during the write.
4. Compression level: `bgzf::io::Writer::new` uses flate2's default (6). Test level 1–3
   with `bgzf::io::writer::Builder` (or `flate2::Compression::new(n)` if you drop to
   flate2 directly). Report file size and time at levels 1, 3, 6; **pick 6 unless a lower
   level is ≥2× faster for < 10% size growth**, and record the choice in `COMPAT.md`. The
   gates compare records, not bytes, so any level is valid.
5. `markdup.bam`: with per-block encode, the reuse path is simply "if no record in this
   block has a changed flag, reuse the sorted write's compressed bytes." Keep it; it's
   free now. Report the reuse fraction.

## B. featureCounts: 33 s → ~10 s
Split `write_featurecounts` timing into `gtf_parse` and `count` rows in `timing.tsv` so the
next profile shows which it is. The GTF is 3.3 GB / 3.68M exon lines; `read_features` is
almost certainly the bulk. If so:
- Parse in parallel: read the file into a `umem` buffer (or `Vec<u8>`), split at line
  boundaries into `threads` chunks, parse each chunk into `Vec<(tid, start, end, gene_id
  bytes)>` with a hand-rolled tab/attribute scanner (no `noodles-gtf`, no per-line String
  for the whole line — only for `gene_id`, and intern those through a `HashMap<&[u8], u32>`
  built after the parallel pass so gene order stays "first appearance in file").
- Gene order for output must remain GTF first-appearance order — the gate compares
  Geneid→count with the golden's row order.
If `count` dominates instead, check the per-tid lookup structure: sorted exon starts +
binary search on `start ≤ read_end`, then scan while `exon_start ≤ read_end` checking
`exon_end ≥ read_start`. Parallel over tids.

## C. index: 9.7 s → < 3 s
`write_index` walks 54M records serially with a `BTreeMap<bin, Vec<chunk>>` per reference.
Make it parallel over references (records are contiguous per tid after sort): each tid
builds its bins/linear index independently, then serialise in order. Use a `Vec` indexed
by bin (37,450 slots) rather than a `BTreeMap` — bins are dense for a whole-genome BAM.

## Finish with
Tier 0 before/after `timing.tsv` (release, `--threads 12`); the level-1/3/6 size+time table
from Tier 0; reuse fraction; gate table; COMPAT.md additions.
