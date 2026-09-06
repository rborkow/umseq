# Card P2A-UMQC-5 — junction_saturation (100% column), inner_distance, dupRadar, Qualimap

Six of the QC outputs are byte-identical. `junction_saturation` stopped correctly: the
source uses unseeded `random.shuffle`, so the 5–95% subsample arrays are not reproducible.
Resolution: **emit the file in RSeQC's exact `.junctionSaturation_plot.r` layout, with the
100% column computed exactly, and gate on the 100% entries only** (the last element of each
of the three arrays: all / known / novel junction counts). Subsample the 5–95% points with
a fixed-seed shuffle (document the seed in COMPAT.md); they'll be statistically equivalent,
not identical. Then continue, in order:

## inner_distance (`mRNA_inner_distance` in `docs/tool-src/rseqc-5.x-relevant.py`)
Read the source. Notes from my reading (source wins if I'm wrong): filters `!unmapped &&
!qcfail && !secondary && mapq >= 30 && is_paired && is_proper_pair? && mate mapped` —
check; only read1 is considered (`is_read1`) and its mate looked up; inner distance
computed from the two reads' fetch_exon blocks with the transcript-aware branch when both
fall in the same BED transcript's exon set (`foundone` with the strict semantics again,
plus a bitset test); `sample_size` 1,000,000 > Tier 0 reads, so no sampling. Output the
`inner_distance_freq.txt` histogram (`low_bound=0, up_bound=1000, step=10`; format per
source) — that's the gate. The per-read `.txt` is optional (20 MB on Tier 0).

## dupRadar (`docs/tool-src/dupradar-1.38-relevant.R`)
Four Rsubread `featureCounts` runs on the **markdup** BAM: `countMultiMappingReads ∈
{TRUE, FALSE}` × `ignoreDup ∈ {FALSE, TRUE}`, `isPairedEnd = TRUE`, `strandSpecific = 0`,
Rsubread defaults otherwise. Check `~/uni-rnaseq-data/tier0/qc/dupradar/dupradar.R` for
`GTF.featureType`. Rsubread `featureCounts(isPairedEnd=TRUE)` **without** `countReadPairs`
in Rsubread ≥ 2.x: verify whether it counts fragments or reads by checking `allCounts`
against your existing `featureCounts.txt` column for a few genes — if they match, it's
fragments with the same rules; if `allCounts ≈ 2×`, it's reads. `countMultiMappingReads =
TRUE` counts every alignment of a multimapper (each NH>1 alignment votes independently,
primary and secondary alike). `ignoreDup = TRUE` drops 0x400 records before counting.
Output columns as in the golden; gate on `ID`, `geneLength`, and the four integer count
columns; float columns to 6 significant digits or COMPAT the format. `N` for RPKM = sum
of `stat[,2]` minus `Unassigned_Unmapped`.

## Qualimap `rnaseq_qc_results.txt`
No source (Java). Infer against the golden using: `uniquely-mapped-reads` (NH == 1),
non-strand-specific, paired, name-sorted input (so Qualimap sees mates adjacent). Get
"Reads alignment" exact first (`reads aligned (left/right)`, `read pairs aligned`, `total
alignments`, `secondary alignments`, `non-unique alignments`, `aligned to genes`,
`ambiguous alignments`, `no feature assigned`), then "Reads genomic origin" (exonic /
intronic / intergenic / overlapping exon — these are read counts classified by the
read's first block against exon / intron / intergenic, where "overlapping exon" is the
subset of intronic+intergenic reads that partly overlap an exon). "Transcript coverage
profile" (5'/3' bias over the top-1000 transcripts, 100-bp windows) last; COMPAT it if
it resists. Gate on the numbers you get exact; document the rest.

Rules unchanged: source beats card silently; stop only on source/golden disagreement or
~2 attempts per output. Gates in `tests/tier0_qc.rs`; `cargo fmt`; strict clippy;
`unsafe` only in `umem`; no commits; don't touch `docs/ bench/ .hermes/ KANBAN.md
crates/umem/` or the Spark. `source ~/miniconda3/etc/profile.d/conda.sh && conda activate
rnaseq && cargo test -p umbam --release -- --ignored && cargo test -p umbam --release`.

Finish with the full per-output gate table (all ten), Tier 0 `timing.tsv` with every
`qc_*` row, and COMPAT.md additions.
