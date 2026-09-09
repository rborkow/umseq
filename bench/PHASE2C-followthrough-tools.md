# Phase 2C follow-through tooling handoff

## Full-depth command (launch this first)

```bash
python3 -B bench/star-integrate/run_full_depth.py \
  --stock STOCK_PATH_FROM_VERIFIED_STOCK_REUSE_JSON \
  --integrated "$HOME/uni-rnaseq-probe-lab/integrate-gate-host24/private/integrated/STAR" \
  --tooling "$HOME/uni-rnaseq-probe-lab/integrate-source-v1/tooling/replay" \
  --base-argv "$HOME/uni-rnaseq-probe-lab/integrate-gate-host24/stages/integrated.argv.json" \
  --mate1 /approved/full/mate1.fastq.gz --mate2 /approved/full/mate2.fastq.gz \
  --expected-pairs APPROVED_FULL_PAIR_COUNT --output "$HOME/uni-rnaseq-probe-lab/full-depth-YYYYMMDD" \
  --timeout-s APPROVED_TIMEOUT_SECONDS
```

Replace the explicit placeholders from the independently verified manifest. If `stock-reuse.json` stores a different key/path shape, extract the exact binary path from that provenance rather than assuming a binary name. The output root must not exist. The runner acquires the shared flock itself or accepts only fd 9 that points to that actual lock; `INTEGRATE_LOCKED=1` alone is not accepted.

It records structured argv, scoped environment, stdout/stderr, GNU time, exits, identities, and preserves failures. It performs a fresh stock run, a strict GPU run, then the unchanged external comparator. It requires comparator `PARITY_MATCH_COUNTERS_ONLY`, SAMs, independent expected count, one clean final sidecar, positive `gpu_consumed`, and no listed strict mismatches. Its timings are diagnostic only.

## Cache screen

```bash
python3 -B bench/star-integrate/cache_policy_bench.py \
  --integrated /verified/integrated/STAR --base-argv /verified/base.argv.json \
  --mate1 /approved/mate1.gz --mate2 /approved/mate2.gz --mode causal \
  --output /approved/fresh-cache-screen --timeout-s 1200
```

`STAR_INTEGRATE_DROP_INDEX_CACHE=1` now reproduces the historical post-load `posix_fadvise(DONTNEED)` behavior; unset, `0`, and any other value do not. THP remains independent. The causal mode warms before each arm; consecutive mode preserves cross-sample state. Neither mode changes host cache policy or calls global `drop_caches`.

## Perf/timing

```bash
bench/star-integrate/perf_differential.sh STOCK INTEGRATED BASE_ARGV_JSON FRESH_OUTPUT TIMEOUT_S [REPEATS]
bench/star-integrate/run_timing_host.sh STOCK INTEGRATED BASE_ARGV_JSON FRESH_OUTPUT TIMEOUT_S [REPEATS]
```

Both reject a pre-existing output root, clear inherited integration flags per arm, and force strict off. Perf retains `perf.data`, GNU time, leaf DSO/symbol and thread-attribution reports. Run an independently validated parity job before performance-only SAM disposal; these runners do not claim parity or a timing result.

## Local verification

No full FASTQ, Spark invocation, timing, cache eviction, or binary build was run. Local tests cover argv replacement, fresh-root rejection, sidecar rejection, environment isolation, source gate generation, and shell syntax. Existing project gates are recorded separately by their command output.

GREEN (local):

```bash
python3 -m unittest bench/star-integrate/test_full_depth_runner.py bench/star-integrate/test_cache_policy_bench.py bench/star-integrate/test_perf_runner.py bench/star-integrate/test_source_patch.py
bash bench/star-integrate/test_abi.sh
python3 bench/star-integrate/test_coordinator.py
python3 bench/star-integrate/test_work.py
python3 bench/star-integrate/test_window_prefix.py
git diff --check
```

The first command ran 17 tests successfully. The existing C++ fixture compiles emitted Clang warnings for upstream VLAs and deprecated `register`; no test failed. RED was the initial full-runner argv test: base argv with a non-`STAR` executable token retained that token. The runner now replaces any leading non-option executable token, and the rerun is GREEN.

## Orchestrator verification after independent review

Luna's review completed with 20 local tests passing; see
`docs/review-followthrough-tools.md`. Two claims needed further repair:

- Stock reuse required `pairs.json` (not produced by the real stock runner) and
  silently skipped absent `identity.json` checks. It now consumes the stock
  producer's actual `preflight.json`, requires the successful independently counted
  manifest, verifies the executable SHA-256 and both input MD5s/stat identities,
  checks exact executed argv and retained outputs, and checks STAR's logged input
  count against the independently established count. The real-format regression
  failed before the fix, then passed; no metadata was manufactured for the golden.
- Profiling had no CPU timing row. `profile_command` now places GNU time **inside**
  `perf record -e cpu-clock -F 499 -g`, timing the target and children rather than
  the profiler. `run_stage` separately preserves wrapper-inclusive diagnostic
  time, executed argv, environment and exit. The command-placement regression
  failed before this fix, then passed. Extraction remains outside both intervals.

Verification actually executed:

```text
Mac: full-depth/cache/perf/source suites — Ran 21 tests in 1.753s, OK
Mac: git diff --check — exit 0
Spark: full-depth/perf suites — Ran 11 tests in 1.046s, OK
Spark: stock_reuse against full-depth-stock-20260908 — PASS, 78619701 pairs
Spark: real perf + GNU time protocol smoke check — PASS_NOT_A_BENCHMARK
Protocol-only row (wall/user/sys/maxRSS/exit): 1.01 / 1.01 / 0.00 / 9148 / 0
perf script (tid,ip,sym,dso) output: 412554 bytes, exit 0
```

The Spark check used temporary copies of the runners under `/tmp` and the shared
resource lock. It read the real golden/inputs without running STAR or modifying
them. The one-second Python CPU loop was exclusively a runner protocol test,
**not a STAR timing or optimization result**; its temporary trace was discarded.
The local source test still emits the upstream Clang VLA warning. No integrated
full-depth comparison or production profile has run as part of this verification.
Cache-policy acceptance and repaired-binary gates remain separate prerequisites.
