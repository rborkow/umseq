# STAR x86 THP portability probe

This bundle answers whether the STAR anonymous-index `madvise(MADV_HUGEPAGE)` hook transfers to the team’s x86 Batch fleet. It does not change the host THP policy, and it does not claim an x86 result before the operator runs it.

## Operator run

1. Choose the team’s Batch compute-environment instance type (placeholder: `BATCH_INSTANCE_TYPE`). Check the actual AMI, kernel, architecture, vCPU count, RAM, and whether `/sys/kernel/mm/transparent_hugepage` is readable. The intended check is the fleet’s real Amazon Linux 2023/ECS-optimized image, not a local workstation.
2. Copy this directory and a clean STAR 2.7.11b source tree to the box. Run `patch-stock-star.sh /path/to/STAR-2.7.11b/source-tree`. The script edits only `PackedArray.cpp` and `Genome_genomeLoad.cpp` and builds with STAR’s own Makefile. Expected build time is not specified here because it must be measured on the selected instance.
3. Set these paths, using the same data sources as the Spark experiment:

   `INDEX_DIR=/path/to/star_full` (Spark source: `~/uni-rnaseq/data/index/star_full`)

   `READ1=/path/to/ERR188140_1.fastq.gz` and `READ2=/path/to/ERR188140_2.fastq.gz` (Spark source: `~/uni-rnaseq/data/samples/ERR188140_20M`; S3 placeholders: `s3://TEAM-BUCKET/uni-rnaseq/index/star_full` and `s3://TEAM-BUCKET/uni-rnaseq/samples/ERR188140_20M`)

   Also set `STOCK_STAR`, `PATCHED_STAR`, and `ARGV_JSON=/path/to/stock-20260908-argv.json`. The JSON is the one-pass argv from `bench/evidence/integrate-full-depth/stock-20260908-argv.json`; the runner overrides the binary, index, output prefix, and read limit to 20,000,000 pairs.

4. Run `run_thp_matrix.sh /absolute/path/to/results`. It records kernel/CPU/memory and THP `enabled`, `defrag`, and `khugepaged/defrag`, warms `Genome`, `SA`, and `SAindex` before every run, and performs three rotated repeats of stock, patched `STAR_THP=0`, and patched `STAR_THP=1`. It samples `AnonHugePages` from the real STAR process during each run and appends timing rows as runs finish.
5. Run `python3 analyze_thp_matrix.py /absolute/path/to/results/matrix.tsv`.

Allow the operator to measure runtime on the selected instance; no runtime estimate is asserted here. The Spark/aarch64 reference was 754.5 CPU-s for the same-binary advice-off comparator and 664.8 CPU-s with advice on (−11.9%, `cmp`-identical outputs); it is context, not an x86 projection.

Decision rule: ≥8% CPU-s for patched-on vs stock funds a launch-template/AMI change; <3% closes the thread; between those values, report the measured result and stop. Ratios are valid only when both `Aligned.out.sam` and `SJ.out.tab` compare identically for every row.

The source confirms H3’s mechanism: STAR allocates `G1` with `new char[...]` and reads the Genome index with `fstream`, while `SA` and `SAi` allocate packed `charArray` buffers. These are anonymous allocations populated from files, not file-backed mmap mappings; therefore anonymous THP policy applies and `defrag`/`khugepaged` observations are relevant. This bundle does not alter those policies.

The operator must decide the real Batch instance type/AMI and provide the S3-to-local paths (or stage the Spark source data), stock STAR binary, clean 2.7.11b source, and argv JSON. No AWS or SSH access is assumed here.
