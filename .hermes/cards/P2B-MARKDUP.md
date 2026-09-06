# Card P2B-MARKDUP — Picard-exact duplicate marking on the GPU, over the resident table

## Context
`crates/umbam` is a one-pass resident BAM chain whose outputs are byte-identical to
samtools/Picard/Subread/bedtools/RSeQC at full depth (78M-pair sample). Its
`mark_duplicates` (in `crates/umbam/src/lib.rs`, ~110 lines, all-Rayon) reproduces Picard
MarkDuplicates exactly — 17,192,968 flags on the full sample, metrics to 5 dp — and runs
in **9 s** over 76M records on 20 threads. A slow reference implementation is kept under
`#[cfg(test)]` (`mark_duplicates_reference`) with an equivalence test.

`crates/umgpu` is the CUDA backend for `umem` (host-resident THP buffers the GB10 reads
in place via ATS; leases + `CudaFence`; a CUB shim). It's proven: the RSeQC duplication
histograms run 8–9× faster than 20 threads on it, byte-identical, zero copies. Read
`docs/design-phase2b.md` (the plan, both results, and the rule that came out of the
first kernel: **the GPU's output order is part of the contract — the CPU consumer must
exploit sorted output, not rebuild a hash map from it**). Read `crates/umbam/src/qc.rs`
→ `gpu_sorted_fingerprints`, `sorted_run_counts`, `position_duplication_gpu` for the
working pattern (lease table+arena, derive keys on device, CUB sort, submit, wait,
parallel run-walk on host). Read `crates/umgpu/shim/umgpu_shim.cu` for the kernel style
and the `RecordHeader` byte offsets the device already uses.

## What must be identical (the oracle is `mark_duplicates`)
Read it in full. In brief:
1. Examined = records with `flag & 0x904 == 0` (primary, mapped, not supplementary).
   Secondary/supplementary → `secondary_or_supplementary` metric; unmapped → `unmapped`.
2. Group examined records by read name: `(name_hash, index)` sorted; within equal-hash
   runs the names are byte-verified and a colliding run is re-sorted by actual name then
   index. A *pair* = the first record with 0x40 and the first with 0x80 in the run (in
   index order) where both have `flag & 9 == 1` (paired, mate mapped). Everything
   examined that isn't in a pair is *unpaired* (including mate-unmapped, single-end, and
   reads whose mate is secondary-only).
3. `FragmentEnd = (tid, unclipped 5′ position, strand)`: forward → `pos − leading S/H`;
   reverse → `pos + ref_len − 1 + trailing S/H`. Score = Σ base qualities ≥ 15 (over
   both reads for a pair).
4. Pairs sorted by `(min_end, max_end, u64::MAX − score, min(rank_a, rank_b), a, b)`
   where `rank` = position in coordinate-sorted order; within each `(min_end, max_end)`
   run, the **first** is kept, the rest are duplicates (both reads). `pair_duplicates`
   += per pair.
5. Unpaired sorted by `(end, u64::MAX − score, rank, index)`; within each `end` run:
   if `end` is in the set of all pair ends → **every** member is a duplicate (pairs
   win); else the first is kept. `unpaired_duplicates` per record.
6. Output: the duplicate index set (→ 0x400 patched into the arena before the markdup
   write) and `DupMetrics { unpaired_examined, pairs_examined,
   secondary_or_supplementary, unmapped, unpaired_duplicates, pair_duplicates }`.

## GPU design
Everything below is CUB calls plus small kernels; all buffers are `umem` (huge: true),
leased for the duration, table + arena leased read-only exactly as `gpu_sorted_fingerprints`
does. Add the kernels to `umgpu_shim.cu` with matching `extern "C"` entries and safe
wrappers in `umgpu/src/cuda.rs` (+ stubs in `stub.rs`).

**K1 — per-record derive** (one thread per record; reads header + arena body): emits
`examined` flag, `name_hash` (already in the header), `is_read2`, `paired_and_mate_mapped`
(`flag & 9 == 1`), `FragmentEnd` packed into a u64 as `(tid:16 | pos:32 signed-offset |
strand:1)` in a sortable layout (**you decide the packing; document it; positions can be
negative after unclipping — bias them**), `score` (needs the qual bytes), `rank`
(inverse of `order[]` — pass `order` in and scatter). Output arrays: `end[i]`, `score[i]`,
`rank[i]`, `flags[i]`.

