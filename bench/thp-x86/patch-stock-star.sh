#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
  echo "usage: $0 STAR-2.7.11b-source-tree" >&2
  exit 2
fi
ROOT=$(cd "$1" && pwd -P)
SCRIPT=$(cd "$(dirname "$0")" && pwd -P)
python3 "$SCRIPT/star_thp_patch.py" "$ROOT"
BUILD_ROOT="$ROOT"
[[ -f "$ROOT/Makefile" ]] || BUILD_ROOT="$ROOT/source"
# CXXFLAGSextra is the Makefile's hook for extra defines; overriding CXXFLAGS_common drops
# -fopenmp (link fails on GOMP_parallel). SIMD flags stay as the Makefile chooses.
make -C "$BUILD_ROOT" STAR CXXFLAGSextra="-DSTAR_THP_PATCH=1"
echo "built $BUILD_ROOT/STAR (STAR_THP=0 disables advice; STAR_THP=1 enables it)"
