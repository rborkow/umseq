# Unified-Memory RNA-seq: Pressure Test + Dataset & Build Plan (rev 2)

**Date:** 2026-09-04 (rev 2 same day, after reframing the old PoC)
**Repo:** `/Users/rborkows/projects/uni-rnaseq`
**Raw numbers:** `bench/RESULTS-2026-09-04.md`; sources under `bench/cuda/` and `bench/metal/`.

---

## Part 0 — Why: the cost curve

Context: a heavily optimized AWS Batch RNA-seq pipeline (research-use, genome-scale
STAR/salmon) bottoms out at **~$12/sample**. The question behind this project is whether an
architectural change — unified-memory workstations — can bend that curve, and ideally turn a
per-sample variable cost into a fixed asset. Network/egress costs are out of scope for now.

### Back-of-envelope

| | AWS Batch | DGX Spark | Mac Studio M5 Ultra 256 GB |
|---|---|---|---|
| Capital | 0 | ~$4k | ~$8–10k (est.) |
| Marginal $/sample | ~$12 | ~$0.03 (≈0.15 kWh) | ~$0.05 (≈0.2 kWh) |
| Break-even vs Batch | — | ~350 samples | ~700–850 samples |
| CPU-only est. per 50M-pair sample (nf-core star_salmon, serial) | n/a | ~45–60 min | ~30–40 min |
| Samples/day at full utilization (CPU-only, QC steps overlapped) | — | ~25–35 | ~40–50 |
| Time to break-even at full utilization | — | ~2 wk | ~2–3 wk |
| At 5% utilization | — | ~9 mo | ~12 mo |

All workstation rows are **estimates to be replaced by Phase 1 measurements.** Key point:
the fixed-asset argument already holds CPU-only. GPU work moves samples/day, which moves
break-even and the number of boxes needed for a given monthly volume — it is upside, not
the justification.

### Why $12 is hard to beat on Batch and easy to beat on a workstation

- Genome-scale STAR needs ~38 GB resident → memory-class instance pricing per sample.
- Index load (~1–2 min) is paid per container start; amortizing it means warm instances,
  which is paying for idle memory.
- On a workstation the index loads once and stays resident across samples
  (`STAR --genomeLoad LoadAndKeep`, or our own `libumem` mmap); unified memory means the
  GPU sees the same resident copy. Multi-sample batching against one resident index is the
  natural throughput design and is what a fixed asset should optimize — throughput over
  latency.

### The metric this project reports

**Samples/day per $ of hardware → $/sample as a function of monthly volume**, with AWS Batch
as a flat $12 line. Every phase produces a datapoint on that curve; the Phase 4 memo is
the curve.

---

## Part 1 — Pressure test of the "PCIe bandwidth bottleneck" hypothesis

### What the old PoC did (as recalled)

Ported BAM-consuming stages of an nf-core/rnaseq-style pipeline to Hopper-class GPUs on EC2.
CPU decompressed BAMs, records were shipped to the GPU over PCIe. Result: slower than the
CPU-only pipeline on comparable hardware. Diagnosis at the time: PCIe bottleneck.

### What we measured today

Microbenchmarks on both current boxes, exercising the two access patterns that dominate the
pipeline: **streaming** (FASTQ/BAM/genome scans, inflate, sort) and **random gather /
dependent pointer-chase** (suffix-array binary search in STAR, k-mer hash probes in salmon).

**DGX Spark (GB10, 128 GB LPDDR5x, 273 GB/s spec)**

| Path | Streaming | Random lookups (4 GiB working set) |
|---|---|---|
| Device-local (`cudaMalloc`) | 224–241 GB/s | 2.5–2.6 G/s |
| GPU reading host memory directly, **2 MiB pages** | 157–205 GB/s | 2.3–2.6 G/s |
| GPU reading host memory directly, **4 KiB pages** | 157–205 GB/s | **9–21 M/s** (≈150× slower) |
| Explicit `cudaMemcpy` H2D/D2H (the PCIe-pipeline pattern) | 50–59 GB/s | n/a |
| CPU single thread | 49 GB/s memcpy | 135 ns/hop |

