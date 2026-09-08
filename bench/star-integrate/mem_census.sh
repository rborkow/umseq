#!/usr/bin/env bash
# Live-window census vs RSS during a 4M-read enabled run (rebuild the integrated STAR from the
# current bench/star-integrate tree first: only star_integrate.cpp changed).
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host21; P=$O/v3-census
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
SRC=$O/integrate-source-v1
cp "$SRC/bench/star-integrate/star_integrate.cpp" "$G/private/integrated/star_integrate.cpp" 2>/dev/null || true
cd "$G/private/integrated" && make -j4 STAR CXX=$(which g++) "CXXFLAGSextra=-DSTAR_INTEGRATE=1 -I$SRC/bench/star-integrate" "LDFLAGSextra=$G/target/release/libumstar.a -L/usr/local/cuda/lib64 -Wl,-rpath,/usr/local/cuda/lib64 -lcudart -ldl -lm -lrt" CXXFLAGS_SIMD= > "$P/make.log" 2>&1 || { echo "BUILD-FAIL"; tail -5 "$P/make.log"; echo CENSUS-DONE; exit 0; }
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
STAR_INTEGRATE=1 STAR_INTEGRATE_MEMLOG=1 STAR_INTEGRATE_SIDECAR=$P/stats.jsonl /usr/bin/time -f "%e s wall %U user %S sys %M KiB maxrss" -o "$P/time.txt" $ARGV --readMapNumber 4000000 --outFileNamePrefix "$P/" >/dev/null 2>"$P/err"
cat "$P/time.txt"
grep MEMLOG "$P/err"
echo CENSUS-DONE
