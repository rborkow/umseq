#!/usr/bin/env bash
# Memory attribution of the enabled (borrowed-index) STAR at 4M: sample RSS every 3 s and
# dump the largest anonymous mappings from /proc/PID/smaps at peak (t=45 s).
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host20; P=$O/v3-mem2
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
STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl "$G/private/integrated/STAR" $ARGV --readMapNumber 8000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err" &
PID=$!
T0=$(date +%s); dumped=0
while kill -0 "$PID" 2>/dev/null; do
  t=$(( $(date +%s) - T0 ))
  rss=$(awk '/^VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
  anon=$(awk '/^RssAnon/{print $2}' /proc/$PID/status 2>/dev/null)
  free=$(awk '/^MemFree/{print $2}' /proc/meminfo)
  echo "$t rss_gb=$(( ${rss:-0}/1048576 )) anon_gb=$(( ${anon:-0}/1048576 )) memfree_gb=$(( free/1048576 ))"
  if [ $dumped -eq 0 ] && [ ${rss:-0} -gt 40000000 ]; then
    # Top anonymous mappings by Rss with their AnonHugePages
    awk '/^[0-9a-f]+-[0-9a-f]+ /{range=$1; perm=$2; name=$6} /^Rss:/{rss=$2} /^AnonHugePages:/{ahp=$2; if (rss>200000) printf "%s %s rss_mb=%d anonhuge_mb=%d %s\n", range, perm, rss/1024, ahp/1024, name}' /proc/$PID/smaps 2>/dev/null | sort -t= -k2 -rn | head -25 > "$P/smaps-peak.txt"
    grep -E "^(Rss|Anonymous|AnonHugePages|Shared_Clean|Private_Clean|Private_Dirty)" /proc/$PID/smaps_rollup > "$P/rollup-peak.txt" 2>/dev/null
    dumped=1
  fi
  sleep 3
done > "$P/mem.log"
wait "$PID"; echo "STAR-EXIT $?"
tail -1 "$P/stats.jsonl" | python3 -c 'import json,sys; d=json.loads(sys.stdin.read()); print({k:d.get(k) for k in ("batches","gpu_consumed","chains_consumed","key_misses","cpu_fallback","setup_wall_s","live_bytes_at_finish")})'
echo "--- rollup at peak ---"; cat "$P/rollup-peak.txt"
echo "--- top mappings at peak ---"; cat "$P/smaps-peak.txt"
echo MEM2-DONE
