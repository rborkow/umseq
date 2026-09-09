# Collapse-enabled gate and cost-inclusive four-arm experiment

## Verified 20M correctness gate

Evidence: `~/uni-rnaseq-probe-lab/integrate-gate-collapse-20260908` on Spark;
tracked summary files in `bench/evidence/collapse-gate/`.

New binary SHA256:
`a6c61b2c5659b2fdd4651c86417e9d66fb2520f9472740760711d6e2a9c70bb0`.

Rebuild, ABI/backend tests and strict ordered comparison passed with collapse,
THP and eviction explicitly enabled. Orchestrator verified:
- 20,000,000 input pairs, 53,710,530 matching ordered alignment records.
- Only the documented SAM-header executable/output-prefix normalization.
- GPU consumed 145,590,534 chains; zero batch faults, rejected results, shift,
  flag and step-count mismatches; zero live byte/request charges at finish.
- Each required collapse syscall attempted a positive range and returned 0,
  errno 0. Sidecar reports 30,175,920,128 index anonymous-huge bytes; this does
  not replace the forthcoming per-VMA observations in both comparator arms.

Raw syscall results (wall duration only, not separately measured CPU time):

| Region | Requested bytes | Elapsed ns | result / errno |
|---|---:|---:|---|
| Genome | 3,263,168,512 | 2,078,505 | 0 / 0 |
| SA | 25,348,276,224 | 5,245,819,882 | 0 / 0 |
| SAindex | 1,564,475,392 | 664,771 | 0 / 0 |

Total synchronous collapse wall duration: **5.248563158 seconds** in this
strict diagnostic gate. It is not free. No timing-gain inference is made from
this run. The same new binary subsequently passed full-depth parity as well;
see the closing section and `bench/PHASE2C-full-depth.md`.

## Verified bounded experiment

Spark root: `~/uni-rnaseq-probe-lab/collapse-matrix-20260908`.
Runner: `~/uni-rnaseq-probe-lab/collapse-matrix-runner-20260908`.
Completed monitor: `proc_54c22dbfc7ac`, launcher PID 802753. Single resource lock, external
7200-second bound, 1200-second stage bounds; no automatic retries.

Three rotated repeats of all four arms, using the SAME new binary:
- advised CPU bypass, collapse off;
- GPU, collapse off;
- advised CPU bypass, collapse on;
- GPU, collapse on.

THP=1 and eviction=1 everywhere. Every invocation is a fresh process after an
untimed complete index-file read. **There is no separate full-workload warmup in
this cohort.** This deliberate new protocol measures fresh-process startup,
collapse and alignment together; it is not directly comparable with the earlier
warmup-based timing cohort. Comparisons will be internal to this matrix.
All target initialization and collapse work are included in GNU user+system CPU
and wall time. No collapse duration will be subtracted. The untimed file prewarm
is retained as an explicit cache-condition preparation, not silently presented
as end-to-end deployment time. These remain isolated slice measurements, not
measured full-depth saturated throughput.

A common 2-second observer records process/host state and large anonymous VMAs
for every arm. Its overhead is outside target CPU but may perturb wall time;
these are instrumented experimental timings, not an unobserved production run.

## Acceptance and interpretation

- Append every accepted row durably, with raw per-stage argv/env/timing/stderr,
  positive GPU sidecar checks, all collapse diagnostics and VMA summaries.
- Collapse-off must report skipped-policy for all three calls. Collapse-on must
  report three successful calls. Missing/failed calls stop the matrix.
- Every arm must have a fully resident sample of the three large advised VMAs.
  For collapse-on, the final such sample must match the requested range sizes
  within alignment-boundary slack and have at least the requested huge bytes
  in each candidate VMA. Sizes/flags are an observational identification, not
  allocation ownership pointers; retain all raw mappings for review.
- Incomplete matrices remain partial evidence, never silently reduced to a
  selected successful subset. No change to the original ordered-output gate.
- Credit CPU-only page-layout gains to CPU. Quote GPU's increment against
  **CPU with collapse also enabled**, and separately compare GPU-on versus
  GPU-off within this same cohort. Both comparisons include collapse cost.

## Runner checks

Added `collapse_matrix.py` and `test_collapse_matrix.py`, reusing the tested
measurement and residency helpers. Three focused tests pass on Mac and Spark:
missing/failed/off-policy intervention rejection, residency coverage rejection,
and complete rotated arm enumeration. Two residency-parser tests pass; replay
of actual retained bypass/GPU smaps streams also succeeded. The frozen seven
Python runner/helper hashes match the reviewed local files.

## Verified result — small layout benefit, narrow threshold crossing

All **12/12** rows completed and were independently checked against their raw
GNU time rows, controls and diagnostics. All six GPU arms positively consumed
GPU output with zero faults, rejections and live charges. No row was discarded.
Tracked evidence: `bench/evidence/collapse-matrix/`, including
`verified-summary.json`, per-arm raw files, all live residency observations and
the runner's durable `accepted.jsonl`.

