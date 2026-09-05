# Card P0-MAC — Mac toolchain for uni-rnaseq

You are setting up the development toolchain on this Apple M4 Pro (macOS 27, 24 GB) for a
project that benchmarks RNA-seq tools on unified-memory machines. Read
`.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md` Phase 0 first.

## Deliverables
1. `scripts/mac_setup.sh` — idempotent bash script that installs/verifies everything below.
   Re-running on a configured machine must be a no-op that prints versions.
2. `docs/env-mac.md` — table of tool → version → install method → path, plus any caveats
   you hit (e.g. tools that had to be built from source, missing bioconda osx-arm64 packages).
3. Actually run the script here and confirm every tool responds to `--version`/equivalent.

## Required tools
- Xcode Metal toolchain: `xcodebuild -downloadComponent MetalToolchain`; verify with
  `xcrun -sdk macosx metal --version`.
- Rust: already present via Homebrew (`cargo 1.98`). Just verify; do not reinstall.
- `nextflow` (Homebrew or the official installer; needs Java 17+ — `java` exists, check version).
- `seqtk`, `aria2c`, `samtools` (Homebrew).
- `STAR` ≥ 2.7.11 and `salmon` ≥ 1.10: try `conda create -n rnaseq -c bioconda -c conda-forge
  star salmon samtools` (conda at /Users/rborkows/miniconda3). If osx-arm64 packages are
  missing, build from source into `~/.local/opt/` with clang and symlink to `~/.local/bin/`.
  Record which path you took. STAR from source: `cd source && make STARforMacStatic CXX=clang++`.
- `picard` and `subread` (featureCounts): conda if available, else note as blocked.
- Docker Desktop is present; verify `docker run --rm --platform linux/arm64 alpine uname -m`
  prints `aarch64` (needed for nf-core containers; GPU work will NOT use Docker).

## Constraints
- Do not modify anything under `bench/` or `.hermes/`.
- Do not `git commit`.
- Do not touch the DGX Spark (host `Sparky`) — another worker owns it.
- If something requires sudo or a GUI interaction, do not attempt it; list it under
  "Needs human" in `docs/env-mac.md`.
- Keep total new disk use under 10 GB.

Finish with a short summary: what's installed, what's blocked, what needs the human.
