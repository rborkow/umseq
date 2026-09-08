#!/usr/bin/env bash
# T5B part 1: three-arm borrowed-index probe on the real request corpus. Sequential, under the lock.
set -u
O=$HOME/uni-rnaseq-probe-lab; SRC=$O/integrate-source-v1; OUT=$O/probe-borrowed; mkdir -p "$OUT"
export PATH=/usr/local/cuda/bin:$HOME/.cargo/bin:$PATH
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
cd "$SRC"
CARGO_TARGET_DIR=$O/integrate-gate-host17/target cargo build -q -p umseed-probe --release --features cuda --bin probe_borrowed 2>"$OUT/build.err" || { echo BUILD-FAIL; cat "$OUT/build.err" | tail -20; echo PROBE-DONE; exit 1; }
BIN=$O/integrate-gate-host17/target/release/probe_borrowed
for arm in umem borrowed borrowed-madvise; do
  echo "=== $arm ==="
  sync; echo 3 | sudo -n tee /proc/sys/vm/drop_caches >/dev/null 2>&1 || true
  /usr/bin/time -f "time %e s wall %U user %S sys %M KiB maxrss" "$BIN" \
    --index /home/rborkows/uni-rnaseq/data/index/star_full \
    --requests "$O/real-requests-host2/real-requests.bin" \
    --config "$O/integrate-gate-host11.prefix-config.bin" \
    --"$arm" 2>&1 | grep -vE "PROBE allocation" | tee "$OUT/$arm.txt"
done
echo PROBE-DONE
