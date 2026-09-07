# P2C seed GPU PROBE — both variants verified; thread selected

**PROBE ONLY.** Real full human STAR index, synthetic requests constructed from
real read bytes. Same-algorithm GPU/CPU agreement is not upstream correctness,
STAR integration, or measured RNA-seq throughput.

## Current result and decision

The final same-binary comparison selects **thread-per-request**: **10.21× the
20-core CPU control at 1,000,000 requests**, with median GPU throughput
**56.83 million requests/s**. Warp-per-request reaches **5.95× CPU20 at 64,000**,
but is slower than thread at every tested batch. Both use median paired ratios
over three repeats. The earlier thread round reached **10.20× at 256,000**;
the small change in best batch is not an established tuning advantage.

**PROBE passes the ≥2× screen.** It funds REPLAY-SCALE under the existing
time-bounded authorization; it does not approve SEED-CUDA, upstream fidelity,
STAR integration or pipeline throughput. All figures are warmed, resident,
synthetic full-SA probe numbers. The conditional warp round is now complete.

SPLIT independently selects **inner-only**: 99.78875% / 99.78868% of compared
bytes on the real 20M / 5M full-index paired runs. Stock/counters mapping outputs
match. Counters add 31.80% / 27.36% CPU overhead, unadjusted. These byte shares
are not fractions of the previously measured 39.03% seeding CPU work.
See the seed worktree's `experiments/star-seed/RESULTS-SEED-SPLIT.md`.

## Original thread round, fully huge-page-backed resident index

| Requests | Median CPU20 wall, s | Median GPU wall, s | Median paired GPU/CPU20 throughput ratio |
|---:|---:|---:|---:|
| 64,000 | 0.012353651 | 0.001264454 | 9.9896× |
| 256,000 | 0.047016210 | 0.004602355 | 10.1957× |
| 1,000,000 | 0.179423890 | 0.017625577 | 10.1692× |
| 4,000,000 | 0.713639490 | 0.071817255 | 9.9369× |

Each cell is derived from all three repetitions, not a selected best run.
The ratio column is the median of paired `cpu_s/gpu_wall_s`, not the quotient
of the two separately reported medians. GPU wall includes the wrapper/lease path;
CUDA event times and device-call wall are retained separately in the raw TSV.
Each batch size first accumulated at least one second of GPU warmup, consuming
and checking every output. All measured outputs **and logical counters** match
the 20-worker independent Rust control over the same resident bytes.

### Page-size control (64,000 identical requests)

| Backing | Median GPU wall, s | Median paired GPU/CPU20 throughput |
|---|---:|---:|
| Verified huge pages | 0.001264454 | 9.9896× |
| Verified `MADV_NOHUGEPAGE` 4K | 0.194113454 | 0.0885× |

The GPU is **153.5× slower** on 4K pages than the huge-page GPU run; it loses
against CPU20 by approximately 11.3× on that control. CPU changes too, so this
page comparison uses GPU wall directly rather than dividing the speedup columns.
SA used HugeTLB fallback; Genome/SAindex used THP. All primary regions had 100%
verified huge coverage; the control had zero huge coverage. This strongly supports
the page-management mechanism, not a claim that this hardware beats a discrete
GPU with a fully VRAM-resident index (no such comparator was tested).

### Contention / overlap control

Both arms received equal disjoint halves; standalone controls used those same
halves. At 4M total requests (2M each), medians: CPU slowdown **1.0409×**, GPU
slowdown **1.0685×**, combined throughput **10.54M requests/s**. This demonstrates
modest contention in this bounded overlap experiment, not optimal scheduling:
equal halves leave the much faster GPU idle after finishing, and combined
throughput is only about 19% of GPU-only throughput. A proportional/continuous
scheduler and actual pipeline capacity remain unmeasured.

## Final thread/warp round — same binary and request artifact

`bench/evidence/seed-probe-warp-host4/`, private source snapshot `probe-source-v4`:

| Requests | Thread median GPU wall, s | Thread median paired GPU/CPU20 | Warp median GPU wall, s | Warp median paired GPU/CPU20 |
|---:|---:|---:|---:|---:|
| 64,000 | 0.001237413 | 10.1952× | 0.002155335 | 5.9456× |
| 256,000 | 0.004597169 | 10.1546× | 0.008720143 | 5.4136× |
| 1,000,000 | 0.017596015 | 10.2052× | 0.031576178 | 5.7692× |
| 4,000,000 | 0.071824403 | 9.9622× | 0.127147259 | 5.6850× |

At 1M, warp GPU wall is **1.7945× thread GPU wall**. Retain warp opt-in as a
measured alternative, not the candidate. Warp's speculative in-range loads beyond
the first mismatch are excluded from scalar-equivalent logical counters; identical
logical work does not mean identical DRAM traffic. No hardware-divergence claim.

Warp's 64k 4K-page control is **0.1337× CPU20**, median GPU wall **0.128413885s**:
**59.58× slower** than its huge-page GPU run. Both variants remain page-sensitive.
At 4M total in the final equal-half overlap control, thread combined throughput is
**10.5664M requests/s**, CPU/GPU slowdowns **1.0576×/1.0912×**; warp combined is
**10.0308M**, slowdowns **1.0907×/1.0530×**. Equal halves still underutilize the GPU.

