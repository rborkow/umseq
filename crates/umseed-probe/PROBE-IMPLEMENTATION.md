# PROBE implementation — 2026-09-06

Implemented the one-request-per-thread PROBE, faithful Rayon CPU20 mirror, resident loader,
deterministic request generator, CLI, agreement checks, page control and disjoint-half overlap.
No Spark, network, SSH, installation, GPU execution, downstream card or commit was performed.
**No GPU performance result exists from this implementation session.** Same-algorithm agreement
is a smoke check, not upstream STAR oracle proof or permission to scale correctness work.

## Changed paths

- `Cargo.toml`, minimal generated `Cargo.lock`: workspace member and existing dependency families.
- `crates/umseed-probe/Cargo.toml`, `src/{lib,main,index,requests,cpu}.rs`.
- `crates/umseed-probe/tests/{probe_search.rs,probe_loader.rs,probe_transport.cpp}`.
- `crates/umseed-probe/scripts/probe-spark.sh`, this report, `LICENSE.star`.
- `crates/umgpu/shim/{seed_probe.cu,seed_probe.h,LICENSE.star}`.
- `crates/umgpu/src/{seed_probe.rs,cuda.rs,stub.rs,lib.rs}`, `build.rs`.

No umem/umbam, docs, bench, cards, seed-worktree or existing kernel changes. Existing shim is
still compiled; the additional optimized PROBE translation unit is archived alongside it.
`umseed-probe/cuda` forwards `umgpu/cuda`; CCCL include and existing stdc++ linkage remain.

## Algorithm and transport

The Rust and C++ implementations retain both endpoint comparisons, three-point endpoint
histories, strict-improvement history shifts, overflow-safe midpoint, upper-on-tie selection,
and separate source-equivalent `findMultRange` expansion. Packed SA uses the original bit
width, an unaligned eight-byte little-endian gather, and `GstrandMask=~(1<<GstrandBit)`.
Reverse genome coordinates are `nGenome-1-a`. Both 200-byte pads contain 5. All four read/genome
directions, including reverse mismatch ordering against genome symbols 4/5, are retained.
Trusted prefixes are not rescanned. Reverse source pointer formation is checked even for
empty comparisons. There is no heap allocation or full-index validation in a search request.

ABI v1 uses consecutive u64 fields (no bool/enums/padding):

- Request, 80 bytes: `tag,s0,s1,read_len,start,length,prefix,low,high,dir`.
- Output, 40 bytes: `length,low,high,count,status`.
- Stats, 48 bytes: `gathers,bytes,loops,comparisons,max_compare,directions`.
- Config, 24 bytes: `n_genome,n_sa,strand_bit`.

Tag 0 is inner search; unknown tags reject. Tag space is reserved for a later source-equivalent
direct-extension request after SPLIT, not implemented speculatively. Status 0 succeeds;
1 means unsupported tag/direction; 2 request bounds; 3 comparator/source-pointer bounds;
4 invalid result interval. CPU rejection or any GPU status/mismatch aborts, without emitting
a successful completion marker. Every returned tuple and instrumentation record is compared;
all outputs enter host checksums, including warmup outputs. Runtime CPU and GPU tuple checks
are outside their compute timing.

All unsafe Rust is in umgpu's Pod/lease/FFI boundary. `seed_probe` checks contexts, extents,
output aliasing, request alignment, ATS and registration-off, retains live leases through a
synchronous stream-draining shim, and scopes an optional CPU callback over immutable Ro views.
The callback cannot access GPU outputs. Reclaim uses existing `submit().wait()` fences.
This provides simultaneous reads of the same resident buffers without umem API changes.
**No missing umem API was found.** The non-CUDA stub reports unavailable rather than simulating
GPU performance. Normal GPU wall includes the scoped helper thread, launch, event setup,
stream synchronization, lease ownership round trip and final reclaim fence.

## Index and requests

