# Card P2A-UMQC-2 — continue `umbam --qc`: the tools' own source is now in the repo

Card 1 (`.hermes/cards/P2A-UMQC.md`) landed `bam_stat` and `seq.DupRate` byte-identical
and stopped on `pos.DupRate` because the key semantics weren't recoverable by inference.
Correct call. I pulled the **actual source** of every RSeQC function involved, plus
dupRadar's `analyzeDuprates`, out of the nf-core containers into
`docs/tool-src/rseqc-5.x-relevant.py` and `docs/tool-src/dupradar-1.38-relevant.R`.
**Read those files first.** Implement what the source does, including its bugs — the
goldens were produced by this exact code.

Gates: `crates/umbam/tests/tier0_qc.rs` (add one per output, same style); goldens in
`~/uni-rnaseq-data/tier0/qc/`. Run everything with `source ~/miniconda3/etc/profile.d/
conda.sh && conda activate rnaseq && cargo test -p umbam --release -- --ignored && cargo
test -p umbam --release`. Same rules as before (`cargo fmt`, strict clippy, `unsafe` only
in `umem`, no commits, don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/` or the
Spark). Stop and report after any output that resists ~2 attempts.

## Semantics, from the source

### pos.DupRate (`readDupRate`)
Filter: `!is_unmapped && !is_qcfail && mapq >= 30` — **no** secondary/supplementary/
duplicate filter. Key = `chrom + ":" + pos + ":" + exon_boundary` where `exon_boundary`
is the concatenation of `"{st}-{end}:"` for each block from `fetch_exon`. **`fetch_exon`
is buggy and must be mirrored exactly**: it walks the CIGAR with `chrom_st` starting at
`pos`; op 0 (M) emits a block `(chrom_st, chrom_st+len)` and advances; 1 (I) nothing;
2 (D) advances; 3 (N) advances; **4 (S) advances the reference position** (wrong, but
that's the code); everything else (H, P, =, X) is ignored. So two reads at the same pos
with different soft-clip lengths get different keys, and `=`/`X` ops (STAR doesn't emit
them, so moot here) produce no blocks. Histogram of key occurrence → `Occurrence\t
UniqReadNumber`, sorted numerically, no up-limit cutoff in the xls (the 500 is only for
the plot).

### read_distribution (`read_distribution.py` main)
Read the `main()` in the source file: it uses `process_gene_model` to build interval
bitsets for the ten groups from the BED12 (`~/uni-rnaseq-data/tier0/qc/chr22.bed`,
produced by nf-core's `gtf2bed`), with UTR/CDS split from `thickStart/thickEnd`. Per read:
skip unmapped, qcfail, **`mapq < 30`**, and — check the source — secondary? Then each
`fetch_exon` block (same buggy function) is a "tag"; assignment is by the priority order in
the code (`foundone` against each bitset in sequence). `Total_bases` per group = bitset
size after merging. Output format: `"%-30s%d"` style headers and `"%-20s%-20d%-20d%-20.2f"`
rows — copy the exact format strings from the source.

### junction_annotation / junction_saturation
`annotate_junction`: filter `!unmapped && !qcfail && mapq >= q_cut(30)`; introns from
CIGAR N ops with size ≥ `min_intron` (50); a junction is (chrom, intron_st, intron_end);
known / partial-novel / novel classification against `refIntronStarts` / `refIntronEnds`
built from the BED12's blocks. Events = per-read, junctions = unique. Output the
`.junction_annotation.log` text (it goes to stderr in the tool — match the golden file
exactly), `.junction.bed`, `.junction.xls` (columns in source). `saturation_junction`:
samples the *event list* at 5..100% by 5 — read how: it uses `random.shuffle`? or
`random.sample`? If seeded non-deterministically, gate on the 100% column of
`junctionSaturation_plot.r`'s arrays only and say so in COMPAT.md.

### infer_experiment (`configure_experiment`)
First `sample_size` (200,000) reads passing `!unmapped && !qcfail && mapq >= 30 &&
!is_secondary`? — check. Strand of gene from BED; read strand + read1/read2 → the
`1++,1--,2+-,2-+` vs `1+-,1-+,2++,2--` fractions, printed `%.4f`. "failed to determine"
= reads overlapping genes of both strands or none. Reproduce the four lines of the golden
including the blank lines.

### inner_distance (`mRNA_inner_distance`)
Proper pairs only, read1 leads; inner distance = mate2 start − mate1 end (with the
"exon-aware" adjustment in the source when both mates fall in the same transcript's
blocks — read it). Histogram bins `low_bound..up_bound step 10` → `inner_distance_freq.txt`
(gate on this; the per-read `.txt` is 20 MB and not gated). `sample_size` 1,000,000 > our
record count, so no sampling on Tier 0.

### dupRadar (`analyzeDuprates`)
Four `Rsubread::featureCounts` runs on the **markdup** BAM: `mh ∈ {TRUE, FALSE}` ×
`dup ∈ {FALSE, TRUE}`, with `isPairedEnd = TRUE, strandSpecific = 0`, otherwise Rsubread
defaults (`GTF.featureType = "exon"`, `GTF.attrType = "gene_id"`, `allowMultiOverlap =
FALSE`, `countChimericFragments = TRUE`, `requireBothEndsMapped = FALSE`,
`countMultiMappingReads` = mh, `ignoreDup` = dup). Note nf-core passes `GTF.featureType =
feature_type` — check `qc/dupradar/dupradar.R` for what it resolved to. Rsubread's
`isPairedEnd=TRUE` without `countReadPairs` counts **reads**, not fragments? — verify
against the golden; our `count_fragments` counts fragments. `countMultiMappingReads =
TRUE` counts every alignment of a multimapper (NH>1) — each secondary alignment votes
too. Columns: `ID geneLength allCountsMulti filteredCountsMulti dupRateMulti
dupsPerIdMulti RPKMulti PKMMulti allCounts filteredCounts dupRate dupsPerId RPK RPKM`;
`N` for RPKM = total assigned+unassigned minus unmapped from the run's `stat`. Gate on the
four integer count columns and `geneLength`; float columns to 6 significant digits, or
document R's `write.table` float formatting divergence in COMPAT.md.

### Qualimap `rnaseq_qc_results.txt`
No source pulled (Java). Reproduce the "Reads alignment" and "Reads genomic origin"
sections by inference against the golden, using `uniquely-mapped-reads` (NH==1) and
non-strand-specific, paired, name-sorted input; get the alignment counts exact first.
"Transcript coverage profile" (5'/3' bias) last, and it's acceptable to COMPAT it.

Finish with the per-output gate table, Tier 0 `timing.tsv` (`--threads 12`, release),
and COMPAT.md additions.
