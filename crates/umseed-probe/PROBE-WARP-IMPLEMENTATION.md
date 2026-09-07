# PROBE warp continuation — 2026-09-06

**Current orchestrator status:** real Spark CUDA build, host tests and measurements
are complete. Warp reaches 5.95× CPU20 at64k; thread remains faster (~10.21× at1M)
and is selected. Both match outputs/logical counters. Independent bounded review
found no blocking probe defect. See `bench/PHASE2C-seed-probe.md` and
`bench/evidence/seed-probe-warp-host4/`; neither result approves STAR correctness.

## Historical worker handoff (superseded for host status)

Implemented `--variant warp`. **Warp host performance NOT RUN.** No network, SSH,
Spark actions, commits, evidence rewrites, or downstream implementation. The user
reports completed thread huge-page agreement and GPU/CPU20 ratios about 9.99–10.20;
that supplied evidence authorizes this conditional implementation, not a warp speedup.
This report supersedes only the deferred-warp statements in PROBE-IMPLEMENTATION.md.

The workload remains **synthetic/optimistic proof-of-capability**, SA_SEARCH_FULL,
not a correctness gate or SEED-CUDA. Actual SPLIT inner mean N~50 and binary-loop
iterations~5.36 differ from PROBE's ~35 loops. Supplied SPLIT parity on 20M/5M and
~99.79% inner compared bytes/~93.64% inner outer requests require no direct-extension
tag. The reported +31.8% CPU counter overhead on 20M is not adjusted; bytes != CPU fraction.

## Implementation

- Four complete warps per 128-thread block; one request per warp. Entire unused
  final warps return before request reads. Counts need not be multiples of 4 or 32;
  `start` remains a request offset, and output indices remain local to the launch.
- Lane 0 unpacks the original unaligned packed SA; a 64-bit shuffle broadcasts it.
  All lanes retain identical endpoint/history/search state, GstrandMask, reverse
  coordinates, all four directions and bounds checks. The same comparator services
  both endpoints, midpoint search, and both findMultRange expansions.
- Each lane compares one in-range byte in a 32-byte chunk. Two full-mask ballots
  reduce mismatches and ordering; first-set-bit selects the first mismatch. No
  lane-specific return precedes a collective. Tail lanes issue no sequence loads.
  Only lane 0 stores output/stats. Exact comparisons preserve the prior ordering.
- All output/status/stat fields must match CPU/thread exactly. Logical compared
  bytes stop at the first mismatch. **Speculative in-range reads after that mismatch
  are actual extra loads excluded from compared_bytes and logical_gpu_GBs.** These
  metrics are not physical traffic. `mean/max_warp_byte_imbalance` remain software
  group-of-32 request-cost proxies, not activity counters or actual intra-warp
  imbalance for warp-per-request. TSV comments now explain both limitations;
  columns are unchanged and variant provenance names the selected kernel.
- Original `seed_probe` API still selects thread. New `seed_probe_variant` uses the
  same checked leases, scoped CPU callback, stream drain, reclaim/fence and existing
  context failure handling. No allocation/copy/registration changes. CPU20, scalar
  comparator, validation, warmup, timing, overlap, page and provenance checks retain
  their computation. Request/output/stats/config records remain ABI v1; the private
  C launch signature adds a u32 selector, so rebuild shim and Rust together.

## CLI — orchestrator only, under the existing lock/watchdog

The wrapper defaults to thread; set `PROBE_VARIANT=warp`. Retain the same verified
request file, completed SPLIT evidence, source/binary archiving, and page controls:

```bash
export PROBE_VARIANT=warp
export PROBE_REQUESTS=/absolute/path/to/existing/probe-requests.bin
export PROBE_SPLIT_EVIDENCE=/absolute/path/to/completed-SPLIT-evidence
export PROBE_SPLIT_SCOPE=inner
bash crates/umseed-probe/scripts/probe-spark.sh huge
# After successful warp huge agreement, supply its TSV for the optional 4K round:
export PROBE_HUGE_AGREEMENT=/absolute/path/to/warp-huge/probe.tsv
bash crates/umseed-probe/scripts/probe-spark.sh small4k
```

Underlying CLI is unchanged except `--variant warp`: counts
`64000,256000,1000000,4000000`, `--repeats 3 --overlap`, and mandatory
`--cutoff-unix 1788750000` (2026-09-06 20:00 PDT). No authorization extension.

## Local verification

- `cargo test --offline -p umseed-probe -p umgpu`: 12 tests pass. Includes unchanged
  Rust CPU/scalar transport agreement on 2,048 requests plus full output/counter
  agreement for the same requests through a 32-host-thread collective emulator.
- 1,255 focused cases checked in every emulated lane: mismatch positions around
  31/32/63/64, lengths 1–97, trusted prefixes, all directions, genome symbols 4/5,
  exact/empty comparisons, rejection paths, and packed widths 33/54. Mapping checks
  cover 0,1,2,3,4,5,31,32,33,127,128,129 requests and nonzero start. Host emulation
  uses bounded collective waits; it does not execute a CUDA kernel.
- Focused C++ harness passes `-fsanitize=address,undefined`; C++17 strict warnings
  pass. `cargo clippy --offline -p umseed-probe -p umgpu --all-targets -- -D warnings`
  passes. rustfmt, clang-format dry-run/Werror, shell syntax and diff checks pass.
- `cargo rustc --offline -p umgpu --lib --target-dir /private/tmp/probe-warp-rust-cfg
  -- --cfg 'feature="cuda"' --emit=metadata` passes: Rust cfg/type check only.
- `PROBE_CLANG=/opt/homebrew/Cellar/llvm/22.1.8_2/bin/clang++ bash
  crates/umseed-probe/scripts/check-probe-cuda-syntax.sh` passes host/device syntax
  checks against explicitly mocked declarations (sm_80 parser target). **Not nvcc,
  CUDA SDK validation, device code generation, linkage, or a GB10 build.**

Real nvcc build, SASS/load inspection, GPU output/counter agreement (including odd
counts), full-index measurements and overlap performance remain for the orchestrator
on GB10 under lock. No independent speedup or hardware divergence claim is made.

Changed files: `umgpu/shim/seed_probe.{h,cu}`; probe sections of
`umgpu/src/{lib,cuda,stub}.rs`; `umseed-probe/src/main.rs`;
`umseed-probe/scripts/{probe-spark,check-probe-cuda-syntax}.sh`;
`umseed-probe/tests/{probe_search.rs,probe_transport.cpp,probe_warp.cpp,probe_warp_host.h,
probe_cuda_syntax/cuda_runtime.h}`; this report. No build.rs change was needed.
