#!/usr/bin/env bash
# Heap attribution: heaptrack on the enabled STAR, 2M reads, 8 threads.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host20; P=$O/v3-heap
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stock-reuse.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))["argv"][1:]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
i=a.index("--runThreadN"); a[i+1]="8"
print(" ".join(a))
EOF
)
if command -v heaptrack >/dev/null; then
  cd "$P" && STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl heaptrack -o "$P/ht" "$G/private/integrated/STAR" $ARGV --readMapNumber 2000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err"
  f=$(ls $P/ht*.zst $P/ht*.gz 2>/dev/null | head -1)
  heaptrack_print -p 0 -a 0 -T 0 -l 0 -n 12 "$f" 2>/dev/null | sed -n '/PEAK MEMORY CONSUMERS/,/^$/p' | head -80 | cut -c1-180
else
  echo "no heaptrack; falling back to malloc_info via LD_PRELOAD-free sampling of /proc smaps by thread"
  # Attribute the anonymous mappings to threads: each mapping's start address vs each thread's stack/arena is not
  # directly available; instead dump per-thread allocated bytes via glibc malloc_info at peak using gdb.
  STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl "$G/private/integrated/STAR" $ARGV --readMapNumber 2000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err" &
  PID=$!; sleep 25
  gdb -p $PID -batch -ex 'call (int)malloc_info(0, (void*)stdout)' -ex 'call (int)fflush(0)' >/dev/null 2>&1
  wait $PID
fi
echo HEAP-DONE