**Apple M4 Pro (24 GB, 273 GB/s spec, Metal via MLX)** — scale model for the M5 Ultra

| Pattern | Result |
|---|---|
| GPU streaming read | 253–259 GB/s (95% of spec) |
| GPU random gather, 4–7 GiB working set | 1.18–1.34 G/s, **no page-size cliff** |
| numpy CPU, same tests | 10 GB/s stream, 63 M/s gather |

### Verdict

1. **"PCIe bandwidth" was almost certainly the wrong diagnosis.** Multi-threaded BGZF inflate
   produces 1–3 GB/s of decompressed records; PCIe 4/5 x16 carries 25–50 GB/s. The link had an
   order of magnitude of headroom on bytes. What saturates in that design is *transactions*
   (small synchronous copies, pageable memory, per-batch launch/sync) and, above all, the
   **producer**: the GPU was starved by CPU decompression and then paid a copy tax on top.
   A stage with cheap compute (sort/count/mark-dup/stats) loses to a CPU that already has the
   bytes in cache under those conditions.
2. **We reproduced a PCIe-looking wall with no PCIe in the loop.** On the Spark, GPU random
   access into 4 KiB-page host memory collapses ~150×. That is TLB reach / access pattern,
   not fabric bandwidth — the same class of failure a discrete GPU shows when it walks a
   host-resident index across PCIe. Fixable with 2 MiB pages and batching, not with a faster link.
3. **Unified memory removes the copy tax but does nothing for a starved GPU.** If the CPU still
   inflates and the GPU still waits, a Spark/Mac pipeline loses for the same reason the Hopper
   one did. The sharper thesis this project tests: **the GPU must own the bytes end-to-end** —
   compressed BAM lands in shared memory once; the GPU inflates, sorts, dedups, counts, and
   re-deflates in place; the CPU touches headers and results. On PCIe that requires staging; on
   unified memory it is the natural design.
4. **Apple's unified memory is the cleaner implementation.** No page-size cliff (16 KiB pages,
   shared page tables), 95% of spec bandwidth from the GPU. The Spark needed THP, which was
   order-sensitive. For memory-resident indexes this matters more than CUDA availability.
5. **The ceiling is modest on today's boxes, not on the target.** 273 GB/s vs 3.3 TB/s HBM.
   The pending M5 Ultra (256 GB, 1.2 TB/s claimed) is 4.4× on streaming, ~4× GPU cores, and
   fits every index. Streaming stages (inflate, sort, count) scale ~linearly with bandwidth;
   random-lookup stages scale sub-linearly (LPDDR latency per hop) but should still land above
   the Spark's device-local rate.
6. **The CPU baseline moves too.** A 32-core, 256 GB M5 Ultra is a strong CPU-only RNA-seq box.
   The comparison that justifies GPU engineering is **GPU-on-Ultra vs CPU-on-Ultra**, not vs
   EC2. The M4 Pro is the scale model: if the harness GPU/CPU ratio is ≥2× on the M4 Pro, it
   holds or improves on the Ultra (GPU side scales faster than CPU side). If ~1×, more bandwidth
   won't rescue it — the CPU gets the same bandwidth.
7. **Caveats.** Haven't profiled real STAR/salmon/samtools hot loops yet (Phase 1). THP on the
   Spark needs a deterministic allocator. `earlyoom` on the Spark kills large-RSS processes past
   ~110 GB total. No artifacts from the old PoC found under `~/projects`; GPU-busy/wall ratio
   from those runs would settle starved-vs-link definitively.
