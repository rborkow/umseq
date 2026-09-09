# Independent review — cache/performance readiness

Scope: Terra's P2C-CACHE-PERF-READY delivery. Reviewed by orchestrator.

**Causal cache screen accepted for host execution after one local correction.**
This is not acceptance of a measured cache policy or of all profiling paths.

Verified the causal loop: AB/BA/AB order, distinct eviction-off warmup before
all six measured arms, same integrated binary, explicit THP=1/strict=0 and
per-arm eviction policy, unique sidecars, positive GPU use, zero fault/rejected/
strict-mismatch/live-charge counters, and incrementally appended accepted rows.
Consecutive mode is explicitly rejected; no consecutive-workload claim is made.

Found and repaired a sampler false success: `_find_target` could set the PID
while every procfs snapshot failed, yet `finish` reported target sampled. Added
an executable regression (RED: RuntimeError not raised; GREEN after repair).
The sampler now requires a successfully written process observation and
rechecks `/proc/PID/exe` before each sample. System-wide memory/swap counters
remain labeled system-wide; they are not per-process causality evidence.

Orchestrator execution:
- Four runner/source suites: **34 tests in 1.610s, OK**, no skips; existing
  upstream VLA warning remains. `git diff --check` passed.
- Spark cache suite: **10 tests in 0.119s, OK**, including a real copied
  `/bin/sleep` process sampled through Linux procfs.
- SHA-256 values for all staged runner/observer/helper/launch files matched
  local files. The launch pins the full-depth-accepted STAR hash
  `ef22723c51540aa2a12178da48f5eb0c3df759965ab7e594f7f6257efba38b6b`.

Host experiment started at
`~/uni-rnaseq-probe-lab/cache-policy-lat-close-20260908`, source runner root
`~/uni-rnaseq-probe-lab/cache-runner-lat-close-20260908`. Spark PID 738912,
monitor `proc_0a20e29efcd1`. Shared resource lock held, 7200-second outer bound,
1200-second per-stage timeout. Reads are the accepted 20M paired slice from
its recorded invocation. Six measured arms plus six separate warmups; every
SAM retained. Launch/readback confirmed running; no result yet.

## Remaining limits before subsequent profiling

- `measurement_runner.py` warmups currently use the selected eviction policy.
  If eviction-on is selected, that does not leave index cache warm for the next
  run. Resolve this before any claimed equal-warm three-arm differential.
- Python entry point requires explicit policy; old shell wrappers have not yet
  been adapted to pass it and are not the supported launch route.
- Timing row files persist per stage, but the performance runner's aggregate
  table is still written at the end. Recover raw stage rows if a later stage
  fails; do not infer a complete repeated experiment from a partial run.
- No live THP-residency comparison, deployment concurrency test, consecutive
  screen, cache-policy decision or new STAR speedup is established here.
