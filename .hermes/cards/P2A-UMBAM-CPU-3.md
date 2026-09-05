# Card P2A-UMBAM-CPU-3 — implement the four remaining stages; the gates are the spec

`crates/umbam` has a working resident layout (decode → `umem` table+arena → sort → BAM
writes → BAI) and the `sorted_bam_gate` **passes**. The other stages are placeholders. The
test harness in `crates/umbam/tests/tier0.rs` was rewritten by the orchestrator and is now
authoritative: each gate fails with a one-line diagnostic telling you exactly what's wrong.
Run with `cargo test -p umbam --release -- --ignored` (needs `samtools` on PATH; run
`source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq` first — the test
harness itself shells out to `samtools view`). Current status:

```
sorted_bam_gate            ok
flagstat_and_idxstats_gate FAIL  flagstat differs
markdup_and_metrics_gate   FAIL  105016 records have a differing 0x400 flag
featurecounts_gate         FAIL  gene row count differs (0 rows written)
genomecov_gate             FAIL  0 actual vs 471909 expected records
timing_gate                ok
```

Original brief: `.hermes/cards/P2A-UMBAM-CPU.md` (architecture, semantics, hard
requirements — all still apply). Don't modify `tests/tier0.rs` except to add tests.

## Do these in order; stop and report after any gate that resists more than ~2 attempts

### 1. flagstat + idxstats (easy — a single parallel reduction over the header table)
Goldens: `~/uni-rnaseq-data/tier0/chr22.flagstat.txt`, `chr22.idxstats.txt`. samtools 1.22
flagstat semantics: QC-fail is flag 0x200; "primary" excludes 0x100 and 0x800; "mapped"
excludes 0x4; "properly paired" = 0x1 && 0x2 && !0x4; "with itself and mate mapped" =
0x1 && !0x4 && !0x8; "singletons" = 0x1 && !0x4 && 0x8; "with mate mapped to a different chr"
= 0x1 && !0x4 && !0x8 && tid != mtid; the "(mapQ>=5)" variant adds mapq >= 5. Percentages
are printed as `%.2f%%` with "N/A" when the denominator is 0. Reproduce the exact text.
idxstats: per @SQ in header order `name\tlength\tmapped\tunmapped`, then `*\t0\t0\t<unmapped
with tid=-1>`. Note: samtools idxstats counts a read as "mapped" for a reference if it has
that tid and !0x4; unmapped-with-tid (placed unmapped mates) count in that reference's
unmapped column.

### 2. markdup + metrics (the hard one — Picard MarkDuplicates semantics)
Golden: `chr22.markdup.sam` (flags) and `chr22.markdup.metrics.txt`. Picard rules
(`--OPTICAL_DUPLICATE_PIXEL_DISTANCE 0`, `--READ_NAME_REGEX null`, default
`DUPLICATE_SCORING_STRATEGY=SUM_OF_BASE_QUALITIES`):
- Secondary (0x100) and supplementary (0x800) reads are **never** examined and never
  flagged by Picard. Unmapped reads are never flagged.
- A *fragment end* is: (library, tid, **unclipped** 5' position, strand). Unclipped 5' for
  forward strand = pos − leading soft/hard clips; for reverse strand = alignment end +
  trailing soft/hard clips (end = pos + reference length of CIGAR − 1).
- **Pairs**: both mates mapped, primary. Key = the two fragment ends ordered (lower first).
  All pairs with the same key form a duplicate set; the pair with the highest score
  (sum of base qualities ≥ 15 over **both** reads) is kept, all other pairs get 0x400 on
  both mates. Ties: Picard breaks by a deterministic hash of the read name — implement as:
  higher score wins; on equal score, the pair whose name sorts **first** under Picard's
  comparator wins. If you cannot reproduce the tie-break exactly, count how many records
  differ and report it — this is the one place an unexplained residual is acceptable, but
  it must be measured and small.
- **Unpaired reads** (mate unmapped or single-end): key = one fragment end. Same
  representative rule. BUT: an unpaired read whose fragment end coincides with a fragment
  end of any *pair* is a duplicate of that pair (pairs always win), regardless of score.
- Metrics: `UNPAIRED_READS_EXAMINED`, `READ_PAIRS_EXAMINED`, `SECONDARY_OR_SUPPLEMENTARY_RDS`,
  `UNMAPPED_READS`, `UNPAIRED_READ_DUPLICATES`, `READ_PAIR_DUPLICATES`,
  `READ_PAIR_OPTICAL_DUPLICATES` (0), `PERCENT_DUPLICATION` =
  (UNPAIRED_DUP + 2×PAIR_DUP) / (UNPAIRED_EXAMINED + 2×PAIRS_EXAMINED), `ESTIMATED_LIBRARY_SIZE`
  (Lander-Waterman; implement Picard's `estimateLibrarySize` or write `null`-equivalent
  empty — the test doesn't check it). Write the file in Picard's layout: `## htsjdk.samtools.
  metrics.StringHeader` lines, `## METRICS CLASS\tpicard.sam.DuplicationMetrics`, header row,
  data row. LIBRARY is the RG's LB, or "Unknown Library".
Parallelize over tid; the pair key spans two positions on possibly different tids — key the
pair on its **lower** fragment end's tid.

### 3. featureCounts
Golden: `chr22.featureCounts.txt`. Settings used: `-p --countReadPairs -s 0 -t exon -g gene_id`,
defaults otherwise. Semantics: build per-gene exon interval sets from the GTF (merge
overlapping exons of the same gene). A *fragment* (both mates, or the one mapped mate)
counts for a gene if ≥1 bp of any of its aligned blocks (CIGAR M/=/X, **not** N/D) overlaps
an exon of that gene. If it overlaps exons of >1 gene → not counted (ambiguous). Multi-mapping
fragments (NH>1) → not counted. Only primary alignments. Fragment is counted once even if
both mates hit. Output format: line 1 `# Program:featureCounts v2.1.1; Command:...` (any
text after `# Program:` is fine, the test ignores it), line 2 header `Geneid\tChr\tStart\tEnd
\tStrand\tLength\t<path>`, then one row per gene **in GTF first-appearance order**; Chr/Start/
End/Strand are `;`-joined per exon (after merge? — check the golden: featureCounts lists the
*original* exons, unmerged, in file order), Length is the merged exon length. Get the count
column right first; the test only compares Geneid→count, but keep the other columns honest.

### 4. genomecov
Golden: `chr22.genomecov.bg` (`bedtools genomecov -ibam sorted.bam -bg -split`). Every
alignment that's not 0x4 contributes its aligned blocks (M/=/X/D — bedtools counts D as
covered; N is a gap with `-split`) — including secondaries and duplicates (bedtools does
not filter on flag beyond unmapped). Output `chrom\tstart\tend\tdepth` (0-based half-open),
zero-depth runs omitted, adjacent equal-depth runs merged, chromosomes in header order.
Byte-identical is the gate.

## Hard requirements (unchanged)
Rust; `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean; readable
code with named helpers, no dense one-liners; `unsafe` only inside `umem`; no `git commit`;
don't touch `docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/`; don't touch the Spark.

Finish with the gate table (all six, pass/fail with the one-line diagnostic), the
`timing.tsv` from a release run with `--threads 12`, and anything you put in `COMPAT.md`.