8. **On Apple's basecalling stat.** The Mac Studio press release claims "up to 3.5× faster
   basecalling in Oxford Nanopore MinKNOW vs M1 Max, 1.9× vs M4 Max." That is Dorado: a
   CRF/transformer network, dense matmul, riding the M5's per-core Neural Accelerators via
   Metal 4 TensorOps. **None of our stages look like that** — inflate, sort, dedup, count, SA
   search are integer/branchy/memory-bound; the tensor units idle. ONT's own tracker calls the
   Apple backend well behind the CUDA one, so the stat is Apple marketing a workload whose Metal
   port is still second-class. Why it still matters: (a) Apple has a named genomics partner and
   an incentive to see more genomics wins on the chip; (b) Dorado's `metal` backend is the best
   public example of production genomics MSL on unified memory — our Phase 0 reference; (c) the
   post-basecall chain (minimap2 → sort → dedup → stats) is CPU-bound and BAM-shaped, so an
   in-place GPU BAM chain is the piece that makes the basecalling stat useful end-to-end;
   (d) salmon EM/VB and DE model fitting are the one place dense linear algebra appears in our
   pipeline — small, but reachable via MLX/MPS if it ever matters. Prediction stays unchanged:
   streaming stages scale with the 4.4× bandwidth, lookup stages scale less, tensor units don't
   participate.

---

## Part 2 — Test dataset plan (human genetics)

Three tiers, all GRCh38 / GENCODE, all public, chosen so correctness is checked against known
biology rather than only "matches CPU output."

### Reference

- **Genome:** GRCh38 primary assembly (no alt contigs), GENCODE release 4x GTF, transcript FASTA,
  salmon decoy set. Source: GENCODE FTP or nf-core iGenomes S3.
- **Indexes (Spark, later Ultra):** full STAR index (`--sjdbOverhang 100`), salmon index with
  full-genome decoys (`-k 31`).
- **Indexes (M4 Pro, 24 GB):** STAR `--genomeSAsparseD 3`; chr20–22-only full-density index for
  kernel work; salmon index (fits).
- Record index file sizes and page alignment; SA and SAindex are what we map with hugepages.

### Tier 0 — smoke (minutes)

- chr22 of GRCh38 + chr22 GTF; 250k read pairs subsampled (seqtk, fixed seed) from Tier 1 and
  filtered to chr22-mapping reads via a first CPU alignment. Also keep the resulting **chr22
  BAM** — it is the primary input for the BAM-stage harness.
- Use: CI, per-commit correctness diff, Mac dev loop.

### Tier 1 — realistic single-sample (tens of minutes CPU)

- GEUVADIS (E-GEUV-1, ENA), LCL RNA-seq, 75 bp PE, ~40–60M pairs. Sample **NA12878** (HG001,
  GIAB truth genotypes → allele-specific-expression checks possible later).
- 5M / 20M / full-depth subsamples. CPU-produced sorted BAM + transcriptome BAM kept as
  harness inputs and as the correctness oracle.
- Use: throughput scaling, memory-footprint curves, correctness vs CPU.

### Tier 2 — differential-expression truth set (hours CPU)

- **A (biological positive control):** 3 male + 3 female GEUVADIS CEU. XIST, RPS4Y1, DDX3Y,
  KDM5D, UTY, EIF1AY *must* be DE; anything else at FDR<0.05 is scrutinized. Do this first.
- **B (quantification gold standard):** SEQC/MAQC-III (GSE47774) UHRR/HBRR with ERCC spike-ins
  and ~1000 TaqMan-validated genes. Score quant accuracy vs TaqMan and ERCC ratios.

### Storage & transfer

- Tier 0 <1 GB; Tier 1 ~15 GB FASTQ + ~10 GB BAMs; Tier 2A ~60 GB; Tier 2B subset ~40 GB.
- Fetch on the Spark (2.7 TB free) with `aria2c`/ENA FTP, checksum, `rsync` Tier 0/1 to the Mac
  (142 GB free). Everything moves to the Ultra when it arrives.

---

## Part 3 — Build & test plan

Principles:
- **Measure the hot loop before porting anything** (the PoC skipped this).
- **Target the stages the old PoC actually touched first** — the BAM chain — because it is
  bandwidth-bound, maps onto the observed failure, and is where 1.2 TB/s pays. STAR seed search
  is the more interesting research problem and the harder bet; it comes second.
- **Metal is a first-class target, not a port.** Every kernel is written against a thin
  two-backend abstraction from day one; the Ultra is the destination platform.

