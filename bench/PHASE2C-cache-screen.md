# Cache causal screen — incomplete, GPU initialization fallback

## Decision

**No completed cache comparison and no new speedup claim.** The runner rejected
`warm-r2-evict-on` because GPU consumption was zero. Despite its label, this was
the common **eviction-off warmup** preceding the scheduled eviction-on arm.
Do not relax the positive-consumption requirement or combine a future rerun
with these partial rows. Keep historical eviction **explicitly on** for accepted
correctness/performance work until a replacement policy is justified.

Evidence: `bench/evidence/cache-policy-lat-close-failed/`; full artifacts remain
on Spark at `~/uni-rnaseq-probe-lab/cache-policy-lat-close-20260908`.

## Raw accepted rows (only two of the requested six)

| Repeat | Measured arm | Wall s | User s | System s | Max RSS KiB | Exit |
|---|---|---:|---:|---:|---:|---:|
| 1 | eviction off | 48.39 | 594.09 | 32.98 | 38823548 | 0 |
| 1 | eviction on | 50.10 | 594.71 | 26.51 | 38632220 | 0 |

Both positively consumed GPU output. These rows are retained, not sufficient
for an aggregate comparison. Live observations exist for these measured arms.

## Failure evidence

- Warmup environment explicitly contains `STAR_INTEGRATE=1`, strict `0`, THP
  `1`, and `STAR_INTEGRATE_DROP_INDEX_CACHE=0`.
- STAR itself exited 0, but its sidecar says `mode=cpu-bypass`, with zero GPU
  batches, submitted chains and consumed chains. No GPU batch fault was reported.
- Warmup raw GNU time: `84.33\t654.55\t37.48\t32720212\t0`.
- STAR finished loading the index at **2026-09-08 12:47:07 host time**. At that
  timestamp, the kernel logged NVIDIA `NV_ERR_NO_MEMORY` failures allocating
  shadow-fault-buffer pages and GPU graphics context buffers. Earlier at
  12:43:00 it had already logged a failed big-page allocation and a retry using
  the default page size.
- `star_integrate.cpp:1027` calls the borrowed-index initializer and silently
  falls back when it returns an error or null context. Its error object is not
  exposed in the sidecar. Consequently the exact FFI error code is **not known**.
- No live memory observer was attached to warmups by the original harness.
  Do not infer the failed process's minimum available RAM or swap activity from
  measurements of other invocations or from post-exit `/proc/meminfo`.
- Post-failure inspection showed no active GPU processes; vLLM remained stopped.
  The reserved HugeTLB pool was still 16,384 free 2-MiB pages. No global cache
  drop, pool reconfiguration or GPU reset was performed.

The leading hypothesis is GPU context allocation failure under memory
pressure/fragmentation after index loading with eviction disabled. Kernel
allocation failure is observed; its precise causal dependence on this policy,
the page cache, THP and HugeTLB reservation is not established. Wrong activation
environment is contradicted by the recorded environment; a failure in a GPU
batch is contradicted by zero submitted work.

## Bounded discriminating probe

A **separate startup-reliability probe**, not a resumed speed experiment,
completed at `~/uni-rnaseq-probe-lab/cache-startup-probe-lat-close-20260908`:
explicit on, on, off, off; no separate warmups, all invocations observed, same
full-depth-accepted binary and 20M inputs, strict off and THP on. Stop at the
first failure; retain kernel logs and per-process observations. The comparison
is about successful GPU activation, not a matched-cache speedup.

All four invocations positively consumed GPU output, had zero fault/rejection
and live-charge counters, and had successful process observations. The retained
kernel log has no entries. **The original failure did not reproduce: disabling
eviction has not been established as its cause.** Neither this success nor the
original failure is discarded. Evidence:
`bench/evidence/cache-startup-probe-lat-close/verified-summary.json` and
`observation-aggregates.json`, backed by the per-invocation raw files.

| Invocation | GPU chains consumed | Wall s | User s | System s | Max RSS KiB | Exit | Process samples |
|---|---:|---:|---:|---:|---:|---:|---:|
| on-1 | 146501158 | 49.67 | 593.25 | 28.45 | 38617668 | 0 | 49 |
| on-2 | 145567621 | 73.03 | 594.91 | 35.48 | 38806212 | 0 | 72 |
| off-1 | 120316033 | 84.13 | 653.48 | 65.44 | 43350368 | 0 | 83 |
| off-2 | 146477517 | 48.58 | 593.68 | 27.21 | 38632660 | 0 | 47 |

These timings describe an ordered startup probe, **not a matched-cache speed
comparison**. Every sampled target VmSwap was zero. System-wide pswpin/pswpout
deltas were respectively 1/0, 0/0, 0/0 and 0/2; these counters are not attributable
to STAR. No new memory-pressure mechanism is inferred from the slow off-1 run.

Conservative operational choice: keep explicit eviction-on, not because these
four trials prove superiority, but because it preserves the accepted historical
policy while the alternative remains insufficiently characterized. The causal
cache task ends as an incomplete experiment with a documented failure, not a
positive performance result. Fresh profiles prewarm the index with a separate
untimed full file read before each invocation; a preceding eviction-on workload
alone is not treated as a warm file-cache precondition.

The probe's launch SSH timed out while the remote background subshell retained
the connection; remote process/log readback confirmed continued execution, so
it was not relaunched.
