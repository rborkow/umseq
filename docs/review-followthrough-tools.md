# Independent review: follow-through tooling

Reviewer: Luna  
Date: 2026-09-08  
Scope: the original worker tooling paths listed in the review request. No benchmark, SSH,
secret, data, runs, coordinator, or window work was performed.

## Original findings

1. `perf_differential.sh` did not select an explicit perf event, used unsupported `perf
   report --sort comm,pid,tid` on this host, and hid both report failures with `|| true`.
   Its analysis text therefore could claim a deliverable that was absent. It also mixed
   profiling with timing semantics and did not make child accounting explicit.
2. Timing order was rotated, but index warmth was not controlled outside each timed arm.
   Cache policy and exact arm flags were not fully recorded.
3. The perf/timing paths did not require a positive GPU-consumption sidecar with clean
   fault counters. They also did not retain strong executable/checker provenance.
4. The cache screen had a held arm that duplicated the off condition, no repeated paired
   off/on comparison, and no explicit THP/advice setting. Its warm-up protocol was not
   represented as a repeatable paired experiment.
5. Full-depth comparator commands were passed through `shlex.join`, while the checker’s
   command parser uses literal space splitting. This is unsafe for the checker’s supported
   input contract. Stock reuse had no fail-closed validation path.
6. Executable/checker identity hashing was conditional on the input-hash switch, and the
   environment scrub covered only a fixed list of integration variables.

## Corrections authored in this review

- Added `measurement_runner.py` as the shared timing/profiling implementation. It records
  explicit `cpu-clock` perf commands, keeps raw `perf.data`, uses `perf script` with
  `tid/ip/sym/dso`, fails on extraction errors, rotates three-arm timing order, and records
  an explicit profile read limit.
- Added outside-the-interval warmups, strict-off arm environments, and required positive,
  clean GPU sidecars for GPU performance rows.
- Changed cache policy metadata to fixed THP-on advice and repeated paired eviction-off /
  eviction-on arms; causal warmups are per arm and per repeat, with no global cache edit.
- Made executable and checker SHA-256 identities unconditional; input identities are
  recorded and bound to the stock-reuse manifest checks. Environment cleanup now removes
  every inherited `STAR_INTEGRATE*` variable.
- Added comparator argv validation and exact space serialization for the checker’s known
  interface. Added optional `--reuse-stock` validation requiring success marker, zero exit,
  independent pair count, matching candidate arguments, executable/input identities, and
  preserved stock output.
- Added fake-process tests covering success, nonzero exit, timeout, missing/malformed/
  unclean sidecars, comparator whitespace rejection, and stock-reuse rejection/acceptance.

## Verification

Passed locally:

```text
python3 -m unittest bench/star-integrate/test_full_depth_runner.py \
  bench/star-integrate/test_cache_policy_bench.py \
  bench/star-integrate/test_perf_runner.py \
  bench/star-integrate/test_source_patch.py
Ran 20 tests in 1.808s — OK
bash -n bench/star-integrate/perf_differential.sh bench/star-integrate/run_timing_host.sh — PASS
bash bench/star-integrate/test_abi.sh — PASS (c11, c++17, probe transport, enabled consumer syntax)
git diff --check — PASS
```

The source-hook test emitted the existing upstream VLA warning; it did not fail. This Mac
does not provide GNU `timeout`/`time`, so the helper’s portable fallback records wall time
and explicit `NA` CPU/RSS fields; this is a tooling check, not a host timing result. No host
timings, perf captures, full-depth mapping, GPU execution, SSH, or byte-identity claim was
made. Orchestrator-owned preflight, stock, and verification scripts were not modified.
