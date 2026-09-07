#!/usr/bin/env bash
# PROBE only: compile both variants; odd launch/offset checks and real-index timing.
set -euo pipefail
CUTOFF=1788750000
LEFT=$((CUTOFF-$(date +%s)))
((LEFT>0)) || exit 124
if [[ ${PROBE_WARP_LOCKED:-0} != 1 ]]; then
 exec flock -n "$HOME/.cache/uni-rnaseq-resource.lock" timeout --signal=TERM --kill-after=20s "${LEFT}s" env PROBE_WARP_LOCKED=1 bash "$0"
fi
LAB="$HOME/uni-rnaseq-probe-lab"
ROOT="$LAB/probe-source-v3"
OUT="$LAB/probe-warp-host2"
mkdir "$OUT"
trap 'rc=$?; printf "exit=%s\n" "$rc" >> "$OUT/probe.status"' EXIT
printf 'PROBE_WARP_RUNNING cutoff=%s\n' "$CUTOFF" > "$OUT/probe.status"
export PATH="/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH" CARGO_BUILD_JOBS=4
cd "$ROOT"
run() {
 local name=$1; shift
 printf '%q ' "$@" > "$OUT/probe-$name.argv"
 /usr/bin/time -f '%e %U %S %M %x' -o "$OUT/probe-$name.time.tsv" "$@" > "$OUT/probe-$name.stdout" 2> "$OUT/probe-$name.stderr"
 printf '%s exit=0\n' "$name" >> "$OUT/probe.status"
}
run preflight python3 - "$OUT" "$LAB" <<'PY'
from pathlib import Path
import hashlib,json,sys,subprocess
out,lab=map(Path,sys.argv[1:]); root=Path.cwd()
assert (lab/'probe-measure-host1/probe.status').read_text().endswith('exit=0\n')
assert '# PROBE COMPLETE agreement' in (lab/'probe-measure-host1/probe-huge.tsv').read_text()
paths=[root/'Cargo.toml',root/'Cargo.lock']
for name in ('umem','umgpu','umseed-probe'):
 paths.extend(p for p in (root/'crates'/name).rglob('*') if p.is_file() and 'target' not in p.parts)
(out/'probe-source-sha256.json').write_text(json.dumps({str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths)},indent=2)+'\n')
print(subprocess.check_output(['nvidia-smi','--query-gpu=name,clocks.sm,utilization.gpu','--format=csv'],text=True))
print(Path('/proc/meminfo').read_text())
PY
run build cargo build --offline --release --locked -p umseed-probe --features cuda
run tests cargo test --offline --release --locked -p umseed-probe --features cuda
run binary-sha sha256sum target/release/umseed-probe
run sass cuobjdump --dump-sass target/release/umseed-probe
run verify-sass python3 - "$OUT" <<'PY'
from pathlib import Path
import json,re,sys
out=Path(sys.argv[1]); data={}
for block in (out/'probe-sass.stdout').read_text().split('Function : ')[1:]:
 name=block.splitlines()[0].strip()
 if any(k in name for k in ('probe_thread_kernel','probe_warp_kernel')):
  data[name]=len(re.findall(r'\bLDG(?:\.|\s)',block))
assert len(data)==2 and all(v>0 for v in data.values()),data
(out/'probe-sass-loads.json').write_text(json.dumps(data,indent=2)+'\n');print(data)
PY
BIN="$ROOT/target/release/umseed-probe"
REQUESTS="$LAB/probe-measure-host1/probe-requests.bin"
INDEX="$HOME/uni-rnaseq/data/index/star_full"
BOUNDARY="$LAB/probe-measure-host1/probe-boundary.json"
run request-sha sha256sum "$REQUESTS" "$BOUNDARY"
COMMON=(--index "$INDEX" --requests "$REQUESTS" --repeats 3 --overlap --split-provenance "inner-only; validated20M/5M=$BOUNDARY; original probe-measure-host1 inputs" --cutoff-unix "$CUTOFF")
run warp "$BIN" run "${COMMON[@]}" --output "$OUT/probe-warp.tsv" --variant warp --pages huge --counts 6,10,62,66,254,258,64000,256000,1000000,4000000
run thread "$BIN" run "${COMMON[@]}" --output "$OUT/probe-thread.tsv" --variant thread --pages huge --counts 64000,256000,1000000,4000000
run small4k "$BIN" run "${COMMON[@]}" --output "$OUT/probe-warp-small4k.tsv" --variant warp --pages small4k --counts 64000
run final-verify python3 - "$OUT" <<'PY'
from pathlib import Path
import csv,statistics,sys
out=Path(sys.argv[1])
for name,expected in [('warp',60),('thread',24),('warp-small4k',6)]:
 s=(out/f'probe-{name}.tsv').read_text(); assert '# PROBE COMPLETE agreement' in s
 rows=list(csv.DictReader((r for r in s.splitlines() if not r.startswith('#')),delimiter='\t'))
 assert len(rows)==expected,(name,len(rows))
 for n in sorted({int(r['requests_per_arm']) for r in rows if r['mode']=='PROBE-isolated'}):
  group=[r for r in rows if r['mode']=='PROBE-isolated' and int(r['requests_per_arm'])==n]
  assert len(group)==3 and {int(r['repeat']) for r in group}=={1,2,3}
  print('PROBE',name,n,'median_gpu_cpu20',statistics.median(float(r['cpu_s'])/float(r['gpu_wall_s']) for r in group))
PY
run clocks nvidia-smi --query-gpu=name,clocks.sm,utilization.gpu,power.draw --format=csv
printf 'PROBE_WARP_AND_THREAD_COMPLETE_REQUIRES_ORCHESTRATOR_REVIEW\n' >> "$OUT/probe.status"
