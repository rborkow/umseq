# P2C-FOLLOWTHROUGH-TOOLS — full-depth runner, cache policy, profiling preparation

Execution authorized for tasks 1–7. HEAD 89f005c, prefetch code 837c2a3. You are Terra; local code only, orchestrator runs Spark. Source beats card silently; stop only if source and golden conflict. Read AGENTS.md and relevant existing files first. Another Astra worker reviews pinned LAT/PREFETCH and writes only docs/review-integrate-lat-prefetch.md. Do NOT touch that report, KANBAN, plans/cards, core coordinator/window C++, Rust crates, data/, runs/, secrets, or any remote host. No commit/push. Format with clang-format/rustfmt before finishing; keep Python readable, multiline, stdlib-only.

Own ONLY:
- NEW bench/star-integrate/run_full_depth.py, test_full_depth_runner.py
- MODIFY bench/star-integrate/make_star_integrate.py, test_source_patch.py
- NEW bench/star-integrate/cache_policy_bench.py, test_cache_policy_bench.py
- MODIFY bench/star-integrate/perf_differential.sh, run_timing_host.sh
- NEW bench/star-integrate/test_perf_runner.py, measurement_common.py (if useful)
- NEW bench/PHASE2C-followthrough-tools.md (handoff, not measured results)

Deliver in PRIORITY order, each independently useful. Full-depth runner first. Do not spend time building a broad framework; minimal parameterized, fail-closed scripts. No real host timing claims. Record exact local tests and RED/GREEN. Run existing test_abi.sh, test_source_patch.py, test_coordinator.py, test_work.py, test_window_prefix.py after generator edits; bounded retries, report blockers honestly.

## 1. Full-depth runner over frozen binaries
Existing run_host.py REBUILDS and hardcodes old 20M stock artifacts/expected-pairs. Leave it unchanged. Build a sibling runner taking explicit existing stock and integrated executables, external accepted tooling directory, base argv JSON, paired full FASTQ paths, verified expected-pairs, fresh output root and timeout. Exact CLI documented in handoff. Do not compile inside this runner. Keep full input read-only; output under ~/uni-rnaseq-probe-lab or a caller-approved temporary test root. No overwrite/delete evidence directories. Structured argv, not shell splitting. Replace exact readFilesIn two-mate values and output prefix, reject ambiguity; no readMapNumber cap in the full-depth accepted run.

Orchestrator verified current baseline invocation:
--runMode alignReads --runThreadN 20 --runRNGseed 777 --genomeDir /home/rborkows/uni-rnaseq/data/index/star_full --genomeLoad NoSharedMemory --readFilesIn <mate1.gz> <mate2.gz> --readFilesCommand zcat --twopassMode None --genomeType Full --genomeTransformType None --outSAMtype SAM --outSAMorder PairedKeepInputOrder --outSAMunmapped Within --outFileNamePrefix <dir/>
Base argv path: ~/uni-rnaseq-probe-lab/integrate-gate-host24/stages/integrated.argv.json. Integrated: same root/private/integrated/STAR. Stock exact path/provenance comes from stock-reuse.json, do not assume binary name. Checker: ~/uni-rnaseq-probe-lab/integrate-source-v1/tooling/replay/seed_split_parity.py, with --stock, --counters, --stock-command, --counter-command, --expected-pairs, --out. Checker status PARITY_MATCH_COUNTERS_ONLY actually means all ordered SAM records match, SJ matches, non-timing logs match; only header executable/output-prefix argv normalized. Copy/use UNCHANGED external checker; do not implement a substitute.

Acquire the actual shared flock or verify inherited fd9 lock robustly; a caller's INTEGRATE_LOCKED=1 string alone is not proof. External timeout around owned subprocess trees; no time-of-day expiration hardcoded. Clear inherited STAR_INTEGRATE* arm flags, set GPU/strict=1 explicitly only for strict arm; preserve intended old binary cache policy. Every stage writes argv, exit, stdout, stderr, raw GNU time. Record binary/input identity incrementally. Expensive hash checks must not be hidden in measured mapping; already independently verified manifests may be explicit inputs rather than hashing 30 GB on every run. Expected pairs must be supplied independently, never inferred from successful output count.

Stock runs fresh at full depth, strict GPU fresh, then unchanged checker. Require process exits zero, checker pass, exact pair count, exactly one final sidecar, positive gpu_consumed, batch_faults/rejected zero, all shift/flag/step-count mismatch zero. No acceptance from mapping rate. Strict time diagnostic only. Preserve failed outputs. Validate binary/config prerequisites BEFORE huge runs.

Tests first: existing output root rejected; mismatched expected pairs fails; missing/failed stock fails; comparator nonzero fails; missing/multiple final sidecars fail; zero GPU consumption fails; strict mismatch fails; correct structured argv and environment; timeout/signal not treated as correct rejection. Mock controlled subprocess results or tiny fake executables clearly TEST fixtures, never pretend they are real evidence.

## 2. Cache policy switch and screen
make_star_integrate.py emits unconditional posix_fadvise(DONTNEED) after load. Keep it as explicit opt-in STAR_INTEGRATE_DROP_INDEX_CACHE=1; unset/0/other do not opt in. Keep madvise advice independent (STAR_INTEGRATE_THP unchanged). Add source and executable small-helper tests proving missing/0/1 gate behavior; no actual eviction of user files during tests. Handle fadvise error semantics correctly (returns error code).

Do not declare eviction universally wrong: prior motivation was memory pressure. Screen runner must compare same binary advice held on, eviction off/on, GPU state fixed per comparison, same warmed cache before causal runs and separate consecutive-sample mode. Record GNU wall/user/sys/rss/exit, stage argv and policy, fresh root. No global drop_caches, no host policy changes. Real process memory sampler should identify STAR child, not /usr/bin/time; keep scope minimal or state host sampler needed. Orchestrator will determine safe default from memory/reclaim and warm startup results. Historical behavior must remain reproducible.

## 3. Refresh perf and timing runners
Existing perf_differential.sh points at host8, rm-rf output, one fixed-order run and inherited flags. Existing run_timing_host.sh locally points host5. Make explicit parameters/env arguments for stock/integrated/base argv/output; reject existing output roots; use helper Python structured argv if simpler. Sanitize inherited enable/strict/advice/eviction flags explicitly per arm. Strict OFF for timings. Stock/advised bypass/GPU three arms, rotate repeated unprofiled runs separately from perf. Controlled cache warming before EVERY causal arm outside timed interval; do not delete SAM before parity can be checked (explicit separate performance-only discard allowed after documented validation, no fake parity).

Perf: retain raw perf.data, per-arm GNU time, leaf/self TSV, DSO and thread attribution where supported. PMU tables must not be summed as one denominator; prefer common cpu-clock sampling across heterogeneous cores. Report children/time denominators transparently; do not multiply STAR-only percentages by child CPU. No need to implement advanced attribution analysis here; preserve evidence and write exact analysis instructions. All stages fail closed on nonzero exit/timeouts, append rows incrementally. Tests for env isolation, fresh output, checked exits and argv preservation.

## Handoff
Finish with files changed, precise commands for orchestrator, local tested vs host-only, and any independent partial delivered. No guessed full FASTQ names/count, source paths beyond verified above, timings, or success claims. Write the full-depth invocation first in the handoff so it can launch while other tasks are under review. Stop after these tooling items; no task-6 optimization or salmon code yet.
