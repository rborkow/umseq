# STAR follow-through 1–7 implementation plan

> **For Hermes:** Use subagent-driven-development and kanban-codex-orchestration skills to execute this plan. This turn is planning only; do not infer workers or benchmarks were launched.

**Goal:** Close full-depth correctness and review gaps, measure remaining STAR CPU overhead, test salmon huge-page upside, and update the executable cost model without writing a memo.

**Architecture:** Two disjoint-file Codex workers plus an orchestrator-owned Spark measurement lane. Serialize all heavy Spark jobs under the shared resource lock. Freeze each measured build and its inputs; optimization follows measured attribution, not earlier estimates.

**Tech stack:** Rust workspace, generated STAR 2.7.11b C++ integration, CUDA on GB10, Python benchmark/model tooling, Bash launchers, Linux perf/GNU time.

## Verified starting state

- Local main is clean at `89f005c`; code landing is `837c2a3` (PREFETCH). No new implementation has started.
- Spark checked 2026-09-08 08:43 PDT: resource lock free, approximately 86 GiB available, no heavy application in the largest-RSS process list, only spark-searxng container running. This is a point-in-time observation, not a future reservation. Leave vLLM stopped.
- Round-9 raw TSV recomputed: stock 753.9133 CPU-s; advised bypass 661.5467; GPU 618.6533. GPU reduction 6.4838% vs bypass, 17.9411% vs stock. Reaching 8% vs this bypass would require another 10.0304 CPU-s at this workload; this is arithmetic, not an estimated optimization yield.
- Gate host24 comparator actually compares ordered SAM records, SJ.out.tab bytes, and non-timing Log.final.out fields. Its legacy status `PARITY_MATCH_COUNTERS_ONLY` does NOT mean aggregate-count-only comparison. Permitted SAM normalization is executable/output-prefix command fields only. Preserve the checker unchanged.
- Gate invocation uses 20M-pair FASTQs, `--twopassMode None`, `--outSAMtype SAM`, `--outSAMorder PairedKeepInputOrder`. Full input exists under `~/uni-rnaseq/data/samples/ERR188140`; exact pair count must come from verified input evidence, not the earlier shorthand “76M.” BAM records and FASTQ pairs are different units.
- `run_host.py` hardcodes reused 20M stock artifacts and expected count. It cannot simply be pointed at full reads.
- `perf_differential.sh` hardcodes host8/old output, removes its output directory, uses one fixed-order run and shell-split argv, and does not sanitize inherited integration flags. `run_timing_host.sh` still hardcodes host5 locally. Neither is ready to rerun unchanged.
- `make_star_integrate.py` unconditionally emits index-cache eviction in Linux integration builds. Eviction originally reduced memory pressure; removing it is a hypothesis about repeated-sample behavior, not an unconditional production fix.
- `scripts/cost_curve.py` still uses a projected 1.07–1.15× pipeline capacity range. The new STAR data are stage measurements on a slice, not measured full-pipeline throughput. Do not substitute stage reductions for pipeline reductions.
- `KANBAN.md` has duplicate sections, stale v1 status and an expired cutoff. Update only the active STAR lane when executing; historical held cards stay held.
- No salmon source/benchmark files were found by the targeted repo filename search. Spark has both `salmon_k31` and `salmon_nfcore_1.10.3`; do not interchange their formats or quantify the wrong pipeline mode.

## Scheduling and ownership

| Wave | Worker slot A | Worker slot B | Spark, orchestrator only |
|---|---|---|---|
| 1 | Astra: task 3 independent LAT/PREFETCH review | Terra: task 2 eviction switch + isolated task-1/task-5 runner preparation | Verify frozen host24 inputs/builds; run full-depth stock + strict GPU once runner is validated |
| 2 | Terra: task 6 only after task-5 evidence supports a target; otherwise task-7 source experiment | Luna: task 4 model implementation; if task 6 is active, prepare task-7 source/loader inventory first and model after | Task 2 cache-policy screen, then task 5 fresh three-arm differential/timings |
| 3 | Terra: task 7 implementation if not already prepared | Astra or Luna: independent review of newly landed cards, risk-dependent | Task 6 validation/timing if justified, then task 7 screen; collect final evidence for task 4 |

Maximum two workers concurrently; an author never independently approves their own card. Review writes only its named docs, even while code changes proceed. Reviewer pins historical LAT/PREFETCH SHAs and explicitly labels any newer code it inspects. Stagger worker startup while doing useful orchestrator preflight, not by launching a third worker.

The hardware sequence is intentionally serial: full-depth correctness → cache-policy screen → differential/controlled timings → any justified optimization → salmon. Local source discovery, model plumbing and review overlap those jobs. Do not promise a ten-minute full-depth run: input decompression, strict double computation and SAM comparison have not been timed at that depth.

