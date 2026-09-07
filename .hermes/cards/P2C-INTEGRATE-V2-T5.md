# P2C-INTEGRATE-V2-T5 — Terra — hook floor back to zero; direct-from-STAR-arrays probe (no second index copy)

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` "Round 5a" and "Round 5b", `docs/design-phase2b.md:54-60` (the T6
access-path finding), `crates/umseed-probe/src/index.rs:91-213` (`probe_load`), and
`crates/umgpu/ffi/star_integrate.rs` (V1/V2 `usi_init`). Another worker (Astra) is editing the
kernel, ABI, `crates/umgpu/src/*`, `crates/umgpu/ffi/star_prefix.rs`, `crates/umstar/**`,
`prefix_config.hpp`. **You own:** `bench/star-integrate/star_integrate.cpp`, `star_integrate_window.cpp`,
`make_star_integrate.py`, `star_integrate.hpp`, `usi.h`, `crates/umseed-probe/src/index.rs`, the
setup/identity side of `crates/umgpu/ffi/star_integrate.rs`, and a new probe binary under
`crates/umseed-probe/src/bin/`. No SSH, no commits.

## Item A — hook floor (+1.8% → ≤ +0.3%)

Round 5a's bypass arm was −0.2%. Round 5b's is **+1.8%** (740 vs 726 user): the V2 hook now sits
*above* STAR's prefix block in `maxMappableLength2strands`, and its disabled-path check runs per
call, per `iDist` iteration. Read the generated code in `make_star_integrate.py` for the V2 hook
and make the disabled path a single predictable branch on a `static const bool` hoisted **outside**
the `iDist` loop (one test per function entry, not per iteration), with the enabled path's frame
lookup also hoisted. Same for `set_chain`/`reverse_suppressed`. Target: bypass within 0.3% of stock;
the orchestrator measures.

## Item B — the second index copy (74–90 s startup, 36 sys CPU-s, 30 GB RSS per run)

`usi_init` → `probe_load` reads Genome/SA/SAindex from disk into `umem` THP buffers every run.
STAR already has the same bytes loaded (`Genome_genomeLoad.cpp:272-289`, `PackedArray.cpp:32`:
`new char[]`). T6 established that on GB10 plain host memory is device-accessible via HMM/ATS
without registration — but T6 measured **THP-backed** `umem` buffers, and the P2C probe's 4K-page
control ran at 0.07×. STAR's `new char[30 GB]` gets THP only if the kernel policy is `always` or
the allocation is `madvise`d; the Spark is `[madvise]`. So this is a measurement, not a change to
the integrated path yet:

1. New binary `crates/umseed-probe/src/bin/probe_borrowed.rs`: load the index the *STAR way*
   (plain `Vec<u8>` / `new char[]`-equivalent, no `umem`, no `madvise`), run the existing V2
   kernel over the real request corpus (`real-requests.bin`, the T3 replay path) with G/SA/SAi
   pointing at the borrowed buffers, and report gathers/s and `/proc/self/smaps_rollup`
   `AnonHugePages` for the index range. Three arms in one binary via a flag:
   - `--borrowed`: as above (4K pages expected).
   - `--borrowed-madvise`: same buffers, `madvise(MADV_HUGEPAGE)` **before** first touch + a
     sequential touch pass; report `AnonHugePages` to prove it took.
   - `--umem`: today's path (control; must reproduce the T3 replay rate).
   All three must produce **identical tuples** (assert against the STAR tuple sidecar — reuse
   `prefix_replay`'s comparison; do not weaken it).
2. Under the `umgpu` safety rules: the kernel launch takes raw host pointers; add the borrowed
   path at the FFI boundary with a `// SAFETY:` naming the lease (the caller's index lifetime),
   nothing else `unsafe`.
3. If `--borrowed-madvise` matches `--umem` within 10% on gathers/s: write the exact integration
   change as a request (STAR's `genomeLoad` gets a `madvise` + touch after `new char[]`; `usi_init`
   takes pointers instead of a directory; identity check becomes resident-vs-resident trivially)
   — **as text in your report, not as an edit** — the switch is a separate card because it
   touches STAR's load path under strict parity.
4. If it doesn't match: that's the finding; report the three rates and `AnonHugePages`.

## Finish

`cargo fmt/clippy/test --workspace`, Python suites, clang-format. `TDD.md` line for item A.
Report: item A done/not; item B binary path, flags, and the exact Spark command (the orchestrator
runs it under the lock); anything you couldn't do as written.
