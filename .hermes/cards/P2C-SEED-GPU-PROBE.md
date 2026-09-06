# P2C-SEED-GPU-PROBE — Terra (CUDA + Rust) — performance probe, NOT a correctness gate

Two repos. Read first, in order:
1. `/Users/rborkows/projects/uni-rnaseq/docs/review-seed-replay.md` (why this card exists)
2. `/Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay/DESIGN.md` §"Exact
   callable boundary" (the algorithm and its four direction cases — this is your spec)
3. `/Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay/{search.cpp,index.cpp}`
   (the independent CPU implementation; you port `search`, and you reuse `index`'s packed-SA
   semantics — `GstrandBit`/`GstrandMask`, `nGenome-1-a` reverse coordinates, the
   `[-200, nGenome+199]` padded genome)
4. `/Users/rborkows/projects/uni-rnaseq/docs/design-phase2b.md` (the umem/umgpu pattern, the
   THP/ATS facts, "no cudaHostRegister", the markdup result for what a finished probe looks like)
5. `/Users/rborkows/projects/uni-rnaseq/crates/umgpu/{src/cuda.rs,shim/umgpu_shim.cu,build.rs}`
   and `crates/umem/src/lib.rs` for `Buf`, `lease`, `submit`, `Pod`.
6. `/Users/rborkows/projects/uni-rnaseq/bench/RESULTS-T6-summary.md` — the random-gather
   ceiling on this box: 2.6 G dependent lookups/s with THP, 137 ns/hop, 15 M/s on 4K pages.

## The question

Can GB10's GPU execute STAR's inner suffix-array search over the **real 30 GB resident index**
faster than 20 Arm cores running the same algorithm? Everything the seed lane has built is
correctness scaffolding for a boundary whose GPU viability is ~40% odds. This card measures
the odds. It lives in `crates/umgpu` + a new `crates/umseed-probe` binary in the **main**
repo (`/Users/rborkows/projects/uni-rnaseq`), behind `--features cuda`, and touches nothing in
the seed lane's worktree. Every artifact says "probe" in its name.

## Deliverable

### 1. Resident index loader (Rust, `crates/umseed-probe/src/index.rs`)
Load `Genome`, `SA`, `SAindex` from `~/uni-rnaseq/data/index/star_full/` on the Spark into
three `umem::Buf<Ro>` (THP, `require_huge`, HugeTLB fallback flagged). Parse
`genomeParameters.txt` for `GstrandBit`, `genomeSAindexNbases`, `nGenome`, `nSA`,
`SAiMarkNmask` etc. Preserve packed SA storage (`PackedArray.h:24–31`: `wordLength =
GstrandBit+1` bits per entry, loaded via a possibly unaligned 64-bit word — implement the
same unpack on both CPU and GPU; do **not** expand to `u64[]`). Genome padding: the lane's
`index.cpp` documents the loader-defined `[-200, nGenome+199]` allocation and sentinel
semantics; reproduce them or reject. Report bytes loaded, THP coverage (`umem`'s smaps
report), and load wall. Reject on any parameter you don't support (two-pass, sparse SA ≠ 1).

### 2. Request generator (`src/requests.rs`)
Synthetic but realistic: sample `M` reads from `ERR188140_20M_1.fastq.gz` (decode ACGT→0..3,
skip reads with N), for each read emit requests at STAR-like offsets — for `dirR=true` start
`S` ∈ {0, 20, 40, …} with `N = min(seedSearchLmax=50 default? → use P.seedSearchLmax from
STAR's defaults: 50; check `parametersDefault`), Lread−S)`; for `dirR=false` mirror from the
read end — and `L=0`, `[i1,i2]` = the SAindex prefix-lookup interval for the first
`genomeSAindexNbases` bases **only if that lookup is straightforward from `index.cpp`**;
otherwise `[0, nSA−1]` (the `SA_SEARCH_FULL` case) and *say so* — it makes the search
deeper than STAR's, which biases against the GPU, an acceptable direction for a probe.
Build `s[1]` as the position-wise complement per DESIGN.md. Emit a fixed-seed deterministic
request file so CPU and GPU consume identical input. Sizes: 64k, 256k, 1M, 4M requests.

