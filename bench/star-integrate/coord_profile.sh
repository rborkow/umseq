#!/usr/bin/env bash
# CPU profile of the coordinator thread: 2M reads, 20 threads, cpu-clock sampling with call
# graphs, reported per-thread so the coordinator's own time splits by symbol.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host21; P=$O/v3-coordprof
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
cd "$P"
STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl perf record -g -e cpu-clock -F 499 -o "$P/perf.data" -- $ARGV --readMapNumber 2000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err"
# Find the coordinator's tid: the thread whose samples include coordinator_main.
perf report -i "$P/perf.data" --stdio --no-children --sort tid,sym -g none 2>/dev/null | grep -vE "^#|^$" > "$P/by-tid.txt"
TID=$(grep -m1 "coordinator_main" "$P/by-tid.txt" | grep -oE "STAR:[0-9]+" | head -1)
echo "coordinator thread: $TID"
echo "=== coordinator thread, self time by symbol ==="
perf report -i "$P/perf.data" --stdio --no-children --tid "${TID#STAR:}" --sort sym -g none --percent-limit 1 2>/dev/null | grep -vE "^#|^$" | head -30 | cut -c1-150
echo "=== coordinator thread, callers of top symbols ==="
perf report -i "$P/perf.data" --stdio --no-children --tid "${TID#STAR:}" --sort sym -g caller,3,callee,function,percent --percent-limit 5 2>/dev/null | grep -vE "^#|^$" | head -70 | cut -c1-150
echo "=== whole process by thread (coordinator share of total CPU) ==="
perf report -i "$P/perf.data" --stdio --no-children --sort tid -g none 2>/dev/null | grep -vE "^#|^$" | head -6 | cut -c1-100
echo COORDPROF-DONE
