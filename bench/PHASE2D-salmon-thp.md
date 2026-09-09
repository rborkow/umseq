# Salmon THP screen — main alignment-quant baseline/profile in progress

## Current status (2026-09-08)

**The main nf-core salmon quantification stage consumes transcriptome BAM, not the large mapping index.** The original “same random-index shape as STAR” hypothesis therefore does not directly describe this stage. A separate salmon invocation uses the index on subsampled FASTQs with `--skipQuant` for library inference. Profile/attribute the correct stage before deciding whether any targeted huge-page patch is worthwhile.

The pinned main alignment-quant baseline/profile is now running. No Salmon code,
allocator policy or THP intervention has been changed; no saving or output parity
is claimed yet. Do not insert a Salmon saving into the cost model.

## Observed invocation and provenance

Read on Spark from the completed full-depth pipeline:

- Run root: `/home/rborkows/uni-rnaseq/runs/tier1-full`
- Main quant work directory: `work/39/70bd8cb234788b453b34549e376c74`
- `.command.env` reports **salmon 1.10.3**.
- `.command.run` names container `community.wave.seqera.io/library/salmon:1.10.3--dc33937abc5bffd1`, linux/arm64.
- Invocation from `.command.sh`: `salmon quant --geneMap gencode.v49.primary_assembly.annotation.filtered.gtf --threads 16 --libType=IU -t genome.transcripts.fa -a NA12716_full.Aligned.toTranscriptome.out.bam -o NA12716_full`.
- Existing `aux_info/meta_info.json`: `num_processed=69176421`, `num_mapped=69176421`, `library_types=["IU"]`; empty `index_seq_hash` and `index_name_hash`.

Resolved read-only inputs:

| Input | Path | bytes |
|---|---|---:|
| Transcriptome BAM | `/home/rborkows/uni-rnaseq/runs/tier1-full/work/d4/03324a5759b9c724530c12ab4d0efc/NA12716_full.Aligned.toTranscriptome.out.bam` | 38804841527 |
| Transcript FASTA | `/home/rborkows/uni-rnaseq/runs/tier1-full/work/8b/ca04bcd0afc478697cd94de0f591ae/genome.transcripts.fa` | 922198580 |
| GTF | `/home/rborkows/uni-rnaseq/runs/tier1-full/work/73/514e12d4643c95e65a62c68a927f7b/gencode.v49.primary_assembly.annotation.filtered.gtf` | 3313195627 |

The other salmon task, `work/86/2859c6caa8f769ec4f4ed91e29f0cc`, invokes `--index salmon_nfcore_1.10.3 -1 NA12716_full.subsampled_R1.fastq.gz -2 NA12716_full.subsampled_R2.fastq.gz --skipQuant --libType=A`, also with 16 threads. Do not confuse it with the main quant stage. Tier2A also has a subsampled `--skipQuant` task, observed at `work/f7/bfdf24dee2ab303b9af878c49d04a6` for NA12775_M.

Installed native `/home/rborkows/micromamba/envs/rnaseq/bin/salmon --version` currently prints **2.7.0**, not 1.10.3. Do not use it as the same-version pipeline baseline. The targeted source search under `~/.local/opt`, `~/src`, and `~/uni-rnaseq-probe-lab` found no salmon-named source distribution; this is not an exhaustive host search.

## Next discriminating check

Additional baseline check: the cached image resolves to immutable digest
`sha256:878e1fb2cdfc25144d17aeebd033a193a3515674557e72d9b9b8c393b29ecd61`.
A read-only, network-disabled invocation of that digest confirms
`/opt/conda/bin/salmon` reports `salmon 1.10.3`. `ldd` shows bundled jemalloc,
TBB/TBBmalloc and Boost dependencies under `/opt/conda/lib`. This is an executable
prerequisite check only; no quantification was run. Allocation-site/backing inspection
must identify the allocator actually serving hot structures, rather than assume glibc.

