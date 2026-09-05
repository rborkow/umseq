# DGX Spark environment

Setup target: `spark-05b5` (aarch64, Ubuntu 24.04, CUDA 13.0), configured by
[`scripts/spark_setup.sh`](../scripts/spark_setup.sh). The script is user-local and
does not use sudo, alter existing containers, or touch `~/uni-rnaseq/data/`.

| Tool | Version verified | Install method | Path / invocation |
|---|---:|---|---|
| Rust / rustup | rustc 1.98.1, cargo 1.98.1 | official rustup minimal profile | `~/.cargo/bin/`; `rustup` |
| micromamba | 2.9.0 | official linux-aarch64 binary | `~/.local/bin/micromamba` |
| STAR | 2.7.3a | bioconda linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/STAR` |
| salmon | 2.7.0 | bioconda linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/salmon` |
| samtools | 1.24 | bioconda linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/samtools` |
| Picard | 3.5.0 | bioconda noarch/linux-aarch64 dependencies in `rnaseq` | `~/micromamba/envs/rnaseq/bin/picard` |
| Subread / featureCounts | 2.1.1 | bioconda linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/featureCounts` |
| seqtk | 1.5-r133 | bioconda linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/seqtk` |
| aria2 | 1.37.0 | conda-forge linux-aarch64 in `rnaseq` | `~/micromamba/envs/rnaseq/bin/aria2c` |
| Nextflow | 26.04.6 | official `get.nextflow.io` installer | `~/.local/bin/nextflow` |
| Java | OpenJDK 17.0.20.1 | Eclipse Adoptium user-local archive | `~/.local/opt/jdk17/bin/java` |
| nvCOMP | 5.0.0.6 CUDA 13 linux-sbsa | NVIDIA developer download | `~/.local/opt/nvcomp/` |
| nvcc | CUDA 13.0, V13.0.88 | preinstalled system CUDA | `/usr/local/cuda/bin/nvcc` |

## Verification

The script was run twice on the Spark. The second run reported the conda packages
already installed and re-verified versions. The CUDA Docker smoke test passed with
`nvidia/cuda:13.0.0-base-ubuntu24.04`, NVIDIA-SMI 580.173.02, and an NVIDIA GB10.
The pre-existing stopped `qwen38-flash` container was not touched.

Some upstream executable names differ from package names: use `STAR`,
`featureCounts`, and `aria2c`; Picard’s version check is
`picard MarkDuplicates --version`; seqtk prints its version in its usage output.

## Caveats / Needs human

- `/usr/local/cuda/include/cub/cub.cuh` is absent, and no CUB header exists elsewhere
  under `/usr/local/cuda`. The script cannot repair a system CUDA installation without
  sudo. Install the matching CUDA CUB headers or provide an approved user-local CUDA
  include tree before compiling CUB-dependent kernels.
- Java 8 was preinstalled; the script therefore installed a user-local OpenJDK 17.
- No requested bioconda/conda-forge package was missing on this Spark, so no source
  builds were needed. If a future solve fails, the script records package names in
  `~/.local/opt/rnaseq-missing.txt`; source builds should remain capped at `-j8` due
  to `earlyoom`.
- No sudo action was required. The downloaded nvCOMP archive was NVIDIA’s public
  CUDA 13 linux-sbsa artifact; if NVIDIA later gates that URL behind login, a human
  must download it and place/extract it at `~/.local/opt/nvcomp/`.

## HugeTLB pool (set by human 2026-09-04, does not persist across reboot)

```
sudo -S -p '' sysctl vm.nr_hugepages=16384          # 32 GiB of 2 MiB pages, reserved
sudo -S -p '' sysctl vm.hugetlb_shm_group=$(id -g)  # only needed for SysV shm; anonymous MAP_HUGETLB works without it
```
Verified: `HugePages_Total: 16384`, unprivileged `mmap(MAP_HUGETLB|MAP_HUGE_2MB)` of 1 GiB succeeds.
Note: reserved pages are unavailable to other processes — effective general memory is ~89 GB while
the pool is empty. vLLM's 100 GB container will not fit alongside it.
To persist: add both lines (without `sudo sysctl`) to `/etc/sysctl.d/90-hugepages.conf`.

## nf-core on the Spark (verified 2026-09-05)

`nf-core/rnaseq -r 3.26.0 -profile test,arm64` with Docker: 181 tasks, completed successfully.
Required config (biocontainers run as root; STAR's `_STARgenome` cleanup then fails on the
host-owned workdir with `AccessDeniedException`):
```
docker.enabled = true
docker.runOptions = "-u 1000:1000"
```
Run dir: `~/uni-rnaseq/runs/nfcore-smoke/`.

## GPU clocks

Idle GPU sits at 208 MHz / ~5 W and takes ~100 ms of load to reach 2411 MHz. First-run numbers
are unrepresentative; warm for ~1 s before timing. Log `nvidia-smi --query-gpu=clocks.sm,power.draw`
before/after every measurement.
