# uni-rnaseq Kanban

Plan: `.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md`. Card briefs: `.hermes/cards/`.
Workers: astra (specialist) / terra (kernels, Rust) / luna (plumbing, review) / inkling (routine) / me.

## In Progress
- **P2C-STAR-TWOPASS-LIFECYCLE** — Terra, dispatched 2026-09-08 ~20:40 PDT: lift the twoPass/sjdbInsert refusal in `admitted()`, re-arm the borrowed index per generation (pre-pass-1 sjdb insert and post-pass-1 insert both rebuild SA/SAi), per-generation sidecar counters. Orchestrator reruns the production gate when it lands.
- **P2C-STAR-PRODUCTION-GATE** — Terra, dispatched 2026-09-08 17:40 PDT: runner + source analysis for nf-core's real argv (two-pass, TranscriptomeSAM, BAM Unsorted). Orchestrator runs the host gate after Salmon.
- **P2C-THP-X86-PORT** — Luna, dispatched 2026-09-08 17:42 PDT: `bench/thp-x86/` bundle (patch stock STAR with the madvise hook, 3-arm rotated matrix, analyzer). Runs on Batch/EC2 by the user/orchestrator.

## Ready (plan `.hermes/plans/2026-09-08_30k-six-threads.md`)
- **P2C-PIPELINE-RUN** — re-gated on the advised-bypass path (−11.9%), GPU off; needs PRODUCTION-GATE green. Orchestrator, Spark.
- **P2C-SALMON-NEXT** — Astra design review; envelope verdict pasted in, dispatchable.
- **P2C-TRIM-FASTQC** — Phase A Luna (goldens/scaffold, after THP-X86-PORT lands); Phase B Terra (port, after PRODUCTION-GATE lands).
- Parked: GPU seed search (−5.6% robust / −8.1% mean-only, eleven rounds; no lever left worth a card).

