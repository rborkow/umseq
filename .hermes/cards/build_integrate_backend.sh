#!/usr/bin/env bash
set -eu
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"
flock -n 9
src="$HOME/uni-rnaseq-probe-lab/integrate-backend-source-v1"
out="$HOME/uni-rnaseq-probe-lab/integrate-backend-host1"
left=$((1788836400-$(date +%s)))
test "$left" -gt 0
mkdir "$out"
export PATH="$HOME/.cargo/bin:/usr/local/cuda/bin:$PATH"
export UMGPU_NVCC=/usr/local/cuda/bin/nvcc CARGO_BUILD_JOBS=4
export CARGO_TARGET_DIR="$out/target"
printf 'RUNNING backend CUDA build\n' > "$out/run.status"
cd "$src"
set +e
timeout --signal=TERM --kill-after=30s "${left}s" bash -c 'cargo build -p umstar --release --features cuda --offline && cargo test -p umstar --features cuda --offline' > "$out/build.log" 2>&1
rc=$?
set -e
printf 'backend exit=%s\n' "$rc" >> "$out/run.status"
if test "$rc" -eq 0; then
  sha256sum "$out/target/release/libumstar.a" > "$out/backend.sha256"
  printf 'COMPLETE\n' >> "$out/run.status"
fi
exit "$rc"
