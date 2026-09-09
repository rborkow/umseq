# Repaired-candidate rotated timings — verified, incremental gain below 8%

## Result

Nine measured rows (three per arm) completed successfully. All six GPU
invocations, including the separate warmups, positively consumed device output
and reported zero faults, rejected results, strict-mismatch counters and live
charges at finish. Strict was off, so zero strict counters are not a substitute
for the separately completed full-depth correctness gate.

Evidence: `bench/evidence/timing-lat-close/`, especially `raw.tsv`,
`verified-summary.json`, per-stage argv/environment/exit records and GPU
sidecars. Full artifacts remain at
`~/uni-rnaseq-probe-lab/timing-lat-close-20260908` on Spark.

| Mean over three repeats | CPU s | Wall s |
|---|---:|---:|
| Stock | 756.4467 | 56.7500 |
| Advised CPU / hooks bypassed | 683.4600 | 54.1933 |
| GPU | 645.0800 | 57.4667 |

- GPU CPU cost is **5.62% below advised CPU**, **14.72% below stock**.
- GPU wall is **6.04% above advised CPU**. These are not saturated-box throughput
  measurements; lower CPU cost does not prove higher saturated throughput.
- The bypass/stock CPU difference is **9.65%** in this matrix, not a new isolated
  causal estimate of THP alone.
- The **8% incremental CPU target is not met**. At these means it would require
  another **16.2968 CPU-s** saved relative to the current GPU arm with this
  advised-CPU comparator held fixed. That arithmetic is not a prediction.
- The earlier exploratory profile's −7.94% was not reproduced by the repeated
  unprofiled mean. No row has been dropped or replaced by the profile result.

## Protocol

Same full-depth-accepted integrated binary and pinned stock comparator as
`bench/PHASE2C-fresh-profile.md`; 20M paired slice and 20 STAR threads.
Rotations: stock/bypass/GPU, GPU/stock/bypass, bypass/GPU/stock. No perf recording.
Same-policy workload warmup before every measured arm; full untimed reads of
Genome, SA and SAindex immediately before both warmup and measured invocation.
All eighteen prewarm records and stage environments were checked. THP-on,
cache eviction-on, strict-off explicitly selected; stock ignores integration
flags. Input-cache reads and warmups are excluded from measured target times.

This is a controlled-precondition invocation experiment, not deployment cost
with cold inputs or consecutive-sample throughput. The earlier failed cache
matrix and subsequent startup probe remain separate datasets.

## Raw GNU time, in execution order

| Repeat | Arm | Wall s | User s | System s | Max RSS KiB | Exit |
|---|---|---:|---:|---:|---:|---:|
| 1 | stock | 56.91 | 727.76 | 29.72 | 32711616 | 0 |
| 1 | bypass | 52.84 | 655.12 | 28.58 | 32686260 | 0 |
| 1 | GPU | 56.87 | 592.16 | 40.55 | 38786660 | 0 |
| 2 | GPU | 60.24 | 611.90 | 42.73 | 39376936 | 0 |
| 2 | stock | 56.43 | 727.92 | 28.74 | 32657028 | 0 |
| 2 | bypass | 52.26 | 651.99 | 27.53 | 32674484 | 0 |
| 3 | bypass | 57.48 | 654.54 | 32.62 | 32712540 | 0 |
| 3 | GPU | 55.29 | 609.44 | 38.46 | 39530276 | 0 |
| 3 | stock | 56.91 | 726.30 | 28.90 | 32739080 | 0 |

## GPU residency and coverage observations

| Repeat | GPU chains consumed | Chains submitted | Key misses | Index anon-huge bytes |
|---|---:|---:|---:|---:|
| 1 | 145048935 | 159509872 | 2052687 | 24385683456 |
| 2 | 139702348 | 159509868 | 9444193 | 24117248000 |
| 3 | 140192652 | 159829348 | 8700012 | 24580718592 |

These reported huge-page totals are below the 30,175,920,128-byte value observed
in some earlier successful invocations. We have no contemporaneous per-VMA
snapshot for the bypass timing arms. Identical THP settings do not prove equal
achieved page layout. Therefore do not attribute the change from round 9 solely
to the correctness repair, nor claim the profile's conversion/atomic buckets
are a sufficient removable budget. This matrix is valid as observed invocation
performance, but incomplete for isolating a page-layout-independent GPU increment.

## Next discriminating check

A separate same-binary bypass/GPU **residency diagnostic** is running under the
resource lock at `~/uni-rnaseq-probe-lab/residency-lat-close-20260908`, monitor
`proc_12a00ff22c23`. Every target sample records large anonymous VMAs, RSS,
AnonHugePages, THPeligible and VmFlags, plus process/system memory observations.
Large VMAs are index candidates, not automatically attributed to arrays. Same
explicit policies and untimed index precondition; no global memory-policy
changes, index rewrite, allocation workaround or core optimization.

Sampler parser tests passed locally and on Spark. The diagnostic timing is not
a replacement for the nine rows above. Source additions:
`bench/star-integrate/observe_index_residency.py` and
`bench/star-integrate/test_index_residency.py`.