Verification: actual CUDA build and nine Spark host tests passed; both kernel SASS
bodies contain **60 LDGs**. All **44** staged file hashes matched local sources
at verification, before the status-only update to `PROBE-WARP-IMPLEMENTATION.md`;
all executable-source hashes still match.
Each variant has 12 isolated and 12 overlap rows; the warp 4K control has six rows.
Orchestrator independently checked row/repeat identities, 20-worker controls,
overlap denominators, and identical cross-variant output checksums/logical counters.
Request and boundary SHA256s match the original archived run. Huge-page region
reports show full coverage; the 4K control shows zero. Overall remote exit is zero.
Odd/partial-tail tests passed in **host emulation**, not at unsupported tiny CUDA
CLI batch sizes. This limitation does not become an upstream correctness approval.

Luna's independent review (`.hermes/cards/P2C-PROBE-WARP-REVIEW-RESULT.md`) found
no blocking probe defect. Its qualifications are tracked explicitly:
- Legacy TSV `mean_request_bytes` / `max_request_bytes` mean **logical compared
  bytes per request**, not read/query length. Preserve archived schemas; rename in
  any successor reporting API before use. The same applies to the original table.
- Only the archived, orchestrator-verified wrapper runs are accepted evidence.
  CLI `--split-provenance` is an annotation, not authenticated gate enforcement;
  arbitrary direct invocations cannot self-approve SPLIT.
- The wrapper's overlap-verifier omission was covered by the orchestrator's
  independent repeat/count/denominator checks above; no raw evidence was rewritten.

Failed attempts are preserved in `seed-probe-warp-host1/` (missing Linux `<cstdio>`),
`host2/` (relocated cached test binary still referenced the old source path), and
`host3/` (wrapper requested unsupported tiny counts). The include was fixed and v4
built from fresh targets; the supported CLI protocol was not weakened. These failures
precede the successful measured round and are not numerical/biological mismatches.

## Scope, workload and provenance

- Real resident arrays: Genome 3,263,384,238 bytes (including padding), packed SA
  25,352,264,482 bytes, SAindex 1,565,873,619 bytes; total **28.11 GiB**.
  Active nGenome=3,263,383,838; nSA=6,146,003,510; GstrandBit=32;
  621,870 annotated junctions, overhang100, no mapping-time insertion.
- Index loading/validation took **62.58s** in the primary round and is excluded
  from warmed throughput. This startup cost matters to eventual deployment.
- Requests: 4M deterministic SA_SEARCH_FULL requests, seed188140, from500,000
  sampled ACGT reads after visiting4,005,246 reads in ERR188140_20M mate1.
  Grid20, both read directions, remaining read length; pinned STAR default
  `seedSearchLmax=0` means unlimited, not an assumed cap of50.
- **Representativeness limit:** this synthetic path searches the full SA, not
  STAR's prefix-narrowed intervals. Probe mean loops~35 versus SPLIT's actual
  inner binary-loop mean5.359; comparison/request distributions differ. The
  positive screen funds replaying actual requests; it does not predict their speed.
- Three whole index-array hashes agree between on-disk and loaded snapshots.
  Actual CPU/GPU lease pointers are identical; `host_register=false`, ATS path.
  The byte-copy value is a documented constant, not a dynamic copy detector.
- SASS of the actual thread kernel has **60 LDG instructions**, and unconditional
  per-request outputs are consumed and compared; the kernel is not an empty sink.
- `mean_request_bytes`, `mean_compare_bytes`, loop/gather counts are logical work,
  not DRAM transaction bytes. `logical_gpu_GBs` is not physical memory bandwidth.
  `mean_warp_byte_imbalance` is a software group-of32 request-cost proxy, not a
  measured warp-activity/divergence counter.
- Luna's measurement review found no blocking defect in the warmed thread
  comparison; its reporting/provenance qualifications are applied here. This
  review is not an independent biological correctness approval.

## Evidence

- `bench/evidence/seed-probe-build-host1/`: build/tests, all39 source-file hashes
  matched against the local compiled sources at staging, binary SHA256, SASS.
- `bench/evidence/seed-probe-measure-host1/`: all24 primary rows (12 isolated,
  12 overlap), six4K-control rows, command/exit/time records, checksums, pointers,
  smaps reports, device/clocks and SPLIT-boundary evidence. Final `probe.status`
  reports successful exits for both runs and overall exit0.
- The400MB request artifact remains on Spark at
  `/home/rborkows/uni-rnaseq-probe-lab/probe-measure-host1/probe-requests.bin`;
  SHA256 `c72a0eea30d9caef930481be96f901261dfed54ea2600a65fcf3087b530ac428`.
  The local archive retains its generation/provenance, not the large binary.
- `.hermes/cards/P2C-PROBE-MEASUREMENT-REVIEW.md`: independent bounded code review.

No installed STAR, services, shared umem code, or BAM/QC code was changed.
All heavy jobs held the shared lock; workers and jobs have an absolute cutoff
of2026-09-06 20:00 PDT under the user's time-bounded downstream authorization.
