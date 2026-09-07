# P2C-INTEGRATE-V2-T3B — Terra — switch the window and consumption to the V2 (prefix-on-device) contract; strict at the outer boundary

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-v2-t3.md` **in full — its "Exact handoff to Terra" section is your
spec**, then `bench/PHASE2C-integrate-1.md` "Round 5a" (where the numbers stand),
`bench/star-integrate/prefix_config.hpp`, `seed_probe_abi.h` (V2 records), and the
generator `make_star_integrate.py`. You are the only worker; you own everything under
`bench/star-integrate/` and `crates/umgpu/ffi/star_integrate.rs` (V1 side). **Do not edit
the kernel, `star_prefix.rs`, `prefix_oracle.rs`, `prefix_replay.rs`, or the V2 ABI.** No
SSH, no commits.

## Where we are

Round 5a: hooks-bypassed arm −0.2% vs stock, GPU arm mapping-phase **+2.3%** with 96% of inner
searches on the device, parity identical (fifth time). The CPU still does STAR's *outer* work
for every served request (`ind1`, SAindex walk, branch selection): `maxMappableLength2strands`
self ~40 CPU-s at 20M, plus the window's own `append_prefix_call` copy of it. T3 put that walk
on the device behind a V2 contract. This card makes the window use it.

## Deliverable

1. **Context switch.** Coordinator uses `UsiPrefixContext` via `usi_init_v2 /
   usi_search_batch_v2 / usi_destroy_v2`; `config = probe_config_v2(mapGen, P.seedSearchLmax,
   identity.sai_file_bytes)` built once from the **loaded** Genome after `genomeLoad`. 88-byte
   request slots, 48-byte output slots. Remove the V1 context from the integrated path (keep
   V1 code compiling; a build flag is fine).
2. **Window generation.** For each initial-start candidate fill
   `q.inner = {1, s0, s1, read_len, piece_start, piece_length, 0, 0, 0, dir_r}; q.distance = 0`.
   **Delete** the CPU `ind1` build, SAi reads, and the prefix-only/unique filtering in
   `append_prefix_call` — the window no longer decides the branch. Keep the sparse /
   `seedSearchLmax` / distance admission and the continuation fallback.
3. **Consumption at the outer boundary.** Regenerate the hook in
   `ReadAlign_maxMappableLength2strands.cpp` to sit **above** STAR's `ind1`/SAi block: on a
   positional hit with `status == 0`, assign `maxL, Nrep, indStartEnd[0], indStartEnd[1]`
   from `o.inner` and skip straight to the existing `maxL+iDist > maxLbest` bookkeeping and
   the single original `storeAligns` — exactly once, unchanged. On miss / non-zero status,
   stock code runs untouched. Count `o.branch` 1/2/3 separately (`prefix_only`, `unique`,
   `searched`) in the sidecar.
4. **Strict mode, fixed.** The current strict check calls only `maxMappableLength` — that no
   longer covers what the device does. Strict must run **stock's full outer block** (`ind1`
   → walk → branch → search) on the CPU and compare all four outputs `(maxL, Nrep,
   indStartEnd[0], indStartEnd[1])` against the device's before consuming. Any mismatch →
   `strict_fail` (fail-closed, as today). Add `read1` byte-equality to the check while you're
   there (the frame hand-off is on the same path).
5. **Config dump for the replay.** Call `write_probe_config_v2(path, config)` once after
   config construction when `STAR_INTEGRATE_CONFIG_DUMP=<path>` is set; nothing otherwise.
   The orchestrator uses it to run Astra's 999,914-request replay.
6. **`run_timing_host.sh` / `run_host.py`:** no changes needed unless the sidecar schema
   check needs the new counters — if so, add them to the schema rather than loosening it.

Tests: coordinator suite (positional, linearity, shuffled completion) against V2 records;
generator test proving the hook lands above the prefix block and that strict calls the stock
outer body; `test_window_prefix` updated for the deleted filtering (the CPU transcription
stays as the strict oracle's reference, not as the generator). Local: `cargo fmt/clippy/test
--workspace`, `python3 -B bench/star-integrate/test_*.py`, clang-format. `TDD.md` lines.

Report: files touched, the sidecar's new counters, and anything in Astra's handoff you could
not do as written (with why) — do not approximate around it.