1. Bind the model's salmon CPU bucket to actual trace task names (main alignment quant versus subsampled inference).
2. Verify the cached 1.10.3 container identity and matching complete buildable source. Exercise that baseline, preserving all inputs read-only and writing only to a fresh lab root.
3. Profile main quant and inspect its hot resident allocations. If the large random-index walk is absent, close that specific hypothesis as inapplicable. Only fund a different allocation-targeted patch if this profile identifies substantial removable translation-sensitive work.
4. Any separate mapping-mode/index experiment is a different workload and cannot silently update star_salmon pipeline savings.

## Active main-stage baseline/profile

The STAR follow-through has completed its collapse-enabled full-depth gate. The
serialized Spark slot now holds this one Salmon job:
`~/uni-rnaseq-probe-lab/salmon-alignment-screen-20260908`, monitor
`proc_32089cc962ea`, launcher PID 847585. Target PID 848286 was positively
identified and the host perf sampler attached. No other heavy job was launched.

The historical task-name binding is explicit: `trace.txt` reports the main
`QUANTIFY_BAM_SALMON:SALMON_QUANT` stage at 528 s / 864.9% CPU, approximately
**76.1112 CPU-min** from rounded Nextflow fields, versus 23.4 s / 252.4%,
approximately **0.98436 CPU-min**, for the subsampled library-inference Salmon
task. These are historical trace-derived estimates, not new measurements or a
change to the cost model's aggregate bucket.

Protocol:
- Exact cached arm64 1.10.3 image digest above, original quant options and 16
  threads; only input/output path prefixes change. BAM, FASTA and GTF mounted
  read-only, output in a fresh private lab directory, network disabled, invoking
  uid/gid, no added capabilities and no-new-privileges. Original work is untouched.
- Hash the actual input files before the run and record image identity and any
  image-provided allocator variables. This warms input caches; no cold-start or
  precision timing-comparison claim is made.
- Time the Salmon executable **inside** the container using Bash's reserved
  `time` (wall/user/system), not the Docker client. `python3` and an external
  `time` executable were not found on the image PATH. A real image/version invocation verified Bash
  timing; a same-user container CPU-loop smoke verified host perf attach (517
  actual samples), not a Salmon timing result.
- Attach `perf` cpu-clock sampling at 199 Hz after target discovery; initial
  startup before attachment is excluded from the profile but included in target
  timing. Observe process memory, swap, anonymous VMAs at least 2 MiB and mapped
  library paths. Mapped jemalloc/TBB libraries alone do not prove which allocator
  serves hot allocations.
- Preserve container state, raw outputs/timing, target exit, real processed and
  mapped counts, and exact `cmp` exits for both quantification tables against the
  old golden. No filtering or numerical normalization. Any differences will be
  reported, not quietly treated as baseline parity.
- Retain the stopped container and copy only mapped public executable/library
  files to a private symbol root for post-run symbolization. A baseline success
  without a valid profile remains partial evidence.
- External 5400-second bound; 3600-second target wait and bounded container stop.
  Frozen runner/helper hashes verified. Scripts and artifacts stay on Spark; no
  source replacement or estimated GPU/THP gain is part of this run.

Upstream source for interpretation was fetched at tag `v1.10.3`, peeled commit
`a2f6912b3f9f9af91e3a4b0d74adcb3bdc4c9a32`, into
`/private/tmp/salmon-v1.10.3-screen-source` on Mac. This is not a claim that an
upstream checkout alone reproduces the container's complete patched build.
`src/Salmon.cpp` routes alignment input to `salmonAlignmentQuantify`; the profile
will determine which main-quant structures, if any, justify further THP work.

## Result (2026-09-08 14:51 PDT, orchestrator analysis)