### 3. CPU control (`src/cpu.rs`)
Port `search.cpp` (the lane's independent implementation, MIT-licensed STAR derivative —
carry `replay/LICENSE.star`) to Rust **verbatim in structure** (endpoint compare, history
pairs, midpoint formula, tie rule, `findMultRange` on each side). Rayon over requests, 20
threads, same resident `Buf`s. This is the control arm; it must be a *good* implementation
(no allocation in the loop, packed unpack inlined).

### 4. GPU probe kernel (`crates/umgpu/shim/seed_probe.cu`, separate from markdup)
One request per thread, same algorithm, reading `G` and `SA` in place through ATS.
Outputs per request: `(L_out, lo, hi, count)` to a `umem` slot. Also per-request
instrumentation in a second buffer: SA gathers performed, genome bytes compared, loop trips.
Then a second variant: **one request per warp**, lanes cooperate on `compareSeqToGenome`
(32 bytes per step, `__ballot_sync` for first mismatch) — the obvious divergence remedy;
build it only if variant one is within 3× of the CPU. Wrapper in `cuda.rs` following the
`markdup` pattern (leases, `check_lease`, fence retains everything, stub on Mac).

### 5. Gate and report (`bench/PHASE2C-seed-probe.md`, written by the orchestrator from your
`probe.tsv`)
- **Agreement**: CPU and GPU tuples identical for every request (same algorithm → this is a
  smoke test, not the oracle; the lane owns the oracle). Any mismatch = stop.
- **Throughput**: requests/s for CPU-20 and GPU at batch sizes 64k/256k/1M/4M, 3 repeats,
  warm index. Achieved GB/s (bytes gathered ÷ time) vs the T6 2.6 G-lookups/s ceiling.
  Divergence stats: mean/max compare length, mean/max loop trips, per-warp imbalance.
- **THP sensitivity**: one run with `require_huge` off (4K pages) — expected to be
  catastrophic per T6; this is the UM-specific measurement.
- **Overlap**: one run with the CPU control and GPU probe on disjoint request halves
  simultaneously, same resident index — does the box's bandwidth carry both?
- Print `zero-copy` assertion as markdup does.

**Decision rule (orchestrator applies):** GPU/CPU-20 < 1.5× at every batch size and both
kernel variants → park P2C entirely, publish the negative with the divergence data.
≥ 2× → fund REPLAY-SCALE at full rigor and a real kernel card. Between → report; user decides.

## Rules
- Format with `cargo fmt` / `clang-format`. Mac: `cargo clippy -p umseed-probe -p umgpu
  --all-targets -- -D warnings` clean without `cuda`. I compile/run on the Spark and send
  back errors verbatim (expect a round: CCCL 3 iterator paths, `Pod` for new types, `!Sync`
  `Buf<Rw>` in rayon closures — see design-phase2b.md "What it took").
- `unsafe` only in `umem` and at `umgpu`'s FFI boundary with `// SAFETY:` naming the lease.
- Don't touch `crates/umbam`, `crates/umem`, docs, bench, KANBAN, or the seed worktree. Add
  `crates/umseed-probe` to the root workspace `Cargo.toml` (you own that one line).
- Never fabricate a timing. If the loader can't hold 30 GB alongside the CPU control's
  working set within the ~89 GB general pool, say so; do not shrink the index silently.
- The lane's `search.cpp` is the spec; where DESIGN.md and `search.cpp` disagree, stop and
  report — don't pick one.

Finish with: file list, the Spark build/run commands, `probe.tsv` schema, and what you
could not verify without the GPU.
