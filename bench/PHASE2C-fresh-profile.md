# Repaired-candidate fresh profile — exploratory, repeated timing pending

## Evidence and status

Spark evidence: `~/uni-rnaseq-probe-lab/profile-lat-close-20260908`.
Tracked summaries: `bench/evidence/profile-lat-close/`. Raw perf binaries and
large stack text remain on Spark. All three profiles and extraction stages
completed; GPU sidecar verified 145,877,626 consumed chains, zero faults,
rejections, strict mismatch counters and live charges at finish.

Accepted integrated binary SHA256:
`ef22723c51540aa2a12178da48f5eb0c3df759965ab7e594f7f6257efba38b6b`.
Stock SHA256:
`f84493ef8c9a6d39e60264c0c9e0c6da388e8a455e6c21c1a1b9a61f52662a29`.

## Protocol

- Same 20M paired input slice, 20 STAR threads, one profiled invocation per arm.
- Stock, integrated hooks-bypassed (advised CPU), integrated GPU; strict off.
- Explicit THP advice on and post-load cache eviction on for integrated arms.
  Stock ignores those integration variables.
- Separate same-policy workload warmup for each arm. Untimed full reads of
  Genome, SA and SAindex immediately before both warmup and measured invocation;
  paths, byte counts and mtimes retained in `index-prewarm.json`. This prevents
  treating an eviction-enabled workload alone as a warm-cache precondition.
- `perf record -e cpu-clock -F 499 -g -- /usr/bin/time ... STAR`; target timing
  excludes perf extraction and index prewarm, but the workload runs under perf.
- GPU warmup and measured sidecars both require positive consumption and clean
  counters. No repeated-speedup conclusion is drawn from these profiles.

## Raw target GNU time

| Arm | Wall s | User s | System s | CPU s (user + system) | Max RSS KiB | Exit |
|---|---:|---:|---:|---:|---:|---:|
| stock | 55.80 | 728.70 | 27.72 | 756.42 | 32684120 | 0 |
| advised CPU | 55.12 | 652.32 | 30.03 | 682.35 | 32644912 | 0 |
| GPU | 56.78 | 587.89 | 40.29 | 628.18 | 38788276 | 0 |

Exploratory profiled CPU delta: GPU **−7.94% versus advised CPU**, **−16.95%
versus stock**. These are single profiled rows, not a revalidated threshold or
an update to the pipeline cost model. Wall did not improve in this profile.

## Attribution

`summarize_profile.py` streams `perf script --fields tid,ip,sym,dso`, retaining
all samples. Kernel classification uses the leaf DSO, not symbol-name guesses;
first user and first STAR callers of kernel leaves are recorded separately.
Malformed/frameless samples fail closed. Two parser tests passed on both hosts.
Self sample shares are scaled by each arm's measured CPU seconds: these are
**sample-based estimates**, not separately timed function costs or causal savings.
The parser does not weight samples by perf's adaptive event period.

| Arm | Total samples | Kernel leaves | Unknown-symbol leaves |
|---|---:|---:|---:|
| stock | 405919 | 14602 | 27955 |
| advised CPU | 369090 | 15808 | 27986 |
| GPU | 341710 | 20665 | 32471 |

Selected GPU-minus-advised-CPU self-sample estimates:

| Symbol/bucket | Estimated CPU-s difference |
|---|---:|
| compareSeqToGenome | −156.81 |
| maxMappableLength2strands | −28.82 |
| maxMappableLength | −6.51 |
| stitchPieces | +16.16 |
| extendAlign | +14.47 |
| convertNucleotidesToNumbers | +13.25 |
| integration lookup | +11.91 |
| coordinator_main self samples | +9.56 |
| __aarch64_cas1_acq_rel | +8.35 |
| libc memcpy | +7.99 |

The kernel sample attribution still names index loading prominently:
`fstreamReadBig` first-STAR-caller estimates are 11.79 CPU-s for bypass and
14.14 CPU-s for GPU. Do not call all extra system time coordinator overhead.
Atomic helpers and memcpy are shared call-site buckets, not independently
removable costs. Likewise increased stitch/extend time does not establish extra
algorithmic work; memory layout and contention remain possible explanations.

## Important memory-layout caveat

The measured GPU sidecar reports **25,415,385,088 index anonymous-huge bytes**;
its preceding GPU warmup reports 23,597,154,304. In the startup probe, values
ranged from 23,727,177,728 to 30,175,920,128. Thus requesting THP did not establish
identical achieved huge-page backing across invocations. The bypass arm has no
corresponding per-VMA snapshot here. This limits attribution of the residual
non-search differences; do not turn a small difference near the numerical gate
into a definitive mechanism claim. Preserve GPU residency sidecars with the
upcoming repeated timings as well.

## Next evidence

A separate **unprofiled** rotated three-repeat stock/bypass/GPU run is active at
`~/uni-rnaseq-probe-lab/timing-lat-close-20260908` (monitor
`proc_ad68817ac3fe`). Same accepted binaries, same runner hashes and explicit
index precondition. Raw outputs and sidecars survive a failed later arm.
No new core optimization is selected yet: confirm the net timing result and
account for the layout caveat before spending a correctness-sensitive card.
