# Card P0-SPARK — DGX Spark toolchain for uni-rnaseq

You are setting up the toolchain on a remote NVIDIA DGX Spark (GB10, aarch64, Ubuntu 24.04,
20 cores, 121 GB unified memory, CUDA 13.0 at /usr/local/cuda, no sudo) for a project that
benchmarks RNA-seq tools on unified-memory machines. Read
`.hermes/plans/2026-09-04_uni-rnaseq-pressure-test-and-plan.md` Phase 0 first.

Access: `ssh Sparky` (alias configured, key auth, user rborkows). Work in `~/uni-rnaseq/` on
the Spark. Do everything via `ssh Sparky '<cmd>'` or by scp'ing a script and running it.

## Deliverables
1. `scripts/spark_setup.sh` — idempotent bash script (runs ON the Spark, no sudo) that
   installs/verifies everything below into `~/.local/` and `~/micromamba/`. Re-running on a
   configured machine is a no-op that prints versions.
2. `docs/env-spark.md` — table of tool → version → install method → path, plus caveats.
3. Actually run the script on the Spark and confirm every tool responds to `--version`.

## Required tools (all user-local, no sudo)
- Rust via rustup (`curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal`).
- micromamba (aarch64 binary) into `~/.local/bin`; then an env `rnaseq` from
  `-c bioconda -c conda-forge`: `star salmon samtools picard subread seqtk aria2`.
  bioconda has linux-aarch64 builds for most of these; record any that are missing and
  build those from source into `~/.local/opt/` with `-march=native`.
- `nextflow` via the official installer into `~/.local/bin` (Java is present; check ≥17).
- nvCOMP: download the CUDA 13 / linux-sbsa (aarch64) tarball from NVIDIA's developer
  site into `~/.local/opt/nvcomp/`; verify `include/nvcomp/deflate.h` exists. If the
  download needs a login, note it under "Needs human".
- Verify `nvcc --version` and that `/usr/local/cuda/include/cub/cub.cuh` exists.
- Check `docker run --rm --gpus all nvidia/cuda:13.0.0-base-ubuntu24.04 nvidia-smi` works
  (docker group membership exists). If the image tag is wrong, find the right 13.x
  arm64 tag.

## Constraints
- Do not touch docker containers that already exist on the Spark (there is a stopped
  `qwen38-flash` container; leave it alone).
- Do not modify anything under `~/uni-rnaseq/data/` on the Spark — a reference download is
  running there. Do not modify `bench/` or `.hermes/` in this repo.
- Do not `git commit`.
- A process named `earlyoom` kills anything pushing memory past ~110 GB; keep builds to
  `-j8` or less.
- Anything needing sudo → list under "Needs human" in `docs/env-spark.md`.

Finish with a short summary: what's installed, what's blocked, what needs the human.
