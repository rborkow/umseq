# Card P2B-NVCOMP — BGZF compression on the GPU via nvCOMP batched Deflate

## Why
`umbam`'s two BGZF writes (`sorted.bam`, `markdup.bam`) are the largest fixed cost in
the chain: ~13 + ~19 s at 76M records, ~56% of all CPU samples in `zlib_rs::deflate`
(`bench/PHASE2A-tier1-validation.md`). The GPU is otherwise idle during them. nvCOMP's
batched Deflate API is exactly our writer's shape: independent ≤64 KB chunks in,
raw deflate streams out.

Read `docs/design-phase2b.md` (the pattern, the two results, the "output order is the
contract" rule), `crates/umbam/src/lib.rs` → `BlockWriter`, `write_bam`,
`write_bam_with_duplicates`, `write_markdup_reusing_blocks`, `compress_block`,
`OUTPUT_BLOCK_SIZE` (the planned-block writer: records are chunked into ≤65,280-byte
uncompressed blocks up front, each block encoded+compressed independently in parallel,
then written sequentially with a `BamLayout` of block offsets + per-record virtual
offsets that the BAI builder consumes). Read `crates/umgpu/shim/umgpu_shim.cu` and
`src/cuda.rs` for the shim/wrapper style and `crates/umbam/src/markdup_gpu.rs` for how
table/arena/workspace are leased and returned.

## Facts
- nvCOMP 5.0.0.6 (CUDA 13, linux-sbsa) is at `~/.local/opt/nvcomp/` on the Spark:
  `include/nvcomp/deflate.h`, `lib/libnvcomp.so`. Not on this Mac.
- `nvcompBatchedDeflateCompressAsync(device_uncompressed_chunk_ptrs,
  device_uncompressed_chunk_bytes, max_uncompressed_chunk_bytes, num_chunks, temp,
  temp_bytes, device_compressed_chunk_ptrs, device_compressed_chunk_bytes, opts,
  device_statuses, stream)`. Chunks ≤ 65,536 B; pointer arrays and size arrays live in
  device-accessible memory (`umem` buffers qualify). `opts.algorithm`: 0 entropy-only …
  **4 "beats zlib level 6 ratio"**, 5 highest. Temp size via
  `nvcompBatchedDeflateCompressGetTempSizeSync`; per-chunk max output via
  `nvcompBatchedDeflateCompressGetMaxOutputChunkSize`; alignment requirements via
  `nvcompBatchedDeflateCompressGetRequiredAlignments`.
- Output is a **raw deflate stream** per chunk. BGZF = gzip member per block: 18-byte
  header (with `BC` extra field carrying `BSIZE` = total block size − 1), the deflate
  stream, then CRC32 of the uncompressed data + ISIZE (8 bytes). Our writer already
  builds these around `zlib_rs` output — reuse that framing; only the deflate bytes
  change. **CRC32 must be computed too**: either on the host (`crc32fast` is likely
  already in the tree via flate2 — check `Cargo.lock`; it's ~2 GB/s/core so ~1 s
  parallel for 4 GB) or add a trivial device CRC32 kernel to the shim. Host is fine for
  v1.
- Host memory from `umem` is device-accessible via ATS (no `cudaHostRegister`; it's 15×
  slower here). Never `cudaMalloc` anything large.

## Deliverable
1. **Shim** (`umgpu_shim.cu`, linked against `libnvcomp` when a new cargo feature
   `nvcomp` is on — `build.rs` adds `-I$UMGPU_NVCOMP/include`, `-L$UMGPU_NVCOMP/lib`,
   `-lnvcomp`, default `UMGPU_NVCOMP=$HOME/.local/opt/nvcomp`; feature implies `cuda`):
   ```c
   int umgpu_deflate_alignments(int algorithm, size_t* input, size_t* output, size_t* temp);
   int umgpu_deflate_temp_size(size_t num_chunks, size_t max_chunk, int algorithm, size_t* temp_bytes);
   int umgpu_deflate_max_output(size_t max_chunk, int algorithm, size_t* max_out);
   int umgpu_deflate_batch(const void* const* in_ptrs, const size_t* in_bytes, size_t max_chunk,
       size_t num_chunks, void* temp, size_t temp_bytes, void* const* out_ptrs,
       size_t* out_bytes, int algorithm, int* statuses /* nvcompStatus_t */, void* stream);
   ```
   plus a `umgpu_nvcomp_error_string`.
2. **Rust wrapper** in `umgpu` (`deflate_batch(ctx, stream, leases…)`) with the usual
   lease/length checks; stubs when the feature is off.
3. **`umbam` writer path** behind `--gpu` (extend the flag; add `--gpu-deflate-level N`,
   default 4): plan blocks exactly as now; write all blocks' uncompressed bytes
   contiguously into one `umem` buffer (they already get encoded in parallel — encode
   into the shared buffer at precomputed offsets instead of per-block `Vec`s); build the
   pointer/size arrays in `umem`; allocate output as `num_chunks × max_out` in one
   `umem` buffer (contiguous, so `out_ptrs[i] = base + i × max_out`); lease everything;
   `deflate_batch`; submit/wait; then on the host, in parallel: CRC32 each block's
   uncompressed bytes, frame each compressed chunk as a BGZF member, and write
   sequentially, producing the same `BamLayout`. The markdup write reuses the same path
   (with the patched flags applied during encode, as now).
4. **Gates**: all Tier 0 gates byte-identical? — **no**: deflate output differs between
   compressors, so `sorted.bam` bytes will differ from the zlib path. The gates compare
   *records* (they `samtools view` the BAM), so they must still pass, and `samtools
   view -c`, `idxstats`, and a region query through the BAI must match the CPU-written
   file. Add to `crates/umbam/tests/gpu.rs`: write Tier 0 both ways, assert
   `samtools view` output identical and the BAI region query identical. Also assert every
   block's `status == nvcompSuccess` and that a re-inflate (host, `flate2`) of each
   chunk round-trips to the original bytes (that's the real correctness test; do it in
   the gate, not just in debug).
5. **Report** (I run these on the Spark): for `algorithm` ∈ {2, 4, 5}: output file size
   vs the zlib-6 file, wall time of the write stage, and the split between GPU
   compression / host CRC+framing / file write. Print a `write_sorted_gpu` timing row
   and sub-rows.

## Rules
- Mac: `cargo clippy -p umbam -p umgpu --all-targets -- -D warnings` and `cargo test`
  clean without the `cuda`/`nvcomp` features. I compile with them on the Spark and send
  back errors verbatim.
- `unsafe` only in `umem` and at `umgpu`'s FFI boundary (`// SAFETY:` naming the lease).
- Don't touch the zlib path (it's the default and the reference); don't touch
  `crates/umem/`, `docs/ bench/ .hermes/ KANBAN.md`, or the Spark. No commits.

Finish with: extern entries, the framing code path, the Spark commands, and what you
couldn't verify without the library.