### Integration model (decided up front)

| | Spark (CUDA) | Mac (Metal) |
|---|---|---|
| nf-core execution | Docker, arm64 containers, GPU passthrough | **Native processes**, local executor, conda/native profile — Docker on macOS is a Linux VM with no GPU |
| Accelerated tools shipped as | container image | signed native binaries + `nextflow.config` process overrides |
| Kernel source | `.cu` | `.metal` (MSL) via metal-cpp |
| Compression lib | nvCOMP (deflate) | hand-written Metal inflate/deflate |
| Sort | CUB radix sort | hand-written Metal radix sort (or MPS if adequate) |

Consequence: "CUDA vs Metal" as an experimental variable means *deployment model + kernel
port*, not syntax. Nextflow process definitions must allow both (`container` vs `conda`/bare).

### Language: Rust host, native kernels

Host code in Rust; GPU kernels in CUDA C++ (`.cu`) and MSL (`.metal`), compiled in `build.rs`
and loaded at runtime — the `candle` pattern. No `wgpu`/`rust-gpu`: they abstract away the
unified-memory semantics we're studying (no host-mmap pointers, discrete-GPU map/unmap model).

| Concern | Crate |
|---|---|
| BAM/SAM/BGZF/GTF | `noodles` (pure Rust); `rust-htslib` for parity checks only |
| CUDA | `cudarc` (`cuda-13000`, `driver`+`runtime`+`sys` for managed/host-register/advise) |
| Metal | `objc2-metal` (Metal 4; `newBufferWithBytesNoCopy` for zero-copy mmap) |
| mmap / hugepages | `memmap2` + `libc` (`madvise`, `/proc/self/smaps` verify) |
| nvCOMP | C API via `bindgen` shim |

Cargo workspace:
```
crates/umem      hugepage-backed mmap allocator, both OSes
crates/umgpu     Backend trait (buffers, launch, sync) + cuda/metal impls + kernel loading
crates/umbam     BAM-chain tool: sort | markdup | count | stats
crates/harness   benchmarks, page-size and multi-sample experiments
kernels/cuda/*.cu   kernels/metal/*.metal
```
`cargo test` = bit-compat against samtools/picard/featureCounts golden outputs on Tier 0.
If Phase 2b passes, decide then between a C-ABI Rust lib called from forked STAR vs a Rust
re-implementation of STAR's seed-search-and-stitch front end.

### Parallelization & delegation

Kanban, per standing routing plus one specialist:
- `gpt-6-astra` ($10/$50 per M; Codex CLI ≥0.153) — **specialist, ~5 cards**: the hardest
  correctness-critical kernels, evidence-gathering for gates, adversarial review of
  load-bearing abstractions, and genomics validation (Fable refuses much life-science work;
  route around that). Not for plumbing; not a Terra replacement.
- `gpt-5.6-terra` — default for kernels and quality-sensitive Rust.
- `gpt-5.6-luna` — plumbing/scripts/CPU references/code-quality review passes.
- `inkling-small` — routine scripting.
Max two concurrent workers. Sparky vLLM not used. Astra one-shots via `codex exec` will look
more ordinary than its stateful-harness benchmark numbers; give it complete context per card.

| Phase | Parallel lanes | Worker | Orchestrator keeps |
|---|---|---|---|
| 0 | Spark toolchain ∥ Mac toolchain ∥ reference+index build (bg, hours) | luna ×2; inkling | Dorado read, `umem` design |
| 1 | nf-core runs are background jobs; Spark ∥ Mac profiling | luna: trace→table scripts; **astra: wall-time-share analysis of traces vs source** | Amdahl gate decision |
| 1.5 | **nvCOMP path (terra) ∥ Metal inflate kernel (astra)** — different models on the two paths as a diversity check on the shared inflate spec | terra + astra | starvation gate |
| 2a | sort ∥ markdup ∥ count, each CUDA+Metal | terra: sort, count; **astra: markdup** (picard tie-break semantics); luna CPU refs + bit-compat tests | Backend trait, page-size expt, cost numbers |
| 2b | alongside 2a when a terra slot frees | terra | correctness vs STAR seeds |
| 3 | umbam subcommands ∥ nf-core overrides ∥ Tier 2 DE | luna plumbing; terra tool; **astra: Tier 2 biology validation** (sex-gene DE, TaqMan correlation, anomaly hunt) | review of astra's reasoning |
| 4 | — | — | memo |

