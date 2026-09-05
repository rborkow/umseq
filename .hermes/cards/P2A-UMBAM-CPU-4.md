# Card P2A-UMBAM-CPU-4 — featureCounts and genomecov (the last two gates)

`crates/umbam` now passes 4 of 6 Tier 0 gates: sorted, flagstat/idxstats, markdup+metrics
(0 differing flags — the orchestrator fixed the tie-break: Picard keeps the *first pair in
stream order*, so the coordinate sort now tie-breaks on input index like samtools; see
`compare_headers` and `best_record_pair`), timing. Remaining:

```
featurecounts_gate  FAIL  gene row count differs (0 rows written)
genomecov_gate      FAIL  0 actual vs 471909 expected records
```

Run: `source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq &&
cargo test -p umbam --release -- --ignored`. Don't modify `tests/tier0.rs` except to add
tests. Original brief `.hermes/cards/P2A-UMBAM-CPU.md` still applies (architecture, hard
requirements). Both stages read the resident table+arena; parallelize over tid with rayon.

## genomecov first (simpler; byte-identical gate)
Golden `~/uni-rnaseq-data/tier0/chr22.genomecov.bg` = `bedtools genomecov -ibam sorted.bam
-bg -split`. Semantics:
- Every record that is not unmapped (0x4) contributes — **including** secondary,
  supplementary, duplicate, and QC-fail. bedtools does not filter on flag.
- Contribution = the reference intervals covered by CIGAR ops M, =, X, **D** (bedtools counts
  D as covered). N is a gap (`-split`). I/S/H/P consume no reference.
- Depth per base; emit `chrom\tstart\tend\tdepth` 0-based half-open runs; omit depth-0 runs;
  merge adjacent runs of equal depth; chromosomes in @SQ header order.
Implementation: per tid, a difference array over the covered span (or a sorted event list —
Tier 0 has 1.02M records; the 20M BAM will have 54M, spread over 3 Gbp, so don't allocate
a full-genome i32 array per tid naively — allocate per tid on demand, sized to that tid's
length, and only for tids that have records). Check the first ten lines of the golden by hand
against your output before running the gate.

## featureCounts (count column must match for every gene)
Golden `chr22.featureCounts.txt` = `featureCounts -p --countReadPairs -s 0 -t exon -g gene_id
-a chr22.gtf`. Semantics (Subread 2.1.1 defaults):
- Features: GTF lines with type `exon`; gene = `gene_id` attribute. Gene's features are its
  exons **unmerged** for the Chr/Start/End/Strand columns, but overlap testing is against the
  union.
- Only primary alignments (skip 0x100/0x800). Skip unmapped. **Multi-mapping reads are
  skipped by default** (`-M` not given) — featureCounts decides "multi-mapping" by the `NH`
  tag > 1 (STAR sets it). Duplicates are NOT skipped by default (no `--ignoreDup`).
- `-p --countReadPairs`: count **fragments**. Both mates are considered together; a fragment
  is assigned if the union of the two mates' aligned blocks (CIGAR M/=/X/D consume reference;
  N does not — featureCounts treats N as a gap when `--splitOnly`/... not given? — NO: by
  default featureCounts treats each M block separately, i.e. N-gaps are not counted as
  overlap) overlaps ≥1 bp with ≥1 exon of exactly one gene. Overlap with exons of ≥2 genes
  → unassigned (ambiguity). `-s 0`: strand ignored.
- Chimeric pairs (mates on different chromosomes) are **not counted** by default
  (`-C` absent … careful: in featureCounts ≥2.0 the flag semantics flipped; `-C` now means
  "do NOT count chimeric fragments". Default: chimeric fragments ARE counted, each mate's
  blocks contribute). Pairs where only one mate is mapped: the fragment is counted using the
  mapped mate's blocks.
- Fragment identity is by read name; both mates must be primary. Do not double count.
- Output: `# Program:featureCounts v2.1.1; Command:"umbam"` (test ignores text), then the
  header `Geneid\tChr\tStart\tEnd\tStrand\tLength\t<absolute input path>`, then genes **in
  the order they first appear in the GTF** with Chr/Start/End/Strand as `;`-joined per exon
  in GTF order, Length = total merged exon length.
If the count column disagrees after a faithful implementation, diff against the golden to
find the class of fragment that differs (spliced? chimeric? mate-unmapped? multi-overlap?)
and either fix the rule or document the disagreement in `crates/umbam/COMPAT.md` with a
concrete example read name. Don't guess rules; measure.

## Hard requirements (unchanged)
Rust; `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean; readable
named helpers; `unsafe` only in `umem`; no `git commit`; don't touch
`docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/`; don't touch the Spark.

Finish with the six-gate table, `timing.tsv` from a release run at `--threads 12`, and any
COMPAT.md entries.
