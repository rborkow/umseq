#!/usr/bin/env bash
# run_tier2a.sh — Phase 1 throughput datapoint: 6 GEUVADIS CEU samples (3M+3F), full depth, one run, Spark.
# Measures samples/day with QC overlapped; also the Tier 2A DE input.
set -uo pipefail
export PATH=$HOME/.local/bin:$HOME/.local/opt/jdk17/bin:$PATH
ROOT=$HOME/uni-rnaseq; REF=$ROOT/data/reference; IDX=$ROOT/data/index; S=$ROOT/data/samples
RUN=$ROOT/runs/tier2a; mkdir -p $RUN; cd $RUN
python3 - "$ROOT/data-manifest.json" "$S" > samplesheet.csv <<'EOF'
import json,sys
m=json.load(open(sys.argv[1])); s=sys.argv[2]
print("sample,fastq_1,fastq_2,strandedness")
for r in m["tier2a"]:
    print(f'{r["sample"]}_{r["sex"]},{s}/{r["run"]}/{r["run"]}_1.fastq.gz,{s}/{r["run"]}/{r["run"]}_2.fastq.gz,auto')
EOF
cat samplesheet.csv
cat > spark.config <<'EOF'
docker.enabled = true
docker.runOptions = "-u 1000:1000"
params { gencode = true; skip_bbsplit = true; skip_umi_extract = true }
process {
  resourceLimits = [cpus: 20, memory: 100.GB, time: 12.h]
  withName: 'STAR_ALIGN'   { cpus = 10; memory = 40.GB }   // 2 STARs can't coexist (2x39 GB + rest); 1 at a time, leave cores for QC
  withName: 'SALMON_QUANT' { cpus = 8;  memory = 20.GB }
  withName: 'PICARD_MARKDUPLICATES' { memory = 28.GB }
  withName: 'QUALIMAP_RNASEQ' { memory = 20.GB }
}
executor { cpus = 20; memory = 100.GB }
EOF
{ date; nproc; free -g | awk '/Mem/{print "mem_total_gb",$2}'; } > env.txt
t0=$(date +%s)
NXF_ANSI_LOG=false nextflow run nf-core/rnaseq -r 3.26.0 -profile arm64 \
  --input samplesheet.csv --outdir results \
  --fasta $REF/GRCh38.primary_assembly.genome.fa \
  --gtf $REF/gencode.v49.primary_assembly.annotation.gtf \
  --star_index $IDX/star_full --salmon_index $IDX/salmon_nfcore_1.10.3 \
  --aligner star_salmon \
  -c spark.config -with-trace trace.txt -with-report report.html -with-timeline timeline.html \
  "$@" > run.log 2>&1
rc=$?
echo "wall_s $(( $(date +%s) - t0 )) exit $rc" | tee -a env.txt
