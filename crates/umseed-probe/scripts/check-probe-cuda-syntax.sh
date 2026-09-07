#!/usr/bin/env bash
# Local compiler sanity only: mock declarations, no CUDA SDK, codegen or execution.
set -euo pipefail
PROBE_ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
PROBE_CLANG=${PROBE_CLANG:-clang++}
for PROBE_PASS in --cuda-host-only --cuda-device-only; do
    "$PROBE_CLANG" -x cuda -std=c++17 -fsyntax-only -Wall -Wextra -Werror \
        -nocudainc -nocudalib --cuda-gpu-arch=sm_80 "$PROBE_PASS" \
        -D__CUDACC__ -include "$PROBE_ROOT/crates/umseed-probe/tests/probe_cuda_syntax/cuda_runtime.h" \
        -I "$PROBE_ROOT/crates/umseed-probe/tests/probe_cuda_syntax" \
        "$PROBE_ROOT/crates/umgpu/shim/seed_probe.cu"
done
printf 'PROBE CUDA syntax-only host/device passes (mock declarations); NOT a CUDA build\n'
