# P2C-INTEGRATE-V2-T5B — Terra — borrowed index: the kernel gathers from STAR's own arrays; no second copy

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` **"Round 6"** (the memory timeline is the whole reason for this
card), `bench/PHASE2C-integrate-v2-t4.md` §"Raw-host-pointer sibling for T5 B" (the API Astra
exposed — `seed_probe_v2_raw_host` in `crates/umgpu/src/`, extent rules: `genome.0` must be
STAR's `G-200` with ≥ nGenome+400 readable; SA must cover the final packed load
`((n_sa-1)*(strand_bit+1)/8)+8`; SAi payload with `sai_offset=0`), `docs/design-phase2b.md:54-60`
(T6: plain host memory is device-accessible via HMM/ATS, measured on THP), and
`crates/umseed-probe/src/index.rs:91-213` (`probe_load`, the copy we are removing). You are the
only worker; you own `bench/star-integrate/**`, `crates/umgpu/ffi/star_integrate.rs`,
`crates/umgpu/ffi/star_prefix.rs`'s context/session side, `crates/umseed-probe/**`, and may add
a V3 raw-host sibling in `crates/umgpu/src/` **mirroring Astra's V2 one exactly** (same SAFETY
contract). Kernel/ABI untouched. No SSH, no commits.

## Why (measured)

Round 6 memory timeline (4M slice): STAR loads its 32 GB index; `usi_init` → `probe_load` reads
the same three files into a second 30 GB resident copy; with ~32 GB of page cache from those
reads the box (121 GB − 32 GB HugeTLB) hits **`MemFree` 0 for 50 s** with all workers blocked
in reclaim, then swaps. This has been the "setup 60–90 s" in every round; V3's larger job
records just made it visible. Mapping itself is fine.

## Deliverable — measure, then switch

### Part 1: three-arm probe (the measurement the plan asked for; now unblocked)

`crates/umseed-probe/src/bin/probe_borrowed.rs`: load the index **the STAR way** (plain
`Vec<u8>`/`new char[]`-equivalent, no `umem`), run the **V2** kernel via
`seed_probe_v2_raw_host` over `bench/evidence/.../real-requests.bin` with the T3 config
(`prefix-config.bin`), and assert **every tuple equals the STAR tuple sidecar** (reuse
`prefix_replay`'s comparison — don't weaken it). Arms by flag:
- `--borrowed`: as-is (4K pages expected). Report gathers/s and `AnonHugePages` for the
  index range from `/proc/self/smaps`.
- `--borrowed-madvise`: `madvise(MADV_HUGEPAGE)` on the three buffers **before first touch**,
  then a sequential touch pass; report `AnonHugePages` to prove it took (Spark THP policy is
  `[madvise]`, so this is the only way STAR-style memory gets huge pages).
- `--umem`: today's `probe_load` path (control; must reproduce the T3 replay rate,
  ~9.5e8 gathers/s on V2 thread).
Also report `VmRSS` at the end of each arm. Exact Spark command in your report; the
orchestrator runs it under the lock.

### Part 2: wire it into the integrated STAR (only if `--borrowed-madvise` ≥ 90% of `--umem`)

1. **`usi_init_v2` gains a borrowed mode**: takes `(G-200 ptr, len)`, `(SA ptr, len)`,
   `(SAi ptr, len)` from the loaded `Genome` instead of an index directory; identity check is
   the resident sampled hash over those pointers (both sides sample the *same* bytes now — say
   so in `CONTRACT.md`). `probe_load` is not called. Lengths: use the loaded `Genome` fields
   and the extent rules above; never reconstruct allocations.
2. **`Genome_genomeLoad.cpp` patch via the generator**: after each `new char[]` for G/SA/SAi
   (`Genome_genomeLoad.cpp:272-289`, `PackedArray.cpp:32`) and before the file read,
   `madvise(ptr, len, MADV_HUGEPAGE)`; after the reads complete,
   `posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)` on each index file so the page cache is
   released. Both are no-ops for correctness; parity gate proves it. Generator test asserts
   the hooks land where stated.
3. Batch buffers (reads/requests/outputs/stats) stay `umem` as today.
4. Sidecar: `index_mode: borrowed|resident`, `index_anon_huge_bytes` (from smaps at setup end),
   `setup_wall_s`.

Tests: Part 1 binary has a CPU-only mode on Mac (stub returns Unsupported → assert the error
path, don't fake a rate); generator tests for the madvise/fadvise hooks; existing suites. Local
gates: `cargo fmt/clippy/test --workspace`, `python3 -B bench/star-integrate/test_*.py`
(including `test_abi.sh`), clang-format.

Report: Part 1 binary + flags + Spark command; Part 2 done/not (if not, why, precisely); the
extent arithmetic you used for the three borrowed ranges.
