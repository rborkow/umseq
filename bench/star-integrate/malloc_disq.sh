#!/usr/bin/env bash
# Disqualifier for LAT item 1: does keeping window allocations inside glibc's heap (no per-window
# mmap/munmap) remove the GPU arm's sys excess? GPU arm, 8M, 20 thr, warm cache, default vs
# MALLOC_MMAP_THRESHOLD_/TRIM_THRESHOLD_ = 1 GB, two repeats each, interleaved.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host22; A=$O/thp-ablation; P=$O/malloc-disq
INTEG=$A/private/integrated/STAR; INDEX=/home/rborkows/uni-rnaseq/data/index/star_full
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stages/integrated.argv.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))
a=a[a.index("--runMode"):]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
print(" ".join(a))
EOF
)
run() { local name=$1; shift; local d=$P/$name; mkdir -p "$d"; cat $INDEX/Genome $INDEX/SA $INDEX/SAindex >/dev/null
  env -u STAR_INTEGRATE_STRICT "$@" STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$d/stats.jsonl /usr/bin/time -f "%e\t%U\t%S\t%M" -o "$d/time.tsv" "$INTEG" $ARGV --readMapNumber 8000000 --outFileNamePrefix "$d/" >/dev/null 2>"$d/err"
  printf "%-16s wall/user/sys/maxrss %s   " "$name" "$(cat $d/time.tsv | tr '\t' ' ')"
  tail -1 "$d/stats.jsonl" | python3 -c 'import json,sys; d=json.loads(sys.stdin.read()); m=d.get("miss_reasons",{}); print("consumed",d["gpu_consumed"],"not_ready",m.get("not_ready"),"residue",m.get("chain_rejected_residue"))'
}
run default-1
run heap-1 MALLOC_MMAP_THRESHOLD_=1073741824 MALLOC_TRIM_THRESHOLD_=1073741824 MALLOC_TOP_PAD_=268435456
run default-2
run heap-2 MALLOC_MMAP_THRESHOLD_=1073741824 MALLOC_TRIM_THRESHOLD_=1073741824 MALLOC_TOP_PAD_=268435456
echo MALLOC-DONE
