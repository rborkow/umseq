# P2C-THP-X86-PORT — does the −12% madvise win port to the x86 Batch fleet?

Model: gpt-5.6-luna. Sandbox: workspace-write. No commit. No ssh, no AWS calls — the
orchestrator/user runs the bundle on Batch; your deliverable is a self-contained, tested
bundle plus the analysis script.

## Context

Read `.hermes/plans/2026-09-08_30k-six-threads.md` (thread #1), `AGENTS.md`, and
`bench/PHASE2C-integrate-1.md` §"Round 7b huge-page ablation" (search `thp_ablation`) and the
table near line 495–505. Finding, on the GB10/aarch64 Spark: `madvise(MADV_HUGEPAGE)` on STAR's
index arrays alone (same binary, `STAR_INTEGRATE_THP=0` vs on, GPU off) is **−11.9% CPU-s**
(754.5 → 664.8) on 20M pairs, outputs `cmp`-identical, index 28.0 GB huge-backed vs 0. It is
the only robust STAR result in the project and the only one that could move the $12/sample
Batch number — IF it holds on x86 under the fleet's THP policy.

The mechanism is `starIntegrateAdviseHuge` in `bench/star-integrate/make_star_integrate.py`
(lines ~97–100: `PackedArray.cpp` / `Genome_genomeLoad.cpp` patches). It is ~15 lines of C
that could be applied to stock STAR with no other integration. Read it.

Unknowns this card exists to resolve — each is a hypothesis with a stated way to fail:

- H1: Batch's AMI (Amazon Linux 2023 / ECS-optimized) has `transparent_hugepage/enabled` =
  `madvise` (default on AL2023) — so madvise works without any system change. Fails if it is
  `never`. If it is `always`, stock STAR already gets huge pages and there is nothing to port.
- H2: The gain on x86 is of the same order. x86 has 2 MiB THP (same as aarch64 4K-granule
  Linux) but a different TLB reach and page-walk cost; the number could be half or double.
  Fails if < 3%.
- H3: `khugepaged`/`defrag` settings matter for an mmap'd read-only file-backed index. Note
  STAR reads the index into anonymous memory (`Genome_genomeLoad.cpp`: `new char[]` +
  `fstream::read`), so this is anonymous THP, not file THP — confirm from source, because it
  decides whether `defrag` policy is in play.

## Deliverables (all under `bench/thp-x86/`, new directory; touch nothing else)

1. `patch-stock-star.sh`: takes a STAR 2.7.11b source tree, applies ONLY the
   `starIntegrateAdviseHuge` hook (advice on the three index arrays, run-time switch
   `STAR_THP=0/1`, `perror` on failure, `sysconf` page size), builds a static-ish Linux binary
   with STAR's own Makefile (`make STAR CXXFLAGS_common=...`). Extract the patch text from the
   generator by reading it, do not hand-retype — and add a test that the extracted hook is
   byte-identical to what the generator emits (`test_patch_stock_star.py`).
2. `run_thp_matrix.sh`: on the target box, records `cat /sys/kernel/mm/transparent_hugepage/
   {enabled,defrag,khugepaged/defrag}`, `uname -r`, CPU model, memory; then runs, rotated,
   3× each: (a) stock binary, (b) patched binary `STAR_THP=0`, (c) patched `STAR_THP=1`, on
   the 20M-pair ERR188140 slice with the one-pass argv from
   `bench/evidence/integrate-full-depth/stock-20260908-argv.json` (path placeholders). Each run:
   `/usr/bin/time -f '%e\t%U\t%S\t%M'` to a TSV row appended as it goes; after the run
   `grep AnonHugePages /proc/<pid>/smaps_rollup` sampled mid-run (background sampler, per the
   pattern in `bench/star-integrate/host_observation.py`); `cmp` of `Aligned.out.sam` and
   `SJ.out.tab` against arm (a)'s first run. Warm the page cache with a `cat` of the index
   before every run (as `thp_ablation.sh` does — read it).
3. `analyze_thp_matrix.py` + `test_analyze_thp_matrix.py`: reads the TSV, prints per-arm mean
   CPU-s (U+S) with n and range, the two ratios with the comparator named
   ((c) vs (a) = "vs stock"; (c) vs (b) = "madvise alone, same binary"), AnonHugePages per arm,
   and the `cmp` results. Refuses to print a ratio if any `cmp` differed. Test with a synthetic
   TSV.
4. `README.md`: exact steps for an operator on a fresh Batch/EC2 box (instance type
   suggestion: whatever the team's Batch compute environment uses — leave a placeholder and
   say what to check), where the index and FASTQ slice come from (S3 paths are placeholders;
   the Spark's `~/uni-rnaseq/data/index/star_full` and `data/samples/ERR188140_20M` are the
   sources), expected runtime, and the decision rule: ≥8% CPU-s (c vs a) funds a
   launch-template/AMI change; <3% closes the thread; between, report and stop.

## Constraints

- Mac only for you: shell-check the scripts (`bash -n`), run the Python tests with
  `python3.13`. You cannot build STAR here for Linux; say so rather than claim it built.
- No numbers in the README that were not measured; the Spark −11.9% is quoted with its
  comparator, nothing is projected for x86.
- Do not modify `make_star_integrate.py` or anything under `bench/star-integrate/`.

Finish with: files touched, tests run, the three hypotheses with what you found in the
source for H3, and anything the operator must decide before running (blocked / needs-human).
