# P2C REAL-REQUESTS — real capture replay and performance

## Decision result

**6.60× faster than the CPU-20 replay control on 999,914 captured STAR inner requests. Both CPU and thread-GPU replay match all 999,914 captured STAR four-output tuples; zero mismatches, zero skipped requests.** The ≥5× representativeness threshold is met. This is a resident-index replay result, not measured STAR or pipeline throughput. Stop for the user's decision before SEED-CUDA/full-rigor continuation or STAR integration implementation.

## Timings

Spark GB10, thread-per-request kernel, three repeats. GPU wall includes the harness's dispatch/synchronization path. Parsing, materialization and index loading are outside the timed rows; the real run reports 57.745112 s index load/validation and 59.025855 s setup. Ratios below divide medians, not best times.

| Workload | Requests | CPU-20 median | GPU wall median | CPU-20 / GPU |
|---|---:|---:|---:|---:|
| Captured real, huge pages | 999,914 | 57.488302 ms | 8.716208 ms | **6.59556×** |
| Synthetic full-SA, fresh control | 1,000,000 | 182.673378 ms | 17.602511 ms | **10.37769×** |
| Captured real, forced 4 KiB | 999,914 | 72.936055 ms | 1,040.492576 ms | **0.07010×** |

Real CPU-20 repeats: 61.748798 / 57.488302 / 57.398270 ms. GPU: 8.710415 / 8.716208 / 8.728271 ms. Paired ratios: 7.08908 / 6.59556 / 6.57613×. GPU range/median is 0.205%; the first CPU repeat is higher, so do not claim all timings are within 1%. All real isolated repeats, including the 4 KiB control, have checksum `6bfa80070ee2d509`.

The real workload reduces the synthetic advantage but does not collapse toward 2×. Logical dependent gathers/s are 1.139 G/s real versus 2.116 G/s synthetic. These are logical comparator/SA-gather counters, not measured physical DRAM traffic.

## Oracle and capture reuse

- Reused all 20 original `real-requests-host1/star/worker-*.ssir` files: 999,914 inner requests, 106,402 complete reads. No new STAR capture, stock run or parity run.
- The accepted full-20M paired-end run completed normally and retains `PARITY_MATCH_COUNTERS_ONLY`, with 53,710,530 alignment records. `capture-reuse.json` records that original result and the hashes of the reused trace files.
- The existing SSIR v1 validator accepted every file against the original externally supplied source/index/runtime identities. The current index inventory and active array identities were checked against capture-time evidence.
- CPU replay is compared directly with the captured `(L_out, lo, hi, Nrep)` tuples; GPU outputs are compared field-for-field against that checked reference, with aggregate logical-counter agreement. The harness aborts on mismatch. The successful real and real-4k stages establish **999,914/999,914 for each replay arm**, not merely agreement between two implementations of the same algorithm.
- Parent `verify_projection.py` independently decoded all original SSIR records and checked every converted request field, both sequence buffers, and every STAR oracle tuple. **999,914/999,914 projections, zero skipped.** This also checked the request and oracle SHA-256 values consumed by the probe.
- The generic legacy TSV closing banner still says “same algorithm, not upstream oracle.” The real-specific header and executed `validate_star_tuples` path establish the additional captured-STAR check; the generic banner is not the full acceptance description.

Request SHA-256: `eeb46b2da2e830b9cc9b70af2569b132412eb1bba9582b54037399de2ca65283`.

Captured-tuple sidecar SHA-256: `cb7e6fce3d1d17e3b879c1c0534277ac3c331b7b39d99f39ac5d5617627117f0`.

## Boundary correction, without filtering or format redesign

The full raw audit distinguishes the two predicates in the old combined reverse guard:

| Predicate/case | Count |
|---|---:|
| `N > S+1` | **0** |
| `L_in > S` | **1,317** |
| `L_in = N = S+1` (fully known, empty comparison) | **1,317** |

Thus the actual failure was the known-prefix pointer condition, not an observed `N > S+1` request. The narrow fix permits the fully known empty comparison without forming a post-prefix sequence pointer, in the validator and Rust/shared C++ replay comparisons. The `N ≤ S+1` span guard, SSIR v1 wire format and all caps remain. No clipping or request selection was used to force a pass.

