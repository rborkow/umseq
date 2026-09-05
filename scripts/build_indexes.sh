#!/usr/bin/env bash
# build_indexes.sh — STAR + salmon indexes on the Spark. Run after fetch_reference.sh completes.
# Records wall time and peak RSS per build into data/reference/index-build.tsv (Phase 1 baseline data).
set -euo pipefail
REF=${REF:-$HOME/uni-rnaseq/data/reference}
OUT=${OUT:-$HOME/uni-rnaseq/data/index}
REL=${GENCODE_RELEASE:-49}
THREADS=${THREADS:-16}
MAMBA=${MAMBA:-$HOME/.local/bin/micromamba}
run() { "$MAMBA" run -n rnaseq "$@"; }
mkdir -p "$OUT"; cd "$OUT"
LOG="$REF/index-build.tsv"; [[ -f $LOG ]] || echo -e "index\tsettings\twall_s\tpeak_rss_gb\tsize_gb" > "$LOG"

timed() { # name settings cmd...
  local name=$1 settings=$2; shift 2
  local t0=$(date +%s)
  /usr/bin/time -v "$@" 2> "$OUT/$name.time" || { echo "FAILED $name"; tail -20 "$OUT/$name.time"; return 1; }
  local wall=$(( $(date +%s) - t0 ))
  local rss=$(awk '/Maximum resident/{printf "%.1f", $6/1048576}' "$OUT/$name.time")
  local sz=$(du -s --block-size=1G "$OUT/$name" | cut -f1)
  echo -e "$name\t$settings\t$wall\t$rss\t$sz" >> "$LOG"
  echo "done $name: ${wall}s, peak ${rss} GB, ${sz} GB"
}

# 1. STAR full-density (needs ~38 GB RAM; --genomeSAsparseD 1)
[[ -f star_full/SA ]] || timed star_full "sjdbOverhang=100,sparseD=1" run STAR --runMode genomeGenerate \
  --runThreadN $THREADS --genomeDir star_full \
  --genomeFastaFiles "$REF/GRCh38.primary_assembly.genome.fa" \
  --sjdbGTFfile "$REF/gencode.v${REL}.primary_assembly.annotation.gtf" --sjdbOverhang 100

# 2. STAR sparse (fits the 24 GB Mac)
[[ -f star_sparse3/SA ]] || timed star_sparse3 "sjdbOverhang=100,sparseD=3" run STAR --runMode genomeGenerate \
  --runThreadN $THREADS --genomeDir star_sparse3 --genomeSAsparseD 3 \
  --genomeFastaFiles "$REF/GRCh38.primary_assembly.genome.fa" \
  --sjdbGTFfile "$REF/gencode.v${REL}.primary_assembly.annotation.gtf" --sjdbOverhang 100

# 3. chr20-22 full-density (Tier 0 / kernel dev on Mac)
if [[ ! -f star_chr20_22/SA ]]; then
  run samtools faidx "$REF/GRCh38.primary_assembly.genome.fa" chr20 chr21 chr22 > "$REF/chr20_22.fa"
  grep -E "^(chr20|chr21|chr22)\s" "$REF/gencode.v${REL}.primary_assembly.annotation.gtf" > "$REF/chr20_22.gtf"
  timed star_chr20_22 "sjdbOverhang=100,sparseD=1,chr20-22" run STAR --runMode genomeGenerate \
    --runThreadN $THREADS --genomeDir star_chr20_22 --genomeSAindexNbases 12 \
    --genomeFastaFiles "$REF/chr20_22.fa" --sjdbGTFfile "$REF/chr20_22.gtf" --sjdbOverhang 100
fi

# 4. salmon with full-genome decoys
[[ -f salmon_k31/versionInfo.json ]] || timed salmon_k31 "k=31,decoys=genome,gencode" run salmon index \
  -t "$REF/gentrome.fa" -d "$REF/decoys.txt" -i salmon_k31 -k 31 --gencode -p $THREADS

echo; cat "$LOG"; echo; du -sh "$OUT"/*
