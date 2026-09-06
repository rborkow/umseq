# Card P2B-UMGPU — the CUDA backend for `umem`: context, fence, CUB shim, one end-to-end sort

## Why
The CPU control arm (`crates/umbam`) is done: byte-identical to the tool chain at full
depth, 55 s on a 20M BAM. Phase 2b asks whether a CUDA kernel consuming the **same
`umem`-resident table** — no copy, no staging — beats 20 Rayon threads on the same stage.
Read `docs/design-phase2b.md` (the plan and the kernel we're building toward) and
`docs/design-umem.md` (the ownership model you're implementing a backend for). This card
is the plumbing: after it, kernels are ~100-line cards.

## Facts you need (measured on the target, `bench/RESULTS-T6-summary.md`)
- Target: DGX Spark, NVIDIA GB10 (aarch64, sm_121 — check `nvcc --list-gpu-arch`; use
  `-arch=native` or `sm_121`), CUDA 13.0 at `/usr/local/cuda`, driver 580. nvcc is at
  `/usr/local/cuda/bin/nvcc` (not on PATH). **CUB lives at
  `/usr/local/cuda/include/cccl/cub/cub.cuh`** (CCCL layout), so `-I/usr/local/cuda/include/cccl`.
- Host memory from `mmap` + `MADV_HUGEPAGE` (what `umem` produces) is device-accessible
  via HMM/ATS **without** `cudaHostRegister`: 161 GB/s streaming, 2.6 G random lookups/s.
  Kernels take the raw host pointer. `cudaHostRegister` is opt-in (once saw an illegal
  access on a THP buffer; not reproduced) — expose it as a `Context` flag, default off.
- Never `cudaMalloc` anything large (T6 §2: on GB10 it's system memory and subject to the
  same 4K-page cliff, and can't be smaps-verified). Scratch (CUB temp storage, output
  arrays) comes from `umem` too.
- Development box (this Mac) has **no CUDA**. The crate must build and its unit tests
  must pass here with a stub backend; the real test runs on the Spark, by me.

## Deliverable: `crates/umgpu`
```
crates/umgpu/
  Cargo.toml          features: cuda (default off). deps: umem (path), thiserror, cc (build)
  build.rs            when feature cuda: compile shim/*.cu with nvcc into libumgpu_shim.a,
                      link cudart. Honour UMGPU_NVCC (default /usr/local/cuda/bin/nvcc) and
                      UMGPU_CUDA_ARCH (default sm_121). Without the feature: no-op.
  shim/umgpu_shim.cu  extern "C" surface (below). Dumb: raw pointers + lengths, returns
                      cudaError_t as i32. No allocation inside except CUB temp-storage
                      *sizing* queries (the caller supplies storage).
  shim/umgpu_shim.h
  src/lib.rs          Context, CudaFence (impl umem::Fence), Stream, and safe wrappers.
  src/stub.rs         cfg(not(feature="cuda")): every entry returns Err(Unsupported), so
                      umbam can depend on umgpu unconditionally.
  tests/sort.rs       the end-to-end test (cfg(feature="cuda"), #[ignore] unless a GPU
                      is present).
```

### extern "C" surface (v1 — exactly these; more come with kernel cards)
```c
int umgpu_init(int* device_props_out /* 8 ints: pageableMemoryAccess,
     pageableMemoryAccessUsesHostPageTables, directManagedMemAccessFromHost,
     hostRegisterSupported, concurrentManagedAccess, major, minor, multiProcessorCount */);
int umgpu_stream_create(void** stream);   int umgpu_stream_destroy(void* stream);
int umgpu_event_create(void** ev);        int umgpu_event_destroy(void* ev);
int umgpu_event_record(void* ev, void* stream);
int umgpu_event_query(void* ev);          /* 0 = done, 1 = pending, <0 = error */
int umgpu_event_sync(void* ev);
int umgpu_host_register(void* p, size_t len); int umgpu_host_unregister(void* p);
/* CUB: radix sort pairs (u64 keys, u32 values), ascending, [begin_bit, end_bit) */
int umgpu_radix_sort_pairs_u64_u32_temp_size(size_t n, size_t* temp_bytes);
int umgpu_radix_sort_pairs_u64_u32(void* temp, size_t temp_bytes,
     const uint64_t* keys_in, uint64_t* keys_out, const uint32_t* vals_in,
     uint32_t* vals_out, size_t n, int begin_bit, int end_bit, void* stream);
/* CUB: run-length encode of sorted u64 keys → unique keys + counts + num_runs */
int umgpu_rle_u64_temp_size(size_t n, size_t* temp_bytes);
int umgpu_rle_u64(void* temp, size_t temp_bytes, const uint64_t* keys, uint64_t* unique_out,
     uint32_t* counts_out, uint32_t* num_runs_out, size_t n, void* stream);
/* CUB: exclusive scan u32 */
int umgpu_exclusive_scan_u32_temp_size(size_t n, size_t* temp_bytes);
int umgpu_exclusive_scan_u32(void* temp, size_t temp_bytes, const uint32_t* in,
     uint32_t* out, size_t n, void* stream);
/* a trivial kernel for the access-path sanity test: out[i] = in[i] + 1 over u64 */
int umgpu_inc_u64(const uint64_t* in, uint64_t* out, size_t n, void* stream);
const char* umgpu_error_string(int code);
```
All pointers are host pointers from `umem` buffers. `num_runs_out` is a device-writable
u32 in a `umem` buffer too. Every function records nothing on its own; the Rust side
records the completion event.

### Rust side
- `Context::new(device: i32, opts: ContextOptions { host_register: bool }) -> Result<Context>`
  — calls `umgpu_init`, stores the 8 props (expose them; `umbam` will log them per
  `design-umem.md` "CUDA specifics"), creates a default `Stream`. Implements/returns a
  `umem::Context` (via `ContextId(device as u64)`).
- `CudaFence { event }` implements `umem::Fence`: `wait` = `umgpu_event_sync`, `try_wait`
  = `umgpu_event_query`. Errors map to `FenceError { message: umgpu_error_string(...) }`.
  `Drop` destroys the event (never panics; log via `eprintln!` on error).
- `Stream` wraps the raw stream; `Drop` destroys it after a sync.
- Safe wrappers that take leases: e.g.
  `fn radix_sort_pairs(ctx, stream, keys: &GpuLease<Rw>, vals: &GpuLease<Rw>, keys_out:
  &GpuLease<Rw>, vals_out: &GpuLease<Rw>, temp: &GpuLease<Rw>, n, bits) -> Result<()>` —
  check every lease's length ≥ what's needed, check `ctx` matches each lease's context,
  then call the shim. Provide `temp_size` helpers so the caller allocates temp via `umem`.
- Finishing a batch of work: `fn submit<M>(stream, leases: Vec<AnyLease>) -> Submission<CudaFence>`
  that records an event on the stream and wraps the leases — mirror whatever
  `umem::Submission` needs (read its constructor; add the minimal `umem` API in your
  summary if it's missing, don't edit `crates/umem`).
- `unsafe` is allowed in `umgpu` **only** at the FFI boundary, each call with a
  `// SAFETY:` comment stating which lease guarantees which pointer/length.

### The end-to-end test (`tests/sort.rs`, feature `cuda`, runs on the Spark)
1. `Buf<Rw>::allocate` (huge: true, require_huge: false) three buffers: keys (N u64,
   random), vals (N u32 = index), and temp (sized by the shim). N = 16M.
2. Lease them, sort on the GPU, `submit`, `wait`, get the buffers back.
3. Verify on CPU: keys ascending, and `vals` is a permutation such that
   `keys_out[i] == keys_in[vals_out[i]]`.
4. Also run `umgpu_inc_u64` on a 1 GB buffer and time it → report GB/s (should be ~100+;
   that's the access-path sanity number).
5. Print the 8 device props.
Also a second variant of step 1 with `host_register: true`, same asserts.

## Rules
- `cargo fmt`; `cargo clippy -p umgpu --all-targets -- -D warnings` clean **without** the
  feature (the Mac); with the feature it must at least `cargo check` cleanly if you can
  fake nvcc — if not, say so and I'll compile on the Spark.
- `cargo test -p umgpu` on this Mac must pass (stub backend tests: `Context::new` returns
  `Unsupported`; the FFI surface isn't linked).
- No commits; don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/ crates/umbam/`
  or the Spark. Add `umgpu` to the workspace `Cargo.toml`.

Finish with: file tree, the extern surface as implemented, what `umem` API you needed
that isn't there (if any), and the exact commands for me to build + run the test on the
Spark (`export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH; cargo test -p umgpu
--features cuda --release -- --ignored`).
