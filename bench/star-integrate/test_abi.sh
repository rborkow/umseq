#!/bin/sh
set -eu
cd "$(dirname "$0")"
abi_tmp=$(mktemp -d /private/tmp/star-integrate-abi.XXXXXX)
trap 'rm -rf "$abi_tmp"' EXIT HUP INT TERM
clang=/opt/homebrew/opt/llvm/bin/clang
"$clang" --version | head -n 1
for mode in c11 c++17; do
  case "$mode" in c11) lang=c ;; *) lang=c++ ;; esac
  "$clang" -x "$lang" -std="$mode" -Wall -Wextra -Werror -pedantic \
    -fsyntax-only abi_compile.c
  "$clang" -x "$lang" -std="$mode" -Wall -Wextra -Werror -pedantic \
    -DABI_LAYOUT_ONLY abi_compile.c -o "$abi_tmp/$mode"
  printf '%s: ' "$mode"
  "$abi_tmp/$mode"
done
# Existing C++ search consumer must still compile after extraction.
"$clang" -x c++ -std=c++17 -Wall -Wextra -Werror -pedantic -fsyntax-only \
  -I../../crates/umgpu/shim ../../crates/umseed-probe/tests/probe_transport.cpp
printf 'existing probe_transport.cpp: PASS\n'
