#!/usr/bin/env bash
# Disqualifier: is the per-read RSS growth in the enabled STAR from the strict oracle
# (STAR_INTEGRATE_STRICT=1, gate only) or from the coordinator/window (both arms)?
# 4M reads, 20 threads, peak RSS per arm, plus the per-thread mapping count at peak.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host21; P=$O/v3-mem4
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stages/integrated.argv.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))
a=a[a.index("--runMode")-1:]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
print(" ".join(a))
EOF
)
for arm in strict0 strict1; do
  mkdir -p "$P/$arm"
  STAR_INTEGRATE=1 STAR_INTEGRATE_STRICT=${arm#strict} STAR_INTEGRATE_SIDECAR=$P/$arm/stats.jsonl /usr/bin/time -f "%e s wall %U user %S sys %M KiB maxrss" -o "$P/$arm/time.txt" $ARGV --readMapNumber 4000000 --outFileNamePrefix "$P/$arm/" >/dev/null 2>"$P/$arm/err" &
  PID=$!; dumped=0
  while kill -0 "$PID" 2>/dev/null; do
    rss=$(awk '/^VmRSS/{print $2}' /proc/$PID/status 2>/dev/null)
    if [ $dumped -eq 0 ] && [ ${rss:-0} -gt 38000000 ]; then
      awk '/^[0-9a-f]+-[0-9a-f]+ /{range=$1; name=$6} /^Rss:/{rss=$2} /^AnonHugePages:/{ahp=$2; if (rss>100000) printf "%s rss_mb=%d anonhuge_mb=%d %s\n", range, rss/1024, ahp/1024, name}' /proc/$PID/smaps 2>/dev/null | sort -t= -k2 -rn > "$P/$arm/smaps-peak.txt"
      dumped=1
    fi
    sleep 2
  done
  wait "$PID"
  echo "$arm: $(cat $P/$arm/time.txt)  dumped=$dumped  big_4k_maps=$(grep -c 'anonhuge_mb=0' $P/$arm/smaps-peak.txt 2>/dev/null) big_4k_total_mb=$(awk -F'rss_mb=' '/anonhuge_mb=0/{split($2,a," "); s+=a[1]} END{print s+0}' $P/$arm/smaps-peak.txt 2>/dev/null)"
  tail -1 "$P/$arm/stats.jsonl" | python3 -c 'import json,sys; d=json.loads(sys.stdin.read()); print("   ", {k:d.get(k) for k in ("batches","gpu_consumed","key_misses","cpu_tails","other_unused","live_bytes_at_finish")})'
done
echo MEM4-DONE