Evidence: `bench/evidence/salmon-alignment-screen/` (`target-time.tsv`, `target.exit`,
`perf-by-dso.txt`, `perf-top20-symbolized.txt`, `protocol.json`, `perf.argv.json`).
Raw `perf.data` (980,393 samples, 114 MB), `host-observation.jsonl` (255 samples),
container `db01a4432cba` (Exited 0) and the copied symbol root stay on Spark under
`~/uni-rnaseq-probe-lab/salmon-alignment-screen-20260908/`. Symbolized against the
container's own `/opt/conda/bin/salmon` (sha256 `fe368fc8fa10eb51…`) and its 12 bundled libs.

**Run:** target exit 0; in-container Bash `time` **535.35 wall / 4857.79 user / 79.23 sys s**
(≈ 82.3 CPU-min at 16 threads). `meta_info.json`: `num_processed = num_mapped = 69,176,421`,
100% mapped, `library_types=["IU"]` — identical counts to the golden's `meta_info.json`.
Historical nf-core trace for the same task: 528 s / 864.9% ≈ 76.1 CPU-min; this run is
+8% CPU on a warm-cache, perf-attached, host-observed process — not a controlled timing.

**Output comparison vs the golden (`cmp`, no normalization): DIFFERS.** `quant.sf`
(509,650 rows) and `quant.genes.sf` (78,899 rows) both differ from line 2. Name column
identical in every row and row order identical; `Length`/`EffectiveLength`, `TPM` and
`NumReads` differ. Transcript level: 137,860 rows differ in TPM, 104,893 in NumReads;
|ΔTPM|/TPM median 2.8e-4, p99 5.8e-2, max 7.9; |ΔNumReads| median 0, p99 2.6, max 3.5e3;
ΣNumReads 69,174,898.9 vs 69,174,898.8. Gene level: 28,797 TPM / 10,425 NumReads rows differ,
p99 |ΔTPM|/TPM 3.6e-2. This is consistent with Salmon's documented run-to-run
nondeterminism under multi-threaded online EM (thread-ordered atomic updates to
`AlignmentModel`/`AtomicMatrix<double>`, then VBEM), **not with an input, version or
protocol mismatch** — but that is a hypothesis; the disqualifier is a second run of the
same container/argv against this run, which was not funded. Either way Salmon has no
byte-identical golden to gate a replacement against; any future Salmon work must first
establish the tool's own run-to-run envelope, per COMPAT.md practice.

**Profile (self time, by DSO):** `salmon` 74.6%, `libgcc_s` 15.9% (`__multf3`/`__divtf3`/
`__addtf3`/`__subtf3`/`__sfp_handle_exceptions` — soft-float **128-bit long double** arithmetic
on aarch64), `libm` 5.2%, `libc` 1.9%, **kernel 1.5%**, `libjemalloc` 0.5%, `libtbb` 0.2%.

Top symbols: `__aarch64_cas8_acq_rel` 22.7% (outlined CAS; 18% of it under one unresolved
caller in the alignment-processing threads, 2% `AlignmentModel::update`, 1% TBB queue pop),
`AlignmentModel::logLikelihood` 18.8%, VBEM `start_for` 6.2% + 1.2%, `ReaderWriterQueue::
try_dequeue` 4.9% (spin), `inflate_fast` 4.6% (BAM BGZF decompression), `bam_get_seq` 2.5%,
`processMiniBatch` 2.2%, `crc32_16bytes` 1.7%.

**Decision: the huge-page hypothesis for Salmon is closed as inapplicable.** The main quant
stage is a streaming BAM consumer whose cost is (a) atomic contention on the shared
alignment-model matrices, (b) the log-likelihood arithmetic itself, (c) soft-float
`long double` on arm64, and (d) BGZF inflate. Kernel time is 1.5% of samples and
`host-observation.jsonl` shows zero `AnonHugePages` in every ≥2 MiB anonymous VMA across
all 255 samples with no swap — there is no TLB/page-walk term to remove. No index walk
exists in this stage; the index is touched only by the ~1 CPU-min `--skipQuant` library
inference task. **No Salmon saving enters the cost model.** The lock-free/contention and
long-double findings are real but are Salmon-internal changes to a tool without a
byte-identical gate; they are not in scope for this project's replacement strategy.
