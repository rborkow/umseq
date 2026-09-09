#!/usr/bin/env bash
set -euo pipefail
if (($# != 1)); then echo "usage: $0 output-directory" >&2; exit 2; fi
OUT=$(cd "$1" && pwd -P)
: "${STOCK_STAR:?set STOCK_STAR}"; : "${PATCHED_STAR:?set PATCHED_STAR}"
: "${INDEX_DIR:?set INDEX_DIR}"; : "${ARGV_JSON:?set ARGV_JSON}"
mkdir -p "$OUT"
{
  echo "kernel: $(uname -r)"; echo "machine: $(uname -m)"
  echo "cpu: $(awk -F: '/model name|Hardware|CPU model/{gsub(/^ +/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null || true)"
  echo "memory: $(awk '/MemTotal:/{print $2, $3; exit}' /proc/meminfo 2>/dev/null || true)"
  echo "pagesize: $(getconf PAGESIZE)"
  for f in enabled defrag khugepaged/defrag; do
    echo "thp_$f: $(cat "/sys/kernel/mm/transparent_hugepage/$f" 2>/dev/null || echo unavailable)"
  done
} | tee "$OUT/environment.txt"
python3 - "$ARGV_JSON" "$OUT/argv.txt" <<'PY'
import json, os, sys
args = json.load(open(sys.argv[1]))
if args and not args[0].startswith("--"): args = args[1:]
def set_arg(name, value):
    global args
    if name in args: args[args.index(name) + 1] = value
    else: args += [name, value]
set_arg("--genomeDir", os.environ["INDEX_DIR"])
if os.environ.get("READ1") and os.environ.get("READ2"):
    i = args.index("--readFilesIn") if "--readFilesIn" in args else len(args)
    if i < len(args): del args[i:i + 3]
    args[i:i] = ["--readFilesIn", os.environ["READ1"], os.environ["READ2"]]
set_arg("--readMapNumber", "20000000")
set_arg("--outFileNamePrefix", "PLACEHOLDER_PREFIX")
open(sys.argv[2], "w").write("\0".join(args))
PY
printf 'repeat\tarm\twall_s\tuser_s\tsys_s\tmax_rss_kib\texit\tanonhuge_pages_kib\tcmp_sam\tcmp_sj\n' > "$OUT/matrix.tsv"
BASE_SAM=""; BASE_SJ=""
run_one() {
  local rep=$1 arm=$2 bin=$3 thp=$4 dir pid sampler status time_row ahp sam sj
  dir="$OUT/r${rep}-${arm}"; mkdir -p "$dir"
  cat "$INDEX_DIR/Genome" "$INDEX_DIR/SA" "$INDEX_DIR/SAindex" > /dev/null
  mapfile -d '' -t argv < "$OUT/argv.txt"
  argv+=("--outFileNamePrefix" "$dir/")
  : > "$dir/anonhuge.tsv"
  env STAR_THP="$thp" /usr/bin/time -f '%e\t%U\t%S\t%M' -o "$dir/time.tsv" "$bin" "${argv[@]}" > /dev/null 2>"$dir/stderr" &
  local time_pid=$!; pid=""
  (while kill -0 "$time_pid" 2>/dev/null; do
    if [[ -z "$pid" ]]; then
      for candidate in $(pgrep -P "$time_pid" 2>/dev/null || true); do
        [[ "$(readlink "/proc/$candidate/exe" 2>/dev/null || true)" == "$bin" ]] && pid=$candidate && break
      done
    fi
    if [[ -n "$pid" && -r "/proc/$pid/smaps_rollup" ]]; then
      grep '^AnonHugePages:' "/proc/$pid/smaps_rollup" >> "$dir/anonhuge.tsv" || true
    fi
    sleep 1
  done) & sampler=$!
  set +e; wait "$time_pid"; status=$?; set -e
  kill "$sampler" 2>/dev/null || true; wait "$sampler" 2>/dev/null || true
  time_row=$(cat "$dir/time.tsv")
  ahp=$(awk '{sum += $2; n++} END {if (n) print sum/n; else print "NA"}' "$dir/anonhuge.tsv")
  sam=NA; sj=NA
  if [[ -n "$BASE_SAM" && -f "$dir/Aligned.out.sam" ]]; then if cmp -s "$BASE_SAM" "$dir/Aligned.out.sam"; then sam=0; else sam=$?; fi; fi
  if [[ -n "$BASE_SJ" && -f "$dir/SJ.out.tab" ]]; then if cmp -s "$BASE_SJ" "$dir/SJ.out.tab"; then sj=0; else sj=$?; fi; fi
  [[ -z "$BASE_SAM" && -f "$dir/Aligned.out.sam" ]] && BASE_SAM="$dir/Aligned.out.sam"
  [[ -z "$BASE_SJ" && -f "$dir/SJ.out.tab" ]] && BASE_SJ="$dir/SJ.out.tab"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$rep" "$arm" $time_row "$status" "$ahp" "$sam" "$sj" >> "$OUT/matrix.tsv"
  echo "completed r${rep}-${arm}: exit=$status anonhuge_pages_kib=$ahp"
}
for rep in 1 2 3; do
  case "$rep" in 1) order='stock patched-off patched-on';; 2) order='patched-off patched-on stock';; 3) order='patched-on stock patched-off';; esac
  for arm in $order; do
    case "$arm" in
      stock) run_one "$rep" "$arm" "$STOCK_STAR" 0;;
      patched-off) run_one "$rep" "$arm" "$PATCHED_STAR" 0;;
      patched-on) run_one "$rep" "$arm" "$PATCHED_STAR" 1;;
    esac
  done
done
