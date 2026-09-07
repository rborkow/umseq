# P2C-STAR-INTEGRATE-1-FIX — Terra — close the seven blocking findings, restore the build

Workdir `/Users/rborkows/projects/uni-rnaseq` (published as github.com/rborkow/umseq; `main`
must build). Read in order: `AGENTS.md`; `docs/review-star-integrate-1.md` (the spec for this
card — seven blocking findings + two should-fix, each with file:line and a required test);
`bench/star-integrate/CONTRACT.md`; `bench/star-integrate/IMPLEMENTATION.md`;
`docs/STAR-INTEGRATE-DESIGN.md` §"Exact dependency trace" and §(a).

## State you are inheriting

The previous orchestrator stopped mid-refactor after the review. Untracked, not building:
- `crates/umstar/` — `Cargo.toml` present, `src/` **empty**. It was the `libumstar.a` facade.
- `crates/umgpu/ffi/star_integrate.rs` — 685 lines, the moved implementation of
  `usi_init_v1` / `usi_search_batch_v1` / `usi_destroy_v1`. Not referenced by any build.
- `crates/umgpu/shim/seed_probe_abi.h` — the fixed-width transport records.
- `bench/star-integrate/` — the private STAR coordinator (`star_integrate*.cpp/hpp`,
  `usi.h`), generator `make_star_integrate.py`, host runner `run_host.py`, unit tests.
`crates/umstar` was removed from the workspace members so `main` builds; you put it back
when it builds.

## Deliverable

1. **Restore the build.** Decide the crate layout (simplest: `crates/umstar/src/lib.rs` as a
   thin `include!("../../umgpu/ffi/star_integrate.rs")` facade, or move the file back — your
   call, one sentence of rationale). Re-add `crates/umstar` to the workspace. `cargo clippy
   --workspace --all-targets -- -D warnings` and `cargo test --workspace` clean on Mac (stub
   backend returns `USI_GPU`, never synthesizes tuples). `cargo fmt --all`.

2. **Close every blocking finding in the review, in its numbered order**, each with the test
   the review names, RED then GREEN, recorded in `bench/star-integrate/TDD.md`:
   1. admission capped at `min(Nstart, 2)`, lookup independently rejects `istart >= 2`;
   2. coordinator made linear: ordered frame cursor, frame-local job ranges, frame-offset
      map; **visit counters** proving linear growth in W and J on the real coordinator
      (not the toy in `test_coordinator.cpp`);
   3. strict mode fails closed on malformed GPU successes, including backend containment
      against `q.low/q.high`; inject out-of-range and wrong-in-bounds successes;
   4. the frozen identity key implemented and compared at `begin_map` and lookup —
      generation, piece bounds, mate/split context, call kind; test same-ordinal-second-map,
      changed bytes, changed index, colliding keys;
   5. work attribution partitioned: CPU fallback bytes/gathers separate from strict-oracle;
      submitted GPU stats split consumed / suppressed-unused / other-unused; backend
      event/wall values retained; CHAIN denominators (13,435,368 / 159,989,084) referenced;
   6. per-window and aggregate pending byte/request caps with CPU backpressure before
      enqueue; multi-worker/underfill/empty-tail/retiring-worker tests under a bounded
      timeout;
   7. `gpu_hits` → `gpu_consumed` (or the reverse — make emitter and `run_host.py` agree)
      and a local test of the emitted row against the gate assertions.
   Plus the two should-fix items (stream restoration fails closed; FFI overlap checks before
   mutation).

3. **Do not** touch the search kernel, the transport (`umgpu_seed_probe`), `umem`,
   `umbam`, or the accepted parity checker `seed_split_parity.py`. Do not reopen held cards.
   No SSH, no commits, no host runs — the orchestrator stages to the Spark and runs gate (i).

Finish with: file list, test counts before/after, the visit-counter table for finding 2, and
the exact Spark build + gate-(i) command from `run_host.py`. If a finding can't be closed
without changing the contract, stop at that finding and say precisely why.
