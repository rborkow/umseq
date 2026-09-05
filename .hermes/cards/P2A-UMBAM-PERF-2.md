# Card P2A-UMBAM-PERF-2 — the last 100 s: markdup compute, index, markdup write, featureCounts

Your first perf pass took the 20M BAM from 408 s → 162 s on the Spark with byte-identical
outputs. Profile now (20 threads, 53.9M records, `bench/umbam-spark-20M-timing-v2.tsv`):

```
decode           7.5
sort             1.2
write_sorted    10.0
markdup         58.7   <-- 1
write_markdup   17.9   <-- 3
index           27.6   <-- 2
featurecounts   32.0   <-- 4
genomecov        5.7
total          162     target < 60
```

Same rules as before: all six Tier 0 gates plus the region-query test stay green after every
step (`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo test
-p umbam --release -- --ignored`); measure on Tier 0 here; I run the Spark. `cargo fmt`,
`cargo clippy -p umbam --all-targets -- -D warnings` clean; `unsafe` only in `umem`; no
`git commit`; don't touch `docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/`.

## 1. markdup compute: 58.7 s → < 10 s
The pair-grouping goes through `HashMap<String, Vec<usize>>` keyed by read name over 54M
records — 39M string allocations + hashing, single-threaded. Replace:
- **Find mates without names.** In a coordinate-sorted stream, a primary paired record's
  mate is identified by (next_ref, next_pos, and the mate's flag having the complementary
  0x40/0x80 bit). Build, in parallel, a hash from
  `(tid, pos, name_hash64, is_read1)` → record index for all primary mapped paired records
  (`name_hash64` = a 64-bit hash of the name bytes computed once during decode and stored
  in `RecordHeader` — there's an existing `name_hash` field; widen if it's 32-bit). Then
  each record looks up its mate at `(next_ref, next_pos, same name_hash, !is_read1)`. Verify
  the name bytes match on hit (hash collision guard) — that's one memcmp per pair, not a
  String. Use a concurrent-friendly approach: sort the key tuples with rayon and pair
  adjacent entries, which avoids a concurrent hashmap entirely.
- **Group by fragment-end key in parallel.** Build `(FragmentEnd_lo, FragmentEnd_hi,
  score, stream_rank, pair_index)` tuples for all pairs, `par_sort_unstable`, then walk runs
  of equal key — the best pair is the first in each run when the sort key puts higher score
  first and lower stream rank second. Same for unpaired reads. No HashMap, no BTreeMap.
- Metrics stay identical by construction; the gate will tell you.

## 2. index: 27.6 s → < 1 s
You have, from the sorted write, every record's virtual offset (block file offset << 16 |
offset within uncompressed block). Build the BAI directly: for each record in sorted order
compute `reg2bin(start, end)` (SAM spec §5.3; `end` = pos + reference length of CIGAR, from
the header table), accumulate chunks per (tid, bin) merging adjacent chunks, and the linear
index per 16 kb window (min virtual offset of any record overlapping the window). Write
with `noodles-bam`'s index writer or by hand (the format is ~40 lines). Do the same for
`markdup.bam` — if you implement block reuse (§3) the virtual offsets are identical and you
write the same BAI twice; otherwise compute from the markdup write's offsets. Test: the
existing region-query test plus `samtools idxstats` on both BAMs must match the golden
(already gated).

## 3. write_markdup: 17.9 s → ~1–2 s
`markdup.bam` = `sorted.bam` with the 0x400 bit set on 8.9M of 53.9M records. Those records
live in some subset of the ~40k BGZF blocks. Keep the compressed blocks from the sorted
write in memory (2.8 GB — fine, or write them and re-read the ones you need); for the
markdup file, copy blocks that contain no patched record byte-for-byte and recompress only
blocks that do. Report the fraction recompressed. If it's > 80% (duplicates are spread
evenly), fall back to just recompressing everything in parallel with more threads and say
so.

## 4. featureCounts: 32 s → ~10 s
3.7M exons / 78.9k genes; the interval structure is built once. Profile where the time is
(building the per-tid interval index from the 3.3 GB GTF? — that parse alone may be most of
it; measure it separately as a `gtf_parse` timing row). If parsing dominates, parse in
parallel by splitting the file at line boundaries into N chunks. If lookup dominates, make
sure the per-tid structure is a sorted array + binary search (or an implicit interval tree),
queried in parallel over records grouped by tid.

## Finish with
Tier 0 before/after `timing.tsv` per stage (release, `--threads 12`), gate table, the
recompressed-block fraction from §3, and anything new in `COMPAT.md`.
