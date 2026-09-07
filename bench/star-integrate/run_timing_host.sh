#!/usr/bin/env bash
# INTEGRATE-1 gate (ii)/(iii): paired CPU-s timings after gate (i) parity passed (host5).
# Three rotated repeats × three arms, same 20M input, 20 threads, no BAM output:
#   stock     — the parity-accepted pinned stock build (seed-split-private-v2/baseline)
#   bypass    — the integrated binary with STAR_INTEGRATE unset (cpu-bypass; measures hook overhead)
#   gpu       — the integrated binary, STAR_INTEGRATE=1, strict oracle OFF
# Records %e %U %S %M %x per run, Log.final.out, and the GPU sidecar. Strict/oracle is never timed.
set -euo pipefail
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock -n 9 || { echo LOCKED; exit 75; }
export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH
G=$HOME/uni-rnaseq-probe-lab/integrate-gate-host5
OUT=$HOME/uni-rnaseq-probe-lab/integrate-timing-host1
test ! -e "$OUT"; mkdir -p "$OUT"
INTEG=$G/private/integrated/STAR
STOCK=$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR  # parity-accepted stock: same pinned source + flags as the integrated build
ARGV=$(python3 -c "import json;a=json.load(open('$G/stages/integrated.argv.json'));print(' '.join(a[1:]))")
# drop the --outFileNamePrefix value; we set our own per run
ARGV=$(python3 -c "
import json;a=json.load(open('$G/stages/integrated.argv.json'))[1:]
i=a.index('--outFileNamePrefix'); del a[i:i+2]; print(' '.join(a))")
sha256sum "$INTEG" "$STOCK" > "$OUT/binaries-sha256.txt"
nvidia-smi --query-gpu=clocks.sm,temperature.gpu --format=csv,noheader > "$OUT/gpu-before.txt"
printf 'repeat\tarm\twall_s\tuser_s\tsys_s\tmax_rss_kib\texit\n' > "$OUT/raw.tsv"
run() { # repeat arm
  local rep=$1 arm=$2 dir="$OUT/r$1-$2" bin env=
  mkdir -p "$dir"
  case $arm in
    stock)  bin=$STOCK ;;
    bypass) bin=$INTEG ;;
    gpu)    bin=$INTEG; env="STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$dir/integrate-stats.jsonl" ;;
  esac
  echo "=== r$rep $arm $(date +%H:%M:%S) ===" | tee -a "$OUT/log"
  set +e
  env $env /usr/bin/time -f "$rep\t$arm\t%e\t%U\t%S\t%M\t%x" -a -o "$OUT/raw.tsv" \
    timeout --signal=TERM --kill-after=30s 1200s \
    "$bin" $ARGV --outFileNamePrefix "$dir/" > "$dir/stdout" 2> "$dir/stderr"
  rc=$?; set -e
  echo "rc=$rc" >> "$OUT/log"
  grep -E "Number of input reads|Uniquely mapped reads %" "$dir/Log.final.out" >> "$OUT/log"
  rm -f "$dir/Aligned.out.sam"
}
# warmup (not recorded): one stock run
mkdir -p "$OUT/warmup"; "$STOCK" $ARGV --outFileNamePrefix "$OUT/warmup/" > /dev/null 2>&1 || true; rm -f "$OUT/warmup/Aligned.out.sam"
for rep in 1 2 3; do
  case $rep in 1) order="stock bypass gpu";; 2) order="gpu stock bypass";; 3) order="bypass gpu stock";; esac
  for arm in $order; do run $rep $arm; done
done
nvidia-smi --query-gpu=clocks.sm,temperature.gpu --format=csv,noheader > "$OUT/gpu-after.txt"
echo TIMING-DONE >> "$OUT/log"
