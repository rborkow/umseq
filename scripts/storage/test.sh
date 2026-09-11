#!/usr/bin/env bash
# Self-test for tier-promote / tier-archive / tier-restore against a throwaway dir.
# Usage: scripts/storage/test.sh [--s3]   (--s3 also exercises archive/restore against rustfs:artifacts/.selftest)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/tier-common"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/tiertest.XXXX"); trap 'rm -rf "$tmp"' EXIT
d="$tmp/selftest-$(date +%s)"; mkdir -p "$d/sub"
echo alpha > "$d/a.txt"; echo beta > "$d/sub/b.txt"; head -c 1048576 /dev/urandom > "$d/blob.bin"

"$here/tier-promote" "$d" _selftest
[ -L "$d" ] || { echo "FAIL: no symlink"; exit 1; }
dst=$(readlink "$d"); check_manifest "$dst" || { echo "FAIL: manifest at dst"; exit 1; }
[ "$(cat "$d/sub/b.txt")" = beta ] || { echo "FAIL: read through symlink"; exit 1; }
ls "$LLM_WORK/_trash" | grep -q "$(basename "$d")" || { echo "FAIL: original not in _trash"; exit 1; }
"$here/tier-promote" "$d" _selftest | grep -q "already promoted" || { echo "FAIL: idempotence"; exit 1; }
printf 'x' | dd of="$dst/blob.bin" bs=1 seek=100 conv=notrunc status=none
if check_manifest "$dst" 2>/dev/null; then echo "FAIL: corruption not detected"; exit 1; fi
printf 'x' | dd of="$dst/blob.bin" bs=1 seek=100 conv=notrunc status=none  # (leave corrupt; rewrite manifest for archive test)
write_manifest "$dst"
echo "promote: PASS"

if [ "${1:-}" = --s3 ]; then
  "$here/tier-archive" "$dst" artifacts/.selftest
  grep -q archived "$dst/ARCHIVED.txt" || { echo "FAIL: ARCHIVED.txt"; exit 1; }
  if "$here/tier-archive" "$dst" artifacts/.selftest 2>/dev/null; then echo "FAIL: overwrite allowed"; exit 1; fi
  "$here/tier-restore" "artifacts/.selftest/$(basename "$dst")" "$tmp/restore"
  cmp "$tmp/restore/$(basename "$dst")/blob.bin" "$dst/blob.bin" || { echo "FAIL: restore differs"; exit 1; }
  rclone purge "rustfs:artifacts/.selftest"
  echo "archive/restore: PASS"
fi
rm -rf "$dst" "$LLM_WORK/_trash/$(basename "$d")".* ; rmdir "$LLM_WORK/_selftest" 2>/dev/null || true
echo "ALL PASS"