The loader derives nGenome from verified `genomeFileSizes`/Genome size, nSA as
`SA_file_bytes*8/(GstrandBit+1)`, and active packed extent as `(nSA-1)*width/8+8`.
SAindex supplies its LE header and offsets; N/absent marker masks follow strand bit +1/+2.
The on-disk format accepted is `versionGenome 2.7.4a`; this says nothing about installed STAR.
No fictional nGenome/nSA/mask parameter keys are expected. All annotation SJ bytes and spacers
remain resident; chromosome and SJ geometry/metadata are validated once. `sjdbInsertSave Basic`
is logged as generation metadata, not interpreted as a mapping-time two-pass request.

SA is allocated first, followed by padded Genome and SAindex, directly through umem. The
primary run requires `huge_bytes == len` on every allocation; HugeTLB fallback is logged.
The real 4K control requests `Anon { huge:false, require_huge:false }` and requires zero
reported huge bytes. Reports include umem's observation timestamp: these are allocation-time
page observations, not continuous residency monitoring. No cudaHostRegister, cudaMemcpy,
expanded SA, GPU index allocation or index staging is used. CPU pointers are compared with
actual GPU lease addresses and context/length provenance is logged. `bytes_copied()==0` is
explicitly identified as a constant, not a dynamic detector.

Whole Genome/SA/SAindex validation and SHA256 are outside timing. Actual loaded array bytes
are hashed through a setup-only host pipe to the local SHA256 utility and checked against
the disk files. Metadata files are also hashed. This host hashing pipe is not a GPU transfer.
SAindex is retained resident even though SA_SEARCH_FULL does not query it.

The deterministic wire file starts with `UMPROBE1`, then LE u64 request count, read arena
bytes and seed; 80-byte explicitly serialized requests precede the two-buffer read arena.
A required `*.probe-provenance.txt` binds the request hash, source FASTQ hash, index parameter
hash and generation method. The run checks request and parameter provenance. The generator
uses fixed-seed xorshift 1/8 selection from the real mate1 source prefix, skips non-ACGT reads,
uses offsets 0,20,40,... and emits forward then mirrored reverse requests. s[1] is a
position-wise complement. `N=remaining read length`: pinned `parametersDefault:575` has
`seedSearchLmax=0` (unlimited), **not 50**. Reads are limited to 4096 bases.

Counts are decimal **64000, 256000, 1000000, 4000000**, never powers of two. Generate one
4000000-request file and use identical prefixes for each batch. These are conspicuously
**SA_SEARCH_FULL synthetic grid requests, not captured STAR scheduler requests**. They can
perform deeper searches than STAR's prefix intervals, so results concern this PROBE boundary.
Index-generation suffix-length provenance is not inferred from current executable defaults;
this is not a proof of upstream index/scheduler correctness.

## Measurements and TSV

The default is three repeats per batch; fewer reject. CPU20 uses a dedicated Rayon pool with
its actual worker count checked. Index load/validation and setup are recorded separately.
Each batch warms until accumulated CUDA event time is at least 1000 ms; wall/check time and
round count are recorded. Device-call wall and CUDA event duration are both exposed.

`probe.tsv` has comment-prefixed provenance followed by these tab-separated columns:

```text
mode requests_per_arm repeat cpu_workers cpu_s gpu_wall_s gpu_event_ms
device_call_wall_s combined_s cpu_rps gpu_rps GPU_RPS_over_CPU20_RPS
cpu_gpu_output_checksum sa_gathers compared_bytes loop_trips comparisons
mean_request_bytes max_request_bytes mean_loops max_loops mean_compare_bytes
max_compare_bytes mean_warp_byte_imbalance max_warp_byte_imbalance
logical_gpu_GBs combined_rps cpu_slowdown gpu_slowdown
```

- Isolated ratio = CPU20 wall / GPU end-to-end wall = GPU_RPS / CPU20_RPS; larger is better.
- Logical bytes = `8*SA_gathers + compared_genome_bytes`; these are not physical DRAM traffic
  or a bandwidth-counter measurement. Stats exclude request transport/output traffic.
