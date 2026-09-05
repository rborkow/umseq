#!/usr/bin/env bash
# bamchain_bench.sh — time each BAM-chain stage standalone with native tools. Same inputs on both boxes.
# Usage: bamchain_bench.sh <unsorted.Aligned.out.bam> <gtf> <outdir> [threads]
# Writes <outdir>/bamchain.tsv: stage, wall_s, user_s, sys_s, peak_rss_gb, threads, out_bytes
set -uo pipefail
IN=$1; GTF=$2; OUT=$3; T=${4:-8}
for t in samtools picard featureCounts bedtools; do command -v $t >/dev/null || { echo "missing: $t (activate the rnaseq env first)" >&2; exit 2; }; done
mkdir -p "$OUT"; cd "$OUT"
S=$(basename "$IN" .Aligned.out.bam)
TSV=bamchain.tsv; echo -e "stage\twall_s\tuser_s\tsys_s\tpeak_rss_gb\tthreads\tout_bytes" > $TSV
if [[ "$(uname)" == "Darwin" ]]; then TIME=(/usr/bin/time -l); rss_field() { awk '/maximum resident set size/{printf "%.2f", $1/1073741824}' "$1"; }
else TIME=(/usr/bin/time -v); rss_field() { awk '/Maximum resident/{printf "%.2f", $6/1048576}' "$1"; }; fi
run() { # name threads outfile cmd...
  local name=$1 th=$2 outf=$3; shift 3
  local t0=$(date +%s.%N)
  "${TIME[@]}" "$@" 2> "$name.time"; local rc=$?
  local wall=$(echo "$(date +%s.%N) - $t0" | bc)
  local user=$(awk '/user/{for(i=1;i<=NF;i++) if($i ~ /user/){print $(i-1); exit}}' "$name.time" | tr -d 's')
  [[ "$(uname)" == "Linux" ]] && user=$(awk '/User time/{print $4}' "$name.time")
  local sys=$(awk '/sys/{for(i=1;i<=NF;i++) if($i ~ /sys/){print $(i-1); exit}}' "$name.time" | tr -d 's')
  [[ "$(uname)" == "Linux" ]] && sys=$(awk '/System time/{print $4}' "$name.time")
  local bytes; if [[ "$(uname)" == "Darwin" ]]; then bytes=$(stat -f %z "$outf" 2>/dev/null || echo 0); else bytes=$(stat -c %s "$outf" 2>/dev/null || echo 0); fi
  printf "%s\t%.1f\t%s\t%s\t%s\t%s\t%s\n" "$name" "$wall" "$user" "$sys" "$(rss_field $name.time)" "$th" "$bytes" | tee -a $TSV
  [[ $rc -ne 0 ]] && echo "  !! $name exit $rc" >&2
}

run samtools_sort_${T}t $T $S.sorted.bam samtools sort -@ $T -m 1G -o $S.sorted.bam "$IN"
run samtools_sort_1t 1 $S.sorted1.bam samtools sort -@ 1 -m 4G -o $S.sorted1.bam "$IN"
run samtools_index_${T}t $T $S.sorted.bam.bai samtools index -@ $T $S.sorted.bam
run samtools_view_decode_${T}t $T /dev/null samtools view -@ $T -c $S.sorted.bam                 # pure inflate+parse
run samtools_view_decode_1t 1 /dev/null samtools view -@ 1 -c $S.sorted.bam
run samtools_stats_1t 1 $S.stats samtools stats -@ 1 $S.sorted.bam
run samtools_flagstat_${T}t $T $S.flagstat samtools flagstat -@ $T $S.sorted.bam
run picard_markdup 1 $S.markdup.bam picard -Xmx8g MarkDuplicates --INPUT $S.sorted.bam --OUTPUT $S.markdup.bam --METRICS_FILE $S.markdup.txt --ASSUME_SORT_ORDER coordinate --VALIDATION_STRINGENCY LENIENT
run samtools_markdup_pipeline_${T}t $T $S.smarkdup.bam bash -c "samtools sort -@ $T -n -m 1G $S.sorted.bam | samtools fixmate -@ $T -m - - | samtools sort -@ $T -m 1G - | samtools markdup -@ $T - $S.smarkdup.bam"
run featurecounts_${T}t $T $S.fc.txt featureCounts -T $T -p --countReadPairs -a "$GTF" -o $S.fc.txt $S.sorted.bam
run bedtools_genomecov 1 $S.bg bash -c "bedtools genomecov -ibam $S.sorted.bam -bg -split > $S.bg"
echo; cat $TSV
