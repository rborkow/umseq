#!/usr/bin/env bash
# run_tier1_mac.sh — Phase 1 CPU baseline on the M4 Pro: nf-core/rnaseq star_salmon, sparse-3 STAR index, Apple containers.
# Usage: run_tier1_mac.sh <20M|5M> [extra nextflow args]
set -uo pipefail
D=$HOME/uni-rnaseq-data; REF=$D/reference; IDX=$D/index; S=$D/samples
T=${1:-20M}; shift || true
R1=$S/ERR188140_$T/ERR188140_${T}_1.fastq.gz; R2=$S/ERR188140_$T/ERR188140_${T}_2.fastq.gz
RUN=$HOME/uni-rnaseq-runs/tier1-$T; mkdir -p $RUN; cd $RUN
printf "sample,fastq_1,fastq_2,strandedness\nNA12716_%s,%s,%s,auto\n" $T $R1 $R2 > samplesheet.csv
cat > mac.config <<'EOF'
appleContainer.enabled = true
params { gencode = true; skip_bbsplit = true; skip_umi_extract = true }
process {
  resourceLimits = [cpus: 14, memory: 21.GB, time: 8.h]
  withName: 'SALMON_QUANT' { cpus = 12; memory = 21.GB }
  withName: 'STAR_ALIGN'   { cpus = 12; memory = 20.GB }
}
EOF
{ date; sysctl -n machdep.cpu.brand_string hw.ncpu hw.memsize; container --version | head -1; } > env.txt
t0=$(date +%s)
NXF_ANSI_LOG=false nextflow run nf-core/rnaseq -r 3.26.0 -profile arm64 \
  --input samplesheet.csv --outdir results \
  --fasta $REF/GRCh38.primary_assembly.genome.fa \
  --gtf $REF/gencode.v49.primary_assembly.annotation.gtf \
  --star_index $IDX/star_sparse3 --salmon_index $IDX/salmon_nfcore_1.10.3 \
  --aligner star_salmon \
  -c mac.config -with-trace trace.txt -with-report report.html -with-timeline timeline.html \
  "$@" > run.log 2>&1
rc=$?
echo "wall_s $(( $(date +%s) - t0 )) exit $rc" | tee -a env.txt