## Task 0 — freeze and repair execution prerequisites

**Owner:** orchestrator, before launches.

**Files:** `KANBAN.md`; new cards `.hermes/cards/P2C-FULLDEPTH.md`, `P2C-CACHE-POLICY.md`, `P2C-LAT-PREFETCH-REVIEW.md`, `P2C-PERF-FINAL.md`, `P2C-COST-STAGES.md`, `P2C-SALMON-THP.md`; task-6 card only after attribution.

1. Record HEAD, dirty paths and live worker ownership; mark new cards Ready, not In Progress before dispatch.
2. Read host24 `preparation.json`, `binary.json`, `backend.json`, `stages/integrated.argv.json`, `stock-reuse.json` and `source-sha256.json`; verify exact reused executables/tooling. Recover commands from those artifacts rather than hand-reconstructing them.
3. Locate verified full-read manifest/count and inspect salmon version, actual nf-core quant argv, index metadata and complete matching source distribution. Inputs remain read-only. Workers must not access `data/`, `runs/` or Spark `~/uni-rnaseq/`.
4. Check Spark lock/other-lane ownership, memory, disk, perf permission and CUDA toolchain again immediately before launch. No service or global THP-policy changes.
5. Stage clean immutable sources into a fresh lab root, excluding build caches. Do not rsync --delete over a shared root. Set a fresh CARGO_TARGET_DIR.
6. Use nohup plus a bounded timeout for the whole owned process tree and the shared flock. Record argv, exit and status per stage; never report success from a wrapper completion marker alone. Separate local gates, commit and launch calls.

## Task 1 — full-depth parity

**Owner:** Terra prepares runner; orchestrator runs/accepts. **Depends on:** Task 0, not task-2 code landing.

**Files:** create `bench/star-integrate/run_full_depth.py`, `bench/star-integrate/test_full_depth_runner.py`, `bench/PHASE2C-full-depth.md`; retain summaries in `bench/evidence/integrate-full-depth/`. Read existing `run_host.py`; do not alter the accepted external checker.

1. Add runner tests first: rejects pre-existing evidence root, mismatched expected pair count, missing/failed stock stage, nonzero comparison exit, missing final sidecar, zero GPU consumption and strict mismatches. Exercise failures with controlled subprocess results, not any-nonzero acceptance.
2. Implement structured argv, fresh stock outputs for the full input, explicit strict-on environment and unchanged comparison invocation. No readMapNumber cap on the accepted full-depth run. Bind expected pair count independently from the measured outputs.
3. First validate invocation/manifest handling on a small real prefix, clearly marked a smoke test, then run full-depth stock and frozen round-9 strict GPU serially. Record resource high-water marks and fallback categories as well as mismatches.
4. Accept only both process exits zero, full ordered SAM/SJ/log comparison pass, exact expected pairs, positive GPU consumption, zero batch faults/rejected results and all strict mismatch counters zero.
5. Preserve failed artifacts without normalization or input filtering. If task 2 or 6 changes the final binary, repeat the full-depth gate for that final candidate; the first run establishes the round-9 baseline only.

**Scope:** larger-input correctness in the already-supported one-pass invocation. It is NOT nf-core two-pass/BAM production integration. That remains a separate future gate, not silently added to items 1–7.

## Task 2 — make file-cache eviction explicit and test the default

**Owner:** Terra. **Files:** `bench/star-integrate/make_star_integrate.py`, `bench/star-integrate/test_source_patch.py`; create `bench/star-integrate/cache_policy_bench.py`, `bench/star-integrate/test_cache_policy_bench.py`, `bench/PHASE2C-cache-policy.md`.

1. Add regression tests proving eviction requires an explicit opt-in (proposed `STAR_INTEGRATE_DROP_INDEX_CACHE=1`), independent of STAR_INTEGRATE_THP and GPU enablement. Test missing, zero and one values; no accidental opt-in from inherited flags.
2. Emit the switch around existing file-drop calls; keep historical behavior available for reproducing prior rounds. Do not change advice sites or kernel/lookup behavior.
3. Run source patch, ABI and neighboring integration tests. Verify generated enabled C++ compilation, not just stubs.
4. Run repeated same-binary eviction on/off pairs with advice held constant. Separate controlled-warm experiments from realistic consecutive-sample runs; record startup/mapping/total wall, user/sys, RSS, swap and reclaim indicators. Prewarm outside timed intervals and sanitize environment identically for causal comparisons.
5. Gate default-off on no correctness regression and acceptable memory/reclaim behavior. If retaining cache materially hurts this memory-constrained configuration, document an explicit deployment policy instead of claiming eviction is universally wrong. Do not erase the old experiment or revise old measurements in place.
6. Independent review and updated full-depth acceptance required for the final candidate.

