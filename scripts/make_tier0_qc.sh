#!/usr/bin/env bash
# make_tier0_qc.sh — QC goldens for the Tier 0 chr22 fixture, produced by the exact tool
# containers and arguments nf-core/rnaseq 3.26.0 used in runs/tier2a on the Spark.
# Run ON THE SPARK (docker + cached wave images). Inputs: the Tier 0 fixture directory
# (chr22.markdup.bam + .bai + chr22.gtf), rsynced from the Mac. Outputs: $T/qc/<tool>/…
set -euo pipefail
T=${1:-$HOME/uni-rnaseq-data/tier0}
GTF_FULL=$HOME/uni-rnaseq/data/ref/gencode.v49.filtered.gtf
BED_FULL=$(find $HOME/uni-rnaseq/runs/tier2a/work -name "gencode.v49.primary_assembly.annotation.filtered.bed" | head -1)
Q=$T/qc; mkdir -p $Q/rseqc $Q/qualimap $Q/dupradar
cd $T
DK="docker run --rm -u 1000:1000 -v $HOME:$HOME -w $T"
SAM="$DK community.wave.seqera.io/library/htslib_samtools:1.23.1--aedf2bc6d9d9dddb"
RSEQC="$DK community.wave.seqera.io/library/rseqc_r-base:b63499fe19f103fe"
QMAP="$DK community.wave.seqera.io/library/qualimap:2.3--8375b60bba97a2a6"
DUPR="$DK community.wave.seqera.io/library/bioconductor-dupradar:1.38.0--1bca173c6feb9be3"
PERL="$DK community.wave.seqera.io/library/perl:5.26.2--2a6adf51d600e047"

# chr22-only GTF → BED12 via nf-core's gtf2bed (Perl), same script the pipeline ran.
G2B=$(find $HOME/uni-rnaseq/runs/tier2a/work -path "*3a/419ce46ab6f6da637589cc99116901/.command.sh" | head -1)
cp $G2B $Q/gtf2bed.pl
sed -i -e "s#\$in = \".*\";#\$in = \"chr22.gtf\";#" -e "s#my \$out_file = \".*\";#my \$out_file = \"qc/chr22.bed\";#" $Q/gtf2bed.pl
$PERL perl $Q/gtf2bed.pl
test -s $Q/chr22.bed || { echo "gtf2bed produced an empty BED"; exit 1; }

BAM=chr22.markdup.bam
# --- RSeQC (all default args, exactly as nf-core) ---
$RSEQC bam_stat.py -i $BAM > $Q/rseqc/bam_stat.txt 2>/dev/null
$RSEQC read_distribution.py -i $BAM -r $Q/chr22.bed > $Q/rseqc/read_distribution.txt 2>/dev/null
( cd $Q/rseqc && $DK -w $Q/rseqc community.wave.seqera.io/library/rseqc_r-base:b63499fe19f103fe read_duplication.py -i $T/$BAM -o chr22 >/dev/null 2>&1 )
( cd $Q/rseqc && $DK -w $Q/rseqc community.wave.seqera.io/library/rseqc_r-base:b63499fe19f103fe junction_annotation.py -i $T/$BAM -r $Q/chr22.bed -o chr22 2> chr22.junction_annotation.log )
( cd $Q/rseqc && $DK -w $Q/rseqc community.wave.seqera.io/library/rseqc_r-base:b63499fe19f103fe junction_saturation.py -i $T/$BAM -r $Q/chr22.bed -o chr22 >/dev/null 2>&1 )
$RSEQC infer_experiment.py -i $BAM -r $Q/chr22.bed > $Q/rseqc/infer_experiment.txt 2>/dev/null
( cd $Q/rseqc && $DK -w $Q/rseqc community.wave.seqera.io/library/rseqc_r-base:b63499fe19f103fe inner_distance.py -i $T/$BAM -r $Q/chr22.bed -o chr22 >/dev/null 2>&1 )

# --- Qualimap rnaseq: nf-core name-sorts first, then --sorted -p non-strand-specific -pe ---
$SAM samtools sort -n -@ 4 -o $Q/chr22.namesorted.bam $BAM
$QMAP qualimap --java-mem-size=8G rnaseq --sorted -bam $Q/chr22.namesorted.bam -gtf chr22.gtf -p non-strand-specific -pe -outdir $Q/qualimap >/dev/null 2>&1

# --- dupRadar: the pipeline's R script with paths swapped ---
DR=$(find $HOME/uni-rnaseq/runs/tier2a/work -path "*28/68602be15e8a657a73fe82d5be3f50/.command.sh" | head -1)
sed -e "s#input_bam <- '.*'#input_bam <- '$T/$BAM'#" -e "s#annotation_gtf <- '.*'#annotation_gtf <- '$T/chr22.gtf'#" -e "s#output_prefix = .*#output_prefix = 'chr22'#" -e "s#threads <- .*#threads <- 8#" $DR > $Q/dupradar/dupradar.R
( cd $Q/dupradar && $DK -w $Q/dupradar community.wave.seqera.io/library/bioconductor-dupradar:1.38.0--1bca173c6feb9be3 Rscript dupradar.R >/dev/null 2>&1 )

find $Q -type f | sort | xargs ls -la | awk '{print $5, $9}'
