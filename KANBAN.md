# uni-rnaseq Kanban

Plan: `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md`. Card briefs: `.hermes/cards/`.
Workers: astra (specialist) / terra (kernels, Rust) / luna (plumbing, review) / inkling (routine) / me.

## Backlog
- P1-NFCORE-SPARK — nf-core/rnaseq Tier 0/1 on Spark, `-with-trace` (bg job; me)
- P1-NFCORE-MAC — nf-core/rnaseq Tier 0/1 on Mac, native/conda profile (bg job; me)
- P1-PROFILE-SPARK — `perf` profiles of STAR/salmon/samtools/picard/featureCounts on Tier 1 (me runs; astra analyses)
- P1-PROFILE-MAC — `xctrace` same (me runs; astra analyses)
- P1-COST-DATAPOINT — 1 cold + 6 batched samples, CPU-only, both boxes → samples/day (me)
- P1-AMDAHL-GATE — decision (me)
- P1.5-INFLATE-CUDA — nvCOMP deflate over BGZF blocks in umem memory (terra)
- P1.5-INFLATE-METAL — Metal inflate kernel (astra)
- P1.5-STARVATION-GATE — GPU inflate vs all-CPU inflate, both boxes (me)
- P2A-* — sort/markdup/count kernels (terra/astra/terra) + CPU refs & bit-compat (luna)
- P2B-* — seed-search harness (terra)

## Ready
- P0-BENCH-T5 — ext4 large-folio test for a read-only mmap'd file with GPU chase (me; decides whether file-backed BAM ever gets the random-access path — not needed before Phase 1.5)

## In Progress

## Review
- P0-SPARK — luna done. Verified: rustup 1.98.1, micromamba env `rnaseq`, nextflow 26.04.6 w/ user-local JDK 17, nvCOMP CUDA13 sbsa at `~/.local/opt/nvcomp`, docker GPU smoke test OK. Corrections by me: bioconda STAR was 2.7.3a (linux-aarch64 lag) → built 2.7.11b from source with `-march=native`, symlinked at `~/.local/bin/STAR`; worker missed `cub.cuh` which exists at `/usr/local/cuda/targets/sbsa-linux/include/cccl/cub/` (CUDA 13 moved it under `cccl/`) — not a blocker.

## Done
- **PHASE 0 CLOSED 2026-09-05 00:40**
- P0-BENCH-T6 — repaired benchmarks on idle Spark (`bench/RESULTS-T6-summary.md`): THP 100% deterministic w/ aligned mmap (3 seeds, 4+12 GiB); **cudaMalloc hits the same cliff when allocated late** → umem owns all large allocs; DVFS warmup needed; GPU dependent latency = CPU (~137 ns); pinned 2.8× worse latency
- P0-BENCH-FIX — luna; reviewed, clang-formatted, compiled, run in T6
- P0-INDEXES — STAR full 19.4 min/38.6 GB/29 GB; sparse3 7.8 min/26.6 GB/14 GB; chr20-22 55 s/3.6 GB/1.5 GB; salmon k31+decoys 10.7 min/16.1 GB/9.1 GB (`data/reference/index-build.tsv`)
- P0-DATA — Tier 1 ERR188140/NA12716 (78.6M pairs) + 5M/20M subsamples; Tier 2A 3M+3F CEU; 51 GB, md5-verified (`data-manifest.json`). NA12878 not in GEUVADIS mRNA arm → deterministic "deepest CEU" rule.
- P0-NFCORE-SMOKE-SPARK — 181 tasks OK with docker `-u 1000:1000`
- P0-MAC — toolchain verified (Metal, nextflow, STAR 2.7.11b, salmon 2.7.0, picard, featureCounts, seqtk, aria2c, samtools); container engine = Apple `container`
- P0-NFCORE-SMOKE-MAC — nf-core/rnaseq 3.26.0 `test,arm64` via **Apple `container`** (`appleContainer.enabled=true`): 234/234 tasks COMPLETED, all images arm64, median 1 s container overhead. No Docker/OrbStack needed. Trace at /tmp/nfcore-smoke/trace.txt.
- P0-REF — GENCODE v49 + GRCh38 primary + gentrome/decoys on Spark, checksummed
- P0-HUGEPAGES — Spark `vm.nr_hugepages=16384` (32 GiB) set by human; unprivileged MAP_HUGETLB verified; documented in env-spark.md
- P0-UMEM-DESIGN-V2 — `docs/design-umem.md` v2: lease/Submission ownership (submission consumes Buf, wait returns it; forget = leak never UB; quarantine on uncertain fence), Ro/Rw type-level modes, unsafe boundary = umgpu kernel wrappers, 100% per-VMA THP w/ MADV_COLLAPSE, startup-copy of index is main path, runtime-queried chunking
- P0-UMEM-REVIEW — astra review `docs/review-umem-astra.md` (7 blocking, 7 should-fix, 3 open questions answered w/ kernel-source citations, 6 tests). Verified its top claim myself: `read_sum` SASS had 0 LDGs. Fixed `bw_paths.cu` (unconditional output + host checksum); re-ran; conclusions hold. Correction note in `bench/RESULTS-2026-09-04.md`.
- P0-UMEM-DESIGN — `docs/design-umem.md` draft; 4 GiB question resolved empirically
- P0-BENCH — microbenchmarks both boxes (`bench/`, `bench/RESULTS-2026-09-04.md`)
- P0-PLAN — plan rev 2 with cost curve, Rust, delegation, Astra
- P0-METAL-PROBE — `maxBufferLength` 13.3 GiB, NoCopy over 12 GiB + file-backed OK (`bench/metal/probe_metal.swift`)

## Blocked / Needs human
- (none yet)
