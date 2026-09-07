#!/usr/bin/env bash
# INTEGRATE v2 Task 0: paired perf differential, stock vs hooks-bypassed vs GPU, same 4M slice.
# Output: $O/{stock,bypass,gpu}.sym.tsv (self-time %, symbol) + wall/cpu per arm.
set -euo pipefail
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH
G=$HOME/uni-rnaseq-probe-lab/integrate-gate-host8
STOCK=$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR
INTEG=$G/private/integrated/STAR
O=$HOME/uni-rnaseq-probe-lab/integrate-v2-perf; rm -rf "$O"; mkdir -p "$O"
ARGV=$(python3 -c "
import json;a=json.load(open('$G/stages/integrated.argv.json'))[1:]
i=a.index('--outFileNamePrefix'); del a[i:i+2]
if '--readMapNumber' in a: i=a.index('--readMapNumber'); del a[i:i+2]
print(' '.join(a))")
run() { # name exe env...
  local name=$1 exe=$2; shift 2
  mkdir -p "$O/$name"
  env "$@" /usr/bin/time -f "%e %U %S" -o "$O/$name/time.txt" \
    perf record -q -F 499 -g -o "$O/$name/perf.data" -- \
    "$exe" $ARGV --readMapNumber 4000000 --outFileNamePrefix "$O/$name/" >/dev/null 2>"$O/$name/err"
  perf report -i "$O/$name/perf.data" --no-children --stdio --sort sym --percent-limit 0.05 2>/dev/null \
    | grep -E "^\s+[0-9]+\.[0-9]+%" | sed -E 's/^\s+([0-9.]+)%\s+\[.\]\s+/\1\t/' > "$O/$name.sym.tsv"
  echo "$name $(cat $O/$name/time.txt)" >> "$O/summary.txt"
}
run stock  "$STOCK"
run bypass "$INTEG"
run gpu    "$INTEG" STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$O/gpu/stats.jsonl
echo "PERF-DONE" >> "$O/summary.txt"
