# P2C-INTEGRATE-2-COORD — Terra #2 — asynchronous coordinator: workers never block on the GPU (you own the consumer side)

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, `bench/PHASE2C-integrate-1.md`
(both rounds), `docs/STAR-INTEGRATE-DESIGN.md` §"Batch and latency budget", then
`bench/star-integrate/star_integrate.cpp` in full (782 lines). **Another worker is concurrently
editing the producer side** (`star_integrate_window.cpp`, `star_integrate_work.*`,
`make_star_integrate.py`, `crates/umgpu/ffi/star_integrate.rs`). **You own `star_integrate.cpp`
only.** Interface changes go in `star_integrate.hpp` as appended declarations, stated exactly in
your report. Do not touch the other worker's files; if you need a producer-side change, write
it as a one-paragraph request.

## Measured

Round 2: GPU arm mapping wall 62 s vs stock 47 s while CPU-s are equal — **20 workers spend
~25% of the run blocked.** Sidecar: 1,481 of 1,657 batches are the 64k minimum; 14.5M requests
went to CPU tails via the 2 ms underfill rule; `coordinator_main` 1.5% self, `__aarch64_cas4_acq`
3.8% (one global `s.mu` + `cv`, taken per window submit *and per frame identity*). The
`USI` call is synchronous: `submit → drain → copy back → notify`, one batch in flight ever.

## What to build

**Double-buffered, asynchronous batches; producers never wait for the GPU.**

1. **Two (or N) in-flight batches.** While batch *k* is on the GPU, batch *k+1* fills. The
   backend already returns only after the stream drains; keep that call but run it on the
   coordinator thread against the *previous* buffer while accepting new frames into the
   next. No worker thread ever calls into USI or waits on a batch fence.
2. **Lookup never blocks — it either hits a completed result or falls back to CPU
   immediately.** Today `lookup` is already non-waiting; keep that. What changes is that
   results arrive earlier because the drain overlaps filling.
3. **Per-worker submission queues, lock-free hand-off.** Each mapping thread appends frames
   to its own SPSC queue; the coordinator drains all 20. `s.mu` should be taken only for
   window lifecycle, never per frame. Move the frame `generation` counter to an atomic.
4. **Batch sizing by fill time, not count.** Replace "64k or 2 ms" with: submit when the
   in-fill buffer reaches 256k *or* the previous batch has drained and there is ≥ 16k
   waiting (keep the GPU busy, don't starve it). Underfilled CPU tails should approach zero
   except at chunk ends. Report batch-size and fill-wait histograms in the sidecar.
5. **Ownership stays exact.** Frames are owned by the window until every candidate is
   consumed or retired; buffers are reused only after the backend returns; the strict-mode
   oracle path is unchanged; `storeAligns` order untouched. All existing tests plus:
   - a test with a fake backend that delays completion, asserting producers keep submitting
     and lookups fall back without waiting;
   - a test asserting two batches in flight, no buffer reused before its drain;
   - the existing visit-counter linearity test still passing.

Do not change: the search kernel/transport, `umem`, the key builder's semantics, the
parity checker, `run_host.py`'s gate. Local gates: `cargo clippy/test --workspace`,
`python3 -B bench/star-integrate/test_*.py`, `cargo fmt`, clang-format. No SSH/commits.

Target (the orchestrator measures): GPU-arm mapping wall ≤ stock's 47 s, and GPU-arm CPU-s
below the hooks-bypassed arm by most of the ~130 CPU-s the GPU serves. Report what you built,
the in-flight depth, and what you expect the fill-wait histogram to look like — as a
hypothesis.
