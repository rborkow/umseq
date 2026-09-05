## Blocking issues

1. **The sketched API does not enforce exclusive access or completion** (design lines 21–22, 43–47).
   `gpu_ptr(&self)` returns an unbounded capability: after submission, safe callers could obtain
   `as_mut_slice`, read `as_slice` while the GPU writes, or drop the allocation. A GPU read
   conflicts with a live exclusive CPU slice; a GPU write conflicts with either CPU slice.
   Coherent caches do not repair Rust aliasing violations. `UnsafeCell` alone does not permit
   races or overlapping `&mut` access. See the [Rust undefined-behavior rules](https://doc.rust-lang.org/reference/behavior-considered-undefined.html).
   Replace this with CPU-access guards and an owned submission state: submission consumes the
   affected buffers/range leases; successful completion returns CPU-accessible ownership.
   Concurrent read leases are permissible only while all writers are excluded. Initially,
   exclude whole buffers; add disjoint-range ownership only with checked overlap tracking.

2. **A borrowing future with a synchronizing destructor is still unsound** (lines 21–22, 47, 95).
   Safe Rust can forget a future/guard; its borrow can then end without running `Drop`, while
   the GPU continues using the borrowed storage. Use owned in-flight allocations or a queue
   registry that independently retains storage and excludes CPU access until completion.
   Forgetting a submission may leak, but must never restore CPU access or free live storage.
   `GpuPtr` must be an opaque, device/context-bound, bounded capability, not a clonable escape
   hatch to a raw pointer or independently usable Metal handle.

3. **Releasing one Metal reference is not GPU completion or final resource destruction**
   (lines 76–78, 95). A command buffer, another handle, or an autorelease pool may retain the
   resource. A no-op deallocator plus immediate `munmap` can leave a live `MTLBuffer` over
   reused memory. Keep the backing owner alive through every encoded and submitted use;
   coordinate final buffer destruction with backing release, including all chunks. Apple's
   [no-copy contract](https://developer.apple.com/documentation/metal/mtldevice/makebuffer%28bytesnocopy%3Alength%3Aoptions%3Adeallocator%3A%29?changes=__2)
   explicitly provides the deallocator for this lifetime relationship.
   CUDA likewise requires completion of **all** users before unregistering and unmapping;
   VA invalidation is not an application-level join. A stale address can fault or address a
   new allocation. CUDA's [file-mapping example](https://docs.nvidia.com/cuda/archive/13.0.0/cuda-c-programming-guide/index.html#file-backed-unified-memory)
   synchronizes before `munmap`. If failure leaves completion uncertain, quarantine storage;
   do not return it to an allocator. Do not call a blocking wait from its own completion handler.

4. **Read-only files cannot implement this unconditional mutable-view API** (lines 32, 44–46).
   Split read-only and writable types/capabilities, including GPU kernel write permissions.
   More fundamentally, a safe `file(path)` returning ordinary slices cannot guarantee safety
   against another process modifying/truncating that inode. `MAP_PRIVATE` is not an immutable
   snapshot; keeping an fd open and using advisory locks does not prevent external writes.
   [memmap2 deliberately makes file-backed constructors unsafe](https://docs.rs/memmap2/latest/memmap2/struct.MmapOptions.html#safety).
   Choose an owned anonymous snapshot for the safe API, an actually enforced immutable backing,
   or an explicitly unsafe mapping contract. Hashes detect some changes; they do not prevent UB.
   For writable files, specify private scratch versus shared persistence: GPU completion is
   not a file flush, and private writes do not update the file. [mmap semantics](https://man7.org/linux/man-pages/man2/mmap.2.html).

5. **The unsafe boundary must include kernel behavior, not just allocation** (lines 21–22, 47).
   Safe launch of arbitrary CUDA/MSL cannot promise memory safety. Reviewed kernel wrappers
   must establish bounds, permissions, element alignment/layout, initialized input bytes,
   initialized output extent, absence of internal races, and nonoverlapping `restrict` arguments.
   Also require live mappings/handles/context, checked sizes and offsets, and correct dependencies
   across every queue. Generic foreign-kernel launch requires an unsafe contract somewhere;
   “no unsafe escapes” is achievable only by restricting safe callers to validated operations.
   Audit any `Send`/`Sync` implementations and raw-pointer provenance across CPU guard lifetimes.

6. **95% huge coverage is not a useful guarantee for random access** (lines 13–15, 55–56, 91).
   Five percent of a 30 GiB index is 1.5 GiB of potentially pathological translations. As an
   illustration, weighting the measured 2.3 G/s and 12 M/s costs gives about 218 M/s, not 1 G/s;
   this is a warning model, not a prediction under GPU concurrency. A hot region in that 5%
   could be worse. Require complete verified coverage of the addressable random-access region,
   explicitly handling padding/tails. Treat THP verification as a timestamped observation,
   since later splitting/reclaim can invalidate the performance assumption.

7. **The one-buffer Ultra commitment has no evidence** (lines 79–87).
   `probe_metal.swift:13–23,34–37` only creates handles and compares CPU pointers; it executes
   no GPU command, touches one byte per 16 MiB, and does not establish a reliable final-release
   ordering before `munmap`. This proves neither full residency nor high-offset GPU correctness.
   A single observed `maxBufferLength` does not establish a RAM-scaling formula for M5 Ultra.
   Query limits at runtime; chunking must be a complete kernel addressing/binding design with
   checked 64-bit logical offsets, not a placeholder in `GpuPtr`. Allocation can fail below
   the advertised limit. The [Metal buffer API](https://developer.apple.com/documentation/metal/mtldevice/makebuffer%28bytesnocopy%3Alength%3Aoptions%3Adeallocator%3A%29?changes=__2)
   also requires one VM region and page-aligned address/extent; adjacent mappings are insufficient.

## Should fix

1. **Benchmark correctness must precede numerical conclusions** (lines 10–15, 63, 93–94).
   In `bw_paths.cu:24–31`, 64 unsigned 32-bit values cannot sum to `UINT64_MAX`: the only
   observable branch is impossible, allowing the gather kernel's loads to disappear entirely.
   `read_sum:14–20` also uses a conditional sink, and line 118 prints a literal zero rather
   than checking device output. Do not assert that streaming loads actually disappeared without
   inspecting generated SASS; require unconditional output and CPU verification in either case.
   The unused CPU `memcpy` destination (115–116) permits elision; chase outputs are also unchecked.

2. **The chase “1T latency” is 128-thread block runtime divided by hops**
   (`chase.cu:18–22,36–45,78`), because block rounding has no active-thread guard. It measures
   multiple chains with contention and scheduling, not a single dependent load latency. The
   64K/1M launches have the intended counts. The eight-hop warmup is not a full working-set
   prefault; repeated timed runs follow identical chains. Both chase sources truncate element
   count to `uint32_t`; at 16 GiB it becomes zero, causing underflow/out-of-bounds cycle building
   and modulo-zero in kernels. Reject that size or change the representation before scaling.

3. **Timing scopes and execution conditions are mixed.** `bw_paths.cu:59–64,96–108` reports
   best-of-five even for “cold”; it does not report the first-fault run. Prefetch is explicitly
   completed outside timing. Pageable copies include host API/synchronization time, while
   pinned/device paths use CUDA events. Events measure a stream interval, not complete host
   setup, allocation, page population, or application latency. Idle gaps and competing work
   can also affect that interval. Report first-use wall time, steady-state device time, and
   steady-state wall time separately, with warmup and distributions rather than only minima.
   Fixed class order, sparse initial warmup, long CPU cycle construction, DVFS and temperature
   can correlate clock state with memory class. Randomize order and log clocks/power/thermals.
   The 1/4 GiB Spark runs had ~100 GiB of vLLM residency; even idle residency changes available
   contiguous memory, reclaim and THP success. Active inference additionally competes for SMs
   and bandwidth. The 12/16 GiB runs are a different condition; the 46 GB/s anomaly is not
   proven to be contention. Rerun all sizes on an idle box, then test controlled contention.
   A “4 GiB working set” can hold five whole-size copies including `chase`'s permutation.

4. **THP attribution is weaker than the results table suggests** (lines 64–70).
   `chase_thp.cu:42–62` measures machine-wide `/proc/meminfo` deltas across initialization of
   another mapping and a temporary permutation. It does not measure the tested VMA. Mapping
   addresses are not explicitly 2 MiB-aligned, mapping failures are unchecked, the control
   lacks `MADV_NOHUGEPAGE`, and the second `madvise` return is ignored. The two `mlk` calls in
   line 77 have unspecified argument evaluation order. Correlate each run with its own mapping,
   not process order or global counters. The recorded registered-THP illegal access remains
   an unresolved correctness failure: disable that optional path pending an isolated reproducer.

5. **Use the strongest practical unprivileged allocation attempt, and admit failure**
   (lines 64–66). Reserve extra anonymous VA, select a 2 MiB-aligned extent, and trim only owned
   excess; do not overwrite arbitrary mappings with `MAP_FIXED`. Advise before sequential
   write-population, then synchronously request `MADV_COLLAPSE` for uncovered aligned extents,
   check every result, and verify before registering/submitting. Use bounded retries and
   reuse a pool allocated before other large workloads. Plausible order effects include physical
   fragmentation, competing residency, temporary arrays, alignment, compaction policy and
   delayed khugepaged. [MADV_COLLAPSE](https://www.man7.org/linux/man-pages/man2/madvise.2.html)
   can perform synchronous reclaim/compaction but is best-effort, not a permanent hugepage promise.
   A reserved HugeTLB pool is the strongest backing-size guarantee among these options; THP
   can succeed without sudo, so `MAP_HUGETLB` is not the only workable allocation mechanism.
   For predictable reserved capacity, the human sets `vm.nr_hugepages=N` (15360 pages for
   30 GiB at 2 MiB/page, plus concurrent huge allocations) and `vm.hugetlb_shm_group=<allowed-gid>`
   for unprivileged `MAP_HUGETLB` use. Verify actual reservation and `Hugepagesize`; if the default
   differs, reserve via `/sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages` instead. Request
   `MAP_HUGE_2MB`, preserve reservations, and prefault before handoff. Fragmentation can prevent
   pool creation; early/boot reservation may be necessary. [HugeTLB administration](https://docs.kernel.org/6.17/admin-guide/mm/hugetlbpage.html),
   [MAP_HUGETLB permissions](https://man7.org/linux/man-pages/man2/mmap.2.html).

6. **“Pinned” and “HMM” are not sufficient platform descriptions** (lines 33, 38, 61–63).
   Record `pageableMemoryAccessUsesHostPageTables`, `directManagedMemAccessFromHost`, and
   registration support. `pageableMemoryAccess=1` alone does not distinguish hardware page-table
   access from software HMM. Under host page-table access, `cudaHostRegister` populates pages
   without page-locking them; `cudaMallocHost` may also not page-lock. Thus registration is not
   a way to freeze THP layout. Check registered-pointer identity/capabilities rather than assume
   it universally. [CUDA 13 memory API](https://docs.nvidia.com/cuda/archive/13.0.0/cuda-runtime-api/group__CUDART__MEMORY.html).
   Managed allocation/advice specifies no 2 MiB contract and cannot implement `require_huge` alone.

7. **Metal performance evidence is MLX evidence, not mmap-buffer evidence** (lines 11–12, 85, 94).
   The raw results name `bw_mlx.py`; `probe_metal.swift` supplies no bandwidth measurements.
   MLX uses warmed, evaluated arrays; its independent gather materializes 64M results and reads
   an index array, unlike CUDA's per-thread dependent chase. Its 64-step chase builds repeated
   `take` operations rather than demonstrating one fused native loop. Host wall time includes
   framework work and synchronization. CPU sum/gather gets a single measurement; NumPy's
   unsigned sum also uses a different accumulator width from the `uint32` GPU sum. Match
   arithmetic, access pattern, output traffic and timing boundaries before comparing rates.
   “No cliff to 7 GiB” does not prove page-table sharing, file-backed behavior, or Ultra scaling.

## Nits

- Lines 10–11 versus 69: define “one allocation” as the CPU/GPU working representation;
  explicitly allow startup file loading and VM migration. Literal “no copies, ever” conflicts
  with the fallback and with copying during THP collapse.
- Line 69: 49 GB/s counts **read plus write**; 30 GB copied at that rate takes about 1.22 s,
  not 0.6 s, even before disk I/O, allocation, zeroing and faults. That baseline also needs repair.
- Lines 27, 52, 76: track logical length, mapped length, base address and offset separately;
  check rounding/overflow, zero lengths and `isize::MAX` slice limits. Never expose file padding
  past EOF to kernels. Managed allocations need initialization and `cudaFree`, not `munmap`.
- Lines 48, 52, 65: `smaps` reports whole VMAs, which may merge; do not prorate a merged VMA's
  huge bytes to infer subrange coverage. Separate `AnonHugePages`, `FilePmdMapped`, and
  `Private_Hugetlb`/`Shared_Hugetlb`; a scalar `page_size` cannot describe mixed mappings.
  Folio size, PMD translation coverage and GPU translation behavior are different observations.
- Lines 71, 86–87: budget live inputs, outputs, scratch, page cache, reservations and other
  processes. Neither ~110 GB nor 256 GB makes memory pressure a solved problem; buffer size
  limits and recommended total GPU working set are distinct constraints.

## Answers to open questions

1. **Spark CPU reads after GPU writes (lines 100–102): wait; no extra CPU cache fence is needed
   after successful CUDA completion synchronization.** Use device synchronization, the actual
   producer stream, or an event recorded after its final write; one stream's event does not
   cover unrelated writers. Finish CPU writes before submitting GPU reads, with ordinary host
   synchronization if another CPU thread submits. CUDA 13 documents this handoff in its
   [unified-memory examples](https://docs.nvidia.com/cuda/archive/13.0.0/cuda-c-programming-guide/index.html#um-unified-memory-programming-hd).
   Native atomic support does not order ordinary racing accesses. Persistent CPU/GPU protocols
   need compatible system-scope atomic release/acquire operations, supported widths/alignment
   and matching host semantics; device-scoped atomics, `volatile`, and a lone CPU fence do not
   suffice. Keep this out of the initial safe buffer API. [CUDA atomic memory model](https://nvidia.github.io/cccl/unstable/libcudacxx/extended_api/memory_model.html).

2. **Metal Shared (lines 103–105): yes, CPU reads/reuse must await the relevant GPU work.**
   Finalize CPU writes before commit; after completion, Shared needs no Managed-style
   `didModifyRange` or blit synchronization. Wait for completion, not scheduling, and check
   command status/error before accepting output. [CPU→GPU ownership](https://developer.apple.com/documentation/metal/synchronizing-cpu-and-gpu-work),
   [GPU→CPU example](https://developer.apple.com/documentation/metal/culling-occluded-geometry-using-the-visibility-result-buffer?changes=l__3&language=objc),
   [completion wait](https://developer.apple.com/documentation/metal/mtlcommandbuffer/waituntilcompleted%28%29?changes=_5).
   Also specify GPU→GPU dependencies: Shared storage is not a barrier. Traditional tracked
   resources do not solve every cross-queue/indirect/alias hazard; `MTL4CommandQueue` does not
   honor resource hazard-tracking mode. Choose the command API explicitly and implement its
   barriers/events and residency requirements. [Metal resource synchronization](https://developer.apple.com/documentation/metal/resource-synchronization).
   Do not infer portable CPU/MSL atomic interoperability from shared DRAM; use API handoffs.

3. **Linux 6.17/ext4 (lines 106–108): a realistic conditional path exists; “ext4 cannot” is too strong.**
   Version-pinned [ext4 inode setup](https://github.com/torvalds/linux/blob/v6.17/fs/ext4/inode.c#L5178-L5208)
   enables large folios for eligible regular files, excluding data journaling and filesystems
   with verity/encryption features. With 4 KiB pages/blocks and THP configured, its maximum
   permits order 9 (2 MiB); see [page-cache order definitions](https://github.com/torvalds/linux/blob/v6.17/include/linux/pagemap.h#L369-L388).
   An aligned, read-only mapping with `MADV_HUGEPAGE` can request PMD-sized fault readahead;
   [filemap fault/readahead and PMD mapping](https://github.com/torvalds/linux/blob/v6.17/mm/filemap.c#L3222-L3238)
   provide a real implementation path. Existing small cached folios and allocation fallback
   can defeat it. Where `CONFIG_READ_ONLY_THP_FOR_FS` is enabled, no writer has the inode open,
   and other eligibility checks pass, `MADV_COLLAPSE` offers an additional synchronous path.
   Crucially, v6.17's [file eligibility code](https://github.com/torvalds/linux/blob/v6.17/mm/huge_memory.c#L85-L98)
   does **not** require `VM_EXEC`; older descriptions and the generic man page are misleading here.
   [File collapse implementation](https://github.com/torvalds/linux/blob/v6.17/mm/khugepaged.c#L1847-L1863).
   Kernel version and “ext4” alone do not establish this Spark's config/features or GPU reach.
   Until test T5 demonstrates sufficient stable PMD coverage and GPU throughput, the index
   **must be copied/read into verified anonymous THP memory at startup**, with reserved HugeTLB
   fallback if strict backing is required. BAM streaming need not pay a whole-file copy merely
   for page size: benchmark direct mapping against bounded anonymous input/output buffers;
   allocate large-page working memory for later random-access stages. Include loading cost and
   peak duplication, and retain the file-lifetime safety contract from blocking issue 4.

## Suggested tests

- **T1 — Ownership/API:** compile-fail cases for CPU slices during submission, writable access
  to read-only buffers, overlapping mutable ranges and using GPU capabilities after owner
  release. Exercise forgotten submissions, cancellation, panics, failed launches, multiple
  queues and dropped futures; a fake delayed backend should demonstrate retained ownership.
- **T2 — Visibility:** repeatedly CPU-fill random per-page/cross-cache-line patterns, GPU-check
  and overwrite, then CPU-check after producer completion. Cover malloc, anon THP, HugeTLB,
  managed, registered memory and Metal Shared. Poison outputs and check every byte/hash,
  including tails and offsets beyond 4 GiB. Separate CPU threads must use an explicit handoff.
- **T3 — Lifetime:** queue a delayed real kernel, release user handles, churn same-size
  allocations, then verify results and exactly-once final cleanup. Cover encoded-but-uncommitted
  Metal buffers and every chunk. Test illegal lifetime misuse only in isolated unsafe negative
  harnesses; ordinary successful tests cannot prove the absence of use-after-free.
- **T4 — THP determinism:** randomize allocation order in fresh processes; compare advice-only,
  populated-plus-collapse, registered variants and reserved HugeTLB at 4 GiB and index scale.
  Log per-VMA coverage before/after registration and after pressure, collapse errors, mapping
  alignment, memory/compaction counters and measured rate. Repair the control with explicit
  no-huge advice. Reproduce the registered illegal access with checked outputs and CUDA tools.
- **T5 — ext4 decision:** record exact kernel config, ext4 feature/mount flags and base page
  size. Use an immutable, fully written test file with all writers closed; compare aligned
  advised mappings, ordinary mappings and anonymous THP copies. Test both fresh and warm-small-
  folio cache states without global cache dropping. Populate, attempt collapse, inspect
  `FilePmdMapped` for the exact range, and GPU-chase/checksum before and after pressure. Failure
  to obtain large translations or sustained rate selects the startup-copy path, not a slow
  file-backed index disguised as `require_huge` success.
- **T6 — Numbers and limits:** after repairing sinks/counts, inspect generated kernels and run
  matching native CUDA/Metal workloads with cold/warm distributions, quiet/controlled-load
  conditions and full memory accounting. Test actual device limits, padded EOF, chunk-boundary
  accesses and allocation failure. Keep ≥1 G/s as a labeled hardware regression measurement,
   separate from correctness tests; rerun on M5 Ultra. This review ran no hardware experiments.