A real multi-piece-read regression fails against the immutable v1 `format.cpp` with the intended reverse-span/pointer diagnostic and passes with the correction. Local ASan/UBSan format tests pass **311 named cases and 2,908/2,908 truncation rejections**. Local Rust tests/clippy and the host CUDA-feature tests pass; the host suite reports 12 passing Rust tests. Held fault/owned-session/cap-matrix work was not resumed.

## Shape distributions

The diagnostic counter includes the search's main binary loop **plus expansion loops**, consistently for both workloads. The earlier ~5.4 SPLIT figure is not substituted for this measured counter. This capture is a bounded per-worker prefix, not the full-20M distribution or a scheduler-global first million.

| Metric | Real mean | Real median / p90 / p99 | Synthetic mean | Synthetic median / p90 / p99 |
|---|---:|---:|---:|---:|
| `N` | 50.0461 | 38 / 75 / 75 | 45 | 35 / 75 / 75 |
| `L_in` | 12.8264 | 14 / 14 / 14 | 0 | 0 / 0 / 0 |
| Main + expansion loop trips | **7.9260** | **7 / 11 / 34** | **35.2513** | **35 / 38 / 43** |
| Logical dependent gathers | **9.9260** | **9 / 13 / 36** | **37.2513** | **37 / 40 / 45** |
| SA interval width | 164,268,428.86 | 37 / 269 / 4,241,200,251 | 6,146,003,510 | 6,146,003,510 / 6,146,003,510 / 6,146,003,510 |

Real `L_in=0` requests: 83,823, retained. The broad tail matters: a small median interval does not imply every real request is shallow. Complete histograms and maxima are in `real-distributions.json` and `synthetic-distributions.json`.

## UM-specific and provenance gates

- Luna's missing-overlap finding is resolved: `--overlap` is present in all three invocations, with three disjoint-half runs each. Real overlap uses 499,957 requests per arm. Output/counter checks pass. Median real CPU slowdown is **1.041335×**; GPU slowdown **1.047683×** against each arm's isolated half-work control. This is a contention/coexecution check, not an optimized load-balancing or pipeline-throughput result.
- Huge-page runs report full coverage for genome, SA, request and read buffers; the forced-4-KiB control loses to CPU-20. CPU/GPU use the resident shared allocations through the existing harness.
- The measured thread kernel contains **60 SASS LDG instructions**. Warm-up, clocks, pointer/page reports, index/request identities, binary hashes, commands and raw timing rows are retained.
- Parent checked **61 measured source-file hashes**, zero differences against the corresponding local sources. Immutable `real-source-v1` and `real-requests-host1` remain untouched; corrected execution used fresh `real-source-v2` and `real-requests-host2`.
- Execution held `/home/rborkows/.cache/uni-rnaseq-resource.lock` under an absolute timeout. The renewed three-hour authorization was capped at Unix `1788756895` (2026-09-06 21:54:55 PDT); the runner completed normally before that cutoff.

## Evidence and stop point

Local:
- `bench/evidence/seed-real-requests-host1/`: reused capture, parity, original failed-convert evidence and `reverse-audit.jsonl`.
- `bench/evidence/seed-real-requests-host2/`: `real.tsv`, `synthetic.tsv`, `real-4k.tsv`, distribution JSONs, `summary.json`, `projection-verification.json`, converted requests/oracle and provenance/stage logs.
- `.hermes/cards/P2C-REAL-MULTIPIECE-RED.log`: expected old-validator regression failure.

Remote: `/home/rborkows/uni-rnaseq-probe-lab/real-requests-host{1,2}` and immutable `/home/rborkows/uni-rnaseq-probe-lab/real-source-v{1,2}`.

The design-only deliverable remains `docs/STAR-INTEGRATE-DESIGN.md`: bounded initial-start speculation with exact matching and stock CPU fallback is recommended, with alternatives and index-memory/C ABI tradeoffs. Actual batching coverage, fallback cost and end-to-end throughput remain unmeasured. **No integration implementation or further rigor/hardening run was started; awaiting the user's decision.**
