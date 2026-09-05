# Card P0-UMEM-REVIEW — adversarial review of the unified-memory allocator design

You are reviewing a design document before any code is written. Be adversarial: your job is to
find the assumptions that will produce silent data corruption, wrong benchmark numbers, or an
API that can't be made safe. Do NOT write code. Do NOT modify any file except creating
`docs/review-umem-astra.md` with your findings.

Read, in order:
1. `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md` — Part 1 (measurements
   and verdict) and the "Language" / "Integration model" sections. Skim the rest.
2. `bench/RESULTS-2026-09-04.md` — raw numbers behind the design.
3. `bench/cuda/bw_paths.cu`, `bench/cuda/chase.cu`, `bench/cuda/chase_thp.cu`,
   `bench/metal/probe_metal.swift` — how the numbers were produced.
4. `docs/design-umem.md` — the document under review.

Hardware facts you can rely on: DGX Spark GB10 (sm_121, CUDA 13.0, driver 580 open kernel
module, Linux 6.17, `pageableMemoryAccess=1`, `concurrentManagedAccess=1`,
`hostNativeAtomicSupported=1`, 128 GB LPDDR5x shared over NVLink-C2C, THP=madvise) and Apple
M4 Pro (24 GB, Metal 4, macOS 27, 16 KiB pages, `maxBufferLength` 13.3 GiB); target platform
also includes a future M5 Ultra 256 GB.

Answer specifically:
- The three "Open questions" at the end of `design-umem.md`, with citations to CUDA / Metal
  documentation or kernel behavior where you can, and a proposed test where you can't.
- Any place the design assumes coherence, ordering, or visibility that the platforms don't
  guarantee — CPU writes → GPU reads, GPU writes → CPU reads, concurrent access, and what
  happens across `munmap`/buffer release.
- Whether the `Buf`/`GpuPtr` API as sketched can be made sound in safe Rust; what invariants
  the `unsafe` boundary must hold; where aliasing rules (`&mut [u8]` while a GPU kernel may
  be in flight) break it and how to restructure.
- The benchmark methodology: anything in the `.cu`/`.swift` sources that would make today's
  numbers unrepresentative (warmup, clock effects, contention from the vLLM process that was
  resident during the 1 and 4 GiB Spark runs, kernel elision, event timing scope).
- The THP order-sensitivity observed on the Spark: plausible causes, and the most
  deterministic allocation strategy without sudo. If `MAP_HUGETLB` is the only reliable
  answer, say so and specify the exact sysctl the human must set.
- File-backed mappings: on Linux 6.17 with ext4, is there a realistic path to 2 MiB folios for
  an mmap'd read-only file that a GPU will random-access? If not, state that the index must be
  copied into anonymous THP memory at startup.

Output: `docs/review-umem-astra.md`, structured as: Blocking issues / Should fix / Nits /
Answers to open questions / Suggested tests. Be concrete; cite line numbers in the design doc.
Keep it under ~250 lines. No preamble.