| Mean of three repeats | CPU seconds | CPU sample SD | Wall seconds | Huge-page coverage range |
|---|---:|---:|---:|---:|
| Advised CPU, collapse off | 693.5233 | 2.0387 | 59.7067 | 66.198–78.504% |
| GPU, collapse off | 637.6600 | 28.6689 | 55.2867 | 76.419–92.269% |
| Advised CPU, collapse on | 679.5267 | 7.8620 | 61.5700 | 99.9746–99.9815% |
| GPU, collapse on | 624.6000 | 6.4980 | 53.4667 | 99.9746–99.9815% |

Collapse-on delivered the requested huge bytes in each candidate VMA for every
on-arm; the coverage fractions above refer to entire advised VMAs, including
small boundary regions outside the inward-aligned collapse requests.

Cost-inclusive comparisons within this cohort:
- **GPU versus advised CPU, both collapse-on:** 54.9267 CPU-s saved,
  **8.0831% less CPU** and **13.1612% less wall time** on the ratio of means.
- **Collapse benefit for GPU:** 13.06 CPU-s, **2.0481% less CPU** versus GPU-off;
  3.2919% less wall time.
- **Collapse benefit for CPU:** 13.9967 CPU-s, **2.0182% less CPU** versus CPU-off;
  wall time increased 3.1208%.
- GPU's off-policy comparison is 8.0550% less CPU than CPU-off, but those arms
  have markedly unequal achieved page coverage and are not a layout-matched
  estimate of GPU-only benefit.

The approximately 2% gain appears in **both** CPU and GPU arms: credit it to
layout stabilization, not GPU seed search. The controlled on/on mean narrowly
exceeds the 8% numerical target, but it is not a robust threshold result: the
three within-repeat CPU reductions were **10.0198%, 8.1224%, 6.0621%**.
Three repeats, rotated rather than perfectly position-balanced, do not establish
a tight performance guarantee. Do not reinterpret the earlier 5.62% cohort as
an 8.08% result; workload warmups/observation protocols differ.

Collapse's mean synchronous wall duration was **3.627627 s in CPU-on** and
**2.314142 s in GPU-on**. These durations were included, not subtracted from
the measured invocation times. Off-policy microsecond diagnostics are not
collapse operations. No separate CPU attribution to the syscall is claimed.

### Raw invocation rows

| Repeat | Arm | Wall s | User s | System s | CPU s |
|---:|---|---:|---:|---:|---:|
| 1 | bypass-off | 59.10 | 660.81 | 33.61 | 694.42 |
| 1 | gpu-off | 48.10 | 586.39 | 30.66 | 617.05 |
| 1 | bypass-on | 68.83 | 643.67 | 44.37 | 688.04 |
| 1 | gpu-on | 53.10 | 583.58 | 35.52 | 619.10 |
| 2 | gpu-off | 62.59 | 626.49 | 43.91 | 670.40 |
| 2 | bypass-on | 59.98 | 642.89 | 35.11 | 678.00 |
| 2 | gpu-on | 49.47 | 590.99 | 31.94 | 622.93 |
| 2 | bypass-off | 59.34 | 657.08 | 34.11 | 691.19 |
| 3 | bypass-on | 55.90 | 641.09 | 31.45 | 672.54 |
| 3 | gpu-on | 57.83 | 590.61 | 41.16 | 631.77 |
| 3 | bypass-off | 60.68 | 659.81 | 35.15 | 694.96 |
| 3 | gpu-off | 55.17 | 587.30 | 38.23 | 625.53 |

## Disposition and remaining gate

Keep collapse **opt-in**: it is useful for matched-layout experiments and showed
a marginal cost-inclusive benefit, not a proven production default or a large
GPU-specific optimization. No more scheduler changes are justified by this
experiment. No saturated/full-depth throughput result or new stock-relative
pipeline projection is inferred from this matrix.

The unchanged matrix binary subsequently completed strict full-depth parity against the
preserved stock golden, with collapse=1 and eviction=1 explicitly recorded:
`~/uni-rnaseq-probe-lab/full-depth-collapse-20260908`, monitor
`proc_407e86129a87`, launcher PID 814194. The full-depth runner gained a tested
explicit `--collapse-index` switch (default off), and common stage-env evidence
now retains that flag. 31 local runner tests and nine full-depth tests on Spark
passed. Frozen runner hashes and binary identity were checked. Full-depth
acceptance subsequently passed: 78,619,701 pairs / 211,097,418 ordered records,
570,386,243 GPU chains, zero strict mismatches/faults/rejections/live charges.
The binary, inputs and comparator identities and all enabled policy flags were
independently checked. All three collapse calls succeeded. See
`bench/PHASE2C-full-depth.md` and `bench/evidence/full-depth-collapse/`.
This correctness result does not improve the matrix's statistical confidence.
