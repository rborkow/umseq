# Phase 2b design — the first CUDA kernel on the resident table (rev 1, 2026-09-06)

## Where the control arm left the question
`umbam` (CPU, Rayon, `umem`-resident table) is validated at full depth and is the bar:
- 20M BAM (54M records): chain 55 s. 78M sample (76M records): chain ~77 s + QC ~460 s
  (QC parallelization in flight; expect ~90 s).
- Per-stage at 76M records, 20 threads: decode 13 (warm) / 46 (cold), sort 1.9,
  write ×2 ~32, markdup 9, index 2, featureCounts 7.6, genomecov 4, then the QC sweeps.
- Nothing is memory-bandwidth-bound. Compression (two BGZF writes, ~32 s) is the largest
  fixed cost and is not GPU work we'd write (nvCOMP has deflate; that's a library call,
  not a thesis).

The plan's original 2b target — STAR seed search — remains the one stage with a
*structural* GPU advantage (massive batched random access into a 30 GB index, which T6
showed the GB10 does at device speed from THP host memory). But it's 10% of pipeline time,
STAR's front end is the hardest code in the project, and the CPU arm has no equivalent to
compare against without forking STAR. It's the right *second* kernel.

## What the first kernel has to prove
Not "a GPU can do X." The thesis is narrower: **on a unified-memory box, can a kernel
consuming the *same resident table* beat the same work on 20 Rayon threads, with no copy,
no staging, and byte-identical output?** If yes at ≥2× on even one real stage, the UM
story has a concrete, honest datapoint; the memo can extrapolate to the Ultra's 1.2 TB/s.
If no, the memo says so and the CPU chain is still the deliverable.

So the first kernel should be the stage where (a) the CPU version is already good, (b)
the work is genuinely parallel per record with a small reduction, (c) output is exact-
countable, and (d) it touches enough bytes that bandwidth matters. Candidates from the
profile:

| stage | CPU (76M) | shape | GPU fit | why / why not |
|---|---|---|---|---|
| seq/pos duplication histograms | 42 + 32 s (serial now; ~5 s parallel expected) | hash 76M keys, count occurrences, histogram of counts | **good** — hash + sort-and-count is textbook CUB (`DeviceRadixSort` + run-length) | after parallelization the CPU number will be ~5 s; GPU has to be <2.5 s. Plausible: 76M × 16 B keys = 1.2 GB, one radix sort. |
| markdup | 9 s | sort-and-pair on (name_hash, idx), then sort on fragment-end tuples, run-walk | **good** — two radix sorts + segmented reductions; Picard tie-break is stream rank, which a stable sort preserves | the most *valuable* kernel (Picard is 64 task-min in nf-core); byte-identical semantics are fully specified and tested against an oracle. Target: <4 s. |
| coordinate sort | 1.9 s | radix sort of 54–76M (tid, pos, idx) | trivial win but already 2 s | not worth a card on its own; falls out of markdup's sort. |
| genomecov | 4 s | per-tid difference array + prefix scan | good (scan) | small; byte-identical run-merging is fiddly on GPU |
| featureCounts | 7.6 s | interval lookup per block + per-fragment vote | ok — binary search per block is fine on GPU; the vote logic is branchy | second-tier |
| read_distribution / infer_exp / inner_dist | ~30–100 s serial | same interval lookups | same as above | after QC parallel pass |
| decode (record-table fill) | 7–13 s warm | parse 76M variable-length records | poor — serial dependency on record lengths (needs the prefix scan of `block_size`) | the prefix scan itself is a GPU scan, but the parse is byte-fiddly |
| BGZF inflate/deflate | 2 s / ~32 s | nvCOMP | library | Phase 1.5 said inflate isn't a wall; deflate is a library call — measure, don't build |

**Decision: first kernel = markdup, with the duplication histograms as the warm-up.**
Both are sort-and-reduce over fixed-width keys derived from the resident table; markdup
is the one with a Picard oracle, a full-depth golden (17.2M flags), and real pipeline
value. The histograms share the radix-sort machinery and prove the `umgpu` plumbing on
something simpler first.

