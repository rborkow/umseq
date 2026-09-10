# P2C-STAR-TWOPASS-LIFECYCLE — engage the integration under nf-core's argv

Model: gpt-5.6-terra. Sandbox: workspace-write. No commit. No ssh. Orchestrator runs host gates.

## What the production gate found (read `bench/PHASE2E-production-gate.md` first)

Under nf-core's real argv the integrated binary ran as **stock**: `admitted()`
(`bench/star-integrate/star_integrate.cpp:511-517`) returns false on `p.twoPass.yes ||
p.sjdbInsert.yes`, so `setup()` never installs; sidecar `mode: cpu-bypass`, `submitted: 0`.
The runner and comparator are done and verified on two stock runs (`run_production_gate.py`,
`--compare namesorted-sam`; normalization documented in the bench doc — do not touch it).

## Invariant, verified by the orchestrator from STAR 2.7.11b source

The index the hook borrows is replaced **twice** per production run, not once:

1. `STAR.cpp:136` `genomeMain.genomeLoad()` → :148-149 `Genome genomeMain1 = genomeMain;
   sjdbInsertJunctions(P, genomeMain, genomeMain1, sjdbLoci)` — fires when `--sjdbGTFfile`
   is given at mapping time (`P.sjdbInsert.yes`), **before pass 1**. This is what tripped
   `admitted()`.
2. `twoPassRunPass1.cpp:15` `Genome genomeMain1=genomeMain;` … :71-73 pass-1 chunks map
   against `genomeMain` … :92 `sjdbInsertJunctions(P, genomeMain, genomeMain1, sjdbLoci)`
   again with pass-1 junctions, then `STAR.cpp:197` builds the pass-2 chunks.

Inside each: `sjdbInsertJunctions.cpp:61` `sjdbBuildIndex(P, Gsj, mapGen.G, mapGen.SA,
SApass1|SApass2, mapGen.SAi, mapGen, mapGen1)`. In `sjdbBuildIndex.cpp`: `G` is extended in
place with junction sequence (`nGenome` grows; the `G1` buffer was allocated with
`genomeInsertL` headroom at `Genome_genomeLoad.cpp`); `SA` is rebuilt into a **new** packed
array — :126 `SA2.defineBits(...)`, :289-290 `SA.defineBits(...); SA.pointArray(SA2.charArray)`
— so `SA.charArray` is a different pointer with a different `lengthByte`; `nSA` changes;
`SAi` is rebuilt (:297-300). `chrStart` does not change (junction sequence is appended past
`chrStart[nChrReal]`). The current `setup()` snapshot (`usi_init_v2_borrowed(g.G - 200,
g_extent, …)`, `sa_extent` check at ~line 1008) is stale after either transition; `same_index(g)`
at ~1002 is what a re-setup must compare against.

## Deliverables

1. **Lifecycle:** make `setup()` re-entrant per index generation. Concretely: `admitted()`
   drops the `twoPass`/`sjdbInsert` refusals; `setup()` records an index identity
   (`g.G`, `g.SA.charArray`, `g.SA.lengthByte`, `g.nSA`, `g.nGenome`); the generated hook
   in `ReadAlignChunk_mapChunk` / `ReadAlign_maxMappableLength2strands` calls a cheap
   `ensure_current(P, g)` that, when the identity differs from the installed one, runs
   `finish()` (drain windows, drop lookahead, release admission — the LAT-CONTRACT-CLOSE
   shutdown path, which is now proven) and `setup()` again against the new buffers. Between
   pass 1 and pass 2 no mapping thread is live (`mapThreadsSpawn` joined at
   `twoPassRunPass1.cpp:73` before :92), so the teardown is single-threaded; the pre-pass-1
   insert happens before any chunk exists. State in a comment where each guarantee comes
   from (file:line). If the hook site cannot see the transition cheaply, install the re-arm
   explicitly at the two call sites via the generator (`make_star_integrate.py`: after
   `STAR.cpp:149` and after `twoPassRunPass1.cpp:92`), keeping the one-pass path
   byte-identical (existing source tests must pass unchanged).
2. **Sidecar:** add `index_generations` (count of setups), and emit the per-generation
   `submitted`/`consumed` so a gate can prove the GPU engaged in **pass 2**, not just pass 1.
   The runner's acceptance already requires `gpu_consumed > 0`; the orchestrator will add
   "generation 2 consumed > 0" on the host.
3. **madvise under two-pass:** `starIntegrateAdviseHuge` is applied at allocation in
   `Genome_genomeLoad.cpp` / `PackedArray.cpp`. After `sjdbBuildIndex` the live `SA` is
   `SA2.charArray`, allocated via `PackedArray::allocateArray` (advised) — confirm from source
   that the new SA and the extended G region are advised, and that `index_anon_huge_bytes`
   is measured on the *current* buffers at finish. If not, fix in the generator.
4. **Tests:** `test_coordinator.py`/`.cpp` — a two-generation scenario: setup, submit and
   consume, swap to a second synthetic index (different pointer/extent), ensure_current →
   re-setup, submit/consume again, finish; assert zero live requests between generations
   and that a stale key from generation 1 is rejected in generation 2 (key mismatch, counted,
   fallback). `test_source_patch.py` — the generated call sites exist exactly once and the
   one-pass output is unchanged.
5. `docs/design-production-gate.md` — replace "Two-pass index lifetime" with the measured
   state and the mechanism you implemented.

## Constraints

- Files: `star_integrate.cpp/.hpp`, `star_integrate_window.cpp`, `make_star_integrate.py`,
  `test_coordinator.{cpp,py}`, `test_source_patch.py`, `test_window_contract.{cpp,py}` if
  needed, the design doc. Not the runner, not the checker, not `run_full_depth.py`.
- Full STAR source for reading: `/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source`
  (407 files, restored from the Spark copy before dispatch).
- Mac toolchain: `python3.13`, `/opt/homebrew/opt/llvm/bin/clang++`,
  `CPATH=/opt/homebrew/opt/libomp/include LIBRARY_PATH=/opt/homebrew/opt/libomp/lib`.
  Run all coordinator/prefix/contract/source tests plus the real-header syntax compile.
- clang-format. The source is the spec.

Finish with: files touched, tests, the mechanism chosen (hook-site check vs explicit call
sites) and why, any generator change to the one-pass path (should be none), and what the
first host round should watch for.
