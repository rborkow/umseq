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
