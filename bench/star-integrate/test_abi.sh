#!/bin/sh
set -eu
cd "$(dirname "$0")"
abi_tmp=$(mktemp -d "${TMPDIR:-/tmp}/star-integrate-abi.XXXXXX")
trap 'rm -rf "$abi_tmp"' EXIT HUP INT TERM
# Prefer clang (Homebrew on macOS, distro on Linux); fall back to whatever cc/c++ is present.
if [ -x /opt/homebrew/opt/llvm/bin/clang ]; then
  cc_bin=/opt/homebrew/opt/llvm/bin/clang; cxx_bin=$cc_bin
elif command -v clang >/dev/null 2>&1; then
  cc_bin=$(command -v clang); cxx_bin=$cc_bin
else
  cc_bin=$(command -v cc); cxx_bin=$(command -v c++)
fi
"$cc_bin" --version | head -n 1
for mode in c11 c++17; do
  case "$mode" in c11) lang=c; comp=$cc_bin ;; *) lang=c++; comp=$cxx_bin ;; esac
  "$comp" -x "$lang" -std="$mode" -Wall -Wextra -Werror -pedantic \
    -fsyntax-only abi_compile.c
  "$comp" -x "$lang" -std="$mode" -Wall -Wextra -Werror -pedantic \
    -DABI_LAYOUT_ONLY abi_compile.c -o "$abi_tmp/$mode"
  printf '%s: ' "$mode"
  "$abi_tmp/$mode"
done
# Existing C++ search consumer must still compile after extraction.
"$cxx_bin" -x c++ -std=c++17 -Wall -Wextra -Werror -pedantic -fsyntax-only \
  -I../../crates/umgpu/shim ../../crates/umseed-probe/tests/probe_transport.cpp
printf 'existing probe_transport.cpp: PASS\n'
