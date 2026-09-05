# `umem` — unified-memory allocator design (v2)

**Status:** v2, 2026-09-04, after Astra adversarial review (`docs/review-umem-astra.md`).
v1's API is summarized in the "What v1 got wrong" table below (the repo had no commits when v1
was replaced, so that table is the record). Load-bearing abstraction; gets a second Astra pass
on the implementation before `umgpu` builds on it.

## Problem

Every GPU-visible buffer — genome index, BAM records, output — must satisfy:

1. **One working representation, two views.** CPU and GPU dereference the same physical pages
   during processing. Startup loads (file → memory) and kernel-internal page migration are
   allowed; per-batch staging copies are not.
2. **Verified large-page backing on Linux for random-access regions.** GPU random access into
   4 KiB-page memory collapses ~150×. Coverage must be **100% of the addressable region**,
   verified per-VMA from `/proc/self/smaps`, and treated as a timestamped observation (reclaim
   can split it later). 95% is not enough: 5% of 30 GiB at 12 M/s dominates.
3. **Startup copy is the main path for the index.** A read-only mmap of a file cannot be made
   safe in Rust (another process can truncate it) and 2 MiB folios for file-backed pages are
   conditional on ext4 config. So: read the index file into verified anonymous THP memory at
   startup (~30 GB at ~25 GB/s effective ≈ 1.2 s + I/O). File-backed mapping stays as an
   `unsafe` opt-in for streaming inputs (BAM) where the caller accepts the contract.
4. **Sound ownership.** No CPU slice can exist while GPU work that touches the buffer is in
   flight. Forgetting a submission must never restore CPU access or free live storage. The
   `unsafe` boundary includes kernel contracts, not just allocation.

## Ownership model

The core idea: **submission consumes the buffer; completion returns it.** There is no shared
state to race on because at any moment a buffer is owned by exactly one of {CPU, a Submission}.

```rust
/// Owned allocation. While you hold this, only the CPU touches the memory.
pub struct Buf<M: Mode> { inner: Arc<Backing>, _m: PhantomData<M> }
pub struct Ro; pub struct Rw;            // Mode: read-only or read-write (type-level)

impl<M: Mode> Buf<M> {
    pub fn len(&self) -> usize;
    pub fn as_slice(&self) -> &[u8];                                   // Ro and Rw
    pub fn page_report(&self) -> PageReport;
}
impl Buf<Rw> {
    pub fn as_mut_slice(&mut self) -> &mut [u8];
    pub fn freeze(self) -> Buf<Ro>;                                    // one-way
}

/// A buffer handed to the GPU. NOT Clone. NOT a pointer. Only `umgpu` can open it.
pub struct GpuLease<M: Mode> { inner: Arc<Backing>, ctx: ContextId, _m: PhantomData<M> }

impl<M: Mode> Buf<M> {
    /// Give the GPU access. The Buf is gone; you get it back from `Submission::wait`.
    pub fn lease(self, ctx: &Context) -> GpuLease<M>;
}
```

`umgpu` (the backend crate) is the only consumer of `GpuLease`. It builds a `Submission`:

```rust
pub struct Submission { leases: Vec<AnyLease>, fence: Fence, ctx: ContextId }

impl Submission {
    /// Block until the GPU is done; every leased buffer comes back CPU-owned.
    pub fn wait(self) -> Result<Vec<AnyBuf>, GpuError>;
    /// Non-blocking. On Err(NotReady) you still own `self`.
    pub fn try_wait(self) -> Result<Vec<AnyBuf>, (Submission, GpuError)>;
}
```

**Forget-safety.** `Submission` holds `Arc<Backing>` for every lease. If a caller
`mem::forget`s the Submission, the Arcs leak — the storage stays alive forever, which is a
leak, not UB. There is no `Drop` impl that returns ownership; ownership only comes back
through `wait`. The `Fence` is device-side; nothing on the CPU side assumes completion without
observing it.

