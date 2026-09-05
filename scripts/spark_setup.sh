#!/usr/bin/env bash
set -Eeuo pipefail

# User-local DGX Spark setup. Run on the Spark from ~/uni-rnaseq.
# Deliberately does not use sudo, inspect or mutate Docker containers, or touch data/.

LOCAL="${HOME}/.local"
BIN="${LOCAL}/bin"
OPT="${LOCAL}/opt"
MAMBA_ROOT_PREFIX="${HOME}/micromamba"
MAMBA="${BIN}/micromamba"
mkdir -p "${BIN}" "${OPT}"
export PATH="${BIN}:${HOME}/.cargo/bin:/usr/local/cuda/bin:${PATH}"
export MAMBA_ROOT_PREFIX

say() { printf '\n== %s ==\n' "$*"; }
warn() { printf 'WARNING: %s\n' "$*" >&2; }

say "Rust"
if ! command -v rustup >/dev/null 2>&1; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
fi
rustup toolchain install stable --profile minimal >/dev/null
rustup default stable >/dev/null
rustc --version
cargo --version

say "Java 17+ for Nextflow"
java_major() {
  java -version 2>&1 | awk -F'[".]' '/version/ { if ($2 == "1") print $3; else print $2; exit }'
}
if ! command -v java >/dev/null 2>&1 || [ "$(java_major || echo 0)" -lt 17 ]; then
  jdk_dir="${OPT}/jdk17"
  if [ ! -x "${jdk_dir}/bin/java" ]; then
    tmp="${OPT}/jdk17.tar.gz"
    curl -fL --retry 3 -o "${tmp}" \
      'https://api.adoptium.net/v3/binary/latest/17/ga/linux/aarch64/jdk/hotspot/normal/eclipse'
    rm -rf "${jdk_dir}.tmp"
    mkdir -p "${jdk_dir}.tmp"
    tar -xzf "${tmp}" -C "${jdk_dir}.tmp" --strip-components=1
    mv "${jdk_dir}.tmp" "${jdk_dir}"
  fi
  export JAVA_HOME="${jdk_dir}"
  export PATH="${JAVA_HOME}/bin:${PATH}"
fi
java -version 2>&1 | head -n 1
if [ "$(java_major)" -lt 17 ]; then
  warn "Java >=17 could not be established; Nextflow may not run"
fi

say "micromamba"
if [ ! -x "${MAMBA}" ]; then
  tmp="${OPT}/micromamba.tar.bz2"
  curl -fL --retry 3 -o "${tmp}" 'https://micro.mamba.pm/api/micromamba/linux-aarch64/latest'
  tar -xjf "${tmp}" -C "${OPT}" bin/micromamba
  install -m 0755 "${OPT}/bin/micromamba" "${MAMBA}"
  rm -rf "${OPT}/bin"
fi
"${MAMBA}" --version

say "rnaseq environment"
if ! "${MAMBA}" env list | awk '{print $1}' | grep -qx rnaseq; then
  "${MAMBA}" create -y -n rnaseq -c bioconda -c conda-forge python=3.12
fi
packages=(star salmon samtools picard subread seqtk aria2)
missing=()
for package in "${packages[@]}"; do
  if ! "${MAMBA}" install -y -n rnaseq -c bioconda -c conda-forge "${package}"; then
    missing+=("${package}")
  fi
done
if [ "${#missing[@]}" -gt 0 ]; then
  warn "Missing from linux-aarch64 conda channels: ${missing[*]}"
  printf '%s\n' "${missing[@]}" > "${OPT}/rnaseq-missing.txt"
else
  rm -f "${OPT}/rnaseq-missing.txt"
fi
"${MAMBA}" run -n rnaseq python --version

say "Nextflow"
if [ ! -x "${BIN}/nextflow" ]; then
  tmp="${OPT}/nextflow"
  curl -sS https://get.nextflow.io -o "${tmp}"
  chmod 0755 "${tmp}"
  mv "${tmp}" "${BIN}/nextflow"
fi
"${BIN}/nextflow" -version | head -n 4

say "nvCOMP CUDA 13 linux-sbsa"
nvcomp_dir="${OPT}/nvcomp"
if [ ! -f "${nvcomp_dir}/include/nvcomp/deflate.h" ]; then
  nvcomp_url='https://developer.download.nvidia.com/compute/nvcomp/redist/nvcomp/linux-sbsa/nvcomp-linux-sbsa-5.0.0.6_cuda13-archive.tar.xz'
  tmp="${OPT}/nvcomp.tar.xz"
  if curl -fL --retry 3 -o "${tmp}" "${nvcomp_url}"; then
    rm -rf "${nvcomp_dir}.tmp"
    mkdir -p "${nvcomp_dir}.tmp"
    tar -xJf "${tmp}" -C "${nvcomp_dir}.tmp" --strip-components=1
    mv "${nvcomp_dir}.tmp" "${nvcomp_dir}"
  else
    warn "nvCOMP download failed (may require NVIDIA developer access)"
  fi
fi
if [ -f "${nvcomp_dir}/include/nvcomp/deflate.h" ]; then
  printf 'nvCOMP headers: %s\n' "${nvcomp_dir}/include/nvcomp/deflate.h"
else
  warn "nvCOMP include/nvcomp/deflate.h is missing"
fi

say "CUDA and CUB checks"
if command -v nvcc >/dev/null 2>&1; then nvcc --version | tail -n 4; else warn "nvcc not on PATH"; fi
if [ -f /usr/local/cuda/include/cub/cub.cuh ]; then
  echo 'CUB: /usr/local/cuda/include/cub/cub.cuh'
else
  warn 'CUB missing: /usr/local/cuda/include/cub/cub.cuh (requires admin install or compatible user-local CUDA headers)'
fi

say "Docker GPU smoke test"
if docker run --rm --gpus all nvidia/cuda:13.0.0-base-ubuntu24.04 nvidia-smi; then
  :
else
  warn 'Docker CUDA 13.0.0 arm64 smoke test failed; inspect image/tag or Docker runtime'
fi

say "Tool versions"
version_checks=(
  'STAR|STAR --version'
  'salmon|salmon --version'
  'samtools|samtools --version'
  'picard|picard MarkDuplicates --version 2>&1 | grep -m1 Version || true'
  'subread|featureCounts -v 2>&1 | grep -m1 featureCounts'
  'seqtk|seqtk 2>&1 | grep -m1 Version || true'
  'aria2|aria2c --version'
)
for check in "${version_checks[@]}"; do
  label="${check%%|*}"
  command_line="${check#*|}"
  printf '%-10s ' "${label}"
  if ! "${MAMBA}" run -n rnaseq bash -lc "${command_line}" 2>&1 | head -n 1; then
    warn "${label} version check failed"
  fi
done
printf '%-10s ' nextflow; "${BIN}/nextflow" -version 2>&1 | grep -m1 'version ' || true
printf '%-10s ' rustc; rustc --version
printf '%-10s ' micromamba; "${MAMBA}" --version

say "Setup complete"
if [ -s "${OPT}/rnaseq-missing.txt" ]; then
  echo "Missing conda packages recorded in ${OPT}/rnaseq-missing.txt"
fi
