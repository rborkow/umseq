# P2C-STAR-PRODUCTION-GATE — integrated STAR under nf-core's real invocation

Model: gpt-5.6-terra. Sandbox: workspace-write. No commit. No ssh. No host runs — the
orchestrator runs the gate on the Spark; your deliverable is a runner that builds, whose
tests pass on the Mac, and a source-patch analysis with tests.

## Context

Read `.hermes/plans/2026-09-08_30k-six-threads.md` (thread #2), `AGENTS.md`, and
`bench/PHASE2C-full-depth.md`. The integrated STAR (generator `bench/star-integrate/
make_star_integrate.py`, source patch into STAR 2.7.11b at `/private/tmp/star-full-source.UVdsuH/
STAR-2.7.11b/source`) has passed strict parity eleven times plus full depth — but every gate used
this invocation (`bench/evidence/integrate-full-depth/stock-20260908-argv.json`):

```
--twopassMode None --outSAMtype SAM --outSAMorder PairedKeepInputOrder --outSAMunmapped Within
--genomeLoad NoSharedMemory --runRNGseed 777
```

nf-core actually runs (from the Spark's `runs/tier1-full/work/d4/03324a5759b9c724530c12ab4d0efc/.command.sh`):

```
STAR --genomeDir star_full --readFilesIn <R1.fq.gz> <R2.fq.gz> --runThreadN 16
  --outFileNamePrefix <S>. --sjdbGTFfile gencode.v49.primary_assembly.annotation.filtered.gtf
  --quantMode TranscriptomeSAM --outSAMtype BAM Unsorted --outSAMattributes NH HI AS NM MD
  --readFilesCommand zcat --twopassMode Basic --runRNGseed 0 --outFilterMultimapNmax 20
  --alignSJDBoverhangMin 1 --outSAMstrandField intronMotif --quantTranscriptomeSAMoutput BanSingleEnd
  --outSAMattrRGline 'ID:<S>' 'SM:<S>'
```

Three things are new to the integrated path and each is a hypothesis about where it breaks:

1. **`--twopassMode Basic`**: STAR maps once, collects junctions, re-inserts them into the
   genome (`sjdbInsertJunctions` → `Genome` re-built in place with new `SA`/`SAindex` extents and
   `chrStart` shifts), then maps again. Our window/pool/prefetch lifecycle, the borrowed-index
   hand-off to the GPU coordinator, and the huge-page advice all assume ONE index for the
   process. Read `twoPassRunPass1.cpp`, `sjdbInsertJunctions.cpp`, `Genome_insertSequences.cpp`
   and trace what our hook sees between pass 1 and pass 2. The round-9 shutdown bug was a
   lifecycle bug of exactly this shape.
2. **`--quantMode TranscriptomeSAM`**: a second output stream (`Aligned.toTranscriptome.out.bam`)
   produced by `ReadAlign_quantTranscriptome.cpp` from the same alignments. Our hook lives in
   `ReadAlign_maxMappableLength2strands` / `ReadAlign_oneRead`; confirm the transcriptome
   projection consumes the same `trMult` set and nothing we reorder or drop.
3. **`--outSAMtype BAM Unsorted` + `--sjdbGTFfile` at mapping time + `--runRNGseed 0`**: BAM
   output is via STAR's bundled htslib `bgzf`; with `--outSAMorder` default (not
   PairedKeepInputOrder) and 16 threads, output order is thread-chunk order. Determine whether
   stock STAR itself is byte-deterministic across two runs under this argv BEFORE asserting
   `cmp` on the BAMs — if it is not, the comparator is `samtools view` (no header) sorted by
   read name, and that normalization must be documented per AGENTS.md, not silently applied.

## Deliverables

1. `bench/star-integrate/run_production_gate.py` + `test_production_gate.py`. Model on
   `run_full_depth.py` (read it; keep its input-hash, lock, fresh-root, status-file, argv-record
   conventions). New: takes `--argv-template <json>` (the nf-core argv above with path
   placeholders), runs stock then integrated with the SAME argv (`--runThreadN 16`), and
   compares: `Aligned.out.bam`, `Aligned.toTranscriptome.out.bam`, `SJ.out.tab`, `Log.final.out`
   (non-timing fields), and `_STARpass1/SJ.out.tab`. Comparison mode is a CLI choice:
   `--compare cmp` (default) or `--compare namesorted-sam` (documented normalization, prints
   exactly what it normalized). Accept only: both exit 0, all comparisons pass, integrated sidecar
   shows `gpu_consumed > 0`, zero faults/rejections/strict mismatches. Tests use controlled
   subprocess results for every rejection path, as `test_full_depth_runner.py` does.
2. `bench/star-integrate/test_production_stock_determinism.py`: a runner mode
   `--stock-twice` that runs stock twice and `cmp`s — so the orchestrator can settle hypothesis 3
   on the host in one launch.
3. `docs/design-production-gate.md`: for each of the three hypotheses, the STAR source path
   (file:line) our hook meets, what you believe happens, and what the gate will show if you are
   wrong. If hypothesis 1 needs a generator change (e.g. re-arm the index hand-off after
   `sjdbInsertJunctions`), make it in `make_star_integrate.py` behind a source test in
   `test_source_patch.py`, keep the existing one-pass behaviour byte-identical (the existing
   tests must still pass), and say exactly what you changed. If you are not sure the change is
   safe without a host run, DO NOT guess — leave it out, state the hypothesis, and the
   orchestrator's first gate round will tell us.

## Constraints

- Files you may touch: the four new files above, `make_star_integrate.py`,
  `test_source_patch.py`. Nothing else. Do not edit `run_full_depth.py` or `run_host.py`
  or the checker `seed_split_parity.py`.
- Mac test toolchain: `python3.13`, `/opt/homebrew/opt/llvm/bin/clang++`,
  `CPATH=/opt/homebrew/opt/libomp/include LIBRARY_PATH=/opt/homebrew/opt/libomp/lib`.
  Run `test_source_patch.py`, `test_coordinator.py`, `test_window_prefix.py`,
  `test_window_contract.py` and your new tests before finishing; report pass/fail per file.
- Also run the syntax-only real-header compile from the skill:
  `c++ -fsyntax-only -std=c++11 -DSTAR_INTEGRATE=1 -I<stubs> -I. star_integrate.cpp`.
- No `mkdir -p` of generator-owned roots. No timing claims. clang-format any C++.
- The STAR source is the spec; where this card and the source disagree, the source wins.

Finish with: files touched (exact), tests run with results, the three hypotheses with your
verdict and confidence, and what the orchestrator should launch first (stock-twice vs full
gate). Blocked / needs-human items listed separately.
