#!/usr/bin/env bash
# (1) GPU-arm sys attribution done properly: perf cpu-clock with kernel+user stacks on the GPU arm
#     and the advised-bypass arm, same 8M / 20 thr, warm cache. Classification by perf's own
#     DSO ([kernel.kallsyms] vs others), never by symbol name. Per thread; kernel leaves; and
#     for kernel samples the first user-space frame (the syscall/fault site's caller).
# (2) MISS counters at 20M: one enabled run (strict off) with the MISS-classified sidecar.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host22; A=$O/thp-ablation; P=$O/gpu-sys
SRC=$O/integrate-source-v1
INDEX=/home/rborkows/uni-rnaseq/data/index/star_full
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH
# rebuild the ablation's private STAR with the MISS-classified star_integrate.cpp (only that TU changed)
cp "$SRC/bench/star-integrate/star_integrate.cpp" "$A/private/integrated/star_integrate.cpp"
(cd "$A/private/integrated" && make -j8 STAR CXX=$(which g++) "CXXFLAGSextra=-DSTAR_INTEGRATE=1 -I$SRC/bench/star-integrate" "LDFLAGSextra=$G/target/release/libumstar.a -L/usr/local/cuda/lib64 -Wl,-rpath,/usr/local/cuda/lib64 -lcudart -ldl -lm -lrt" CXXFLAGS_SIMD= > "$P/make.log" 2>&1) || { echo BUILD-FAIL; tail -5 "$P/make.log"; echo GPUSYS-DONE; exit 0; }
INTEG=$A/private/integrated/STAR
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
prof() { # arm envs...
  local arm=$1; shift; local dir=$P/$arm; mkdir -p "$dir"; cd "$dir"
  cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null
  env -u STAR_INTEGRATE -u STAR_INTEGRATE_STRICT "$@" /usr/bin/time -f "%e\t%U\t%S\t%M" -o "$dir/time.tsv" perf record -g -e cpu-clock -F 499 -o "$dir/perf.data" -- "$INTEG" $ARGV --readMapNumber 8000000 --outFileNamePrefix "$dir/" >/dev/null 2>"$dir/err"
  echo "== $arm: wall/user/sys/maxrss $(cat $dir/time.tsv | tr '\t' ' ')"
  perf script -i "$dir/perf.data" -F tid,ip,sym,dso 2>/dev/null > "$dir/script.txt"
  python3 - "$dir/script.txt" <<'EOF'
import sys,collections
tid=None; cur=[]; N=0
per_tid=collections.defaultdict(lambda:[0,0]); kleaf=collections.Counter(); kcaller=collections.Counter(); ucaller2=collections.Counter()
def flush():
    global cur,N
    if tid is None or not cur: cur=[]; return
    N+=1
    leaf_sym,leaf_dso=cur[0]
    k = leaf_dso.startswith("[kernel") or leaf_dso=="[unknown]" and leaf_sym=="[unknown]" and False
    k = leaf_dso.startswith("[kernel")
    per_tid[tid][1 if k else 0]+=1
    if k:
        kleaf[leaf_sym]+=1
        u=[s for s,d in cur if not d.startswith("[kernel")]
        kcaller[u[0] if u else "(no user frame)"]+=1
        ucaller2[" < ".join(u[:3]) if u else "(no user frame)"]+=1
    cur=[]
for l in open(sys.argv[1],errors="replace"):
    s=l.rstrip("\n")
    if not s.strip(): flush(); continue
    if not s.startswith("\t"): flush(); tid=s.strip(); cur=[]; continue
    parts=s.strip().split(" ",1)
    rest=parts[1] if len(parts)>1 else ""
    # "sym+0x.. (dso)"
    dso=rest[rest.rfind("(")+1:-1] if rest.endswith(")") else "?"
    sym=rest[:rest.rfind("(")].strip().split("+0x")[0] if "(" in rest else rest
    cur.append((sym,dso))
flush()
ks=sum(v[1] for v in per_tid.values()); us=N-ks
print(f"   samples {N}: user {us} ({100*us/N:.1f}%)  kernel {ks} ({100*ks/N:.1f}%)   [by DSO]")
print("   --- threads by kernel samples (top 5) ---")
for t,(u,k) in sorted(per_tid.items(), key=lambda x:-x[1][1])[:5]: print(f"     tid {t}: user {u:6d} kernel {k:6d}  ({100*k/max(1,u+k):.0f}% kernel)")
print("   --- kernel leaf symbols (top 12) ---")
for s_,n in kleaf.most_common(12): print(f"     {100*n/max(1,ks):5.1f}%  {s_[:80]}")
print("   --- first user-space frame under kernel samples (top 12) ---")
for s_,n in kcaller.most_common(12): print(f"     {100*n/max(1,ks):5.1f}%  {s_[:110]}")
print("   --- user stacks (3 frames) under kernel samples (top 8) ---")
for s_,n in ucaller2.most_common(8): print(f"     {100*n/max(1,ks):5.1f}%  {s_[:170]}")
EOF
}
prof bypass-thp
prof gpu STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/gpu/stats.jsonl

echo "== MISS counters, 20M, GPU on, strict off =="
mkdir -p "$P/miss20m"; cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null
STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/miss20m/stats.jsonl /usr/bin/time -f "%e\t%U\t%S\t%M" -o "$P/miss20m/time.tsv" "$INTEG" $ARGV --outFileNamePrefix "$P/miss20m/" >/dev/null 2>"$P/miss20m/err"
echo "time: $(cat $P/miss20m/time.tsv | tr '\t' ' ')"
tail -1 "$P/miss20m/stats.jsonl" | python3 -c '
import json,sys; d=json.loads(sys.stdin.read())
for k in ("submitted","gpu_consumed","key_misses","no_window","cpu_fallback","cpu_tails"): print(f"  {k}: {d.get(k)}")
print("  miss_reasons:", json.dumps(d.get("miss_reasons")))
print("  not_ready_where:", json.dumps(d.get("not_ready_where")))
print("  device_stop_status:", json.dumps(d.get("device_stop_status")))'
echo GPUSYS-DONE
