# Phase 2C cache/performance harness readiness

This delivery is host-experiment tooling only. It is neither a measurement nor
an independent acceptance of the frozen integrated binary.

**Orchestrator update:** causal screen independently reviewed in
`docs/review-cache-perf-ready.md`. Corrected the observer to require a successful
sample, not merely a discovered PID. Reruns: 34 local tests and 10 Spark cache
tests pass. Six measured arms were launched under the shared lock at
`~/uni-rnaseq-probe-lab/cache-policy-lat-close-20260908`; the screen stopped
after two accepted rows when an eviction-off warmup silently fell back to CPU
alongside NVIDIA context-allocation OOM messages. See
`bench/PHASE2C-cache-screen.md`; no completed cache comparison is claimed.
Remaining performance-runner limits are recorded in the review, not approved
by this causal-screen launch.

## Causal cache screen

Run only the accepted integrated binary and a fresh output root:

```bash
python3 -B bench/star-integrate/cache_policy_bench.py \
  --integrated /verified/integrated/STAR --base-argv /verified/base.argv.json \
  --mate1 /approved/mate1.fastq.gz --mate2 /approved/mate2.fastq.gz \
  --mode causal --output /approved/fresh-cache-screen --timeout-s 1200 --repeats 3
```

The causal screen uses THP `1`, strict `0`, and an explicit post-load eviction
policy in every process. For every measured arm it first runs an eviction-off
warmup outside the measured GNU-time interval. Odd repeats are AB
(`evict-off`, `evict-on`) and even repeats BA. Warmup and measured execution
have distinct sidecars and output roots. There is no global `drop_caches` call.

Each completed measured arm retains its `run_stage` GNU-time row and contributes
one appended row to `accepted-arms.jsonl` only after exactly one final sidecar
row confirms positive `gpu_consumed`, zero faults/rejections/strict mismatch
counters, and zero `live_bytes_at_finish` and `live_requests_at_finish` from
the repaired binary. Failures preserve prior stage files and earlier accepted
rows.

`consecutive` is deliberately disabled. The previous behavior only carried
state across alternating policies, not consecutive same-policy invocations, so
it cannot support that claim without a separate design and protocol.

Measured arms also write `host-observation.jsonl` incrementally and a summary.
On Linux it samples the actual PID only after `/proc/PID/exe` matches the
integrated binary, recording process VmRSS/VmHWM/VmSwap and I/O plus system-wide
MemAvailable/Cached/SwapFree and pswpin/pswpout/pgmajfault before, during, and
after. A target that is never sampled rejects the arm. Non-Linux/macOS runs are
explicitly labelled procfs-unavailable rather than reporting zero counters.

## Timing and profiling runner

Use the Python entry point until the external shell launchers are updated to
pass the required explicit eviction option:

```bash
python3 -B bench/star-integrate/measurement_runner.py \
  --stock /verified/stock/STAR --integrated /verified/integrated/STAR \
  --base /verified/base.argv.json --mate1 /approved/mate1.fastq.gz \
  --mate2 /approved/mate2.fastq.gz --output /approved/fresh-timing \
  --timeout-s 1200 --repeats 3 --drop-index-cache 0
```

It now records THP `1`, strict `0`, and the selected `--drop-index-cache 0|1`
in provenance and each stage environment. Its three-arm ordering and profiling
layout are unchanged: GNU time remains inside `perf record`, while `run_stage`
retains wrapper-inclusive diagnostics separately. GPU-sidecar validation also
requires the final zero-live-charge fields.

## Limitations

The observer’s system counters are system-wide, not attributable solely to
STAR. They are evidence for deciding whether a policy warrants reclaim/swap
investigation, not a numerical performance or memory attribution. Sampling is
one second and is intentionally outside the target GNU-time accounting where
the wrapper topology permits it.
