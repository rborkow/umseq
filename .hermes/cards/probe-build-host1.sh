#!/usr/bin/env bash
# PROBE build/preflight only: no full-index load, no performance claim.
set -euo pipefail
CUTOFF=1788750000
LEFT=$((CUTOFF-$(date +%s)))
((LEFT>0)) || exit 124
if [[ ${PROBE_BUILD_LOCKED:-0} != 1 ]]; then
  exec flock -n "$HOME/.cache/uni-rnaseq-resource.lock" timeout --signal=TERM --kill-after=20s "${LEFT}s" env PROBE_BUILD_LOCKED=1 bash "$0"
fi
ROOT="$HOME/uni-rnaseq-probe-lab/probe-source-v1"
OUT="$HOME/uni-rnaseq-probe-lab/probe-build-host1"
[[ ! -e "$OUT" ]] || exit 73
mkdir "$OUT"
exec > "$OUT/probe-build.log" 2>&1
trap 'rc=$?; printf "exit=%s\n" "$rc" >> "$OUT/probe-build.status"' EXIT
printf 'PROBE_BUILD_RUNNING cutoff=%s\n' "$CUTOFF" > "$OUT/probe-build.status"
export PATH="/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=4
cd "$ROOT"
date --iso-8601=seconds
uname -a
rustc --version
cargo --version
nvcc --version
nvidia-smi --query-gpu=name,clocks.current.sm,utilization.gpu --format=csv
python3 - "$OUT" <<'PY'
import hashlib,json,sys
from pathlib import Path
root=Path.cwd(); paths=[root/'Cargo.toml',root/'Cargo.lock']
for name in ('umem','umgpu','umseed-probe'):
    paths.extend(p for p in (root/'crates'/name).rglob('*') if p.is_file() and 'target' not in p.parts)
manifest={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths)}
(Path(sys.argv[1])/'probe-source-sha256.json').write_text(json.dumps(manifest,indent=2,sort_keys=True)+'\n')
print('PROBE source_files',len(manifest))
PY
/usr/bin/time -f '%e %U %S %M %x' -o "$OUT/probe-build.time.tsv" cargo build --offline --release --locked -p umseed-probe --features cuda
/usr/bin/time -f '%e %U %S %M %x' -o "$OUT/probe-tests.time.tsv" cargo test --offline --release --locked -p umseed-probe --features cuda
sha256sum target/release/umseed-probe > "$OUT/probe-binary-sha256.txt"
cuobjdump --dump-sass target/release/umseed-probe > "$OUT/probe.sass"
python3 - "$OUT" <<'PY'
from pathlib import Path
import re,sys,json
out=Path(sys.argv[1]); text=(out/'probe.sass').read_text(); functions={}
for block in text.split('Function : ')[1:]:
    name=block.splitlines()[0].strip()
    if 'probe_thread_kernel' in name:
        functions[name]=len(re.findall(r'\bLDG(?:\.|\s)',block))
assert functions and all(n>0 for n in functions.values()),'PROBE SASS has no actual global loads'
(out/'probe-sass-loads.json').write_text(json.dumps(functions,indent=2)+'\n')
print('PROBE SASS actual loads',functions)
PY
printf 'PROBE_BUILD_AND_HOST_TESTS_PASSED_NOT_MEASURED\n' >> "$OUT/probe-build.status"
