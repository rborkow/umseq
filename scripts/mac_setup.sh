#!/usr/bin/env bash
set -u

# Native macOS toolchain for uni-rnaseq.  This script deliberately does not use sudo.
# It is safe to rerun: Homebrew and conda install operations are only run when needed.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONDA_BIN="${CONDA_BIN:-/Users/rborkows/miniconda3/bin/conda}"
CONDA_ENV="rnaseq"
LOCAL_BIN="${HOME}/.local/bin"
BREW_BIN="${HOMEBREW_BIN:-$(command -v brew 2>/dev/null || true)}"

log() { printf '[mac_setup] %s\n' "$*"; }
need_human=0

if [[ -z "$BREW_BIN" ]]; then
  log "Homebrew is missing; install it manually, then rerun."
  need_human=1
else
  # These are the native tools used by the Mac profile.  samtools is also
  # requested in conda below for a reproducible nf-core reference environment.
  brew_pkgs=(aria2 seqtk samtools openjdk@17 nextflow)
  missing=()
  for pkg in "${brew_pkgs[@]}"; do
    "$BREW_BIN" list --formula "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
  done
  if ((${#missing[@]})); then
    log "Installing Homebrew formulae: ${missing[*]}"
    "$BREW_BIN" install "${missing[@]}" || { log "Homebrew install failed"; need_human=1; }
  fi
fi

if [[ -x /usr/bin/xcodebuild ]] && ! xcrun -sdk macosx metal --version >/dev/null 2>&1; then
  log "Metal toolchain is absent; requesting Apple's Xcode component download"
  if ! xcodebuild -downloadComponent MetalToolchain; then
    log "MetalToolchain download failed; a human must complete it in Xcode or rerun with access."
    need_human=1
  fi
fi

if [[ ! -x "$CONDA_BIN" ]]; then
  log "Conda not found at $CONDA_BIN; conda tools are blocked."
  need_human=1
else
  if ! "$CONDA_BIN" env list | awk '{print $1}' | grep -qx "$CONDA_ENV"; then
    log "Creating conda environment $CONDA_ENV (ARM64 Bioconda + conda-forge)"
    if ! "$CONDA_BIN" create -n "$CONDA_ENV" -c bioconda -c conda-forge \
        star salmon samtools picard subread -y; then
      log "Conda solve/install failed; STAR, salmon, Picard and featureCounts are blocked."
      need_human=1
    fi
  fi
fi

# Java 17 is keg-only, so put it first for this process without modifying shell startup.
if [[ -n "$BREW_BIN" ]]; then
  brew_prefix="$($BREW_BIN --prefix 2>/dev/null || true)"
  if [[ -x "$brew_prefix/opt/openjdk@17/bin/java" ]]; then
    export PATH="$brew_prefix/opt/openjdk@17/bin:$PATH"
  fi
fi
export PATH="$LOCAL_BIN:$PATH"

version_or_missing() {
  local label="$1"; shift
  if "$@" >/tmp/uni-rnaseq-mac-setup.out 2>&1; then
    printf '%-16s %s\n' "$label" "$(awk 'NF { print; exit }' /tmp/uni-rnaseq-mac-setup.out)"
  else
    printf '%-16s MISSING/FAILED\n' "$label"
    need_human=1
  fi
}

seqtk_probe() { seqtk 2>&1 | head -n 2; return 0; }

log "Tool versions"
version_or_missing "Metal" xcrun -sdk macosx metal --version
version_or_missing "Rust" cargo --version
version_or_missing "Java" java -version
version_or_missing "Nextflow" nextflow -version
version_or_missing "seqtk" seqtk_probe
version_or_missing "aria2c" aria2c --version
version_or_missing "Homebrew samtools" samtools --version

if [[ -x "$CONDA_BIN" ]] && "$CONDA_BIN" env list | awk '{print $1}' | grep -qx "$CONDA_ENV"; then
  version_or_missing "STAR (conda)" "$CONDA_BIN" run -n "$CONDA_ENV" STAR --version
  version_or_missing "salmon (conda)" "$CONDA_BIN" run -n "$CONDA_ENV" salmon --version
  version_or_missing "samtools (conda)" "$CONDA_BIN" run -n "$CONDA_ENV" samtools --version
  picard_probe() {
    # Picard 3.x prints its version for a tool-level --version probe but exits
    # nonzero because --version is not a valid Picard subcommand.  Capture the
    # version line and treat that documented response as a successful probe.
    "$CONDA_BIN" run -n "$CONDA_ENV" picard MarkDuplicates --version 2>&1 | grep -m1 '^Version:'
    return 0
  }
  version_or_missing "picard (conda)" picard_probe
  version_or_missing "featureCounts" "$CONDA_BIN" run -n "$CONDA_ENV" featureCounts -v
else
  log "Skipping conda tool verification because $CONDA_ENV is unavailable"
fi
version_or_missing "Docker" docker --version
if docker run --rm --platform linux/arm64 alpine uname -m 2>/tmp/uni-rnaseq-docker.err | grep -qx aarch64; then
  log "Docker arm64 check: aarch64"
else
  log "Docker arm64 check failed: $(tr '\n' ' ' </tmp/uni-rnaseq-docker.err)"
  need_human=1
fi

rm -f /tmp/uni-rnaseq-mac-setup.out /tmp/uni-rnaseq-docker.err
if ((need_human)); then
  log "Setup completed with items needing human attention."
  exit 1
fi
log "Setup and verification complete."
