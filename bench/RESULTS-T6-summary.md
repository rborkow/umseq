# T6 — repaired benchmarks on idle Spark, 2026-09-05 00:26 EDT

Raw: `bench/RESULTS-T6-spark.txt`. Binaries: repaired `bench/cuda/*.cu` (unconditional outputs,
host checksums, randomized class order, per-VMA smaps, 2 MiB-aligned mmap, 1-chain guard, 64-bit
indices). Box idle (nf-core smoke finished, load 2.9 falling), 32 GiB HugeTLB pool reserved.

## 1. THP is deterministic once the mmap is 2 MiB-aligned

`chase_thp`, three seeds, three allocation orders, 4 GiB and 12 GiB:

| class | THP VMA coverage | Mlk/s @1M threads |
|---|---|---|
| `MADV_HUGEPAGE` (aligned, advise before touch) | **100.0% ×3** | 2627–2631 |
| `MADV_HUGEPAGE` + populate + `MADV_COLLAPSE` | **100.0% ×3**, 0 collapse errors | 2587–2631 |
| `MADV_NOHUGEPAGE` control | 0.0% | 15–16 |
| `cudaMalloc` reference (chase.cu) | — | 2744 |

The order-sensitivity seen yesterday is gone. Likely causes were the unaligned first/last 2 MiB
of the old mappings and the global `/proc/meminfo` measurement. THP host memory is
**indistinguishable from device memory** for random access. HugeTLB pool remains belt-and-braces.

## 2. `cudaMalloc` is not immune to the cliff on GB10

`bw_paths` seed 1 allocated classes in order managed → plain → pinned → **device (4th, after
12 GiB of others)**; seed 2 allocated device first.

| | device gather, allocated 4th | device gather, allocated 1st |
|---|---|---|
| time for 6.3M lookups | 736 ms (≈ 8.5 M/s) | 2.3 ms (≈ 2.7 G/s) |

Same binary, same size, 300× apart. On GB10 `cudaMalloc` is system memory and evidently subject
to the same small-page backing when physical memory is fragmented by prior large allocations.
**Design consequence:** "allocate the index first and verify backing" applies to *every*
allocator, not just mmap. `umem` should own all large allocations, including what would
otherwise be `cudaMalloc`, and prefer THP/HugeTLB-backed host mappings (which we *can* verify via
smaps) over `cudaMalloc` (which we cannot).

## 3. DVFS: the first ~100 ms of a session are not representative

Seed 1 started with the GPU at **208 MHz / 4.9 W** (idle); seed 2 at 2411 MHz / 11 W.

| streaming GB/s | seed 1 (cold clocks) | seed 2 (warm) |
|---|---|---|
| device | 33 | **236** |
| pinned | 168 | **196** |
| plain malloc HMM | 31 | **161** |
| managed | 154 | **162** |

Seed-2 numbers are the reference. Every benchmark and every `umgpu` throughput measurement needs
a clock-warming kernel (~1 s) before timing. Worth adding `nvidia-smi` clock logging to the
harness permanently, as Luna did here.

## 4. Latency, now measured with one real chain

| class | 1-chain ns/hop, 16 MiB WS | 4 GiB WS |
|---|---|---|
| CPU (single thread, 1 GiB) | 133 | — |
| `cudaMalloc` | 138 | 167 |
| managed | 136 | 140 |
| plain malloc THP-eligible | 140 | 1019 (4K cliff) |
| pinned (`cudaMallocHost`) | **379** | 1863 (4K cliff) |

GPU dependent-load latency to LPDDR5x equals CPU latency (~135 ns) — the fabric adds nothing.
Pinned memory is ~2.8× worse even when small, which suggests it's mapped uncached/write-combined;
avoid `cudaMallocHost` for anything read repeatedly. At the 4K cliff a hop costs 1–1.9 µs: a
page-table walk per access.

## 5. Not run

`bw_paths 16384` was killed (earlyoom): 6 × 16 GiB buffers + 32 GiB HugeTLB reservation exceeds
the box. Re-run with fewer classes if a 16 GiB streaming number is ever needed; 4 GiB results
already sit at the bandwidth ceiling.

## Updated headline numbers (Spark, idle, warm clocks, 4 GiB)

- Streaming: device 236 GB/s, pinned 196, managed 162, plain-malloc 161, CPU memcpy 55.
- Random access: 2.6–2.7 G lookups/s for device / managed / THP host; 15 M/s for 4 KiB pages.
- Dependent latency: ~137 ns/hop, same as CPU.
