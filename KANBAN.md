# uni-rnaseq Kanban

Plan: `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md`. Card briefs: `.hermes/cards/`.
Workers: astra (specialist) / terra (kernels, Rust) / luna (plumbing, review) / inkling (routine) / me.

## Backlog
- P1-MARKDUP-SWAP — nf-core `--skip_markduplicates` + samtools markdup path or umbam later; 4× on Spark today, zero engineering (me)
- P1-MAC-IO — Mac sys-time overhead on sort/featureCounts (42 s / 64 s vs 8 / 3 on Spark); check APFS/page-cache before blaming hardware (luna)
- P1-MAC-MARKDUP — samtools markdup pipeline errored on Mac 1.22.1; upgrade + rerun (luna)
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
- **P2A-UMQC** — absorb Qualimap/RSeQC/dupRadar into the resident pass; goldens generating on Spark (scripts/make_tier0_qc.sh)
- P1-PROFILE-SPARK — `perf` unlocked (paranoid=1); next: perf record on picard/samtools/STAR standalone

## Review
- P1-BAMCHAIN-20M — standalone BAM-chain profile both boxes (`bench/PHASE1-bamchain-20M.md`). Key: decode ≈ 2 s (not the bottleneck → **Phase 1.5 demoted from gate to measurement**); Picard MarkDup 279 s single-thread is the wall; samtools markdup 4× for free; single-thread perf identical Spark vs Mac; chain re-reads BAM 5-6×. **Amdahl: 22% of serial latency, 57% of CPU-heavy throughput → proceed to 2a on throughput grounds.**

## Review
- P0-SPARK — luna done. Verified: rustup 1.98.1, micromamba env `rnaseq`, nextflow 26.04.6 w/ user-local JDK 17, nvCOMP CUDA13 sbsa at `~/.local/opt/nvcomp`, docker GPU smoke test OK. Corrections by me: bioconda STAR was 2.7.3a (linux-aarch64 lag) → built 2.7.11b from source with `-march=native`, symlinked at `~/.local/bin/STAR`; worker missed `cub.cuh` which exists at `/usr/local/cuda/targets/sbsa-linux/include/cccl/cub/` (CUDA 13 moved it under `cccl/`) — not a blocker.

## Done
- **P2A-UMBAM-PERF** — four passes, 408 s → 55 s on 20M BAM (8.2× over tool chain), outputs byte-identical throughout (bench/PHASE2A-umbam-20M.md)
- **P2A-UMBAM-CPU** — CPU control arm, 6/6 Tier 0 gates, 416 s on 20M BAM (parity; profile in bench/PHASE2A-umbam-20M.md)
- **P1-TIER2A-THROUGHPUT** — 6×78M overlapped on Spark: 208 min, 41.5 samples/day, 32% util; QC single-thread = 58% (bench/PHASE2-tier2a-throughput.md)
- P2-UMEM-IMPL — `crates/umem`, terra + 2 review rounds (mine, astra `docs/review-umem-impl-astra.md`). Fixed from astra pass: **B1** panic in `Fence::wait`/destructor could free storage before completion → `RetentionGuard` field-order trick (guard declared before fence; `completed` flag set only on observed success; anything else quarantines on unwind), proven by two catch_unwind tests; **B2** file contract now covers all writers, all derived leases/submissions, forgotten device work; **S1** file EOF padding vs page-rounded VMA; **S2** `Poll::{Ready,Pending,Failed}` replaces panicking retry; **S3** populate distinguishes EINVAL (fallback) from ENOMEM/EFAULT (error); **S5** real PROT_NONE guard pages owned on both sides (prevents VMA merge; smaps now accepts exact contiguous tiling for the split case); **S6** `Submission` owns `Vec<AnyLease>` + one fence + one context (`unsafe fn new` with `ContextMismatch` returning leases), `AnyBuf` preserves Ro/Rw, `GpuLease::len()` for wrapper bounds. Nits: `huge:false` → MADV_NOHUGEPAGE control; `FilePmdMapped` counted; checked KiB parse. 11/11 tests both boxes; THP 100% @4 GiB in 0.06 s ×3.
- P1-NFCORE-SPARK-FULL — 78M pairs: 105 min wall, 160 cpu-min. Everything scales linearly with depth; Qualimap 33 min + Picard 18 min single-threaded = critical path; GPU-shaped share 21% serial. Cost row: ~14/day serial, est. 25-35/day overlapped (`bench/PHASE1-depth-scaling.md`)
- P1-NFCORE-SPARK-20M — 54 min wall (28 min/sample amortized); STAR 5.8 min @32% eff; QC/BAM chain single-threaded ~22 min (`bench/trace-spark-tier1-20M.txt`)
- P1-NFCORE-MAC-20M — **abandoned**: STAR OOM at 20 GB container cap even with sparse-3 index. M4 Pro = kernel-dev box, Ultra = pipeline box. Mac profiles use Spark-produced BAMs.
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
