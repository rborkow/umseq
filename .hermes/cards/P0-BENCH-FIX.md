# Card P0-BENCH-FIX — repair the CUDA microbenchmarks per review

Project context: `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md` Part 1. The
three CUDA sources under `bench/cuda/` produced the numbers in `bench/RESULTS-2026-09-04.md`.
An adversarial review (`docs/review-umem-astra.md`, "Should fix" items 1–4) found methodology
defects. Fix them. The target machine is a remote DGX Spark (`ssh Sparky`, aarch64, CUDA 13.0
at `/usr/local/cuda`, `nvcc -O3 -arch=native`); you may compile there to check syntax but
**do not run** the binaries — a large index build is using the box. I will run them.

## Fixes required

`bench/cuda/bw_paths.cu` (already partly fixed — read it first):
- Verify the `gather` and `read_sum` kernels now have unconditional per-thread output and the
  host checks the sums. Keep that.
- Report, per memory class, three numbers separately: first-touch wall time (the very first
  kernel run on a fresh allocation, `steady_clock` around launch+sync), steady-state device
  time (CUDA events, best and median of 5), steady-state wall time (median of 5).
- Randomize the order in which memory classes are tested (seed from argv[2], default
  `time(0)`), print the order used.
- The CPU memcpy destination must be consumed (checksum a few words) so it can't be elided.
- Print `nvidia-smi --query-gpu=clocks.sm,clocks.mem,temperature.gpu,power.draw --format=csv,noheader`
  via `popen` before and after the run, so clock state is on the record.

`bench/cuda/chase.cu`:
- The "1T latency" launch runs a full 128-thread block. Add an active-thread guard so only
  `threadsTotal` threads chase (`if (tid >= threadsTotal) return;`) and pass `threadsTotal`
  in. Rename the column `lat ns/hop (1 chain)`.
- `uint32_t n = bytes/4` overflows at 16 GiB. Use `uint64_t` for counts/indices throughout
  (host and device); reject sizes > 32 GiB with a clear message.
- Warmup must touch the whole working set before timing (one full pass), not 8 hops.
- Write each chase result to a device output array, copy back, and fold it into a printed
  checksum so results are consumed.

`bench/cuda/chase_thp.cu`:
- Measure THP coverage **for the tested VMA**, not machine-wide `/proc/meminfo`: parse
  `/proc/self/smaps` for the range containing the buffer and report `AnonHugePages` for that
  VMA. Print coverage as a percentage next to each row.
- `mmap` a 2 MiB-aligned region: over-allocate by 2 MiB, align the pointer up, `munmap` the
  excess head/tail (never `MAP_FIXED` over foreign mappings).
- The 4K control must use `madvise(MADV_NOHUGEPAGE)`.
- Check every `mmap`/`madvise` return value; abort with `perror` on failure.
- The two `mlk(...)` calls in one `printf` have unspecified evaluation order — sequence them.
- Add a variant row: `MADV_HUGEPAGE` + populate + `madvise(MADV_COLLAPSE)` on any
  uncovered 2 MiB extent (kernel 6.17 supports it; `#ifndef MADV_COLLAPSE #define MADV_COLLAPSE 25`),
  reporting collapse errno if any.
- Randomize buffer allocation order per run (seed from argv), print it.
- Drop the `cudaHostRegister` variant (it faulted once; it's disabled pending an isolated repro).

Shared: create `bench/cuda/common.h` for the `CK` macro, timing helpers, and the smaps parser
so the three files stop duplicating them. Keep each `.cu` self-contained enough to compile with
`nvcc -O3 -arch=native -I. -o X X.cu`.

## Constraints
- Don't touch anything outside `bench/cuda/`.
- Don't run the binaries on the Spark. Compile-check only:
  `ssh Sparky 'cd ~/uni-rnaseq/bench/cuda && export PATH=/usr/local/cuda/bin:$PATH && nvcc -O3 -arch=native -I. -o /tmp/x FILE.cu'`
  after `scp`-ing the files to `Sparky:~/uni-rnaseq/bench/cuda/`.
- No `git commit`.

Finish with a summary of what changed per file and the exact commands to run each binary.
