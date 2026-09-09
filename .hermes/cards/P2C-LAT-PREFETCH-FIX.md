# P2C-LAT-PREFETCH-FIX — close independent review findings

User authorized full follow-through tasks 1–7. Review task 3 found a reproducible blocker at accepted HEAD 89f005c. You are Terra implementation worker, slot replacing completed Astra review. Read docs/review-integrate-lat-prefetch.md in full; it is the spec, not this paraphrase. Read AGENTS.md. Source beats card silently, stop only if source and golden conflict. No commit/push or remote execution; no production input/data/runs/credentials. Format with clang-format/rustfmt before finishing.

Owned files ONLY:
bench/star-integrate/star_integrate.cpp
bench/star-integrate/star_integrate.hpp
bench/star-integrate/star_integrate_window.cpp
bench/star-integrate/test_coordinator.cpp
bench/star-integrate/test_coordinator.py
bench/star-integrate/test_window_prefix.cpp
bench/star-integrate/test_window_prefix.py
NEW bench/star-integrate/test_window_contract.cpp
NEW bench/star-integrate/test_window_contract.py
NEW bench/PHASE2C-lat-prefetch-fix.md (implementation handoff, no host claims)

Another Terra worker currently owns make_star_integrate.py, test_source_patch.py, measurement_common.py, full-depth/cache/perf/timing runners/tests. DO NOT edit those. Request any generator change by exact patch in handoff instead. Orchestrator owns board/cards/host scripts/evidence. Reviewer writes docs/review-integrate-lat-prefetch.md. Read these as needed but no writes.

## Verified B1 (fix first, independently useful)
Orchestrator read the relevant lifetime/queue code and independently executed /tmp/lat-review-destruction. Actual result: return -6 (SIGABRT), stdout `after finish queued=40000 live_requests=40000 live_bytes=30720176 tails=240000`, stderr `libc++abi: terminating due to uncaught exception of type std::__1::system_error: mutex lock failed: Invalid argument`.
Scratch code exists /tmp/lat-review-destruction.cpp and /tmp/lat-review-probe.cpp; pinned exports /tmp/lat-prefetch-review-{lat,pinned}. Read before use. Seven producers publish one 40000-job fixture each, close worker windows, then stopping coordinator starts; cap 262144 admits six and stopping branch drops out leaving seventh slot. Queue member destruction destroys pool before owning slots; deleter uses dead pool mutex/vector.

Add failing repo regression reproducing this with ordinary process teardown (NO _Exit masking). Fix shutdown to resolve and relinquish EVERY queued window, including whole-window cap exclusions, before state/pool destruction. Release final owners outside State::mu since Window::release locks it. Prove empty queues, zero active charges, normal exit. Also delayed-backend scenario with workers closed. Do not just reorder members to hide undrained work. Bound lifecycle test synchronization (no arbitrary sleep). No changes to kernel or stock fallback decisions.

B2 was historical LAT-only charge bug already fixed by PREFETCH. Preserve direct CURRENT-window refusal regression in addition to next refusal; do not re-fix or revert working charge clearing.

## S1–S4 (same owner, real stream contract)
- S1: budget overflow reads and rejects a frame, then records its AFTER position but last-admitted+1 ordinal. Save boundary after last ADMITTED record or rewind rejected record. Distinct paired records, force byte/candidate budget break (not only loop max), assert all frame ordinals and both stream positions through rotation; stock still sees every read.
- S2: no usable -1 EOF endpoint; mark no successor, avoid repeated peeks. Any restore failure, including initial seek-to-next failure rollback, must fail as existing fatal_restore requires. Per-mate EOF/failbit, partial tail, empty successor, mismatched mate end, injected second-mate rollback failure. Respect real STAR readLoad semantics.
- S3: old ordinal supplied BEFORE oneRead is not next ordinal; exact consume checks protect correctness but stale frame can strand remainder. Specify safe cursor/retirement recovery; test forward gap AND actual overshoot through real generated call order. No wrong-frame handoff.
- S4: restore genuine pool reuse/reset test after dropping all earlier owners; third submission reacquires object, pointers rebuilt, counters reset; separately retained shared_ptr prevents early reuse. Add targeted real producer/coordinator stream test linked through generated mapChunk/oneRead handoff, not just unlinked compile smoke or manually made frames. Existing local upstream source is /private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source if present; do not assume unavailable source or invent mocks that bypass exact bug. If a small generated call-order fixture is necessary, clearly distinguish what upstream semantics it actually executes.

## S5 accounting (after correctness, keep fast path narrow)
Read actual sites; don't infer partitions from names. Separate CPU-resolved admission fallback from actual device stops; attribute retirement to the Window supplied (not current_window when closing next); count whole-chain stats ONCE per chain, not once per step. Make definitions explicit and tests demonstrate any remaining overlaps/omissions. Preserve existing fields where possible but document changed semantics; do not retroactively rewrite old evidence. No per-job heavyweight maps or broad counter framework. Keep timing hot-path overhead low and leave measurement to orchestrator.

## Acceptance/handoff
RED/GREEN tests for each actual bug, relevant integration ABI/source/producer/coordinator suites, clang-format on owned C++ then git diff --check. No claiming a generated object compile proves runtime stream contract. New test rejection must distinguish intended diagnostics from sanitizer crashes/timeouts. Run ordinary process-destruction regression with sanitizer where supported; no real CUDA claim from stub tests.

Summarize B1, B2, S1–S5 separately, exact tests and unresolved items; list owned files changed. State host strict slice/full-depth parity and repeated performance are NOT RUN. Write B1 result/handoff as soon as local verified, then continue; final independent review and Spark acceptance are orchestrator's. Stop only for a genuine unresolved correctness/source constraint after bounded attempts, not for routine handoff.
