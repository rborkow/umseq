# Card P2A-UMQC-7 — dupRadar and Qualimap (the last two QC outputs)

All seven RSeQC outputs are byte-identical to Tier 0 goldens (8/10 counting
junction_saturation's 100% gate). Two remain. Do both in this card.

## dupRadar → `D/dupradar/dupMatrix.txt`
Source: `docs/tool-src/dupradar-1.38-relevant.R` (`analyzeDuprates`). Golden:
`~/uni-rnaseq-data/tier0/qc/dupradar/chr22_dupMatrix.txt`; the exact R invocation nf-core
used is `~/uni-rnaseq-data/tier0/qc/dupradar/dupradar.R` — read it for `GTF.featureType`
and any `...` args.

Four Rsubread `featureCounts` runs on the **markdup** BAM (our `markdup.bam` semantics —
same records, 0x400 flags set):
```
allCountsMulti      = count(countMultiMappingReads=TRUE,  ignoreDup=FALSE)
filteredCountsMulti = count(countMultiMappingReads=TRUE,  ignoreDup=TRUE)
allCounts           = count(countMultiMappingReads=FALSE, ignoreDup=FALSE)
filteredCounts      = count(countMultiMappingReads=FALSE, ignoreDup=TRUE)
```
with `isPairedEnd=TRUE, strandSpecific=0, isGTFAnnotationFile=TRUE`, Rsubread 2.x defaults
otherwise: `GTF.featureType="exon"`, `GTF.attrType="gene_id"`, `allowMultiOverlap=FALSE`,
`countChimericFragments=TRUE`, `requireBothEndsMapped=FALSE`, `minMQS=0`,
`primaryOnly=FALSE`, **`countReadPairs=TRUE`** (Rsubread ≥2.2 default when
`isPairedEnd=TRUE` — verify: `allCounts` should equal our `featureCounts.txt` column for
genes, since that run was `-p --countReadPairs` with NH>1 skipped; if instead
`allCounts ≈ 2×` it's counting reads and you adapt). `countMultiMappingReads=TRUE`: every
alignment with NH>1 votes — Rsubread counts a multi-mapping *fragment* once per reported
alignment pair (primary + each secondary), each assigned independently. `ignoreDup=TRUE`:
records with 0x400 are dropped before pairing.

Columns (tab-separated, header row, `write.table(quote=F, row.names=F)`):
`ID geneLength allCountsMulti filteredCountsMulti dupRateMulti dupsPerIdMulti RPKMulti
RPKMMulti allCounts filteredCounts dupRate dupsPerId RPK RPKM`.
- `geneLength` = Rsubread's `annotation$Length` = merged exon length per gene.
- `dupRate* = (all − filtered) / all` (R gives `NaN` for 0/0 → prints `NA`).
- `dupsPerId* = all − filtered`.
- `RPK = counts × (10³ / width)`, `RPKM = RPK × (10⁶ / N)`, `N` = Σ `stat[,2]` −
  `Unassigned_Unmapped` from *that run's* stat table (= number of fragments processed
  minus unmapped fragments; compute the same total our counter sees).
- Row order = Rsubread's, which is GTF gene first-appearance order (same as our
  featureCounts output).
**Gate**: `ID`, `geneLength`, and the four integer count columns byte-identical. Float
columns: compare to 6 significant digits; R's `write.table` uses up to 15 significant
digits so a byte gate is unrealistic — note in COMPAT.md.

## Qualimap → `D/qualimap/rnaseq_qc_results.txt`
Golden: `~/uni-rnaseq-data/tier0/qc/qualimap/rnaseq_qc_results.txt`. Qualimap 2.3, `rnaseq
--sorted -p non-strand-specific -pe`, on the **name-sorted** markdup BAM, `counting
algorithm = uniquely-mapped-reads`. No source; infer against the golden, section by
section, and gate on each section as you get it exact:

**Reads alignment**: `reads aligned (left/right)` = mapped read1 / read2 counts (primary?
— test), `read pairs aligned`, `total alignments` = all records (1,024,221 = our record
count, so this includes secondaries and unmapped-with-tid), `secondary alignments` =
0x100 count, `non-unique alignments` = NH>1 records (83,488), `aligned to genes` /
`ambiguous alignments` / `no feature assigned` = per-*read* (not fragment) union-exon
assignment of uniquely-mapped (NH==1) primary reads: 734,257 + 83,587 + 41,425 = 859,269
— check that equals your NH==1 primary mapped read count. "ambiguous" = read overlaps
exons of >1 gene; "no feature" = overlaps none.
**Reads genomic origin**: `exonic` = aligned-to-genes (same number); `intronic` = reads
overlapping no exon but inside a gene span; `intergenic` = neither; `overlapping exon` =
subset of intronic/intergenic reads that *partly* overlap an exon (Qualimap's
"overlapping exon" bucket — reads straddling an exon boundary that weren't assigned).
Percentages `%.2f` of the 859,269 (verify: 734,257/775,682? — compute what denominator
gives 94.66% and use that).
**Transcript coverage profile** (5'/3' bias): top 1,000 transcripts by coverage, 100-bp
end windows, `5' bias = mean(5' window cov / max?)…` — attempt if the first two sections
are exact; otherwise COMPAT it with the golden values noted.
Output the file with the exact text layout of the golden (headers, indentation, thousands
separators, blank lines). Gate on the sections you match; document the rest.

Rules unchanged: source beats card silently; stop only on source/golden disagreement or
~2 attempts per output. Gates in `tests/tier0_qc.rs`; `cargo fmt`; strict clippy; `unsafe`
only in `umem`; no commits; don't touch `docs/ bench/ .hermes/ KANBAN.md crates/umem/` or
the Spark. `source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq && cargo
test -p umbam --release -- --ignored && cargo test -p umbam --release`.

Finish with the ten-row gate table, Tier 0 `timing.tsv` with every `qc_*` row, COMPAT.md.