### Follow-through completed preparation / review
- **P2C-STAR-PRODUCTION-GATE** — runner + comparator DONE (`3c26aa2`): stock STAR under nf-core argv is order-nondeterministic (BAM Unsorted chunk order) and the transcriptome BAM's primary flag / HI is chosen by a per-thread RNG (`ReadAlign_quantTranscriptome.cpp:69`); documented normalization verified identical on two stock runs. **Integrated `ef22723c…` did not engage** — `admitted()` refuses twoPass/sjdbInsert, ran as stock, gpu_consumed=0. All prior STAR numbers are from the non-production invocation. `bench/PHASE2E-production-gate.md`.
- **P2C-SALMON-ENVELOPE** — DONE: identical run 2 differs from run 1 with the same shape as either vs golden (quant.sf 134,886 TPM rows differ; rel ΔTPM p99 5.5e-2; gene-level p99 3.8e-2, low-count genes move 10–60×). **Salmon is not run-to-run deterministic; no byte gate exists.** Run 2: 507.39 wall / 4537.08 user / 66.75 sys s. `bench/PHASE2D-salmon-thp.md` §Envelope, `bench/evidence/salmon-alignment-screen/envelope.tsv`.
- **P2C-SALMON-ALIGNMENT-SCREEN** — complete, exit 0: 535.35 wall / 4857.79 user / 79.23 sys s in-container, 69,176,421 processed = mapped (matches golden). `quant.sf`/`quant.genes.sf` **differ** from golden (Name/order identical; TPM/NumReads p99 rel 5.8e-2; consistent with Salmon's multithreaded-EM nondeterminism — no byte-identical gate exists). Profile: kernel 1.5%, zero AnonHugePages, hot = CAS contention 22.7% + logLikelihood 18.8% + soft-float long double 15.9%. **Huge-page hypothesis closed as inapplicable; no Salmon saving in the cost model.** `bench/PHASE2D-salmon-thp.md`, `bench/evidence/salmon-alignment-screen/`.
- **P2C-COLLAPSE-FULLDEPTH** — verified same matrix binary on 78,619,701 pairs / 211,097,418 ordered records; 570,386,243 GPU chains, zero strict mismatches/faults/rejections/live charges. All collapse calls succeeded, identities and explicit policies verified. `bench/evidence/full-depth-collapse/`; correctness does not upgrade the narrow 8.0831% performance estimate.
- **P2C-COLLAPSE-MATRIX** — 12/12 rows and raw observations verified. Both on arms 99.9746–99.9815% huge-backed; mean CPU-s 679.5267 CPU-on / 624.6000 GPU-on, GPU −8.0831% CPU / −13.1612% wall versus CPU-on. Narrow point-estimate crossing, NOT robust: paired reductions 10.0198/8.1224/6.0621%. Collapse saves ~2% CPU in both paths (cost included), so credit layout not GPU. Keep opt-in; `bench/PHASE2C-collapse-matrix.md`.
- **P2C-COLLAPSE-HOST-GATE** — verified strict 20M parity: 53,710,530 ordered records, 145,590,534 GPU chains, zero mismatches/faults/rejections/live charges. All three collapse calls succeeded, total 5.248563158 s wall; not a timing gain. New binary `a6c61b2c…` is slice-gated only; prior binary retains full-depth approval. Evidence `bench/evidence/collapse-gate/`.
- **P2C-COLLAPSE-DISQUALIFIER** — Terra delivery independently reviewed in `docs/review-collapse-disqualifier.md`. Array extents/order traced; null/overflow/off-value tests expanded; portable paths and glibc mock declaration fixed. Three focused tests pass on Mac and Spark without skips, source/window regressions pass locally. Accepted for host gating only, not an optimization result.
- **P2C-RESIDENCY-DIAGNOSTIC** — verified 43 bypass/40 GPU target samples and clean GPU sidecar. Large advised regions 98.995% huge-backed in bypass vs94.72–94.78% in GPU; prefix-index-sized region 92.95% vs47.95% at first full residency. Confound confirmed, performance causality not established. `bench/PHASE2C-residency.md`; diagnostic times do not supersede repeated timings.
- **P2C-FRESH-TIMING** — nine rotated unprofiled rows verified, all GPU warmup/measured sidecars clean. Mean CPU-s stock 756.4467 / advised CPU 683.46 / GPU 645.08: GPU −5.62% versus advised CPU, −14.72% versus stock, wall +6.04% versus advised CPU. 8% incremental target NOT met; GPU huge-page totals vary, bypass layout unobserved. `bench/PHASE2C-fresh-timing.md`; no rows discarded.
- **P2C-FRESH-PROFILE** — all three 20M profiles verified and attributed by leaf DSO; GPU consumed 145,877,626 chains with zero faults/rejections/live charges. Profiled CPU-s stock 756.42 / advised CPU 682.35 / GPU 628.18; exploratory −7.94% GPU/advised CPU, not repeated timing evidence. `bench/PHASE2C-fresh-profile.md` documents unknown symbols, approximate self-time allocation and varying huge-page backing. No new optimization chosen.
- **P2C-CACHE-STARTUP-PROBE** — all four on/on/off/off invocations verified active on GPU, no new kernel errors, successful memory observations. Original OOM/fallback did not reproduce; eviction-off causality remains unproven. Failed causal matrix (2/6 rows) retained separately. Keep historical eviction-on conservatively; no cache speedup claim. `bench/PHASE2C-cache-screen.md`.
- **P2C-CACHE-PERF-READY** — Terra delivered; orchestrator independently reviewed and fixed PID-found-without-sample false success. 34 local runner/source tests and 10 Spark cache tests pass. Causal screen accepted for execution; performance warm-cache policy and shell-wrapper follow-ups named in `docs/review-cache-perf-ready.md`.
- **P2C-FULLDEPTH-INTEGRATED** — verified repaired-binary parity: 78,619,701 pairs, 211,097,418 ordered alignment records, documented header normalization only. 537,438,099 GPU chains consumed, zero faults/rejections/strict mismatches and zero live charges. `bench/PHASE2C-full-depth.md`; strict diagnostic timing is NOT performance evidence.
- **P2C-LAT-HOST-GATE** — repaired binary `ef22723c…38b6b` passed build/backend tests and strict 20M ordered-output parity: 53,710,530 alignment records; 145,953,932 GPU chains consumed; zero faults, rejected results, strict mismatch counters or live charges at finish. Evidence `bench/evidence/lat-contract-close/slice-*`. No performance claim.
- **P2C-LAT-CONTRACT-CLOSE** — Astra delivery independently reviewed by orchestrator; local targeted repair accepted for host gating. Personally reran 13 stream cases, coordinator/prefix, eight source/ABI tests, ASan ordinary/delayed shutdown; workspace fmt/clippy/tests pass. Evidence `bench/evidence/lat-contract-close/`; host gates remain pending.
- **P2C-COST-STAGES** — Luna delivered; independently reviewed in `docs/review-cost-stages.md`. Model generator/test pass; raw three-arm means independently reproduced, prior baseline buckets unchanged. Provisional whole-pipeline projections: 140.5721 CPU-min advised / 136.8967 GPU versus 148.4867 umbam CPU baseline. Charts inspected and clipped title repaired. Not measured pipeline throughput or final-build approval.
- **P2C-FOLLOWTHROUGH-TOOLS-REVIEW** — Luna review delivered. Orchestrator repaired real-preflight stock reuse and target-only profiling time; 21 Mac tests and 11 Spark tests pass. Actual frozen golden reuse and real perf/time protocol verified under the Spark lock. `bench/PHASE2C-followthrough-tools.md`; cache-policy and final-binary measurements still pending.
- **P2C-LAT-PREFETCH-FIX** — Terra partial delivery superseded by the locally verified LAT-CONTRACT-CLOSE repair above; no standalone final acceptance of the earlier partial.
- **P2C-FULLDEPTH-STOCK** — exit 0, 78,619,701 pairs, stock SAM retained (51,288,594,018 bytes). One diagnostic run: 175.82 wall / 2863.32 user / 68.92 sys seconds. `bench/PHASE2C-full-depth.md`; NOT integrated parity or a controlled performance comparison.
- **P2C-FULLDEPTH-PREFLIGHT** — PASS inputs only: both mate MD5s match and each has 78,619,701 four-line records. Host24 binary identity verified; all pinned index files and unchanged comparator verified. Local `bench/evidence/integrate-full-depth/preflight-20260908.json`.
- **P2C-LAT-PREFETCH-REVIEW** — Astra finished; `docs/review-integrate-lat-prefetch.md`. BLOCKED current HEAD on reproduced B1; fixes now active. Existing output parity is not invalidated, but is insufficient lifecycle coverage.
- **P2C-FOLLOWTHROUGH-TOOLS** — Terra delivered; reported 17 focused tests plus existing integration suites green. Not yet accepted for host measurements; independent review/corrections active.

## Follow-through queue (current, authorized 2026-09-08)
- Plan: `.hermes/plans/2026-09-08_084336-star-followthrough-1-7.md`. User authorized items 1–7; no memo. Max two disjoint workers plus one serialized Spark measurement lane.

- **P2C-PERF-FINAL** — profiling and repeated timings complete for the repaired candidate; the subsequent collapse matrix adds a residency-matched CPU/GPU comparison. Cohorts and caveats are separate in bench docs; no saturated throughput claim.
- **P2C-FINAL-OPT** — bounded collapse attempt complete and independently reviewed; cost-inclusive matrix shows a marginal shared-layout benefit, and new-binary full-depth parity passed. Keep opt-in. No further scheduler rewrite planned from this evidence.
- **P2C-SALMON-THP** — main alignment-quant mode verified and baseline/profile active above. The separate subsampled index task is not the costly main quant stage; no unmeasured Salmon saving enters the model.
- Current STAR state: repaired binary `ef22723c…38b6b` has contract review, strict slice/full-depth parity and measured GPU −5.62% CPU versus advised CPU in its warmup cohort. Opt-in-collapse binary `a6c61b2c…c70bb0` now has slice/full-depth parity; its fresh-process, residency-matched matrix measures −8.0831% GPU CPU versus CPU-on, narrowly on the mean only. Historical round-9 figures remain historical, not interchangeable with these cohorts. Older v1 deadlines below are superseded; held replay-hardening work remains held.

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

- **P2A-UMQC-QUALIMAP** — resolve Qualimap union-exon classification (704,856 vs 734,257 gene-assigned) — needs Qualimap Java source; 5'/3' bias needs a transcript coverage sweep. Optional: MultiQC consumes RSeQC/dupRadar/Picard already.
- **P2A-UMBAM-TIER1** — run `umbam chain --qc` on a full 78M Tier 2A sample on the Spark; compare every output to that sample's nf-core results (the real-scale golden); time it.
- P0-BENCH-T5 — ext4 large-folio test for a read-only mmap'd file with GPU chase (me; decides whether file-backed BAM ever gets the random-access path — not needed before Phase 1.5)

## In Progress
- **P2C-STAR-INTEGRATE-1** — resumed by user2026-09-07; Astra contract/C-header freeze first, Terra implementation next, then one Astra consumption-order review. Initial-only, duplicate G/SA, worker windows/coordinator64k, exact-tuple lookup, stock fallback/oracle; storeAligns untouched. Report suppressed-reverse waste against13,435,368/159,989,084. **STOP at strict full20M parity before timing runs.** Deadline2026-09-07 20:00PDT; shared lock unchanged.



- **P2C-REPLAY-SCALE — SUPERSEDED**: real capture + STAR/CPU/GPU oracle payload is complete and accepted. Remaining rigor is shipping work. PE-HOST-FAULTS/2, OWNED-SEARCH-SESSION, OWNED-REQUEST-TESTS and FULL-INDEX-CONTRACT cap matrix remain **HELD; do not resume**.
- P1-PROFILE-SPARK — `perf` unlocked (paranoid=1); next: perf record on picard/samtools/STAR standalone

## Review
- P1-BAMCHAIN-20M — standalone BAM-chain profile both boxes (`bench/PHASE1-bamchain-20M.md`). Key: decode ≈ 2 s (not the bottleneck → **Phase 1.5 demoted from gate to measurement**); Picard MarkDup 279 s single-thread is the wall; samtools markdup 4× for free; single-thread perf identical Spark vs Mac; chain re-reads BAM 5-6×. **Amdahl: 22% of serial latency, 57% of CPU-heavy throughput → proceed to 2a on throughput grounds.**

## Review
- P0-SPARK — luna done. Verified: rustup 1.98.1, micromamba env `rnaseq`, nextflow 26.04.6 w/ user-local JDK 17, nvCOMP CUDA13 sbsa at `~/.local/opt/nvcomp`, docker GPU smoke test OK. Corrections by me: bioconda STAR was 2.7.3a (linux-aarch64 lag) → built 2.7.11b from source with `-march=native`, symlinked at `~/.local/bin/STAR`; worker missed `cub.cuh` which exists at `/usr/local/cuda/targets/sbsa-linux/include/cccl/cub/` (CUDA 13 moved it under `cccl/`) — not a blocker.

## Done
- **P2C-STAR-INTEGRATE-1** — gate (i) **PASSED**: STAR with inner seed search on the GPU (91.5% of requests consumed, 1.3 G gathers in place) produces byte-identical alignments under a strict CPU oracle. Gate (iii) **failed**: after fixing a 30 GB hash in the mapping phase, GPU arm = 800 CPU-s = hooks-bypassed arm; net zero CPU, −30% wall. The synchronous coordinator spends exactly what the kernel saves. `bench/PHASE2C-integrate-1.md`. v2 (async batches, borrowed read bytes) not authorized.
- **P2C-CHAIN-POSITION** — **76.4015868% of inner gathers at Lmapped==0** (1,431,388,309/1,873,505,995);88.7551% inner bytes. One locked full20M counters run, unchanged stock parity across53,710,530 SAM records/SJ/non-timing logs;7 tests, one independent review. **Select mechanism(a) only; no grid20. STOP/report checkpoint reached**, no implementation launched. `bench/PHASE2C-chain-position.md`.
- **P2C-COST-SEED-PROJECTION** — dashed orange explicitly PROJECTED1.07–1.15× capacity scenario atop umbam CPU; model/memo/charts updated,4 tests and visual check pass. Not a measured integration result.
- **P2C-REAL-REQUESTS** — **6.60× CPU20** on 999,914 real requests; CPU and thread GPU each match **999,914/999,914 captured STAR tuples**, zero skipped. Reused full20M parity/capture, unchanged SSIRv1/caps. Three repeats/overlap/forced4K verified; fresh synthetic10.38×. Accepted by main review `e3b6cee`; glue committed `5d3ba38`. `bench/PHASE2C-real-requests.md`. Inner-kernel question closed; no pipeline gain measured.
- **P2C-STAR-INTEGRATE-DESIGN** — `docs/STAR-INTEGRATE-DESIGN.md`; one independent pass incorporated. Bounded implementation now authorized; initial-start-only mechanism selected by CHAIN-POSITION. Continuations dropped, grid20 not selected, duplicate arrays accepted. Actual hit/fallback/waste remains the integration gate.
- **P2C-SEED-GPU-PROBE** — both variants verified on Spark, bounded independent review without blockers. Thread selected: **10.21× CPU20 at1M**, **56.83M requests/s** (original round10.20× at256k). Warp best **5.95× at64k**, slower at every batch;4K warp0.1337×, prior thread0.0885×. Outputs/logical counters match; actual CUDA builds, page reports, load instructions and provenance verified. **Synthetic full-SA performance screen, NOT upstream correctness or STAR/pipeline speedup.** `bench/PHASE2C-seed-probe.md`.
- **P2C-SEED-SPLIT** — seed worktree: **99.78875% /99.78868% inner compared bytes** on20M/5M full-index paired runs; all biological SAM/SJ/non-timing stats match stock. Counter CPU overhead **+31.80%/+27.36%**, unadjusted. Inner-only selected, not CPU-time attribution. Seed `experiments/star-seed/RESULTS-SEED-SPLIT.md`; raw attempts and continuation preserved.
- **P2A-UMQC** — `umbam --qc`: all 7 RSeQC outputs + dupRadar dupMatrix byte-identical to nf-core containers on Tier 0 (9/10 gates); Qualimap partial (headline counts match; gene/ambiguous split and 5'/3' bias documented in COMPAT.md). Tool sources in docs/tool-src/.
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

- **P2C-PIPELINE-RUN — authorized only if INTEGRATE-1 CPU reduction>=8%**: six Tier2A samples, integrated STAR+umbam, nf-core trace to CPU-min/sample and cost curve. Same lock, cutoff2026-09-07 20:00PDT.
