# Card P2B-DUPHIST — first kernels on the resident table: the RSeQC duplication histograms

## Why this first
`crates/umgpu` works on the GB10 (16M-key CUB radix sort over `umem` leases; 83 GB/s
through the ATS host-pointer path; see `docs/design-phase2b.md`, "P2B-UMGPU result").
This card is the warm-up before the markdup kernel: prove the *pattern* — derive
fixed-width keys from the resident `RecordHeader` table + arena, radix-sort on the GPU,
run-length-encode, reduce — on the two simplest exact-count outputs, with a byte-identical
gate against the CPU path that already matches RSeQC.

Read: `docs/design-phase2b.md`, `crates/umgpu/src/{lib,cuda}.rs` (the API you call),
`crates/umgpu/shim/umgpu_shim.{h,cu}` (the extern surface; you'll add to it),
`crates/umbam/src/qc.rs` → `position_duplication`, `sequence_duplication`,
`bam_sequence_hash`, `sequence_layout`, `bam_cigar` (the semantics to reproduce exactly).

## What the CPU computes (must be identical)
**pos.DupRate**: for every record with `flag & 0x204 == 0` (mapped, not QC-fail —
secondaries/supplementaries/duplicates *retained*) and `mapq ≥ 30`, key = `(tid, pos,
blocks)` where `blocks` is the list of `(ref_start, ref_end)` M-runs from walking the
CIGAR with the RSeQC `fetch_exon` bug (M emits a block and advances; D, N, **and S**
advance; I/H/P/=/X ignored). Count records per key; then histogram: for each occurrence
count `k`, number of keys with that count. Output `Occurrence\tUniqReadNumber` sorted by
occurrence.
**seq.DupRate**: for every record with `flag & 0x904 == 0` (mapped, primary, not QC-fail —
duplicates retained) and `mapq ≥ 30`, key = the read's 4-bit-packed SEQ bytes (`l_seq`
nibbles; the unused low nibble of an odd-length read masked to 0). Count per distinct
sequence; same histogram.

## GPU design (both share it)
1. **Key derivation kernel** (`umgpu_dup_keys`, new): one thread per record; reads
   `RecordHeader[i]` (48 B, `#[repr(C)]` — the shim gets the struct layout as offsets, not
   the Rust type) and the record body from the arena via `offset/len`; applies the
   filter; emits a **64-bit hash** of the key (FNV-1a over the same bytes the CPU hashes —
   for seq use exactly `bam_sequence_hash`'s byte sequence; for pos, hash
   `(tid, pos)` + the block list) into `keys[i]`, and `i` into `vals[i]`; filtered-out
   records emit key `u64::MAX` with a sentinel so a single `DeviceSelect`/partition
   drops them (or emit a `u8` flag array and compact with `umgpu_select_flagged`, new).
2. **`umgpu_radix_sort_pairs_u64_u32`** (exists) on the compacted keys.
3. **`umgpu_rle_u64`** (exists) → unique hashes + counts.
4. **Collision verification** — the CPU path verifies equal hashes by byte-comparing
   bodies and *never* lets a collision change the histogram. Mirror it: for each RLE run
   with count > 1, a kernel (`umgpu_verify_runs`, new) byte-compares every member's key
   bytes against the run's first member (the sorted `vals` give record indices; bodies
   are in the arena) and, for any member that differs, marks it. Runs are short (dup
   counts are small) so a per-run loop is fine. Report `collisions_found` (a device
   counter). If > 0, fall back for those runs to the CPU (split them by exact key on the
   host); it will be 0 in practice but the gate demands identical output regardless.
5. **Histogram of counts**: sort the `counts` array (radix, u32) + RLE, or a
   `DeviceHistogram`; tiny. Return `(occurrence, n_keys)` pairs to the host.
All buffers are `umem::Buf` leased to the GPU; temp storage sized via the shim's
`_temp_size` calls and allocated through `umem`. **No `cudaMalloc`, no `cudaMemcpy`.**
Add an `umgpu::stats` (or similar) that counts bytes copied — it must read 0.

## Integration
- `umbam::qc` gets `position_duplication_gpu(resident, ctx) -> Result<HashMap<u32, u64>>`
  and the seq variant, behind `#[cfg(feature = "cuda")]` (umbam gains a `cuda` feature
  forwarding to `umgpu/cuda`). `umbam chain --qc --gpu` selects them; default stays CPU.
- Timing rows `qc_pos_duplication_gpu` / `qc_seq_duplication_gpu` when used.
- **Gate**: a `#[cfg(feature = "cuda")]` `#[ignore]` test in `crates/umbam/tests/gpu.rs`
  that runs both CPU and GPU variants on Tier 0 and asserts the `HashMap`s equal, plus the
  existing `.xls` goldens. I run it on the Spark. Also a synthetic-data unit test of the
  key-derivation kernel vs the CPU hash on ~10k handmade records (runs on Spark only).

## Rules
- Mac: `cargo clippy -p umbam -p umgpu --all-targets -- -D warnings` clean **without**
  `cuda`; `cargo test -p umbam -p umgpu` pass. The `.cu` additions must compile — you can't
  run nvcc here, so keep them simple and standard; I compile on the Spark and will send
  back errors verbatim if any.
- `unsafe` only in `umem`, and in `umgpu` at the FFI boundary with `// SAFETY:` naming the
  lease that guarantees each pointer. Nothing `unsafe` in `umbam`.
- No commits; don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/` or the Spark.

Finish with: the new extern entries, the exact Spark commands for build + gate + a timed
run (`umbam chain --qc --gpu` on `~/uni-rnaseq/data/ref/NA12716_20M.Aligned.out.bam` with
`--gtf ~/uni-rnaseq/data/ref/gencode.v49.filtered.gtf --bed <the BED>`), and anything you
couldn't verify without a GPU.
