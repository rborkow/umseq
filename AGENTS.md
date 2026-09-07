# AGENTS.md — working in this repository

This file is for coding agents and for humans pairing with them. It states how work here is
gated and what "done" means. The README explains what the project is; this explains how not
to break it.

## The one rule

**Byte-identical or documented.** Every replacement of a bioinformatics tool is gated
against that tool's actual output. A change that alters any gated output is wrong until
proven otherwise — even if the new output looks more correct. If the tool has a bug, we
reproduce the bug and record it in `crates/umbam/COMPAT.md`. Never relax a gate, filter
inputs, or normalize outputs to make a test pass.

The tools' own source is the specification. Verbatim excerpts are in `docs/tool-src/`.
Where a card, a comment, or this file disagrees with the source, the source wins, silently.
Stop only when the source and the golden disagree with each other.

## Gates

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                                # unit + trybuild (Mac and Linux)
cargo test -p umbam --release -- --ignored            # Tier 0 gates: chr22 fixture vs samtools/Picard/
                                                      #   featureCounts/bedtools/RSeQC/dupRadar/Qualimap goldens
scripts/tier1_validate.sh <sample>                    # full-depth (76M-record) diff vs nf-core outputs; Spark only
```

Tier 0 needs `samtools` on `PATH` and the fixture from `scripts/make_tier0.sh` /
`scripts/make_tier0_qc.sh` (goldens are generated from the nf-core containers, not hand-written).
CUDA features (`--features cuda`, `nvcomp`) only build on the GB10 box; the stub backend must
stay clean on Mac. A change to a GPU path is not done until the Tier 0 gate has been run with
`--gpu` on the Spark and the outputs `cmp` identical to the CPU path.

## Measurements

- Never fabricate, estimate, or "correct" a timing. Report what ran, with repeats and the
  raw rows. If a number couldn't be measured, say so.
- Time the stage, not the region between two `Instant::now()` calls that happen to bracket
  it. Two stages in this repo were mis-attributed for a day each because of this.
- Speedups are quoted against the CPU control arm (20 threads of Rust), never against the
  legacy tool. Throughput claims are CPU-minutes per sample on a saturated box; wall time is
  reported separately and is not the cost-curve number.
- `perf` before hypotheses. When a stage is slow, profile it; the last three "obvious" causes
  in this repo were wrong.
- Every result lands in `bench/*.md` with its evidence directory named. Summaries
  (`.md/.json/.tsv`) are tracked; raw binaries/traces/SAMs are gitignored and kept on the
  measurement host.

## Memory model (`umem`)

- `unsafe` lives only in `crates/umem` and at `crates/umgpu`'s FFI boundary, each with a
  `// SAFETY:` naming the lease that keeps the memory alive.
- Host memory is THP-backed with `PROT_NONE` guard pages; HugeTLB is the fallback and is
  flagged in the report. No `cudaHostRegister` (15× slower on GB10). No `cudaMalloc` for
  anything large. Zero copies is asserted, not assumed.
- `Buf<Rw>` is deliberately `!Sync`. Borrow slices once outside a rayon closure.
- A GPU kernel's output order is part of its contract: a sorted result consumed by a serial
  host `HashMap` is slower than not using the GPU. Design the consumer with the kernel.

## Process

- Work is dispatched as cards in `.hermes/cards/<ID>.md`; the board is `KANBAN.md`. A card
  says what was asked; the commit says what landed; the bench doc says what was measured.
  Where they disagree, the bench doc wins.
- One independent review per card. Reviews are adversarial and go in `docs/review-*.md`.
- Two workers on one tree at a time, on disjoint files. Say exactly which files you touched.
- Commit only separable files if another worker is mid-edit elsewhere in the tree.
- If a card's assumption is wrong (it will be — three markdup tie-break rules, two RSeQC
  filters, and one validator precondition were wrong this month), do not fudge to satisfy
  it. Stop, state the hypothesis, and let the golden decide.
- Cheap disqualifiers before rigor. Each hypothesis has a stated way to fail and a card that
  tests it before hardening is spent. Hardening a research artifact (fault injection,
  capacity matrices) waits until the measurement it protects has justified it.

## Hosts

- Spark (GB10, 20 cores, 121 GB, 32 GiB HugeTLB reserved): one heavy job at a time under
  `~/.cache/uni-rnaseq-resource.lock` (`flock`). `earlyoom` kills processes past ~110 GiB RSS.
  Long jobs run `nohup … &` with an absolute `timeout`; foreground ssh dies at ~7 minutes.
- Mac (Apple Silicon): CPU chain, gates, stub backend, cost model. No CUDA, no Metal yet.

## Don't

- Don't touch `data/`, `runs/`, or anything under `~/uni-rnaseq/` on the Spark from a worker.
- Don't rsync `--delete` over another session's tree.
- Don't expand the packed suffix array, register memory, or stage the index per batch.
- Don't claim "identical" from a mapping rate, a checksum of sorted output, or a subset of
  fields. Identical means `cmp` of the tool's output, or a documented, exact normalization.
