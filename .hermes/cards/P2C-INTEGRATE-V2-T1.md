# P2C-INTEGRATE-V2-T1 — Terra — the three measured cheap items: setup ≤ 5 s, positional consumption, hot-loop floor

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`.hermes/plans/2026-09-07_153800-star-integrate-v2.md` **including the "Task 0 result"
section at the end** — every number below is measured there, in
`bench/evidence/integrate-v2-perf/{stock,bypass,gpu}.sym.tsv`. Another worker (Astra) is
concurrently editing `crates/umgpu/shim/seed_probe.cu`, `crates/umgpu/shim/seed_probe_abi.h`
and the kernel-side of `crates/umgpu/ffi/star_integrate.rs` (the probe request path). **You own
everything else under `bench/star-integrate/` and the identity/setup code in
`crates/umgpu/ffi/star_integrate.rs` (lines ~140–160, `sampled_hash`, `UsiIdentityV1`).** Don't
touch the probe request struct or kernel. No SSH, no commits.

## Item A — identity setup resident-to-resident (94 CPU-s per run, the largest fixed cost)

Measured: the GPU arm spends **~94 CPU-s before "Started mapping"** at any input size — 40 in
`libcrypto` SHA-256 and ~54 in kernel page-cache copy/smmu/spinlock — because `setup()`
re-reads `Genome`/`SA`/`SAindex` from disk to sample them, and the USI side maps them again.
At 20M that alone is 12% of stock.

Fix: no file I/O at setup. STAR has the arrays loaded (`mapGen.G`, `mapGen.SA` (PackedArray,
`charArray`), `mapGen.SAi`) and the USI context has its own resident copy. Sample both with
the existing sampled scheme (first/last 1 MiB + 64 blocks — keep `CONTRACT.md`'s definition
byte-for-byte so the bound identity is unchanged) directly from memory, and use the
already-known array lengths for the size fields. Replace SHA-256 with a fast non-crypto hash
(xxh64 or a 64-bit FNV over the sampled blocks is fine — this is a consistency check between
two in-process copies, not a security boundary; say so in `CONTRACT.md`). Target: `setup_wall_s`
≤ 5 s, zero `libcrypto` samples, and the coordinator's index mapping (`probe_load`) done once,
before mapping starts (it already is, keep it).

Test: existing setup-precedes-frame test; add one asserting `setup()` opens no files (wrap the
loader; count opens).

## Item B — positional consumption (`lookup` 9.5 CPU-s at 4M ≈ 45 at 20M)

Measured: `star_integrate::lookup` is the largest integration symbol (9.5 CPU-s / 4M) —
hash-bucket lookup + full 22-field key compare per call, plus `submit_window` 2.3 and
`call_hash` 0.6.

The window enumerates a read's candidates in `(piece, dir, istart)` order; STAR's
`mapOneRead` visits initial starts in exactly that order (`ReadAlign_mapOneRead.cpp:43-70`).
So the k-th *initial-start* `lookup` for a read is candidate *k*. Replace the multimap with a
per-frame dense array + cursor: `lookup` reads `frame.jobs[cursor]`, compares the key as an
**assertion** (fast-path compare `start,length,dir,piece,istart` first; full compare only on
mismatch → `positional_miss` counter + CPU fallback, never a search), advances the cursor.
Continuations (`Lmapped > 0`) are not in the window today; they must fall through without
disturbing the cursor (they already `note_cpu_fallback`). Batches become contiguous slices of
a chunk's dense job array; drain writes results at the same index. Drop `unordered_multimap`,
`call_hash`, and per-frame heap allocations for job storage.

Tests: coordinator suite (visit-counter linearity must stay); new: shuffled completion order
across 1,000 synthetic reads, every consumed result lands on its own candidate; strict oracle
unchanged.

## Item C — the bypass floor (+12 CPU-s at 4M, ~+55 at 20M)

Measured: hooks-bypassed arm is +7.0% over stock; `compareSeqToGenome` 44.3 → 50.0 is half of
it — `star_integrate_work::compared(N-L)` patched into the inner loop
(`make_star_integrate.py:34-36`) costs ~6 CPU-s even when counters are "compiled out" (the
call and the `N-L` remain). The rest is `set_chain`/`inner_call`/`reverse_suppressed` on the
disabled path.

Fix: (1) remove `compared()` from `compareSeqToGenome` entirely — compute the byte count at the
generated call site in `maxMappableLength2strands` from the returned `maxL` when
`STAR_INTEGRATE_COUNTERS` is on, nothing otherwise; (2) every generated hook is behind a single
`if (star_integrate::enabled_fast())` that reads a `static bool` once (no function call on the
disabled path). Target: bypass arm within **1%** of stock CPU-s; the orchestrator measures.

## Finish

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `python3 -B bench/star-integrate/test_*.py` (all), clang-format on
every C++ file touched. `TDD.md` one line per item. Report per item done/not-done, and for
each the profile symbol you expect to vanish.