Rules:
1. **Gates are the orchestrator's** (Amdahl, starvation, ≥2×). Workers self-report; rerun the
   numbers before a card moves to Done.
2. **Spec + CPU reference + golden outputs before any kernel card is dispatched.** A card ships
   when `cargo test` matches Tier 0 goldens byte-for-byte.
3. **Hardware is the contention point.** Sandbox workers can't drive the GPU or a 30 GB mmap.
   Card deliverable = "builds, unit tests pass on synthetic/Tier 0 data, ready for host
   benchmark"; orchestrator runs real benchmarks on Spark/Mac.
4. Terra kernel cards and host crates get a luna code-quality pass before orchestrator review.
   `umem` and the Backend trait get an **astra adversarial review** (coherence assumptions,
   page alignment, mmap lifetime handed to GPU) and then personal review — load-bearing.
5. Commits require explicit authorization (standing rule).


### Phase 0 — Environment (1–2 days)

- [ ] Read Dorado's `dorado/basecall/metal/` backend before writing `backend_metal.mm`: borrow
      its metal-cpp buffer management, heap/residency patterns, and the unified-memory pitfalls
      recorded in its issue tracker.
- [ ] Spark: STAR, salmon, samtools via bioconda `linux-aarch64` or source `-march=native`;
      Nextflow + Docker; `nf-core/rnaseq -profile arm,docker`; nvCOMP; CUB (ships with CUDA).
- [ ] Mac: STAR/salmon/samtools from source with clang; Nextflow native; Xcode CLT + metal-cpp;
      nf-core `-profile conda` (or `mamba`) so accelerated processes can run natively.
- [ ] `libumem`: hugepage-backed `mmap` (Linux: `MADV_HUGEPAGE` verified via `/proc/self/smaps`,
      fallback hugetlbfs; macOS: plain `mmap`), single API used by every harness/tool.
- [ ] Reference download + index builds (Part 2). Baseline wall time / peak RSS.

### Phase 1 — CPU baselines, profiles, and the Amdahl gate (2–3 days)

- [ ] `nf-core/rnaseq --aligner star_salmon` on Tier 0/1/2A (Spark), Tier 0/1 (Mac),
      `-with-trace` for per-process CPU time and peak RSS.
- [ ] **First cost datapoint:** end-to-end wall time per Tier 1 sample, CPU-only, each box,
      (a) single sample cold, (b) 6 samples (Tier 2A) with STAR `--genomeLoad LoadAndKeep`
      and QC steps overlapped. Convert to samples/day and $/sample at 100% and 5% utilization;
      replace the estimates in Part 0.
- [ ] Profile standalone with `perf` (Spark) / `xctrace` (Mac):
      - BAM chain: `samtools sort`, `samtools index/stats/flagstat`, `picard MarkDuplicates`,
        `featureCounts`/`salmon quant --alignments`, `bedtools genomecov`. Split each into
        inflate / compute / deflate / I/O.
      - STAR: % in SA search vs stitching vs output. salmon: % in k-mer lookup vs chaining vs EM.
- [ ] Deliverable: table of wall-time share per phase per tool, plus **inflate share of the BAM
      chain**. Gates: a stage is a candidate only if the GPU-addressable share is ≥40% (else
      Amdahl caps gain at <1.7×).

### Phase 1.5 — GPU decompression spike (3–5 days) — the starvation test

The single experiment that most directly re-litigates the old PoC.

- [ ] Spark: nvCOMP deflate over BGZF blocks of the Tier 1 BAM, input in `libumem` memory,
      output records left in place. Measure GB/s decompressed vs `samtools view -@20`.