- Per-warp byte imbalance = maximum per-request compared bytes / mean within each consecutive
  group of up to 32 requests. Reported mean/max are workload imbalance proxies, not hardware
  warp efficiency. `max_compare_bytes` is maximum bytes actually examined by one comparator;
  `mean_compare_bytes` is total bytes / comparator calls.
- Overlap uses CPU first half and GPU second half over the exact same resident index/arena.
  Each repeat measures standalone controls of those same halves first, logs their times and
  checksums, then measures overlap. CPU slowdown = overlapped CPU time / CPU half solo time;
  GPU slowdown = overlapped device-call wall / GPU half solo device-call wall. Larger is worse.
  GPU device-call timing includes event creation, launch and synchronization. Combined wall
  includes both tasks, leases and final fence; combined RPS = `2*half/combined_wall`.
  GPU reclaim waits for the CPU callback, so attributing combined wall entirely to the GPU
  would be wrong; hence separate device-call wall. The ratio column in overlap compares
  disjoint request halves and must not be used for the isolated viability decision.
- A final `# PROBE COMPLETE agreement` marker is emitted only after all requested rounds pass.
  A partial TSV is not agreement. No fixed-asset capacity statement follows from these RPS.

## Spark commands — orchestrator only, after SPLIT

Work from the reviewed main-repo source state. The runner builds from a fresh target directory,
logs git HEAD/diff plus all source hashes (including untracked files), binary hash, compiler
versions, clocks, input hashes, and copies SPLIT provenance. It holds the shared heavy-job lock
for its whole round; run SPLIT first. It stops before **1788750000 = 2026-09-06 20:00 PDT**
using an external SIGKILL watchdog as well as CLI cutoff checks. No services are changed.

Set the two evidence/input paths from the orchestrator's preflight; their exact locations were
not supplied to this worker. Do not substitute the 5M input or infer the SPLIT decision.

```bash
cd /home/rborkows/uni-rnaseq
export PROBE_FASTQ=/absolute/preflight/path/ERR188140_20M_1.fastq.gz
export PROBE_SPLIT_EVIDENCE=/absolute/path/to/completed-SPLIT-evidence
export PROBE_SPLIT_SCOPE=inner  # only after orchestrator confirms inner-only boundary
bash crates/umseed-probe/scripts/probe-spark.sh huge
```

The runner prints its fresh `PROBE_ROOT`. After successful huge agreement, reuse that exact
request file for the page control, in another fresh root:

```bash
export PROBE_REQUESTS=/home/rborkows/uni-rnaseq/runs/PROBE-seed-huge-XXXXXXXX/probe-requests.bin
export PROBE_HUGE_AGREEMENT=/home/rborkows/uni-rnaseq/runs/PROBE-seed-huge-XXXXXXXX/probe.tsv
bash crates/umseed-probe/scripts/probe-spark.sh small4k
```

The 4K round requires the successful huge completion marker and runs 64000 requests, three
repeats, plus disjoint-half overlap. Both modes select `--variant thread` explicitly.
The runner executes these underlying commands under its lock/watchdog (do not run a second
full-index job alongside it):

```bash
cargo build --offline --release --locked -p umseed-probe --features cuda
"$PROBE_BIN" generate --fastq "$PROBE_FASTQ" \
  --index /home/rborkows/uni-rnaseq/data/index/star_full \
  --output "$PROBE_REQUESTS" --count 4000000 --seed 188140
"$PROBE_BIN" run --index /home/rborkows/uni-rnaseq/data/index/star_full \
  --requests "$PROBE_REQUESTS" --output "$PROBE_ROOT/probe.tsv" \
  --variant thread --pages huge --counts 64000,256000,1000000,4000000 \
  --repeats 3 --overlap --split-provenance "$PROBE_SPLIT_EVIDENCE" \
  --cutoff-unix 1788750000
```

