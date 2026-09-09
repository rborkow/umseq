# Full-depth STAR gates — repaired and opt-in-collapse candidates verified

## Current status

**The collapse-enabled candidate is now full-depth verified**, using the same
binary as the cost-inclusive matrix:
`a6c61b2c5659b2fdd4651c86417e9d66fb2520f9472740760711d6e2a9c70bb0`.
Stock, both FASTQs and the unchanged comparator have exactly the same identity
records as the earlier repaired-binary gate below. The explicit strict, THP,
eviction and collapse settings all read back as 1.

- **78,619,701 pairs; 211,097,418 ordered alignment records matched.** Only the
  existing SAM-header executable/output-prefix normalization was applied.
- **570,386,243 GPU chains consumed**; zero batch faults, rejected results,
  shift/flag/step-count mismatches and live byte/request charges at finish.
- All three collapse calls succeeded; recorded total synchronous collapse wall
  duration was **2.865389408 seconds**. Sidecar reports 30,175,920,128 anonymous
  huge bytes for the index. Per-VMA matching is established by the separate
  matrix, not inferred solely from successful syscall returns.
- Strict diagnostic invocation: **235.95 wall / 3285.00 user / 93.97 system
  seconds**, 3378.97 CPU-s, max RSS 38607504 KiB, exit 0. This is **not a
  performance comparison**: strict mode repeats searches through the CPU oracle.

Evidence: `bench/evidence/full-depth-collapse/`, host
`~/uni-rnaseq-probe-lab/full-depth-collapse-20260908`. Monitor
`proc_407e86129a87` completed; the orchestrator independently verified identity,
environment, parity, counters and collapse diagnostics. The flag remains opt-in.
The matrix's 8.0831% GPU/advised-CPU mean is narrow, not a robust threshold claim;
full-depth correctness does not upgrade its statistical confidence.

## Earlier repaired-binary gate (retained separately)

**Full-depth ordered-output parity verified on the repaired candidate: 78,619,701 input pairs and 211,097,418 alignment records.** The unchanged comparator returned `PARITY_MATCH_COUNTERS_ONLY`; only its documented SAM-header executable/output-prefix normalization was applied. Both STAR and comparator exited zero.

The strict sidecar reports **537,438,099 GPU chains and 739,856,548 steps consumed**.
Batch faults, rejected results, shift/flag/step-count mismatches, live bytes and
live requests at finish are all zero. Candidate SHA-256 and actual strict/THP/
cache environment were independently read back and verified. The full-depth
golden was reused with mandatory input identities, pair-count and exact argv
checks; no sorting or output-field filtering was added.

Local full-depth evidence: `bench/evidence/integrate-full-depth/lat-close/`
contains `parity.json`, `gate-summary.json`, `identity.json`, executed argv,
strict environment, raw GNU time and final log. The complete SAMs remain on Spark.

One **strict correctness diagnostic**, not a performance comparison:

| Stage | wall s | user s | sys s | user+sys s | max RSS KiB | exit |
|---|---:|---:|---:|---:|---:|---:|
| Repaired full-depth, strict/oracle on | 232.92 | 3386.67 | 118.27 | 3504.94 | 42617912 | 0 |

Strict mode repeats consumed searches through the CPU oracle. Its timing must
not be compared with stock to infer accelerator speed. Consumption was
537,438,099 of 610,565,136 submitted chains (88.02%); fallback counters are
events with documented overlap, not a disjoint work partition. Fresh strict-off
timings and attributed profiles remain required.

Candidate SHA-256: `ef22723c51540aa2a12178da48f5eb0c3df759965ab7e594f7f6257efba38b6b`.
The slice comparator reports 53,710,530 alignment records, with only the existing
header executable/output-prefix normalization. The sidecar records 145,953,932
GPU chains consumed; batch faults, rejected results, shift/flag/step-count
mismatches and both finish-time live-charge counters are all zero. CUDA backend
build/tests, enabled ABI check and STAR build also exited zero. Slice evidence:
`bench/evidence/lat-contract-close/slice-{gate-summary,parity,binary}.json`.

Full-depth host root: `~/uni-rnaseq-probe-lab/full-depth-lat-close-20260908`.
Launched PID 706859; monitor `proc_9e7e73431d85`. The same binary hash was checked
before launch. Shared lock held, outer timeout 7200 seconds, each stage capped at
3600 seconds. THP and post-load cache eviction explicitly enabled to match the
slice gate; this does not decide the later cache-policy screen.

The runner now takes `--drop-index-cache 0|1` rather than allowing its environment
scrub to silently turn eviction off. Regression observed RED then GREEN; 22
local runner/source tests and eight full-depth runner tests on Spark pass.
Runner SHA-256: `2a991ce5e02a03a89e3228e8d38417963d6581cf082c489fd3c01a7728532625`.

The original round-9 binary is not retroactively approved. This result applies
to the repaired binary named above; local contract review is recorded in
`docs/review-integrate-lat-prefetch-fix.md`.

## Evidence

Host root: `/home/rborkows/uni-rnaseq-probe-lab/full-depth-stock-20260908`.
Input/index preflight: `/home/rborkows/uni-rnaseq-probe-lab/full-depth-preflight-20260908`.
Local summaries: `bench/evidence/integrate-full-depth/`:

- `preflight-20260908.json`: both full FASTQ MD5 values match the manifest, both decompress to 78,619,701 complete four-line records, gzip CRC verified at EOF. Stock hash recorded, host24 integrated hash matches accepted binary.json.
- `index-checker-20260908.json`: each present role-2 index file matches the prior binding manifest; comparator matches its accepted hash.
- `stock-20260908-argv.json`: actual stock argv, including full FASTQ paths.
- `stock-20260908-time.tsv`: raw diagnostic GNU time row.
- `stock-20260908-Log.final.txt`: full stock final log.

Stock executable `/home/rborkows/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR`, SHA-256 `f84493ef8c9a6d39e60264c0c9e0c6da388e8a455e6c21c1a1b9a61f52662a29`.

The stage read back as `PASS_STOCK_ONLY_NOT_PARITY`, exit 0. `Log.final.out` reports exactly 78,619,701 input reads (paired-input records). Do not label that as an alignment-record count.

| Stage | wall s | user s | sys s | user+sys s | max RSS KiB | exit |
|---|---:|---:|---:|---:|---:|---:|
| Full-depth stock, one diagnostic run | 175.82 | 2863.32 | 68.92 | 2932.24 | 33009644 | 0 |

This row includes startup, input children and output writing as GNU time accounts for them. It is one correctness-baseline run, not a controlled repeat or a performance speedup. No comparison with the 20M means is inferred.

Output sizes read back on Spark:

| File | bytes |
|---|---:|
| Aligned.out.sam | 51288594018 |
| SJ.out.tab | 10312326 |
| Log.final.out | 2034 |

These sizes establish retained artifacts, not content equality. The comparator must read every ordered SAM record, SJ bytes and non-timing final-log fields; only its existing exact header executable/output-prefix normalization is permitted. No sorting, field filtering or aggregate-rate substitute.

## Remaining acceptance

1. Independent local review and Spark enabled build/strict slice gate: complete.
2. Repaired full-depth run and unchanged comparator: complete.
3. Positive GPU consumption, zero faults/rejections/strict mismatches, zero live charges: verified.
4. Strict diagnostics recorded above. Cache-policy screen, fresh strict-off
   profiling/repeats and the collapse matrix are documented separately; the
   failed cache screen was not converted into a successful comparison. Separate
   GPU/CPU Tier-0 remains pending. None of these is a measured full-pipeline
   throughput result.
