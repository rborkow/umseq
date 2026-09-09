# Same-binary residency diagnostic — unequal achieved huge-page backing

## Verified evidence

Spark: `~/uni-rnaseq-probe-lab/residency-lat-close-20260908`.
Tracked evidence: `bench/evidence/residency-lat-close/`, including
`verified-residency-summary.json`, full per-process observation JSONL, exact
argv/environments, prewarm byte records and STAR logs.

Both invocations completed on the full-depth-accepted binary. The sampler
recorded **43 bypass samples and 40 GPU samples**, with actual target executable
identity checked. GPU consumed 146,229,964 chains with zero faults, rejected
results, strict-mismatch counters and live charges at finish. Strict was off;
this does not replace the previous full-depth strict parity gate.

Same 20M slice, 20 threads, explicit THP-on/eviction-on, strict-off. Full untimed
index-file read before each invocation; no separate workload warmup. No global
memory changes. This is a residency diagnostic, **not a replacement timing
matrix**. Sampling overhead and order are not controlled for a speedup claim.

## What was observed

Three large writable anonymous VMAs carry `hg` and `THPeligible=1`. Their byte
extents are consistent with the advised page-interiors of STAR's SAindex, SA
and Genome allocations. These are identified by sizes and advice flags, not a
runtime dump of the original array pointers. Other large anonymous VMAs were
retained in the raw evidence and not conflated with the index candidates.

For samples in which all three advised VMAs had `Rss == Size`:

| Region by extent | Extent bytes | Bypass first-full huge coverage | GPU first-full huge coverage |
|---|---:|---:|---:|
| Prefix index / SAindex-sized | 1565868032 | 92.95% | 47.95% |
| Packed SA-sized | 25352257536 | 99.25% | 96.94% |
| Genome-sized | 3263377408 | 99.93% | 99.93% |

Combined coverage: bypass **98.99482%** throughout the fully resident samples;
GPU **94.72151–94.77710%**. First-full huge totals are 29,878,124,544 bytes in
bypass and 28,588,376,064 in GPU. GPU later rises to 28,605,153,280 bytes. The
first GPU total agrees exactly with its setup-time sidecar. All values are
calculated from observed per-VMA AnonHugePages/Size, not machine-wide deltas.

Thus identical THP settings **did not produce equal achieved backing in this
pair**. This is a demonstrated confound; it is not proof that layout caused the
entire timing difference or that the preceding nine timing runs had these exact
layouts. In particular, the prefix-index-sized allocation is much less covered
in the GPU invocation, while the Genome-sized allocation is essentially equal.
No source-level duplicate-work or synchronization optimization is selected from
this observation alone.

## Raw diagnostic timings (not a speedup estimate)

| Arm | Wall s | User s | System s | Max RSS KiB | Exit |
|---|---:|---:|---:|---:|---:|
| bypass | 46.30 | 642.23 | 22.00 | 32698256 | 0 |
| GPU | 45.88 | 586.59 | 28.92 | 38761684 | 0 |

These do not supersede the nine rotated rows in
`bench/PHASE2C-fresh-timing.md`, whose 8% incremental target remains unmet.

## Bounded next experiment

Terra is implementing **opt-in, Linux-only post-load MADV_COLLAPSE** in the
existing generated STAR patch, not a new allocator or scheduler. Default off;
THP=0 disables it too. Restrict each call to complete 2-MiB-aligned extents inside
owned arrays, after index reads/cache policy and before device initialization.
Record failures and elapsed time; do not infer residency from syscall success.

Card: `.hermes/cards/P2C-COLLAPSE-DISQUALIFIER.md`, process
`proc_6ff83c1e265c`. No worker permission to run on Spark or commit. Orchestrator
must independently review, build/re-gate the new candidate, and compare collapse
off/on on the **same new binary**, applying the intervention to both CPU and GPU
arms. Include collapse CPU/setup time and reject the hypothesis if achieved
backing fails to stabilize or its total cost erases any benefit. Do not raise a
gate, force a successful retry, or claim a gain from partial resident coverage.

This is an evidence-backed cheap disqualifier, not accepted production behavior.
