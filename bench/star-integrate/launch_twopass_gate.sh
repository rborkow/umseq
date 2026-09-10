#!/usr/bin/env bash
# Build the TWOPASS-LIFECYCLE candidate from a fresh staged source root, then run the
# production-argv gate (stock reused from production-gate-integrated-20260908/stock, which is
# byte-verified stock output under this argv). Backend libumstar.a is the accepted lat-close
# artifact (unchanged by this card).
set -u
lab="$HOME/uni-rnaseq-probe-lab"
src="$lab/integrate-source-twopass-20260908"       # staged by the orchestrator (git archive)
private="$lab/integrate-private-twopass-20260908"
out="$lab/production-gate-twopass-20260908"
lib="$lab/integrate-gate-lat-close-20260908/target/release/libumstar.a"
tooling="$lab/integrate-source-lat-close-20260908/tooling/replay"   # per the accepted gate's preparation.json
here="$src/bench/star-integrate"
PINNED="$HOME/uni-rnaseq-seed-lab/full-source-UVdsuH/STAR-2.7.11b/source"
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
for name in ${!STAR_INTEGRATE@}; do unset "$name"; done
unset MAKEFLAGS MFLAGS CFLAGS CPPFLAGS CXXFLAGS LDFLAGS
export PATH="$HOME/.cargo/bin:/usr/local/cuda/bin:$PATH"
status() { printf '%s\n' "$1" > "$lab/twopass-gate.status"; }
status prepare
python3 -B "$here/make_star_integrate.py" --tooling "$tooling" --source "$PINNED" --private-root "$private" || { status FAILED_prepare; exit 1; }
grep -c "star_integrate::rearm" "$private/integrated/STAR.cpp" "$private/integrated/twoPassRunPass1.cpp"
status build
make -j8 STAR CXX="$(command -v g++)" CC="$(command -v gcc)" \
  "CXXFLAGSextra=-DSTAR_INTEGRATE=1 -I$here" \
  "LDFLAGSextra=$lib -L/usr/local/cuda/lib64 -Wl,-rpath,/usr/local/cuda/lib64 -lcudart -ldl -lm -lrt" \
  CXXFLAGS_SIMD= BUILD_DATE=twopass-private BUILD_PLACE=twopass-private \
  -C "$private/integrated" > "$lab/twopass-build.log" 2>&1 || { status FAILED_build; exit 1; }
sha256sum "$private/integrated/STAR"
status gate
export STAR_INTEGRATE_DROP_INDEX_CACHE=1
timeout --signal=TERM --kill-after=30s 10800s python3 -B "$lab/production-gate-runner-20260908/run_production_gate.py" \
  --stock "$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR" \
  --integrated "$private/integrated/STAR" \
  --argv-template "$lab/production-gate-runner-20260908/nfcore_star_argv.json" \
  --genome-dir "$HOME/uni-rnaseq/data/index/star_full" \
  --sjdb-gtf "$HOME/uni-rnaseq/runs/tier1-full/work/73/514e12d4643c95e65a62c68a927f7b/gencode.v49.primary_assembly.annotation.filtered.gtf" \
  --mate1 "$HOME/uni-rnaseq/data/samples/ERR188140_20M/ERR188140_20M_1.fastq.gz" \
  --mate2 "$HOME/uni-rnaseq/data/samples/ERR188140_20M/ERR188140_20M_2.fastq.gz" \
  --samtools "$HOME/micromamba/envs/rnaseq/bin/samtools" \
  --output "$out" --timeout-s 3600 --compare namesorted-sam
rc=$?
status "GATE-EXIT $rc"
printf 'TWOPASS-GATE-EXIT %s\n' "$rc"