**Drop of a `Submission`** (normal path, not forget): the `Drop` impl blocks on the fence
before releasing the Arcs. This is the one blocking wait, and it is never invoked from a
completion handler. On a failed/uncertain fence (device error, timeout), the Arcs are moved to
a process-wide **quarantine** list and never freed — storage is lost, not reused.

**Backing drop** (`Arc<Backing>` count → 0) runs in this order:
1. Metal: release the `MTLBuffer` (the no-copy deallocator closure holds a strong ref to the
   mapping's owner, so `munmap` cannot precede Metal's final release — Apple's contract).
   CUDA: `cudaHostUnregister` if registered (only ever after a fence has been observed).
2. `munmap` / `cudaFree` / `cudaFreeHost`.
Because the Arc count is 0, no `Buf`, `GpuLease`, or `Submission` exists — so no GPU work
can be in flight (every in-flight work item holds an Arc via its Submission).

**Chunking.** A `Backing` is one VM region, but on Metal it may be bound as N `MTLBuffer`s
(`maxBufferLength` is 13.3 GiB on the 24 GB M4 Pro; queried at runtime, never assumed).
`GpuLease` exposes `chunks() -> &[ChunkBinding { buffer, byte_offset, byte_len }]` and every
kernel argument-binding helper in `umgpu` takes a 64-bit logical offset and resolves it to
(chunk, offset) with a bounds check. On CUDA there is one chunk. Kernels that index across a
chunk boundary receive the base pointers of all chunks and the chunk size as constants — this is
a kernel-authoring rule, enforced by the `umgpu` launch wrappers, not a `umem` concern.

## The `unsafe` boundary

`umem` has exactly these `unsafe` sites, each with a stated invariant:
- `mmap`/`madvise`/`munmap` syscalls — invariants: region is ours, length is page-aligned,
  pointer came from our mmap.
- `Backing::as_ptr()` → `*mut u8` — only `umgpu` calls it, only inside a Submission build.
- File-backed constructor `Buf::<Ro>::map_file_unchecked(path)` is `unsafe fn`; contract:
  caller guarantees no other writer to the inode for the lifetime of the Buf.

`umgpu`'s kernel wrappers are where safety is actually established. Each wrapper is a
reviewed function that takes typed leases, checks lengths/alignment/offsets against the
kernel's declared element types, requires `Rw` leases for outputs, and guarantees outputs are
fully written or the Submission reports error. Arbitrary kernel launch is `unsafe fn` with a
written contract; safe callers get only the reviewed wrappers. **This is the deal: no `unsafe`
escapes past `umgpu`, at the cost of every kernel getting a wrapper.**

`Send`/`Sync`: `Buf<M>` is `Send`, and `Buf<Ro>` is `Sync`; `Buf<Rw>` is `Send + !Sync`.
`GpuLease` is `Send + !Sync`. `Submission` is `Send + !Sync`. Raw pointers never cross a
thread boundary except inside these types.

## Allocation on Linux (no sudo)

**T6 finding (2026-09-05):** `cudaMalloc` on GB10 hit the same ~300× random-access cliff when
allocated after 12 GiB of other buffers. It is system memory with unverifiable backing. Therefore
`umem` owns *all* large allocations; `cudaMalloc` is not used for anything ≥ 2 MiB. THP with
2 MiB-aligned mmap + advise-before-touch gave 100% coverage in 3/3 randomized-order trials at 4
and 12 GiB, so step 5 (HugeTLB) is a fallback, not the expected path.


Strategy, in order, all verified per-VMA before returning:
1. Reserve `len + 2 MiB` anonymous VA; align base up to 2 MiB; `munmap` the excess head/tail.
2. `madvise(MADV_HUGEPAGE)` before first touch.
3. Populate sequentially (`MADV_POPULATE_WRITE` on 6.17).
4. Parse `/proc/self/smaps` for our VMA; if `AnonHugePages < len`, issue
   `madvise(MADV_COLLAPSE)` on each uncovered 2 MiB extent; re-verify. Bounded retries (3).
5. If still short and `require_huge`: try `MAP_HUGETLB | MAP_HUGE_2MB` (works only if the
   human reserved a pool: `vm.nr_hugepages` and `vm.hugetlb_shm_group`). Else `Err(NotHuge {
   covered, len })` with the observed coverage — the caller decides. Never silently degrade.

Pools: allocate the index buffer **first** at process start, before anything fragments memory.
Record `PageReport { len, huge_bytes, kind: Thp|Hugetlb|Small, observed_at }`.

## Allocation on macOS

- `mmap` anonymous, 16 KiB-aligned (page size). No THP concept; no cliff observed to 7 GiB
  (MLX evidence — to be re-confirmed with native Metal chase in test T6).
- Bind via `newBufferWithBytesNoCopy` in ≤ `maxBufferLength` chunks; `.storageModeShared`.
  The deallocator block captures an `Arc<MappingOwner>`; `munmap` happens in that owner's
  drop, so it is sequenced after Metal's last release by construction.
- Completion: `Submission::wait` = `waitUntilCompleted` + status check; `Error` status →
  quarantine path.
- Command API: **Metal 3-style `MTLCommandQueue`** with hazard-tracked resources initially.
  `MTL4CommandQueue` ignores hazard tracking and needs explicit barriers/residency; revisit
  when a profile shows encoder overhead matters.

## CUDA specifics

- Record at init: `pageableMemoryAccess`, `pageableMemoryAccessUsesHostPageTables`,
  `directManagedMemAccessFromHost`, `hostRegisterSupported`, `concurrentManagedAccess`.
  Log them in every benchmark output.
- Completion: `Submission::wait` = `cudaEventSynchronize` on an event recorded after the last
  kernel on the producing stream. No extra CPU fence is needed after that (Astra Q1); CPU writes
  before submit are ordered by the submit itself.
- Cross-CPU-thread handoff of a `Buf` uses ordinary Rust `Send` — no atomics protocol in v1.
- `cudaHostRegister` is **off** by default (illegal-access seen once on a THP buffer; not
  reproduced). Opt-in flag for the streaming benchmark only.
- `cudaMallocManaged` is a `Backing::Managed` variant for comparison runs; it cannot satisfy
  `require_huge` and says so.

## What v1 got wrong (for the record)

| v1 | v2 |
|---|---|
| `gpu_ptr(&self)` alongside `as_mut_slice` | lease consumes the Buf; slices only on CPU-owned Buf |
| Drop-synchronising guard | Submission with Arc-held storage; forget = leak, never UB |
| Release MTLBuffer then munmap | deallocator owns the mapping; munmap after final release |
| safe `file()` with mutable view | `unsafe map_file_unchecked`, `Ro` only; startup copy is the main path |
| "no unsafe escapes" via allocator alone | kernel wrappers in `umgpu` are the boundary |
| 95% huge coverage | 100% per-VMA, timestamped; `MADV_COLLAPSE`; explicit `NotHuge` error |
| "≥ 1 G/s" as a test assertion | hardware regression measurement, labeled, separate from correctness |
| Ultra fits in one buffer | runtime query; chunking is a complete design |

## Tests (from review, T1–T6)

- **T1 ownership** — `trybuild` compile-fail cases: slice while leased; `as_mut_slice` on `Ro`;
  clone a lease; use lease after `wait`. Runtime: forget a Submission on a fake delayed
  backend → storage still alive, no UB under Miri for the CPU-side logic.
- **T2 visibility** — CPU fills random per-line patterns, GPU verifies + overwrites, CPU verifies
  after `wait`; every byte; offsets past 4 GiB; all backings.
- **T3 lifetime** — queue a slow kernel, drop every user handle, churn allocations of the same
  size, then `wait` and verify results; exactly-once cleanup counter.
- **T4 THP determinism** — fresh process per trial, random allocation order, coverage per VMA
  before/after collapse and after memory pressure; log compaction counters.
- **T5 ext4 large folios** — immutable test file, aligned RO mapping + `MADV_HUGEPAGE`,
  `FilePmdMapped` for the range, GPU chase rate before/after pressure. Decides whether file-backed
  BAM input ever gets the random-access path.
- **T6 numbers** — repaired benchmarks on an idle box, cold/warm distributions, clocks logged.
