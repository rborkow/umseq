#!/usr/bin/env bash
# PROBE host rounds only. Orchestrator must wait for SPLIT and approve inner scope.
set -euo pipefail
PROBE_CUTOFF=1788750000 # 2026-09-06 20:00:00 America/Los_Angeles
PROBE_REPO=/home/rborkows/uni-rnaseq
PROBE_LOCK=/home/rborkows/.cache/uni-rnaseq-resource.lock
PROBE_PAGES=${1:?PROBE usage: probe-spark.sh huge|small4k}
case "$PROBE_PAGES" in huge|small4k) ;; *) echo 'PROBE unsupported page mode' >&2; exit 2;; esac
PROBE_VARIANT=${PROBE_VARIANT:-thread}
case "$PROBE_VARIANT" in thread|warp) ;; *) echo 'PROBE unsupported variant' >&2; exit 2;; esac
export PROBE_VARIANT
PROBE_REMAINING=$((PROBE_CUTOFF-$(date +%s)-1))
if (( PROBE_REMAINING <= 0 )); then echo 'PROBE cutoff reached' >&2; exit 2; fi
if [[ ${PROBE_UNDER_LOCK:-0} != 1 ]]; then
    export PROBE_UNDER_LOCK=1
    exec flock -n "$PROBE_LOCK" timeout --signal=KILL "${PROBE_REMAINING}s" bash "$0" "$PROBE_PAGES"
fi
: "${PROBE_SPLIT_EVIDENCE:?PROBE requires absolute path to completed SPLIT evidence}"
: "${PROBE_SPLIT_SCOPE:?PROBE requires orchestrator decision: inner}"
[[ "$PROBE_SPLIT_SCOPE" == inner ]] || { echo 'PROBE STOP: direct-extension boundary requires implementation before final measurement' >&2; exit 2; }
[[ "$PROBE_SPLIT_EVIDENCE" == /* && -f "$PROBE_SPLIT_EVIDENCE" ]] || exit 2
cd "$PROBE_REPO"
# Every round builds from the current reviewed sources into a fresh root.
mkdir -p "$PROBE_REPO/runs"
PROBE_ROOT=$(mktemp -d "$PROBE_REPO/runs/PROBE-seed-${PROBE_PAGES}-XXXXXXXX")
exec > >(tee "$PROBE_ROOT/probe-host.log") 2>&1
printf 'PROBE_ROOT=%s\nPROBE_CUTOFF=%s\n' "$PROBE_ROOT" "$PROBE_CUTOFF"
date --iso-8601=seconds
uname -a
rustc --version
cargo --version
/usr/local/cuda/bin/nvcc --version
git rev-parse HEAD
git status --short
git diff --binary > "$PROBE_ROOT/probe-source.diff"
# Include untracked implementation files in exact provenance as well as HEAD/diff.
find crates/umseed-probe crates/umgpu -type f -not -path '*/target/*' -print0 | sort -z | xargs -0 sha256sum > "$PROBE_ROOT/probe-source-sha256.txt"
sha256sum Cargo.toml Cargo.lock >> "$PROBE_ROOT/probe-source-sha256.txt"
cp "$PROBE_SPLIT_EVIDENCE" "$PROBE_ROOT/probe-split-evidence.txt"
PROBE_SPLIT_HASH=$(sha256sum "$PROBE_SPLIT_EVIDENCE" | cut -d' ' -f1)
nvidia-smi --query-gpu=name,clocks.current.sm,memory.used,power.draw --format=csv
export CARGO_TARGET_DIR="$PROBE_ROOT/probe-target"
cargo build --offline --release --locked -p umseed-probe --features cuda
PROBE_BIN="$CARGO_TARGET_DIR/release/umseed-probe"
sha256sum "$PROBE_BIN" > "$PROBE_ROOT/probe-binary-sha256.txt"
if [[ -z ${PROBE_REQUESTS:-} ]]; then
    [[ "$PROBE_PAGES" == huge ]] || { echo 'PROBE 4K must reuse the huge round request file' >&2; exit 2; }
    : "${PROBE_FASTQ:?PROBE set the absolute real ERR188140_20M_1.fastq.gz path}"
    [[ "$PROBE_FASTQ" == /* && -f "$PROBE_FASTQ" && $(basename "$PROBE_FASTQ") == ERR188140_20M_1.fastq.gz ]] || exit 2
    PROBE_REQUESTS="$PROBE_ROOT/probe-requests.bin"
    "$PROBE_BIN" generate --fastq "$PROBE_FASTQ" --index "$PROBE_REPO/data/index/star_full" \
        --output "$PROBE_REQUESTS" --count 4000000 --seed 188140
fi
[[ "$PROBE_REQUESTS" == /* && -f "$PROBE_REQUESTS" ]] || exit 2
sha256sum "$PROBE_REQUESTS" > "$PROBE_ROOT/probe-request-sha256.txt"
cp "${PROBE_REQUESTS%.*}.probe-provenance.txt" "$PROBE_ROOT/probe-request-provenance.txt"
if [[ "$PROBE_PAGES" == small4k ]]; then
    : "${PROBE_HUGE_AGREEMENT:?PROBE requires prior successful huge agreement TSV}"
    [[ "$PROBE_HUGE_AGREEMENT" == /* && -f "$PROBE_HUGE_AGREEMENT" ]] || exit 2
    # Orchestrator attests completion; a partial TSV is not sufficient.
    grep -q '^# PROBE COMPLETE agreement' "$PROBE_HUGE_AGREEMENT"
    sha256sum "$PROBE_HUGE_AGREEMENT" > "$PROBE_ROOT/probe-huge-agreement-sha256.txt"
    PROBE_COUNTS=64000
else
    PROBE_COUNTS=64000,256000,1000000,4000000
fi
"$PROBE_BIN" run --index "$PROBE_REPO/data/index/star_full" --requests "$PROBE_REQUESTS" \
    --output "$PROBE_ROOT/probe.tsv" --variant "$PROBE_VARIANT" --pages "$PROBE_PAGES" \
    --counts "$PROBE_COUNTS" --repeats 3 --overlap \
    --split-provenance "inner-only orchestrator decision; evidence=$PROBE_SPLIT_EVIDENCE;sha256=$PROBE_SPLIT_HASH" \
    --cutoff-unix "$PROBE_CUTOFF"
nvidia-smi --query-gpu=name,clocks.current.sm,memory.used,power.draw --format=csv
date --iso-8601=seconds
printf 'PROBE finished: %s/probe.tsv\n' "$PROBE_ROOT"
