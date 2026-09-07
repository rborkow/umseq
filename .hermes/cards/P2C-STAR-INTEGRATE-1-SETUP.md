# P2C-STAR-INTEGRATE-1-SETUP — Terra — take the 30 GB hash out of the mapping phase; cut coordinator overhead

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` (the measurement this card answers — read it fully),
`bench/evidence/integrate-1-host/perf-3M-self.txt`, then `bench/star-integrate/star_integrate.cpp`
(`setup()` ~478–500, `file_matches()` ~175, `submit_window`, the coordinator/lookup) and
`star_integrate_window.cpp` (`prepare_window`).

## What was measured

Gate (i) passed: byte-identical alignments, 91.5% of inner requests consumed from the GPU.
Gate (iii): 303 s wall vs stock 55 s. `perf`: **44% of samples in `ssir::Sha256::block`** from
`setup()` ← `prepare_window()`, i.e. the first window hashes Genome+SA+SAindex (30 GB,
single-threaded) *after* "Started mapping", with all 20 workers blocked for ~225 s. After
subtracting that, mapping is ~60–65 s vs stock 47 s: `submit_window` 7%, coordinator CAS 5%,
read re-preparation 11%, `lookup` 3%, `memcpy`/`malloc` 5%. The hooks-bypassed arm is +7% CPU.

## Deliverable

1. **Identity binding at load time, cheaply.** Two acceptable designs; pick one, one-line rationale:
   (a) hash in `setup()` invoked from the `STAR.cpp` hook right after `genomeLoad` returns
   (before "Started mapping"), **multi-threaded** (split each array into `runThreadN` ranges,
   hash each, then hash the concatenated digests — define this composite as the identity and
   record the scheme in `CONTRACT.md`); or (b) bind identity as file byte lengths + SHA-256 of
   the first and last 1 MiB + 64 evenly spaced 64 KiB samples of each array, compared to the
   same scheme on the resident bytes. (a) keeps the full-hash contract; (b) is what a shipped
   tool would do. Either way: **setup ≤ 5 s wall**, measured and printed in the sidecar as
   `setup_wall_s`, and it must not be inside the mapping phase.
2. **Coordinator overhead, bounded work only** (do not redesign the scheduler):
   - `submit_window` 7%: find what it's doing per window (moves? allocation? the
     `unordered_multimap` insert per candidate?) and fix the obvious one. Reserve, don't grow.
   - `USI-reads` reallocation: 12 `umem` allocations of ~10 MB per run in the backend
     (`crates/umgpu/ffi/star_integrate.rs`) — keep the owned buffers at high-water mark, don't
     reallocate per batch.
   - Read re-preparation (`convertNucleotidesToNumbers`/`complementSeqNumbers` 11%): inherent
     to lookahead; leave it, but make sure it's done once per read, not once per candidate.
   - The 2 ms underfill timer and the 64k minimum: leave as-is; report batch-size histogram.
3. **Test**: the existing suites plus one that asserts `setup()` runs before the first frame
   is published (RED on current code, where it's lazy in `prepare_window`).
4. Local gates: `cargo clippy/test --workspace`, `python3 -B bench/star-integrate/test_*.py`,
   `cargo fmt`, clang-format. No kernel/transport/umem/parity-checker changes. No SSH/commits.

The orchestrator reruns gate (i) (must still be `PARITY_MATCH`, `gpu_consumed` ≥ 120M) and
`run_timing_host.sh`. Finish with: the identity scheme chosen, the per-item change list, and
what you expect the bypass-arm overhead to be after (2) — as a hypothesis, not a number.