**K2 — name grouping**: `DeviceSelect::Flagged` on examined → compact indices; radix-sort
`(name_hash, index)` (existing shim); a kernel over sorted positions marks run starts
(`key[i] != key[i-1]`); `DeviceScan` → run ids; a kernel per run (or per element with
atomics — pick the deterministic one) finds first-0x40 and first-0x80 satisfying `flag &
9 == 1` in index order, emits the pair `(a, b)` or marks members unpaired. **Hash
collisions**: a run whose members' names aren't all byte-equal must be split by actual
name — do the byte-compare on device against the run's first member; for the (rare)
mismatch, set a `collision` flag on the run and let the host resolve those runs exactly as
the CPU does. The CPU oracle sorts colliding runs by `(name bytes, index)`; the result must
match, so the simplest correct thing is: host fixes up collided runs, then everything
downstream is identical.

**K3 — pairs**: build the 6-tuple key. Radix sort on a composite: CUB sorts one key type,
so either (a) pack `(min_end, max_end)` into a 128-bit key and do two passes
(sort by the secondary tuple `(!score, min_rank, a, b)` first, then **stable** sort by
`(min_end, max_end)` — CUB radix sort is stable, so two passes give lexicographic
order), or (b) build a u64 surrogate rank via a preliminary sort. Then run-start marks →
keep first, flag the rest. `pair_duplicates` = `DeviceReduce::Sum` of (run_length − 1).

**K4 — unpaired**: same with `(end, !score, rank, index)`; membership of `end` in the
sorted, deduped pair-end array by device binary search (`ends` from K3, sorted + `Unique`).

**K5 — output**: a `u8` flag array over record index (or a compacted index list via
`DeviceSelect`) + the six metrics via `DeviceReduce`. Host builds `MarkdupResult` from
them; `duplicates: HashSet<usize>` is the existing type — populate it from the compacted
list (or change `MarkdupResult` to carry a `Vec<bool>`/bitset if the `HashSet` build is
measurable; the write path consults it per record).

## Gates (all must pass; I run them on the Spark)
- `crates/umbam/tests/gpu.rs`: extend the existing GPU test — run `mark_duplicates` (CPU)
  and `mark_duplicates_gpu` on Tier 0 and assert identical duplicate sets **and** metrics.
  This makes the oracle test three-way (reference / Rayon / GPU).
- `umbam chain --gpu` (the flag now also selects GPU markdup; QC histograms already use
  it) on the 20M BAM: `markdup.bam` dup-flag count and `markdup.metrics.txt` identical to
  the CPU run's. I'll also run the full-depth sample (17,192,968 flags).
- `umgpu::stats::bytes_copied() == 0`.
- Synthetic unit tests for K1's packing (negative unclipped positions, tid boundaries,
  strand ordering must sort the same as the CPU's `FragmentEnd` `Ord`).

## Performance target
CPU markdup is 9 s at 76M records (8.7 s at 54M). Target **< 3 s** including all
allocations, launches, and the host-side result build. Report the per-kernel breakdown
(record `cudaEvent` timestamps between stages) and bytes touched per stage so I can
compute achieved GB/s against the T6 ceilings (161 GB/s stream, 2.6 G random lookups/s).

## Rules
- Mac (this box) has no CUDA: `cargo clippy -p umbam -p umgpu --all-targets -- -D warnings`
  and `cargo test -p umbam -p umgpu` must pass here **without** the `cuda` feature. With
  it, the crate must at least be plausible — I compile on the Spark and will return nvcc /
  rustc errors verbatim; keep the `.cu` standard C++17 + CUB, no exotic features.
- `unsafe` only in `umem`, and in `umgpu` at the FFI boundary with `// SAFETY:` naming the
  lease guaranteeing each pointer. None in `umbam`.
- Don't change `mark_duplicates` (CPU) or `mark_duplicates_reference`; they're the oracle.
- No commits; don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/` or the Spark.

Finish with: the kernel list and extern entries; the key packing spec; how collisions are
resolved; the exact Spark commands; anything you believe the oracle gets wrong vs Picard
(with a concrete case — there's one known divergence in `COMPAT.md`, multi-library sets).