## Task 3 — independent LAT/PREFETCH review

**Owner:** Astra, no implementation changes. **Output:** `docs/review-integrate-lat-prefetch.md`.

Read pool landing `f6647ac`, prefetch `837c2a3`, current C++/headers/tests and actual host24 evidence. Review as two named card scopes in one report:

- Window pool ownership, custom deleter/state lifetime, charge release exactly once, reset completeness and thread synchronization.
- WindowEnd per-mate stream positions, real STAR istringstream lifetime, EOF/failbit restore, partial/tail windows, ordinal binding, refusal fallback and chunk boundaries.
- Finish/join before borrowed genome release; retained next/current windows at shutdown.
- Test coverage of actual generated call sites versus hand-constructed stub-only invariants.
- Accounting discrepancy: host24 has 877,083 `device_stopped` misses with status 0 despite zero batch faults. Determine whether this is expected shutdown fallback, counter naming or a defect; do not assume either. Also reconcile claims about unused chains rather than treating non-partitioned counters as a disjoint sum.

Deliver Blocking / Should fix / Nits with source citations and reproducing tests. Orchestrator verifies each blocking claim; fixes are a separate owned card followed by gates. No repeated review ceremonies without new findings.

## Task 4 — executable cost model, no memo

**Owner:** Luna; independent review by non-author. **Files:** `scripts/cost_curve.py`, `scripts/tests_cost_curve.py`, generated `bench/fig/cost-model.json` and relevant figures; new `bench/PHASE2C-cost-model-update.md`.

1. Add tests for stage-only substitution, evidence provenance, explicit comparator, correct units and measured-versus-projected labeling. Preserve existing non-STAR baseline values unless separately evidenced corrections are needed.
2. Derive unrounded arm ratios from accepted raw TSVs, not headline percentages. Keep CPU advice and GPU increment separately identifiable; do not add their percentages.
3. Add two modeled scenarios atop the same umbam CPU comparator: advised STAR and advised+GPU STAR. Modify only STAR's CPU-minute bucket. Leave salmon unchanged until task 7 has valid evidence.
4. Mark the STAR arm measurements as measured for their exact input, but their application to a full-depth nf-core stage and samples/day as PROJECTED. Current one-pass SAM run is not the two-pass pipeline stage; a matching full-depth performance comparison would improve depth evidence, not automatically establish production-mode equivalence.
5. Run `python3 scripts/tests_cost_curve.py` with a verified numpy/matplotlib interpreter; skipped tests are not success. Generate figures into a fresh temporary directory first, independently recompute model JSON and inspect rendered charts before replacing tracked outputs.
6. Finalize evidence selection after task 2/5/6; do not double-count old projected kernel gains and measured integration ratios.

## Task 5 — fresh symbol-level differential and fair baseline

**Owner:** Terra prepares tooling; orchestrator measures/analyzes. **Files:** `bench/star-integrate/perf_differential.sh`, `bench/star-integrate/run_timing_host.sh`; create `bench/star-integrate/test_perf_runner.py`, `bench/PHASE2C-perf-final.md`; any Python helper has an explicit card-owned path.

1. Parameterize immutable gate/source/output roots, reads and threads; refuse existing evidence roots. Preserve structured argv, exact command strings, exits and input/binary identities. Explicitly unset integration/strict flags for CPU controls, strict off for all performance arms, and control advice/eviction independently.
2. Use current parity-accepted stock / advised bypass / GPU builds with matched build options. Define equal cache preparation before every timed arm. Run unprofiled rotated repeats separately from perf runs; do not report profiler overhead as throughput.
3. Screen on the same reduced input and full index, with per-thread self samples and DSO attribution. Handle the two CPU PMUs separately or use a common software sampling event; report unknown samples rather than assigning them to the kernel.
4. Attribute setup, mapping and teardown separately. Include waited-for input children in CPU accounting, while not attributing their CPU to STAR-only symbol percentages. Normalize each profile against its matching measured process/phase CPU denominator.
5. Compare GPU minus advised-bypass self cost for qualitySplit/read preparation, allocations/copies, synchronization/coordinator and output. Inclusive percentages are not additive; duplicated work's removable cost is not the whole GPU-side symbol share.
6. Re-run controlled 20M rotated timing to establish the new baseline and variability. Old round-9 results stay historical and are not silently 'corrected'.

**Decision for task 6:** select the largest demonstrated removable bucket. Recompute the additional CPU-s needed to reach 8% from the new controlled means (round-9 reference was 10.0304 CPU-s). If the profile cannot support that budget, explicitly lower the goal or park the target, not the tests. A small measured improvement can still be worth keeping; no unsupported ≤1–2% ceiling or promised −9% outcome.

