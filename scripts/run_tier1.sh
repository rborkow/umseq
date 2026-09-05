#!/usr/bin/env bash
# run_tier1.sh — Phase 1 CPU baseline: nf-core/rnaseq star_salmon on a Tier 1 subsample, Spark.
# Usage: run_tier1.sh <20M|5M|full> [extra nextflow args]
set -euo pipefail
export PATH=$HOME/.local/bin:$HOME/.local/opt/jdk17/bin:$PATH
ROOT=$HOME/uni-rnaseq; REF=$ROOT/data/reference; IDX=$ROOT/data/index; S=$ROOT/data/samples
T=${1:-20M}; shift || true
case $T in
  full) R1=$S/ERR188140/ERR188140_1.fastq.gz; R2=$S/ERR188140/ERR188140_2.fastq.gz ;;
  *)    R1=$S/ERR188140_$T/ERR188140_${T}_1.fastq.gz; R2=$S/ERR188140_$T/ERR188140_${T}_2.fastq.gz ;;
esac
RUN=$ROOT/runs/tier1-$T; mkdir -p $RUN; cd $RUN
printf "sample,fastq_1,fastq_2,strandedness\nNA12716_%s,%s,%s,auto\n" $T $R1 $R2 > samplesheet.csv
cat > spark.config <<'EOF'
docker.enabled = true
docker.runOptions = "-u 1000:1000"
params { gencode = true; skip_bbsplit = true; skip_umi_extract = true }
process {
  resourceLimits = [cpus: 20, memory: 80.GB, time: 8.h]
  withName: 'STAR_ALIGN'   { cpus = 16; memory = 48.GB }
  withName: 'SALMON_QUANT' { cpus = 16; memory = 24.GB }
}
EOF
# nvidia-smi + clocks not relevant (CPU-only), but record CPU governor/temps for the memo
{ date; nproc; lscpu | grep -iE "model name" | head -2; cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || true; } > env.txt
t0=$(date +%s)
NXF_ANSI_LOG=false nextflow run nf-core/rnaseq -r 3.26.0 -profile arm64 \
  --input samplesheet.csv --outdir results \
  --fasta $REF/GRCh38.primary_assembly.genome.fa \
  --gtf $REF/gencode.v49.primary_assembly.annotation.gtf \
  --star_index $IDX/star_full \
  --aligner star_salmon \
  -c spark.config -with-trace trace.txt -with-report report.html -with-timeline timeline.html \
  "$@" > run.log 2>&1 || true
rc=$? || true
echo "wall_s $(( $(date +%s) - t0 )) exit $rc" | tee -a env.txt
