# Compatibility notes

## RSeQC `bam_stat` and sequence `read_duplication`

`umbam chain --qc` writes byte-identical Tier 0 text for `rseqc/bam_stat.txt`,
`rseqc/seq.DupRate.xls`, and `rseqc/pos.DupRate.xls`. These reductions run against the resident mark-duplicate decision,
not the original input flags: the Tier 0 input is pre-markdup, so its 105,016 duplicate bits
exist only after the resident markdup stage.

`pos.DupRate.xls` is also byte-identical. Its key mirrors RSeQC's `fetch_exon` bug: a soft
clip advances the reference coordinate, while `=` and `X` contribute no exon block. It retains
secondary, supplementary, and duplicate records, filtering only unmapped/QC-failed records and
MAPQ below 30.

## RSeQC `read_distribution`, junction annotation, and infer experiment

`read_distribution.txt` is byte-identical on Tier 0. Its midpoint lookup deliberately mirrors
`bx-python`'s zero-width `Intersecter.find(mid, mid)` query: an interval is a hit only when
`start < mid < end`. In particular, a midpoint exactly at a BED interval start is unassigned;
ordinary half-open point membership would be wrong here. BED chromosomes and read chromosomes
are both normalized to uppercase, as RSeQC's `build_bitsets` and read path do.

The deterministic `chr22.junction_annotation.log` and `infer_experiment.txt` outputs are also
byte-identical. `infer_experiment` scans coordinate order and stops after RSeQC's default
200,000 usable gene-overlapping reads. Mixed-strand gene overlaps retain RSeQC's reference
runtime spelling (`-:+`), which therefore falls into its failed-to-determine bucket.

`junctionSaturation_plot.r` is emitted in RSeQC's source layout. RSeQC calls an unseeded
`random.shuffle`, so the 5–95% samples are inherently run-dependent; `umbam` uses a local
xorshift64* Fisher--Yates shuffle with seed `0x52534551435f5341` for those statistically
equivalent points. The Tier 0 compatibility gate deliberately checks only the final (100%)
entries of `y`, `z`, and `w` (known, all, and novel junction totals).

## RSeQC `mRNA_inner_distance`

`chr22.inner_distance_freq.txt` is byte-identical on Tier 0. Although the library method defaults
to 0–1000 bp in 10 bp windows, the CLI supplies −250–250 in 5 bp windows. The bx-python interval
query makes each histogram window `(left, right]`: an observed distance exactly equal to a left
edge belongs in the preceding window. Accepted records are scanned in coordinate order and skip
QC-failed, duplicate, secondary, unmapped, unpaired, mate-unmapped, and MAPQ<30 alignments.
Pysam's `qlen` here is `query_alignment_length` (M/I/=/X), so soft clips are excluded. For a
same-transcript positive genomic gap, RSeQC inserts the unioned exonic distance when it is
positive; otherwise it inserts the genomic distance. Different-chromosome pairs are reported as
`NA` and do not contribute to the histogram.

## featureCounts paired-end ambiguity

Measured against Subread 2.1.1 (`featureCounts -p --countReadPairs -R CORE`),
`umbam` first computes each mapped mate's gene set independently. A hit is at
least one base of an `M`, `=`, or `X` CIGAR block overlapping an exon; `N` and
`D` are gaps. If both mates have hits, use their non-empty intersection; if
the intersection is empty, use their union. If only one mate has hits (which
also covers a mate-unmapped fragment), use that mate's set. Assign exactly one
candidate gene; otherwise leave the fragment unassigned. Reads with `NH > 1`
or non-primary/supplementary records are not counted.

The CORE comparison initially exposed 658 missing assignments, all from the
one-mate-hit / mate-unmapped branch that the implementation had accidentally
discarded. Their breakdown was: both-mates-hit 0, one-mate-hit 658, spliced
156, chimeric 0, mate-unmapped 658. Retaining the mapped mate made all 1,747
Tier 0 gene counts byte-identical to featureCounts. The key paired example is
`ERR188140.30725820`: mate 1 hits `ENSG00000290397.2` and
`ENSG00000310416.1`, mate 2 hits only `ENSG00000290397.2`, and the
intersection assigns `ENSG00000290397.2`.

## genomecov deletions

The Tier 0 `bedtools genomecov -bg -split` golden does not cover `D` CIGAR
positions, despite the card stating that deletions are covered. For example,
at chr1:35351419, several `3D` operations end and the golden depth falls from
21 to 9. `umbam` follows the measured bedtools output for byte-compatible
genomecov output: `M`, `=`, and `X` cover bases; `D` and `N` advance the
reference but are gaps.

## markdup: multi-library single-end sets (known divergence from Picard, preserved)

Picard's duplicate sets are per library (`LB` of the read group): two unpaired reads at the
same fragment end but with different `LB` are never duplicates of each other. `umbam`'s
`FragmentEnd` key does not include the library, so it would mark the lower-quality one.
Concrete case: two single-end reads at `chr1:101`, both `10M`, `RG`s with different `LB` —
Picard 3.5.0 marks neither, `umbam` marks one. Tier 0 has a single library and cannot
observe this. Preserved deliberately until a multi-library fixture exists; the metrics'
`LIBRARY` column is likewise single-valued.

## BGZF write layout and compression level

The coordinate writer plans record-aligned BGZF blocks before launching parallel encoders.
Normal BAM records are never split across blocks; BAI virtual offsets are derived from the
completed block table. The production compression level is flate2 level 6 (the noodles
default). On the 12-thread Tier 0 fixture, level 1 wrote sorted/markdup BAMs in 0.132/0.164 s
at 82,242,545/82,408,612 bytes; level 3 in 0.178/0.213 s at 60,967,730/61,116,967 bytes; and
level 6 in 0.247/0.262 s at 56,872,010/57,015,572 bytes. Level 1 is faster but grows output by
44.6%, so it does not meet the less-than-10% size-growth criterion; level 6 remains selected.
`UMBAM_BGZF_LEVEL` is a process-scoped benchmarking override only.

## dupRadar

`umbam chain --qc` writes `dupradar/dupMatrix.txt`.  It reproduces dupRadar 1.38's four
Rsubread calls over the resident mark-duplicate decisions, including secondary alignments in
the multi-mapping calls and `HI`-paired secondary mates.  Tier 0 IDs, merged exon lengths, and
the four count columns are byte-identical.  The numeric gate compares rates and RPK/RPKM values
at six significant digits: R's `write.table` and Rust's float renderer select different final
decimal digits despite equivalent IEEE-754 values.

## Qualimap RNA-seq

`qualimap/rnaseq_qc_results.txt` currently emits the deterministic alignment and exon-assignment
reduction, but does not claim Qualimap 2.3 parity.  The Tier 0 report's transcript profile needs
the top-1,000 transcript coverage sweep, and its junction motif distribution needs the reference
sequence, neither of which is resident in the BAM/GTF inputs.  The genomic-origin subdivision
also remains incompatible: the resident union-exon assignment finds the same 859,269 uniquely
mapped primary reads but categorizes 704,856 / 130,307 / 24,106 rather than Qualimap's
734,257 / 83,587 / 41,425 (gene / ambiguous / no-feature).  Consequently no Qualimap golden
gate is enabled yet.
