#!/usr/bin/env bash
# Who grows the heap? Trace mmap/mprotect (glibc arena growth + large allocations) with call
# stacks on the enabled STAR, 1M reads, 8 threads. Attributes the linear per-read RSS growth.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host21; P=$O/v3-heap
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stages/integrated.argv.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))
a=a[a.index("--runMode")-1:]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
i=a.index("--runThreadN"); a[i+1]="8"
print(" ".join(a))
EOF
)
cd "$P"
export STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl
# Tracepoints need root here; page-faults (software event, no permission needed) attributes
# first-touch of new memory by caller, which is what grows RSS. Sample every 64th fault.
perf record -g -e page-faults -c 64 -o "$P/perf.data" -- $ARGV --readMapNumber 1000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err"
echo "samples: $(perf report -i $P/perf.data --stdio -F overhead 2>/dev/null | grep -m1 'Event count' | cut -c1-80)"
echo "=== by symbol (self) ==="
perf report -i "$P/perf.data" --stdio --no-children --sort sym --percent-limit 1.5 -g none 2>/dev/null | grep -vE "^#|^$" | head -25 | cut -c1-140
echo "=== callers of the top faulting sites (caller graph) ==="
perf report -i "$P/perf.data" --stdio --no-children --sort sym -g caller,2,callee,function,percent --percent-limit 4 2>/dev/null | grep -vE "^#|^$" | head -90 | cut -c1-150
echo HEAP-DONE
