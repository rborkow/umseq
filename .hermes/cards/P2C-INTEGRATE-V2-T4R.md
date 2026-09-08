# P2C-INTEGRATE-V2-T4R — Astra — resume T4 with the measured chain-length distribution

Workdir `/Users/rborkows/projects/uni-rnaseq`. Your T4 stop was correct: CHAIN-POSITION had no
per-chain distribution and the capture has no chain identity. Both findings are recorded
(`bench/PHASE2C-integrate-v2-t4.md`, commit `<see git log>`). The prerequisite has now been
**measured**, not authorized:

## Measured (20M, integrated V2 enabled, sidecar `chain_length_histogram`; evidence
`bench/evidence/integrate-1-host/chain-length-histogram-20M.json`)

Per-chain count of **all** `maxMappableLength2strands` calls of one `(read, piece, dir, istart)`
chain, including prefix-only/unique steps (counted at `set_chain`, flushed at chain start and
read boundary; 146,553,716 chains, 201,733,339 outer calls):

| len | chains | cumulative |
|---|---|---|
| 1 | 113,648,373 | 77.547% |
| 2 | 16,689,937 | 88.936% |
| 3 | 11,612,145 | 96.859% |
| 4 | 3,154,247 | 99.011% |
| 5 | 1,442,415 | 99.9955% |
| 6 | 6,599 | 100.000% |
| ≥7 | **0** | — |

Max observed 6. Continuation steps = 55.2M = 27.4% of outer calls (CHAIN-POSITION's 24% of
gathers is consistent). Denominator: executed chains only; suppressed reverse chains are not in
the histogram (they are never executed). Input: ERR188140 20M, admitted profile.

**Capacity: 8 steps.** Covers every observed chain with two of headroom; a chain exceeding 8
returns a distinct status (`chain_overflow`), falls back to CPU, and is counted — the strict gate
will show the count (expected 0). Not a percentile claim: the distribution has no tail.

## Everything else in `P2C-INTEGRATE-V2-T4.md` stands, with these amendments

- Item 4 (real replay chain mode): **dropped** per your finding. The strict 20M gate (stock body
  per step, four outputs + `Read1`) is the real-chain proof. The synthetic-grid chain oracle
  (item 3) is the pre-Spark proof.
- Add to your deliverable (small, same file set): a **raw-host-pointer launch** at the `umgpu`
  FFI boundary — the existing V2 `seed_probe_v2` takes `umem` leases; add a sibling that takes
  `(ptr, len)` for G/SA/SAi with a `// SAFETY:` naming the caller's index lifetime. No other
  `unsafe`. Terra's borrowed-index probe (T5 item B) is blocked on exactly this; expose it and
  state the signature in your report.
- Ownership unchanged: kernel, ABI, `crates/umgpu/src/*`, `star_prefix.rs`, `crates/umstar/**`,
  `prefix_config.hpp`. Terra's files untouched; handoff as exact text.

Same finish: fmt/clippy/test, clang-format, `TDD.md`, report with exact ABI and Terra handoff.
