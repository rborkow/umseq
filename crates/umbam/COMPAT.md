# Compatibility notes

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
