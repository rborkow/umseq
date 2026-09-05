# Card P2A-UMBAM-CPU-2 — continue `umbam`: finish the chain and pass the Tier 0 gates

You are continuing work on `crates/umbam`. The previous run built the spine (decode →
`umem` record table → parallel sort → BAM writes → BAI) and stopped: markdup, flagstat,
idxstats, featureCounts, genomecov, and metrics are empty placeholders, and there are no tests.
Read `.hermes/cards/P2A-UMBAM-CPU.md` for the full original brief — everything in it still
applies. Read `crates/umbam/src/lib.rs` for what exists.

## The `umem` API gap is closed
`umem` now has:
- `unsafe trait Pod` + `Buf::as_pod_slice::<T>()` / `Buf<Rw>::as_pod_mut_slice::<T>()` — view
  a buffer as `&[T]` / `&mut [T]` for `#[repr(C)]` integer-only types. Implement `Pod` for
  `RecordHeader` (it must have **no padding that carries meaning** — it currently has explicit
  `_padding` fields, which is fine; assert `size_of::<RecordHeader>() == 48` in a test).
- `umem::Arena` (bump allocator over `Buf<Rw>`, `push(&[u8]) -> Option<Span>`, `get(Span)`,
  `into_buf()`) and `umem::Span { offset: u64, len: u32 }`.
See `crates/umem/src/lib.rs` and the test in `crates/umem/tests/round_trip.rs`.

Use them to **finish the resident layout**: drop `Vec<StoredRecord>` and `RecordBuf` retention.
Decode each record once, write its `RecordHeader` into the table via `as_pod_mut_slice`, and
push the variable part (read name, CIGAR ops as u32s, seq, qual, aux bytes — pick one
documented encoding, e.g. the BAM on-disk record body verbatim) into an `Arena`, storing the
`Span` in `offset`/`len`. Sort permutes an index `Vec<u32>` (or sorts the table in place — your
call, document it). Writers re-encode from the table + arena. Two arenas sized from the input
file size (compressed × 3 is a safe upper bound; record the ratio you observe in the summary)
are acceptable if a single one can't be pre-sized.

## Order of work — stop after each gate passes and record its status
1. **Resident layout + sort + sorted.bam** → gate: `sorted.bam` records == `chr22.sorted.sam`
   under the stable secondary sort described in the original card.
2. **flagstat + idxstats** → byte-identical to goldens.
3. **markdup + metrics** → 0 records with differing 0x400; metrics equal to 6 dp.
4. **featureCounts** → identical count column.
5. **genomecov** → byte-identical bedGraph.
6. `timing.tsv` with real `peak_rss_bytes` (`libc::getrusage` is fine).

Write `crates/umbam/tests/tier0.rs` **first**, with all six gates, each `#[ignore]`-gated on
`~/uni-rnaseq-data/tier0/MANIFEST.tsv` existing (use `std::env::var("HOME")`). Run them with
`cargo test -p umbam -- --ignored`. A gate that can't pass yet should fail with a clear message,
not be skipped.

## Golden-format notes (so you don't have to guess)
- `samtools flagstat` output format: see `~/uni-rnaseq-data/tier0/chr22.flagstat.txt`. The
  "primary", "secondary", "supplementary", "duplicates", "primary duplicates" lines are all
  present in 1.22.
- `samtools idxstats`: `name\tlength\tmapped\tunmapped` per reference in header order, then
  `*\t0\t0\tN`.
- Picard metrics file: header comment lines start with `#`; the metrics table is preceded by a
  `## METRICS CLASS` line; compare only the numeric columns named in the original card.
- featureCounts output: first line is a comment beginning `# Program:`; second line is the
  header `Geneid\tChr\tStart\tEnd\tStrand\tLength\t<bam name>`; compare on `Geneid` → count.
- bedGraph from `bedtools genomecov -bg -split`: `chrom\tstart\tend\tdepth`, zero-depth runs
  omitted, adjacent equal-depth runs merged.

## Hard requirements (unchanged)
Rust; `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean; no dense
one-liners; `unsafe` only inside `umem` (the `Pod` impl for `RecordHeader` is the one
permitted `unsafe impl`, with a `// SAFETY:` comment stating repr(C)/no invalid bit patterns);
no `git commit`; don't touch `docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/`; don't
touch the Spark.

Finish with: gate-by-gate status with numbers; `timing.tsv` on Tier 0 (release build,
`--threads 12`); the arena size ratio you observed; anything in `COMPAT.md`.