## `umgpu` — what sits between `umem` and the kernel
From `docs/design-umem.md`: `Buf<Rw>` → `lease()` → `GpuLease` + `Fence`; `Submission`
groups leases; `Poll`/`wait` returns ownership; quarantine on failure. `umgpu` is the
CUDA backend that implements `Fence` over a `cudaEvent`, and provides:
- `Context` (one per device; `cudaSetDevice`, a stream pool).
- `Kernel<T>`-style launch helpers that take `GpuLease<M>` handles and hand the *host
  pointer* to the kernel. **Access path (settled by T6, `bench/RESULTS-T6-summary.md`):
  on GB10 a plain `mmap` + `MADV_HUGEPAGE` host buffer is device-accessible through
  HMM/ATS with no `cudaHostRegister` — 161 GB/s streaming, 2.6 G random lookups/s
  (device memory: 236 / 2.7). `cudaHostRegister` (pinned) gets 196 GB/s streaming but
  the same random-access rate; worth a flag on `Context` to register `umem` buffers on
  lease, measured not assumed. `cudaMallocManaged` gives nothing over THP host and can't
  be smaps-verified. So: `umem` buffer → raw host pointer → kernel. No probe card needed.)
- Scratch allocation through `umem` (per T6 §2: never `cudaMalloc` for anything large).
- CUB via a thin C++ shim (`cub::DeviceRadixSort::SortPairs`, `DeviceRunLengthEncode`,
  `DeviceScan`) compiled with nvcc into a static lib linked from Rust (`cc` crate,
  `build.rs`), exposed as `extern "C"` functions taking raw pointers + lengths. Rust owns
  the lifetimes via leases; the shim is dumb.
- No Metal in this phase. The `Backend` trait boundary is the `extern "C"` shim surface;
  a Metal shim implements the same five or six entry points later.

## Kernel plan — markdup (byte-identical to `umbam::mark_duplicates`)
Inputs (all from the resident table, already in THP host memory): `RecordHeader[]`
(48 B × N), sorted `order[]`, and for examined primaries the derived
`(name_hash u64, is_read2, idx)` and `(FragmentEnd lo, hi, score, stream_rank)` tuples
that the CPU path already computes. Steps, each a CUB call or a small kernel:
1. Filter examined primaries → compacted index list (`DeviceSelect::Flagged`).
2. Radix-sort `(name_hash, idx)`; run-length on `name_hash`; a kernel walks runs to
   emit pairs (first 0x40 + first 0x80 in idx order, 0x1 set, 0x8 clear) and the
   unpaired list. Hash collisions: verify name bytes on the device (the arena is
   resident too) — a kernel with a byte-compare loop; collisions are rare so divergence
   is fine.
3. Build pair keys `(lo_end, hi_end, !score, min_rank)` as a 256-bit sortable key
   (or two-pass: sort by rank/score then stable-sort by ends); radix-sort; run-length on
   the end pair; first of each run is the keeper; flag the rest.
4. Unpaired: same on `(end, !score, rank)`; a kernel checks membership of `end` in the
   sorted pair-end set (binary search) for the pairs-win rule.
5. Output: a `u8` flag array (or bitset) over record index, plus the six metrics as a
   reduction. The CPU applies the flags to the arena for the write, exactly as now.
Gate: identical `duplicates` set + metrics to `mark_duplicates` on Tier 0 (the existing
oracle-equivalence test, now three-way), and identical 17,192,968-flag `markdup.bam` on
NA11832_F. Target: markdup 9 s → <4 s including all launches, with the 76M-record table
never copied.

## Measurements that make it a *unified memory* result, not just a GPU result
- **Copy-free**: assert (via `umem`) that no `cudaMemcpy` of the table happens; report
  bytes moved = 0. The comparison arm on a PCIe box would have to stage 3.6 GB of table
  + arena each way.
- **Bytes touched vs time** → achieved GB/s per kernel; compare to T6's 160–236 GB/s
  streaming ceiling and to the CPU's achieved rate on the same stage.
- **Page-size sensitivity**: run once with THP 100% and once with `MADV_NOHUGEPAGE`
  (T6 saw 170× on random access). This is the plot the memo needs.
- **Overlap**: run the GPU markdup while the CPU does the BGZF write of `sorted.bam`
  (independent work on the same table) and see whether either slows — the fabric-
  contention question from the plan.