- [ ] Mac: Metal inflate kernel (one threadgroup per BGZF block; Huffman tables in threadgroup
      memory). Same measurement vs `samtools view -@14`.
- [ ] Also measure the reverse: GPU deflate for BAM output.
- **Gate:** GPU inflate must beat all-CPU-threads inflate on the same box. If it does, the
  producer bottleneck is gone and the BAM chain is viable. If not, the BAM-chain thesis fails on
  unified memory too — stop and report; STAR seed search (Phase 2b) remains as a separate bet.

### Phase 2a — BAM-chain harness (1–2 weeks)

**Rev 3 (2026-09-05), after Phase 1 profiles.** The BAM chain is parallelism-bound, not
bandwidth-bound: decode is ~2 s, Picard/Qualimap/dupRadar/RSeQC are 39% of task time at one
core, GPU-shaped work is 21% of serial time. Any GPU kernel must therefore beat a *good
multi-threaded CPU implementation of the same one-pass design*, not Picard. So:

1. **Control arm first: `umbam-cpu`.** Same `umem` buffers, same single resident decode, same
   sort → markdup → stats → count → coverage pass, Rayon over records, 20 threads. Byte-compatible
   with samtools/picard/featureCounts on Tier 0. This is the honest baseline for every GPU
   number and may be the result on its own.
2. **CUDA arm second**, on the Spark, against that control.
3. **Metal arm demoted** to after the M5 Ultra arrives and the CPU arm exists. Kernel-dev on a
   24 GB box against a workload that isn't bandwidth-bound is effort in the wrong place.
4. **Phase 1.5 (GPU inflate) is a measurement, not a gate** — nvCOMP is a library call; run it
   when the CUDA arm exists, report the number, move on.
5. **Phase 2b (seed search)** keeps its slot as the one experiment where the GPU has a structural
   advantage the CPU can't match (batched random access into a 30 GB index at 2.6 G lookups/s),
   but sequences after 2a since STAR is 10% of time and it's the hardest code.

Gate for the CUDA arm: ≥2× over `umbam-cpu` at 20 threads on the same box, same outputs.

```
bench/harness/
  umem.{h,cc}               hugepage/plain mmap, both OSes
  backend.h                 minimal two-backend interface: buffers, launch, sync
  backend_cuda.cu / backend_metal.mm
  inflate_{cuda,metal}      from Phase 1.5
  sort_{cuda,metal}         key = (tid, pos, strand) radix sort of record offsets
  markdup_{cuda,metal}      hash on (tid,pos,mate pos,strand,library); tie-break by quality
  count_{cuda,metal}        featureCounts-style interval binning against GTF
  deflate_{cuda,metal}
  driver.cc                 unsorted BAM in → sorted, dup-marked BAM + counts out, in place
```

- [ ] Each kernel: CPU reference, unit test for equality vs `samtools`/`picard`/`featureCounts`
      output on Tier 0, then throughput on Tier 1.
- [ ] Experiments that decide the project:
  1. End-to-end BAM chain GB/s and wall time: CPU-all-threads vs GPU, each box.
  2. Bytes moved by the CPU during the GPU run (should be ~headers only) — proves "GPU owns
     the bytes."
  3. Page-size sensitivity on the Spark (4 KiB vs 2 MiB backing of the BAM) — the plot that
     validates or kills the Part 1 diagnosis.
  4. Bandwidth scaling: run at throttled buffer sizes to fit the M4 Pro's 24 GB; extrapolate
     to 1.2 TB/s. Re-run unchanged on the Ultra when it lands — the first real datapoint for
     the purchase.
  5. Multi-sample throughput: N samples' BAMs against one resident reference/GTF, GPU chain
     interleaved; samples/hour vs N. This is the number that feeds the cost curve.

### Phase 2b — Seed-search harness (1–2 weeks, parallel or after 2a)

- [ ] `sa_search_{cpu,cuda,metal}` over the real STAR Genome/SA/SAindex via `libumem`; one
      warp/SIMD-group per read; batches of 64k–1M reads; seed intervals written to shared
      output buffer the CPU stitches without copy.
