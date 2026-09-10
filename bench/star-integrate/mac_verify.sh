#!/usr/bin/env bash
# Mac verification of the star-integrate test suite (toolchain quirks: llvm@22 symlink, libomp).
cd ~/Projects/uni-rnaseq || exit 1
export CPATH=/opt/homebrew/opt/libomp/include LIBRARY_PATH=/opt/homebrew/opt/libomp/lib
for t in test_coordinator.py test_window_contract.py test_window_prefix.py test_source_patch.py test_production_gate.py; do
  python3.13 "bench/star-integrate/$t" > "/tmp/$t.log" 2>&1
  echo "$t rc=$? $(grep -E '^OK|FAIL|Error' "/tmp/$t.log" | tail -1)"
done