## Cards (in order; Terra unless noted)
1. ~~P2B-PROBE~~ — answered by T6 (above).
2. **P2B-UMGPU** (terra, then **astra review**): the crate — `Context`, CUDA `Fence`,
   the CUB shim with `build.rs`, and one end-to-end test: lease a `Buf`, radix-sort it on
   the GPU, `wait`, verify on CPU. Tier 0-sized synthetic data; runs on the Spark only
   (`#[cfg]`-gated), so the card deliverable is "builds on Mac without CUDA (stub
   backend), and the orchestrator runs the test on the Spark."
3. **P2B-DUPHIST** (terra): seq/pos duplication histograms on GPU via `umgpu`; gate =
   byte-identical `.xls` to the CPU path on Tier 0; benchmark at 76M.
4. **P2B-MARKDUP** (astra — Picard semantics are correctness-critical): the kernel plan
   above; three-way oracle test; full-depth flag identity.
5. Then the measurements section, orchestrator-run, into `bench/PHASE2B-*.md`.

Seed search (the original 2b) becomes **2c**, after this lands, if the ≥2× gate passes.

## P2B-UMGPU result (2026-09-06, Spark)
`crates/umgpu` builds on both boxes (stub backend on the Mac) and the GPU test passes on
the GB10: 16M-key `(u64, u32)` CUB radix sort over `umem` buffers through lease →
submit → `CudaFence::wait`, verified on CPU. Device props: `pageableMemoryAccess=1`,
**`pageableMemoryAccessUsesHostPageTables=1`** (ATS — the GPU walks the CPU page tables,
which is why THP vs 4K is a 170× effect), `hostRegisterSupported=1`,
`concurrentManagedAccess=1`, sm_12.1, 48 SMs.

`inc_u64` over a 1 GB `umem` buffer (read + write, so 2 GB moved): **83 GB/s** with the
plain HMM/ATS path; **5.4 GB/s with `cudaHostRegister`** — registration is 15× slower on
this hardware, not merely unnecessary. `host_register` stays default-off and the memo
should say so. The 83 GB/s is below T6's 161 GB/s streaming read; a read+write kernel on
a single 1 GB buffer with no launch tuning is a lower bound, not a ceiling — the markdup
kernel card measures bytes-touched/time properly.

## P2B-DUPHIST result (2026-09-06, Spark, 20M BAM = 53.9M records)
First kernels on the resident table, `umbam chain --qc --gpu`:

| output | CPU 20 threads | GPU | speedup | identical |
|---|---|---|---|---|
| seq.DupRate | 19.2 s | **2.0 s** | 9.5× | md5 match |
| pos.DupRate | 14.2 s | **1.7 s** | 8.2× | md5 match |

Pipeline: `umgpu_dup_keys` (one thread per record: filter on flag/mapq, FNV-1a over the
same bytes the CPU hashes — sequence nibbles or `(tid, pos, M-blocks)` from the CIGAR with
the RSeQC `fetch_exon` bug — writing `keys[i]`, `vals[i] = i`) → CUB `SortPairs` → back
to the CPU, which walks the sorted runs in parallel and byte-verifies any run with >1
member against the arena so a hash collision can never change the histogram. Table +
arena are leased, never copied (`umgpu::stats::bytes_copied() == 0` asserted). The GPU
time includes the four `umem` allocations (keys, vals, sorted ×2, temp ≈ 1.5 GB), the
lease/submit/fence round trip, and the CPU run-walk.

The version that returned sorted keys into a serial `HashMap` on the host was *slower*
than the CPU path (12.8 / 19.3 s): the win is only real if the consumer respects that
the data comes back sorted. Worth stating as a rule for every kernel card: **the GPU's
output order is part of the contract; the CPU side must exploit it, not rebuild it.**

This is a *unified-memory* datapoint in the sense the memo needs: the input is 3.6 GB of
CPU-built records that the GPU consumed in place. On a PCIe box the same kernels would
first stage 3.6 GB across the bus (~0.3 s at 12 GB/s each way, before pinning costs) and
the histogram would then be a 2 s GPU job either way — so here the UM advantage is
convenience and the absence of a copy, not a bandwidth win. The bandwidth question is
markdup's (larger working set, two sorts, random access into the arena for verification).
