#!/usr/bin/env bash
# PROBE only: validated SPLIT boundary, real full index, synthetic requests.
set -euo pipefail
CUTOFF=1788750000
LEFT=$((CUTOFF-$(date +%s)))
((LEFT>0)) || exit 124
if [[ ${PROBE_MEASURE_LOCKED:-0} != 1 ]]; then
  exec flock -n "$HOME/.cache/uni-rnaseq-resource.lock" timeout --signal=TERM --kill-after=30s "${LEFT}s" env PROBE_MEASURE_LOCKED=1 bash "$0"
fi
LAB="$HOME/uni-rnaseq-probe-lab"
ROOT="$LAB/probe-source-v1"
OUT="$LAB/probe-measure-host1"
[[ ! -e "$OUT" ]] || exit 73
mkdir "$OUT"
exec > "$OUT/probe-host.log" 2>&1
trap 'rc=$?; printf "exit=%s\n" "$rc" >> "$OUT/probe.status"' EXIT
printf 'PROBE_RUNNING cutoff=%s\n' "$CUTOFF" > "$OUT/probe.status"
export PATH="/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH"
cd "$ROOT"
run() {
  local label="$1" left rc; shift
  left=$((CUTOFF-$(date +%s))); ((left>0)) || exit 124
  printf '%q ' "$@" > "$OUT/probe-$label.argv"; printf '\n' >> "$OUT/probe-$label.argv"
  set +e
  /usr/bin/time -f '%e %U %S %M %x' -o "$OUT/probe-$label.time.tsv" timeout --signal=TERM --kill-after=30s "${left}s" "$@" > "$OUT/probe-$label.stdout" 2> "$OUT/probe-$label.stderr"
  rc=$?; set -e
  printf '%s exit=%s\n' "$label" "$rc" >> "$OUT/probe.status"
  ((rc==0)) || exit "$rc"
}
run preflight python3 - "$OUT" "$LAB/probe-build-host1" <<'PY'
from pathlib import Path
import hashlib,json,sys,subprocess
out,build=map(Path,sys.argv[1:]); root=Path.cwd(); home=Path.home()
manifest=json.loads((build/'probe-source-sha256.json').read_text())
assert all(hashlib.sha256((root/p).read_bytes()).hexdigest()==h for p,h in manifest.items()),'probe source changed since compiled build'
assert (build/'probe-build.status').read_text().endswith('exit=0\n')
raw=home/'uni-rnaseq-seed-lab/evidence/seed-split-run2'
continuation=home/'uni-rnaseq-seed-lab/evidence/seed-split-run2-continuation'
assert (continuation/'run.status').read_text().endswith('exit=0\n'),'SPLIT not completed'
sys.path.insert(0,str(home/'uni-rnaseq-seed-lab/tooling-seed-split-v3/replay'))
from seed_split_summary import validate
cases={}; identities={}
for key,base,pairs in [('20m',raw,20000000),('5m',continuation,5000000)]:
    summary=base/f'{key}-counters/seed-split-counters.json'
    d=validate(json.loads(summary.read_text()))
    parity=continuation/f'{key}-parity/parity.json'
    p=json.loads(parity.read_text())
    assert d['reads']==pairs and p['status']=='PARITY_MATCH_COUNTERS_ONLY'
    assert p['input_reads']=={'stock':pairs,'counters':pairs}
    inner=sum(d['inner']['compared_bytes_by_direction']); direct=sum(d['direct']['compared_bytes_by_direction'])
    assert inner+direct>0 and inner*10>=(inner+direct)*7,'SPLIT requires direct extension before final PROBE'
    cases[key]={'pairs':pairs,'inner_bytes':inner,'direct_bytes':direct,'inner_fraction':inner/(inner+direct),'boundary':'inner-only','hook_overhead':'report separately; do not correct CPU timings'}
    for artifact in (summary,parity,continuation/'run.status'):
        identities[str(artifact)]=hashlib.sha256(artifact.read_bytes()).hexdigest()
    assert not list((base/f'{key}-counters').glob('*.ssir'))
(out/'probe-boundary.json').write_text(json.dumps({'status':'PROBE_INNER_BOUNDARY_VALIDATED','cases':cases,'source_sha256':identities},indent=2,sort_keys=True)+'\n')
print(json.dumps(cases,indent=2)); print(subprocess.check_output(['nvidia-smi','--query-gpu=name,clocks.current.sm,utilization.gpu,memory.used','--format=csv'],text=True))
PY
run verify-binary sha256sum --check "$LAB/probe-build-host1/probe-binary-sha256.txt"
run wrapper-hash sha256sum "$LAB/probe-measure-host1.sh"
BIN="$ROOT/target/release/umseed-probe"
INDEX="$HOME/uni-rnaseq/data/index/star_full"
READS="$HOME/uni-rnaseq/data/samples/ERR188140_20M/ERR188140_20M_1.fastq.gz"
REQUESTS="$OUT/probe-requests.bin"
run generate "$BIN" generate --fastq "$READS" --index "$INDEX" --output "$REQUESTS" --count 4000000 --seed 188140
BOUNDARY_HASH=$(sha256sum "$OUT/probe-boundary.json"); BOUNDARY_HASH=${BOUNDARY_HASH%% *}
run huge "$BIN" run --index "$INDEX" --requests "$REQUESTS" --output "$OUT/probe-huge.tsv" --variant thread --pages huge --counts 64000,256000,1000000,4000000 --repeats 3 --overlap --split-provenance "inner-only; evidence=$OUT/probe-boundary.json; sha256=$BOUNDARY_HASH; counter overhead not corrected" --cutoff-unix "$CUTOFF"
run verify-huge python3 - "$OUT/probe-huge.tsv" <<'PY'
from pathlib import Path
import csv,statistics,sys
text=Path(sys.argv[1]).read_text(); assert '# PROBE COMPLETE agreement' in text
rows=list(csv.DictReader((s for s in text.splitlines() if not s.startswith('#')),delimiter='\t'))
for n in (64000,256000,1000000,4000000):
    r=[x for x in rows if x['mode']=='PROBE-isolated' and int(x['requests_per_arm'])==n]
    assert len(r)==3 and {int(x['repeat']) for x in r}=={1,2,3}
    print('PROBE thread',n,'median_gpu_over_cpu20',statistics.median(float(x['cpu_s'])/float(x['gpu_wall_s']) for x in r))
PY
run small4k "$BIN" run --index "$INDEX" --requests "$REQUESTS" --output "$OUT/probe-small4k.tsv" --variant thread --pages small4k --counts 64000 --repeats 3 --overlap --split-provenance "inner-only; evidence=$OUT/probe-boundary.json; sha256=$BOUNDARY_HASH; same requests as successful huge round" --cutoff-unix "$CUTOFF"
run clocks nvidia-smi --query-gpu=name,clocks.current.sm,utilization.gpu,power.draw --format=csv
printf 'PROBE_THREAD_ROUNDS_COMPLETE_REQUIRES_ORCHESTRATOR_DECISION\n' >> "$OUT/probe.status"