- [ ] `kmer_lookup_*` pufferfish-style minimizer → hash probe (salmon pattern).
- [ ] Reads/s CPU (all threads) vs GPU, Tier 1 20M subsample; page-size plot; CPU-stitch /
      GPU-search overlap to see whether both can run at full rate on the shared fabric.
- **Gate:** ≥2× on the profiled hot loop on the M4 Pro.

### Phase 3 — Integration (only past a Phase 2 gate)

- [ ] BAM chain: a single native tool `umbam` (`sort`, `markdup`, `count`, `stats` subcommands,
      output byte-compatible with samtools/picard). nf-core `nextflow.config` process overrides
      point `SAMTOOLS_SORT`, `PICARD_MARKDUPLICATES`, `SUBREAD_FEATURECOUNTS` at it.
      Container on Spark; native conda-profile process on Mac.
- [ ] Seed search (if 2b passed): fork STAR with `--seedSearchGPU`; bit-identical BAMs.
- [ ] Tier 2A sex-gene DE reproduces; Tier 2B TaqMan correlation matches CPU pipeline.

### Phase 4 — Decision memo

The cost curve: $/sample vs monthly volume for AWS Batch (flat $12), Spark CPU-only, Spark
GPU, Mac Studio CPU-only, Mac Studio GPU; break-even points; boxes needed at the team's
actual monthly volume. Supporting: wall time, energy, CPU-bytes-touched per configuration; a
rented PCIe Hopper box running the same harness with explicit staging for contrast; the
page-size plot. Ultra results split into **bandwidth-scaled** (our kernels, no tensor units —
the honest 1.2 TB/s test) and **tensor-scaled** (anything reaching MPS/TensorOps), so the
result isn't misread as riding the basecalling headline. Decide: continue / narrow / stop.

### Milestones

| Phase | Effort | Exit criterion |
|---|---|---|
| 0 | 1–2 d | nf-core Tier 0 runs on both; `libumem` hugepages verified on Spark |
| 1 | 2–3 d | wall-time-share table; inflate share known; Amdahl gate applied |
| 1.5 | 3–5 d | GPU inflate vs all-CPU inflate, both boxes; starvation gate |
| 2a | 1–2 wk | BAM chain GB/s table; CPU-bytes-touched; page-size plot; bandwidth-scaling curve |
| 2b | 1–2 wk | reads/s CPU vs CUDA vs Metal |
| 3 | 1–2 wk | byte-compatible outputs; Tier 2 biology reproduced |
| 4 | 1 d | memo; re-run 2a unchanged on the M5 Ultra |

### Risks / open questions

- **Metal inflate is real engineering** — no library to lean on. Budget it honestly; it is also
  the most reusable artifact (every BGZF/CRAM/gzip consumer benefits).
- **Byte-compatibility with samtools/picard** (sort stability, dup tie-breaking) is fiddly; test
  on Tier 0 from day one.
- **Native-process nf-core on macOS** is off the beaten path; bioconda osx-arm64 gaps may force
  source builds of half the pipeline.
- **THP determinism on the Spark**; mitigate with hugetlbfs in `libumem`.
- **M4 Pro 24 GB** — Metal results are on reduced sizes until the Ultra arrives; label them.
- **Old PoC evidence** — worth an hour of digging; GPU-busy/wall ratio would confirm the
  starvation reading.
- **STAR seed search is latency-bound** (dependent binary search, ~600 ns/hop on LPDDR);
  throughput only from massive batching, which changes STAR's streaming structure. Salmon's
  independent probes are friendlier. Phase 1 profile decides which.
- **Don't let the basecalling stat inflate expectations** for a non-matmul workload. If the
  Ultra beats its own CPU by 2× on the BAM chain, that's the real result.
- **Long-read option (later, not now):** a public ONT direct-RNA sample (GIAB HG002) would test
  the "post-Dorado chain" story on the Ultra with no kernel changes. Tier 1/2 stay short-read.
- **Utilization is the real cost driver** for a fixed asset. The team's actual monthly sample
  volume is the input that turns the curve into a decision; get that number.