`--variant warp` deliberately rejects with the continuation: implement it only after actual
first thread-kernel GPU wall is no more than 3x CPU20 wall (ratio >=1/3). No timing exists to
resolve that conditional yet. Orchestrator inspects generated SASS for actual loads and handles
Spark compiler/integration rounds. If SPLIT says inner <70%, first add the tagged direct-
extension request and its CPU/kernel tests; do not call this inner-only run final evidence.
The runner rejects any `PROBE_SPLIT_SCOPE` other than `inner`.

## Local verification actually performed

Mac, offline, no GPU. `cargo test --offline -p umseed-probe -p umgpu` passes **10 tests**:
3 umgpu, 1 request-wire unit test, 1 loader/generator integration test, 5 search/transport tests.
The transport test compiles the exact CUDA algorithm header as ordinary C++17 with
`-O2 -Wall -Wextra -Werror` and compares 2048 cases, all output/stat/status bytes, with Rust.
It exercises history/expansion paths, both read directions, both strand bits, trusted prefixes
and pointer rejection. It is **not nvcc compilation, device execution, or upstream oracle proof**.
The loader test includes an authored annotation-SJ/Basic fixture and sparse-SA rejection;
fixtures are unit tests, never substituted for the real index in performance runs.

Actual command output summary:

```text
cargo test --offline -p umseed-probe -p umgpu
 test result: ok. 3 passed; 0 failed
 test result: ok. 1 passed; 0 failed
 test result: ok. 1 passed; 0 failed
 test result: ok. 5 passed; 0 failed
cargo clippy --offline -p umseed-probe -p umgpu --all-targets -- -D warnings
 Checking umseed-probe v0.1.0 (/Users/rborkows/projects/uni-rnaseq/crates/umseed-probe)
 Finished `dev` profile
```

Also passed `cargo fmt --all -- --check`, clang-format dry-run/Werror on the three new C++/CUDA
files, `bash -n` on the runner, `git diff --check`, and the CLI `--help` stub build.
Additionally, the CUDA **Rust cfg only** compiled locally using:

```bash
cargo rustc --offline -p umgpu --lib \
  --target-dir /private/tmp/umseed-probe-cuda-rust-check \
  -- --cfg 'feature="cuda"' --emit=metadata
```

This deliberately does not set Cargo's CUDA feature/build-script flag: it checks the real Rust
CUDA wrapper's types without nvcc or CUDA linkage. It passed; it must not be reported as a
successful CUDA build. Real CUDA compilation, GPU tuple smoke, full-index loader acceptance,
page coverage, SASS, timing, overlap, performance conditional and final boundary remain for
Spark and SPLIT. Unsupported source states reject with their concrete cause rather than
silently shrinking/excluding arrays.

Source provenance read locally (SHA256):

```text
replay/search.cpp 82fce22c1f6c4bfa87a22e211205e0f6db128dd4f88588bd689f95fa3a5875ed
replay/index.cpp d92ea8819b79fcf8b340e5519a923a5fffc41c83eb9246bc819cdf4f337b27b5
replay/DESIGN.md 35d07defcc8adeaae994eaabc6e24cd8fe9eab719c696c49195f8f5f9ec3241c
baseline/parametersDefault 8cf9d46f8817921592f4fac6c2da3a094c0e54b1be0d67b4a9eb5f4b34d35a80
```

## Unsupported regimes

Non-2.7.4a format, sparse SA !=1, transformed/non-Full genomes, an explicit non-None two-pass
state, malformed/inconsistent packed arrays or chromosome/SJ metadata, empty arrays, strand
bits outside 32..53, prefix header outside 1..15, genome >8GiB, SA disk >64GiB, read length >4096,
non-ACGT request arena, noncomplementary s[1], invalid ranges/pointers, unknown request tags,
non-ATS devices, registration-enabled contexts, incomplete huge backing or nonzero huge
coverage in the 4K control. Mapping-time insertion and scheduler state are not represented by
this immutable on-disk snapshot. Unknown index-generation suffix-length provenance is not
upgraded to a source-equivalence claim. CPU/kernel per-comparison bounds errors stop the smoke.
