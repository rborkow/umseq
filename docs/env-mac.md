# macOS development environment

Last verified: 2026-09-04 on Apple arm64, macOS 27.0, 24 GB.

| Tool | Version | Install method | Path |
|---|---|---|---|
| Xcode Metal toolchain | Apple metal 32023.921 (metalfe-32023.921.5) | Xcode component `MetalToolchain` | Xcode toolchain via `xcrun` |
| Rust/Cargo | 1.98.0 | Already installed by Homebrew; not reinstalled | `/opt/homebrew/bin/cargo` |
| Java | 17.x | Homebrew `openjdk@17` | `/opt/homebrew/opt/openjdk@17/bin/java` |
| Nextflow | 26.04.6 | Homebrew | `/opt/homebrew/bin/nextflow` |
| seqtk | 1.5 | Homebrew | `/opt/homebrew/bin/seqtk` |
| aria2c | 1.37.0 | Homebrew | `/opt/homebrew/bin/aria2c` |
| samtools (native) | 1.24 | Homebrew | `/opt/homebrew/bin/samtools` |
| STAR | 2.7.11b | `/Users/rborkows/miniconda3/bin/conda create -n rnaseq -c bioconda -c conda-forge` | `/Users/rborkows/miniconda3/envs/rnaseq/bin/STAR` |
| salmon | 2.7.0 | Same ARM64 Bioconda environment | `/Users/rborkows/miniconda3/envs/rnaseq/bin/salmon` |
| samtools (reproducible env) | 1.22.1 | Same ARM64 Bioconda environment | `/Users/rborkows/miniconda3/envs/rnaseq/bin/samtools` |
| Picard | 3.5.0 | Same ARM64 Bioconda environment | `/Users/rborkows/miniconda3/envs/rnaseq/bin/picard` |
| featureCounts (Subread) | 2.1.1 | Same ARM64 Bioconda environment | `/Users/rborkows/miniconda3/envs/rnaseq/bin/featureCounts` |
| Docker Desktop | 29.4.0 | Already installed; not reinstalled | `/usr/local/bin/docker` |

## Notes and caveats

- The requested ARM64 Bioconda solve succeeded. No source builds or `~/.local/opt` symlinks
  were needed; STAR is above the required 2.7.11 and salmon is above 1.10.
- `openjdk@17` is keg-only. The setup script prepends its bin directory for its process and
  does not edit shell startup files. For interactive Nextflow use, add
  `/opt/homebrew/opt/openjdk@17/bin` to `PATH` or set `JAVA_HOME` yourself; linking the JDK
  into `/Library/Java/JavaVirtualMachines` would require sudo and was not attempted.
- Docker validation is `docker run --rm --platform linux/arm64 alpine uname -m` and must print
  `aarch64`. Docker is for nf-core Linux containers only; GPU work remains native on macOS.
- The script never invokes sudo or GUI actions. If the Metal component download cannot run,
  complete it in Xcode or rerun `xcodebuild -downloadComponent MetalToolchain` as a human.
- The initial verification found no system Java and no Metal toolchain; both are handled by the
  setup script. Homebrew warned that macOS 27 is a pre-release/unsupported platform.
- Docker CLI is installed, but this host currently has the `orbstack` context selected and its
  socket is not running. The required `alpine uname -m` check therefore remains blocked until
  a human starts Docker Desktop and selects its `desktop-linux` context.

## Container engine: Apple `container` (preferred over Docker/OrbStack)

`container` CLI 1.0.0 at `/usr/local/bin/container`. Start with `container system start`.
Nextflow ≥ 26.04 supports it natively — config scope is **`appleContainer`** (not `apple`):
```
appleContainer.enabled = true
```
Verified 2026-09-04: Nextflow emits `container run -i --cpus N -v work:work -w ... image /bin/bash -ue .command.sh`
and an `ubuntu:24.04` task returns `aarch64`. Images need `/bin/bash` (alpine fails with 127).
nf-core/rnaseq `-profile test,arm` smoke run via Apple container: see KANBAN P0-NFCORE-SMOKE.
GPU work never runs in containers on macOS — native processes only.
