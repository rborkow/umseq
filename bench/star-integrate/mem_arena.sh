#!/usr/bin/env bash
# Disqualifier for the "glibc per-thread arena growth" hypothesis: same 8M run with
# MALLOC_ARENA_MAX=1 + MALLOC_TRIM_THRESHOLD_ (forces returns to the OS). If peak RSS falls
# to ~index + O(in-flight windows), the ~55 GB at 20M is arena retention, not a leak.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host20; P=$O/v3-mem3
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
for arm in default arena1; do
  mkdir -p "$P/$arm"
  if [ "$arm" = arena1 ]; then export MALLOC_ARENA_MAX=1 MALLOC_TRIM_THRESHOLD_=67108864 MALLOC_MMAP_THRESHOLD_=1048576; else unset MALLOC_ARENA_MAX MALLOC_TRIM_THRESHOLD_ MALLOC_MMAP_THRESHOLD_; fi
  STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/$arm/stats.jsonl /usr/bin/time -f "%e s wall %U user %S sys %M KiB maxrss" -o "$P/$arm/time.txt" "$G/private/integrated/STAR" $ARGV --readMapNumber 8000000 --outFileNamePrefix "$P/$arm/" >/dev/null 2>"$P/$arm/err"
  echo "$arm: $(cat $P/$arm/time.txt)  mapping=$(grep -E 'Started mapping|Finished on' $P/$arm/Log.final.out | awk -F'\t' '{print $2}' | tr '\n' ' ')"
done
echo MEM3-DONE
