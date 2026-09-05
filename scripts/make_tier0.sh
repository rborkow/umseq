#!/usr/bin/env bash
# make_tier0.sh — Tier 0 fixture: chr22-only BAM from the Tier 1 20M alignment + golden outputs
# from samtools / picard / featureCounts / bedtools. These are the byte-compat targets for umbam.
# Run on the Mac (native rnaseq conda env). Output: ~/uni-rnaseq-data/tier0/
set -euo pipefail
D=$HOME/uni-rnaseq-data; IN=$D/bams/tier1-20M/NA12716_20M.Aligned.out.bam
GTF=$D/reference/gencode.v49.primary_assembly.annotation.gtf
OUT=$D/tier0; mkdir -p $OUT; cd $OUT
S=chr22

# 1. Extract chr22 pairs from the *unsorted* STAR output, keeping both mates and secondaries
#    whose primary is on chr22 (filter by read name so pairs stay complete).
[[ -f $S.names.txt ]] || samtools view -@ 8 "$IN" chr22 2>/dev/null | cut -f1 | sort -u > $S.names.txt \
  || { samtools sort -@ 8 -m 1G -o tmp.sorted.bam "$IN"; samtools index tmp.sorted.bam; samtools view tmp.sorted.bam chr22 | cut -f1 | sort -u > $S.names.txt; }
[[ -f $S.unsorted.bam ]] || samtools view -@ 8 -b -N $S.names.txt -o $S.unsorted.bam "$IN"
echo "chr22 unsorted: $(samtools view -c $S.unsorted.bam) alignments, $(stat -f %z $S.unsorted.bam) bytes"

# 2. Goldens (all from the unsorted input, so umbam does the same work)
samtools sort -@ 8 -o $S.sorted.bam $S.unsorted.bam
samtools index $S.sorted.bam
samtools flagstat $S.sorted.bam > $S.flagstat.txt
samtools stats $S.sorted.bam > $S.stats.txt
samtools idxstats $S.sorted.bam > $S.idxstats.txt
picard -Xmx4g MarkDuplicates --INPUT $S.sorted.bam --OUTPUT $S.markdup.bam --METRICS_FILE $S.markdup.metrics.txt \
  --ASSUME_SORT_ORDER coordinate --VALIDATION_STRINGENCY LENIENT --QUIET true 2>/dev/null
samtools index $S.markdup.bam
grep "^chr22\s" "$GTF" > $S.gtf
featureCounts -T 4 -p --countReadPairs -a $S.gtf -o $S.featureCounts.txt $S.markdup.bam 2>/dev/null
bedtools genomecov -ibam $S.markdup.bam -bg -split > $S.genomecov.bg

# 3. Canonical text forms for byte comparison (BAM bytes differ by compression; compare SAM records)
samtools view $S.sorted.bam > $S.sorted.sam
samtools view $S.markdup.bam > $S.markdup.sam
{ echo -e "file\tbytes\tsha256"; for f in $S.*; do printf "%s\t%s\t%s\n" "$f" "$(stat -f %z "$f")" "$(shasum -a 256 "$f" | cut -d' ' -f1)"; done; } > MANIFEST.tsv
rm -f tmp.sorted.bam tmp.sorted.bam.bai
du -sh .; cat MANIFEST.tsv