## Task 6 — profile-selected final optimization

**Owner:** Terra, conditional on task 5. **Likely files:** `bench/star-integrate/star_integrate.cpp`, `star_integrate.hpp`, `star_integrate_window.cpp`, `make_star_integrate.py`, `test_coordinator.cpp`, `test_coordinator.py`, `test_window_prefix.cpp`, `test_source_patch.py`; exact subset frozen in the card. One worker owns producer and consumer together.

1. If duplicated quality splitting is a demonstrated bucket, read upstream oneRead/qualitySplit and prove split arrays, read orientation, clipping/quality settings and lifetime are equivalent at the handoff. Add a failing regression through the generated production call site before implementing reuse.
2. If coordinator CPU is the demonstrated bucket instead, optimize only its identified allocation/copy/wait operation with a failing invariant test. No generic scheduler rewrite.
3. Keep fallback decisions and storeAligns semantics unchanged unless the measured hypothesis specifically requires a reviewed change.
4. Run local integration suite, independent review, Spark enabled build/tests, strict slice then full-depth parity, followed by controlled repeated unprofiled timings.
5. Retain only justified code; classify a correct negative result honestly. Stop speculative tuning after this measured attempt unless evidence reveals a distinct new mechanism. Task 7 continues regardless of an honest negative.

## Task 7 — salmon huge-page screen

**Owner:** Terra source work; orchestrator runs. **Proposed new files:** `bench/salmon-thp/README.md`, `make_variant.py`, `run_screen.py`, `test_source_patch.py`, `test_runner.py`, `bench/PHASE2D-salmon-thp.md`; evidence summaries in `bench/evidence/salmon-thp/`. Exact upstream patch sites TBD by source inspection, not invented now.

1. Establish the salmon executable/version, complete matching source+dependencies, actual nf-core mode and index identity. In star_salmon, quant may consume transcriptome alignments rather than map FASTQ through a pufferfish index: verify the recorded command before assuming the random-index hypothesis applies.
2. Build and exercise unchanged baseline with those inputs before authorizing instrumentation. Missing source/build ingredients are a prerequisite blocker, not a reason to replace the executable/index with a newer incompatible version.
3. Profile that actual workload and inspect the hot resident structures' allocation/backing. If index walking is absent or not a meaningful CPU bucket, record the disqualifier and stop the patch. A mapping-mode side experiment is labeled a different workload and cannot update the pipeline model.
4. For relevant large anonymous arrays, add optional advice before first touch with source tests and measured real-process page backing. For file-mapped structures, first establish the appropriate backing/promotion mechanism; do not assume anonymous MADV_HUGEPAGE behavior transfers.
5. Same-build advice off/on, balanced repeats, matched warm/cache state, wall/user/sys/RSS and all numerical outputs. Prefer exact bytes; prove baseline reproducibility and document any existing nondeterministic fields before selecting exact permitted normalization. No invented tolerance to get a pass.
6. A short input is a disqualifier only. Scale to representative depth only if backing changed, outputs pass, and the repeated CPU reduction exceeds observed run variation. If no measurable benefit, close with the negative result; no automatic optimization ladder.

## Common acceptance and reporting

- All new code gets RED/GREEN tests, relevant formatting and one independent review per card.
- Integration checks: `bash bench/star-integrate/test_abi.sh`; `python3 -B bench/star-integrate/test_coordinator.py`; `test_source_patch.py`; `test_work.py`; `test_window_prefix.py` (each with full path).
- Rust changes additionally require `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; actual CUDA paths require the real Spark build/tests and relevant GPU parity gates, not Mac stubs.
- Do not weaken or bypass incumbent comparators. Report positive consumption, full input counts and raw repeat rows. CPU-work savings are not measured saturated throughput; report wall separately.
- Commit only verified, separable owned files under existing execution authorization. No commits/pushes or board edits in this planning turn.
- Stop for a genuine source/golden conflict, unresolved correctness finding, unavailable authorized hardware/input, or a needed user decision about scope. Do not stop for routine handoffs or a negative screen with a predefined decision rule.
- Out of scope: memo writing, Batch x86 measurement, Mac Ultra experiments, six-sample production pipeline deployment and held replay-hardening cards.

## Execution starting point

First dispatch task-3 historical review and task-2/tooling preparation to disjoint files. Orchestrator validates the frozen host24/full-input prerequisites concurrently. Start task-1 full-depth baseline as soon as the runner is locally verified; do not wait for cache-policy implementation or the cost-model worker. This keeps two local slots useful while preserving one uncontended measurement at a time on Spark.
