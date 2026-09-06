#!/usr/bin/env bash
# tier1_validate.sh — run umbam chain --qc on one full-depth Tier 2A sample and compare every
# output to that sample's nf-core results (the real-scale golden). Runs on the Spark.
set -uo pipefail
cd ~/uni-rnaseq
S=${1:-NA11832_F}
RUN=runs/tier2a; R=$RUN/results/star_salmon
BAM=$(find $RUN/work -name "$S.Aligned.out.bam" | head -1)
GTF=data/ref/gencode.v49.filtered.gtf
BED=$(find $RUN/work -name "gencode.v49.primary_assembly.annotation.filtered.bed" | head -1)
OUT=/tmp/umbam-tier1-$S
SAMTOOLS=$(ls ~/micromamba/envs/rnaseq/bin/samtools)
export PATH=$HOME/.cargo/bin:$PATH
cargo build -q -p umbam --release 2>&1 | grep -E "^error" && exit 1
rm -rf $OUT
echo "=== $S: $(ls -la $BAM | awk '{print $5/1e9}') GB, $(date) ==="
/usr/bin/time -v ./target/release/umbam chain --in $BAM --gtf $GTF --bed $BED --out-dir $OUT --threads 20 --qc 2>&1 | grep -E "Elapsed|Maximum resident"
cat $OUT/timing.tsv | tr '\n' ' '; echo

cmpf() { # label ours theirs
  if cmp -s "$2" "$3"; then echo "IDENTICAL  $1"; else echo "DIFFERS    $1  ($(wc -l < "$2") vs $(wc -l < "$3") lines; first diff: $(diff "$2" "$3" | grep '^[<>]' | head -1 | cut -c1-120))"; fi
}
echo "=== vs nf-core ==="
cmpf flagstat  $OUT/flagstat.txt  $R/samtools_stats/$S.markdup.sorted.bam.flagstat
cmpf idxstats  $OUT/idxstats.txt  $R/samtools_stats/$S.markdup.sorted.bam.idxstats
# Picard metrics: compare the data row's numeric columns
ours=$(grep -A1 "^LIBRARY" $OUT/markdup.metrics.txt | tail -1 | cut -f2-9)
theirs=$(grep -A1 "^LIBRARY" $R/picard_metrics/$S.markdup.sorted.MarkDuplicates.metrics.txt | tail -1 | cut -f2-9)
[ "$ours" = "$theirs" ] && echo "IDENTICAL  picard metrics" || { echo "DIFFERS    picard metrics"; echo "  ours:   $ours"; echo "  theirs: $theirs"; }
# markdup flags: count of 0x400 in ours vs theirs
a=$($SAMTOOLS view -c -f 0x400 $OUT/markdup.bam); b=$($SAMTOOLS view -c -f 0x400 $R/$S.markdup.sorted.bam)
[ "$a" = "$b" ] && echo "IDENTICAL  dup-flag count ($a)" || echo "DIFFERS    dup-flag count ours=$a theirs=$b"
# genomecov: nf-core's bedGraph is from the markdup BAM; ours from sorted — both same records, compare
cmpf genomecov $OUT/genomecov.bg $(find $RUN/work -name "$S.bedGraph" | head -1)
for f in bam_stat read_distribution infer_experiment; do cmpf rseqc/$f $OUT/rseqc/$f.txt $R/rseqc/$f/$S.$f.txt; done
cmpf rseqc/pos.DupRate $OUT/rseqc/pos.DupRate.xls $R/rseqc/read_duplication/xls/$S.pos.DupRate.xls
cmpf rseqc/seq.DupRate $OUT/rseqc/seq.DupRate.xls $R/rseqc/read_duplication/xls/$S.seq.DupRate.xls
cmpf rseqc/inner_distance_freq $OUT/rseqc/inner_distance_freq.txt $R/rseqc/inner_distance/txt/$S.inner_distance_freq.txt
cmpf rseqc/junction_annotation.log $OUT/rseqc/junction_annotation.log $R/rseqc/junction_annotation/log/$S.junction_annotation.log
# dupRadar integer columns
cut -f1-4,9,10 $OUT/dupradar/dupMatrix.txt > /tmp/dm.ours; cut -f1-4,9,10 $R/dupradar/gene_data/${S}_dupMatrix.txt > /tmp/dm.theirs
cmpf dupradar/dupMatrix-ints /tmp/dm.ours /tmp/dm.theirs
echo "=== done $(date) ==="
