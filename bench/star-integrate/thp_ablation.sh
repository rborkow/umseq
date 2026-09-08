#!/usr/bin/env bash
# Huge-page ablation (docs/review-integrate-v2-huge.md, Blocking 1–2 and Suggested 1–3).
# Rebuilds the integrated STAR from the current generator (STAR_INTEGRATE_THP=0 switch), then:
#   arms, each 3 rotated repeats at 20M / 20 threads, page cache warmed identically before every run
#   (cat of the three index files; every arm's own fadvise/DONTNEED is thereby neutralised):
#     stock       — parity-accepted stock binary
#     bypass-thp  — integrated binary, STAR_INTEGRATE unset, advice ON  (round 7's bypass arm)
#     bypass-4k   — integrated binary, STAR_INTEGRATE unset, STAR_INTEGRATE_THP=0 (advice OFF, fadvise kept)
#   For every run: the real STAR pid (child of time, verified via /proc/<pid>/exe), its index VMAs
#   from /proc/<pid>/smaps sampled once during mapping (Rss, AnonHugePages, VmFlags), smaps_rollup.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host22; P=$O/thp-ablation
SRC=$O/integrate-source-v1
STOCK=$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR
INDEX=/home/rborkows/uni-rnaseq/data/index/star_full
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH

# --- rebuild the private integrated STAR from the current generator ------------------------------
PRIV=$P/private
python3 -B "$SRC/bench/star-integrate/make_star_integrate.py" --tooling "$SRC/tooling/replay" --source "$HOME/uni-rnaseq-seed-lab/real-index-input-2.7.11b/source" --private-root "$PRIV" > "$P/generate.log" 2>&1 || { echo "GENERATE-FAIL"; tail -5 "$P/generate.log"; echo ABLATION-DONE; exit 0; }
cd "$PRIV/integrated" && make -j8 STAR CXX=$(which g++) "CXXFLAGSextra=-DSTAR_INTEGRATE=1 -I$SRC/bench/star-integrate" "LDFLAGSextra=$G/target/release/libumstar.a -L/usr/local/cuda/lib64 -Wl,-rpath,/usr/local/cuda/lib64 -lcudart -ldl -lm -lrt" CXXFLAGS_SIMD= > "$P/make.log" 2>&1 || { echo "BUILD-FAIL"; tail -5 "$P/make.log"; echo ABLATION-DONE; exit 0; }
INTEG=$PRIV/integrated/STAR
{
echo "kernel: $(uname -r)  pagesize: $(getconf PAGESIZE)"
echo "thp: $(cat /sys/kernel/mm/transparent_hugepage/enabled) / defrag: $(cat /sys/kernel/mm/transparent_hugepage/defrag) / hpage_pmd_size: $(cat /sys/kernel/mm/transparent_hugepage/hpage_pmd_size)"
for d in /sys/kernel/mm/transparent_hugepage/hugepages-*; do echo "  $(basename $d): $(cat $d/enabled)"; done
echo "glibc: $(ldd --version | head -1)  GLIBC_TUNABLES=${GLIBC_TUNABLES:-<unset>}  LD_PRELOAD=${LD_PRELOAD:-<unset>}"
echo "stock: $STOCK $(sha256sum $STOCK | cut -c1-16)"
echo "integ: $INTEG $(sha256sum $INTEG | cut -c1-16)"
} > "$P/env.txt"; cat "$P/env.txt"

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
printf "repeat\tarm\twall_s\tuser_s\tsys_s\tmax_rss_kib\texit\tindex_rss_gb\tindex_anonhuge_gb\n" > "$P/raw.tsv"

star_pid() { # pid of the STAR child under /usr/bin/time
  local parent=$1 pid
  for pid in $(pgrep -P "$parent"); do
    if [ "$(readlink /proc/$pid/exe 2>/dev/null)" = "$2" ]; then echo $pid; return; fi
  done
  echo ""
}
run() { # rep arm binary envs...
  local rep=$1 arm=$2 bin=$3; shift 3
  local dir=$P/r$rep-$arm; mkdir -p "$dir"
  cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null        # identical warm cache every run
  env -u STAR_INTEGRATE -u STAR_INTEGRATE_STRICT -u STAR_INTEGRATE_SIDECAR "$@" /usr/bin/time -f "%e\t%U\t%S\t%M\t%x" -o "$dir/time.tsv" "$bin" $ARGV --outFileNamePrefix "$dir/" >/dev/null 2>"$dir/err" &
  local tpid=$! spid="" sampled=0
  while kill -0 $tpid 2>/dev/null; do
    sleep 3
    [ -z "$spid" ] && spid=$(star_pid $tpid "$bin")
    if [ $sampled -eq 0 ] && [ -n "$spid" ] && grep -q "Started mapping" "$dir/Log.out" 2>/dev/null; then
      cp /proc/$spid/smaps_rollup "$dir/smaps_rollup.txt" 2>/dev/null
      # index VMAs: anonymous rw-p mappings ≥ 1 GB
      awk '/^[0-9a-f]+-[0-9a-f]+ /{r=$1; perm=$2; name=$6} /^Size:/{sz=$2} /^Rss:/{rss=$2} /^AnonHugePages:/{ahp=$2} /^VmFlags:/{ if (sz>=1048576 && name=="") printf "%s %s size_gb=%.1f rss_gb=%.1f anonhuge_gb=%.1f flags=%s\n", r, perm, sz/1048576, rss/1048576, ahp/1048576, $0}' /proc/$spid/smaps > "$dir/index-vmas.txt" 2>/dev/null
      sampled=1
    fi
  done
  wait $tpid
  local irss=$(awk -F'rss_gb=' '{split($2,a," "); s+=a[1]} END{printf "%.1f", s}' "$dir/index-vmas.txt" 2>/dev/null)
  local iahp=$(awk -F'anonhuge_gb=' '{split($2,a," "); s+=a[1]} END{printf "%.1f", s}' "$dir/index-vmas.txt" 2>/dev/null)
  printf "%s\t%s\t%s\t%s\t%s\n" "$rep" "$arm" "$(cat $dir/time.tsv)" "${irss:-?}" "${iahp:-?}" >> "$P/raw.tsv"
  echo "r$rep $arm: $(cat $dir/time.tsv | tr '\t' ' ')  index rss=${irss} anonhuge=${iahp} GB  pid=${spid}"
}
for rep in 1 2 3; do
  case $rep in 1) order="stock bypass-thp bypass-4k";; 2) order="bypass-4k stock bypass-thp";; 3) order="bypass-thp bypass-4k stock";; esac
  for arm in $order; do
    case $arm in
      stock)      run $rep $arm "$STOCK" ;;
      bypass-thp) run $rep $arm "$INTEG" ;;
      bypass-4k)  run $rep $arm "$INTEG" STAR_INTEGRATE_THP=0 ;;
    esac
  done
done
echo "=== raw.tsv ==="; cat "$P/raw.tsv"
echo "=== parity of the three arms' outputs (records) ==="
for d in r1-stock r1-bypass-thp r1-bypass-4k; do printf "%-14s " $d; grep -c . "$P/$d/Aligned.out.sam" 2>/dev/null; done
cmp <(grep -v '^@' $P/r1-stock/Aligned.out.sam) <(grep -v '^@' $P/r1-bypass-4k/Aligned.out.sam) && echo "stock vs bypass-4k: identical alignment records"
cmp <(grep -v '^@' $P/r1-stock/Aligned.out.sam) <(grep -v '^@' $P/r1-bypass-thp/Aligned.out.sam) && echo "stock vs bypass-thp: identical alignment records"
echo ABLATION-DONE
