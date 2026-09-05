# Card P2-UMEM-IMPL — implement the `umem` crate from the v2 design

You are implementing a Rust crate in a fresh Cargo workspace. Read these first, in order:
1. `docs/design-umem.md` — the design you are implementing. Follow it exactly; where it is
   silent, choose the simplest sound option and note it in a `// DESIGN:` comment.
2. `docs/review-umem-astra.md` — the adversarial review that produced v2; the "Blocking
   issues" section explains *why* the ownership model is shaped as it is.
3. `bench/RESULTS-T6-summary.md` — the measured behavior the crate must preserve (THP
   coverage, the cudaMalloc cliff, aligned mmap).

## Deliverables

Cargo workspace at repo root:
```
Cargo.toml                 [workspace] members = ["crates/umem"]
rust-toolchain.toml        stable
crates/umem/Cargo.toml
crates/umem/src/lib.rs     public API exactly as in design §"Ownership model", minus GpuLease/Submission
                           internals that belong to umgpu (define the types + a `Backend`-agnostic
                           `Fence` trait stub so umgpu can plug in later)
crates/umem/src/backing.rs mmap/madvise/munmap; Linux THP path (§"Allocation on Linux"), macOS path
crates/umem/src/smaps.rs   Linux: per-VMA parse of /proc/self/smaps → PageReport; macOS: page size only
crates/umem/src/quarantine.rs  process-wide list of Arc<Backing> that must never be freed
crates/umem/tests/         T1 (trybuild compile-fail), T2 CPU-side visibility round-trip w/ fake backend,
                           T3 lifetime/quarantine, T4 THP coverage (Linux only, #[ignore] unless UMEM_THP_TEST=1)
crates/umem/benches/       criterion bench: alloc+populate+verify 1 GiB, both Anon{huge} and Hugetlb
```

## Hard requirements
- `#![forbid(unsafe_op_in_unsafe_fn)]`, `#![deny(missing_docs)]`. Every `unsafe` block has a
  `// SAFETY:` comment naming the invariant from design §"The unsafe boundary".
- `Buf<Rw>`: `Send + !Sync`. `Buf<Ro>`: `Send + Sync`. Enforce with `static_assertions`.
- `Buf::lease(self, ...)` consumes; there is no way to obtain a slice while leased. Prove it with
  a `trybuild` compile-fail test.
- Forgetting a `Submission` must not run any Drop that returns ownership. Test with a fake
  `Fence` that never completes: `mem::forget` → the `Arc` strong count stays ≥ 1 forever.
- Linux allocation: over-allocate by 2 MiB, align base up, `munmap` head/tail (never
  `MAP_FIXED`), `madvise(MADV_HUGEPAGE)` **before** first touch, populate with
  `MADV_POPULATE_WRITE` (fallback: touch one byte per 4 KiB), then verify via smaps for the
  exact VMA; if `AnonHugePages < len` → `madvise(MADV_COLLAPSE)` per uncovered 2 MiB extent,
  re-verify, bounded to 3 rounds. `require_huge` and still short → `Err(NotHuge{covered,len})`.
- `Backing::Hugetlb`: `MAP_HUGETLB | MAP_HUGE_2MB`; on ENOMEM return a distinct error naming
  `vm.nr_hugepages`.
- macOS: `mmap` anonymous, 16 KiB alignment, no THP concept; `PageReport{kind: Small, huge_bytes: 0}`.
- `Buf::<Ro>::map_file_unchecked` is `unsafe fn`, doc comment states the no-external-writer contract.
- No CUDA or Metal code in this crate. No `cudarc`/`objc2-metal` deps. `libc`, `memmap2` (or raw
  libc mmap), `thiserror`, `static_assertions`, `trybuild`(dev), `criterion`(dev) only.
- `cargo fmt`, `cargo clippy -- -D warnings` clean. `cargo test` passes on macOS (this machine).
  Linux tests must compile under `cargo check --target aarch64-unknown-linux-gnu` if that target
  is installed (`rustup target add aarch64-unknown-linux-gnu`); if the target can't be added
  without network, note it and gate Linux code with `#[cfg(target_os = "linux")]`.

## Constraints
- Do not modify anything under `docs/`, `bench/`, `scripts/`, `.hermes/`, or `KANBAN.md`.
- Do not touch the DGX Spark (`Sparky`). All work is on this Mac.
- Do not `git commit`.
- Format with `cargo fmt` before finishing; no dense one-liners.

Finish with: what's implemented, `cargo test` output summary, any design ambiguities you resolved
(list the `// DESIGN:` comments), and what remains for the Linux-side verification I will run on
the Spark.
