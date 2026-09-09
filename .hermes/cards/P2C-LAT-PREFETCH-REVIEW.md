# P2C-LAT-PREFETCH-REVIEW — independent review of two landed cards

User authorized tasks 1–7 and execution. Current accepted HEAD 89f005c; pool landing f6647ac, prefetch 837c2a3. Review those exact landed versions, not concurrent uncommitted generator work.

You are the independent Astra reviewer. Write ONLY `docs/review-integrate-lat-prefetch.md`. No commits/pushes; no SSH, GPU runs, production inputs, data/, runs/, .env or credentials. Another Terra worker owns generator/tests/new runner files; orchestrator owns cards/KANBAN/host evidence. Do not modify their files. Format any illustrative C++/Rust with clang-format/rustfmt before finishing.

Read AGENTS.md. Source wins silently where card and source disagree; stop only when source and golden disagree. Bench records win over older KANBAN status. Read bench/PHASE2C-integrate-1.md latest rounds and the actual artifacts:
- bench/evidence/integrate-1-host/gate-round9-host24.json
- bench/evidence/integrate-1-host/timing-round9-raw.tsv
- bench/evidence/integrate-1-host/timing-round9-r1-gpu-stats.jsonl
Orchestrator opened these. Host24 has positive GPU consumption, strict 0/0/0, batch_faults/rejected 0. Its 877083 device_stopped misses with status 0 need explanation, not an assumed fault. The actual external comparator checks every ordered SAM record (only executable and output-prefix header argv normalized), SJ bytes and non-timing logs; its legacy PARITY_MATCH_COUNTERS_ONLY status names the historical instrumentation, NOT a counts-only check. 20M pairs, 53710530 SAM records.

Review files through git show at the pinned SHAs if necessary:
bench/star-integrate/{star_integrate.cpp,star_integrate.hpp,star_integrate_window.cpp,make_star_integrate.py,test_coordinator.cpp,test_coordinator.py,test_window_prefix.cpp,test_source_patch.py}; trace Rust/FFI lifetimes as necessary. Existing source distribution may not be on Mac; existing test fixtures/source patches are locally available. Do not invent external source facts; label host-only gaps.

Two separate named verdicts, LAT and PREFETCH, in one independent report:
1. Pool shared_ptr deleter/state lifetime, active admission charge exactly once, reset completeness, reuse synchronization and destruction.
2. WindowEnd stream positions, per-mate EOF/failbit handling, ordinal matching, tail/partial windows, refusal, current/next rotation, chunk boundaries and ordinal overshoot.
3. Coordinator join before genome free, borrowed index ownership, outstanding shared_ptr windows at finish.
4. Tests exercise generated producer/consumer contracts rather than only manually made stubs. Add suggested concrete regressions; run existing focused tests or standalone scratch probes if feasible, but write no repo test files.
5. Explain device_stopped status-0 fallback and unused-chain counter overlaps from actual increment sites. Do not claim a partition without proving it. Verify the pool and prefetch correctness independently of performance claims.

Output Blocking / Should fix / Nits / Verified tests / Unverified host requirements. Source file:line citations. No sweeping hardening proposal; identify real correctness gaps. If no blockers say so precisely. Summarize files touched, tests run, remaining host-only checks. No performance estimates.
