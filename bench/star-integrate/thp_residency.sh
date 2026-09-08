#!/usr/bin/env bash
# Per-arm index page backing from the real STAR pid (review Blocking 2): 4M reads, sample when
# RSS > 30 GB (index loaded, mapping under way). Same binaries as thp_ablation.sh.
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host22; A=$O/thp-ablation; P=$O/thp-residency
STOCK=$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR
INTEG=$A/private/integrated/STAR
INDEX=/home/rborkows/uni-rnaseq/data/index/star_full
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
star_pid() { local pid; for pid in $(pgrep -P "$1"); do [ "$(readlink /proc/$pid/exe 2>/dev/null)" = "$2" ] && { echo $pid; return; }; done; echo ""; }
run() {
  local arm=$1 bin=$2; shift 2; local dir=$P/$arm; mkdir -p "$dir"
  cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null
  env -u STAR_INTEGRATE -u STAR_INTEGRATE_STRICT -u STAR_INTEGRATE_SIDECAR "$@" /usr/bin/time -f "%e\t%U\t%S\t%M" -o "$dir/time.tsv" "$bin" $ARGV --readMapNumber 4000000 --outFileNamePrefix "$dir/" >/dev/null 2>"$dir/err" &
  local tpid=$! spid="" sampled=0
  while kill -0 $tpid 2>/dev/null; do
    sleep 2
    [ -z "$spid" ] && spid=$(star_pid $tpid "$bin")
    if [ $sampled -eq 0 ] && [ -n "$spid" ]; then
      rss=$(awk '/^VmRSS/{print $2}' /proc/$spid/status 2>/dev/null)
      if [ "${rss:-0}" -gt 30000000 ]; then
        sleep 4   # let mapping start
        grep -E "^(Rss|AnonHugePages)" /proc/$spid/smaps_rollup > "$dir/smaps_rollup.txt" 2>/dev/null
        awk '/^[0-9a-f]+-[0-9a-f]+ /{r=$1; perm=$2; name=$6} /^Size:/{sz=$2} /^Rss:/{rss=$2} /^AnonHugePages:/{ahp=$2} /^THPeligible:/{el=$2} /^VmFlags:/{ if (sz>=1048576 && name=="") printf "%s %s size_gb=%.2f rss_gb=%.2f anonhuge_gb=%.2f thp_eligible=%s flags=%s\n", r, perm, sz/1048576, rss/1048576, ahp/1048576, el, $0}' /proc/$spid/smaps > "$dir/index-vmas.txt" 2>/dev/null
        sampled=1
      fi
    fi
  done
  wait $tpid
  echo "== $arm (pid $spid, exe $(readlink /proc/$spid/exe 2>/dev/null || echo exited)): time $(cat $dir/time.tsv | tr '\t' ' ')"
  cat "$dir/smaps_rollup.txt"; sed 's/flags=VmFlags://' "$dir/index-vmas.txt"
}
run stock "$STOCK"
run bypass-thp "$INTEG"
run bypass-4k "$INTEG" STAR_INTEGRATE_THP=0
echo RESIDENCY-DONE
