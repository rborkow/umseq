# Independent adversarial review — P2C-INTEGRATE-V2-T4R

2026-09-07. Independent read-only reviewer `/root/review_t4`; no files edited
by the reviewer. Review covered the changed CUDA/header/ABI, Rust records,
CUDA/stub launch boundary, star_prefix owning/C boundary, and synthetic
chain oracle/harness against STAR 2.7.11b mapOneRead:40–93,
maxMappableLength2strands and storeAligns. A second pass covered search_timed
and the optional CUDA same-step V2/thread/warp benchmark.

No blocking source correctness finding. The reviewer confirmed direction
encoding, reverse shift, strict seedMapMin inequality, the intentionally
piece-relative flag condition, capacity checked after terminal condition,
whole-chain invalidation on rejection, 88/56/472-byte layouts, V2 signature
compatibility, complete-warp geometry and uniform collective control flow.
Raw pointer checks cover supplied extents, overflow, writable overlap and
batch lease context under the unsafe caller lifetime contract. Flattened V2
requests retain residual length/shift, including rejected-chain prefixes;
stats aggregate consistently and five repeats rotate arm order after warm-up.

Three review recommendations were applied:

1. Ordinary-table coverage assertions are unconditional, including an explicit
   flag-true assertion. The prior conditional could hide lost flag coverage.
2. Raw-host API docs and handoff explicitly require STAR G-200 and nGenome+400
   readable bytes, plus the final packed-SA load extent.
3. Benchmark rows label the ordinary/missing-A datasets so raw groups are
   distinguishable.

The review is not CUDA compilation or runtime evidence. Device grid, raw-host
execution, strict 20M per-step/Read1 parity, Tier 0 GPU/CPU cmp, and gathers/s
remain pending Spark. Mac test results are in
`bench/evidence/integrate-v2-t4-mac/validation.json`.
