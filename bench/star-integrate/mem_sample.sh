#!/usr/bin/env bash
# Sample RSS/swap of the integrated STAR every 3 s on a 4M slice (V3 memory diagnosis).
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host17; P=$O/v3-mem
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stock-reuse.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))["argv"][1:]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
print(" ".join(a))
EOF
)
STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl "$G/private/integrated/STAR" $ARGV --readMapNumber 4000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err" &
PID=$!
T0=$(date +%s)
while kill -0 "$PID" 2>/dev/null; do
  rss=$(awk '/^VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
  anon=$(awk '/^RssAnon/{print $2}' /proc/$PID/status 2>/dev/null)
  file=$(awk '/^RssFile/{print $2}' /proc/$PID/status 2>/dev/null)
  swp=$(awk '/^VmSwap/{print $2}' /proc/$PID/status 2>/dev/null)
  free=$(awk '/^MemFree/{print $2}' /proc/meminfo)
  sfree=$(awk '/^SwapFree/{print $2}' /proc/meminfo)
  phase=$(grep -q "Started mapping" "$P/Log.out" 2>/dev/null && echo map || echo setup)
  echo "$(( $(date +%s) - T0 )) $phase rss_gb=$(( ${rss:-0}/1048576 )) anon_gb=$(( ${anon:-0}/1048576 )) file_gb=$(( ${file:-0}/1048576 )) swap_mb=$(( ${swp:-0}/1024 )) memfree_gb=$(( free/1048576 )) swapfree_mb=$(( sfree/1024 ))"
  sleep 3
done > "$P/mem.log"
wait "$PID"; echo "STAR-EXIT $?"
tail -1 "$P/stats.jsonl" | python3 -c 'import json,sys; d=json.loads(sys.stdin.read()); print({k:d.get(k) for k in ("batches","gpu_consumed","key_misses","setup_wall_s")})'
echo MEM-DONE
