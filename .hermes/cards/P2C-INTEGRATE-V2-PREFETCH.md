# P2C-INTEGRATE-V2-PREFETCH — consume one window behind the one last submitted

## Context (read first)

`bench/PHASE2C-integrate-1.md` "Round 7c" and "Round 8". Current state on the Spark, 20M
reads, 20 threads, 3 rotated repeats: GPU arm 629.4 CPU-s vs 662.1 for the huge-page
baseline (−4.9%); tenth parity match. 8.0M of 160.0M chains still fall back to the CPU,
and the MISS counters (`bench/evidence/integrate-1-host/timing-round8-r1-gpu-stats.jsonl`)
say every recoverable one is the same thing:

```
not_ready 5,858,292   (queued 3,111,112 / filling 153,140 / draining 2,594,040)
chain_rejected_residue 2,201,920   (later steps of those chains)
key_mismatch 0, positional_exhausted 0, device_stopped 0, no_window 0, everything else 0
```

Mechanism, measured: one window per batch (1,000 of 1,001 batches are a single 120–175k-job
window), coordinator fill wait p50 590 µs, dispatch+drain ≈ 40 ms per batch; a worker
consumes its window in ≈ 800 ms. The producer submits a window and immediately starts
consuming it, so the first ~5% of every window's reads reach `lookup()` before the batch
has been popped (`queued`) or before its drain returned (`draining`). The identity
contract is exact; this is purely a race the producer starts against itself.

Fix: each worker keeps **two** windows — `current` (being consumed, submitted one window
ago) and `next` (submitted just now). When `current` is exhausted, `next` becomes `current`
and a new `next` is prepared and submitted. Only the first window of each chunk races.

## Why it is safe to peek two windows ahead (verified by the orchestrator)

- `ReadAlign::readInStream[mate]` is an `std::istringstream` over the chunk's in-memory
  buffer (`ReadAlignChunk.cpp:21-28`: `pubsetbuf(chunkIn[ii], …)`). `save_seekable` /
  `restore_streams` in `star_integrate_window.cpp` already `tellg`/`seekg` it; a seek to
  any earlier position inside the chunk buffer is O(1) and exact. Peeking two windows
  (≤ 2 × 32,768 records) is the same operation over a longer span.
- Frames bind to STAR's reads by ordinal (`begin_map`: `f.ordinal != ra.iReadAll → return`),
  not by position in the window, so consuming `current` while `next` is already submitted
  cannot misattribute a frame.
- Window admission (`MAX_INFLIGHT_BYTES` 4 GB, `live_bytes` released by the `Window`
  deleter) already bounds two-per-worker: round 8 ran at 1.3–3.8 GB live with one. If
  admission refuses `next`, the worker degrades to today's behaviour (prepare on
  exhaustion), never to a wait.

## Deliverables

1. **Rotation** in `star_integrate.cpp`: `thread_local std::shared_ptr<Window> next_window`
   beside `current_window`. `window_remaining()` (called at the top of `prepare_window`)
   currently returns false and closes `current` when exhausted; make it rotate
   `next → current` first (after `close_window()` on the old current) and return true if the
   rotated window has frames whose ordinals are ≥ the read STAR is about to map. Then
   `prepare_window` fills a new `next` — which means `prepare_window` must peek from the
   **end of `next`'s reads**, not from STAR's current stream position. Cleanest: `Window`
   records the stream position (`std::streampos` per mate, plus the `iReadAll`-equivalent
   ordinal) at which its peek *ended*; `prepare_window` seeks there before its peek loop and
   restores STAR's real position after, exactly as it does today via `save_seekable` /
   `restore_streams`. If `next` is empty/absent (first window of a chunk, admission refusal),
   fall back to today's path: prepare from STAR's position and make it `current` directly.
2. **`end_chunk()`** must drop `next` as well as `current` (a chunk boundary invalidates
   any lookahead); `finish()` likewise. `close_window()` stays per-window.
3. **Sidecar**: add `"prefetch_windows"` (count rotated in as `current` from `next`),
   `"prefetch_refused"` (admission said no, fell back), and keep every existing field.
4. **Tests** (`test_coordinator.cpp`, driven by `test_coordinator.py`): (a) rotation — two
   windows submitted on one worker before either is consumed; `map_and_check` in order;
   assert the second consumed without `not_ready` and `prefetch_windows == 1`; (b) chunk
   boundary — a `next` present at `end_chunk()` is dropped and its charge released
   (`live_bytes` back to 0); (c) admission refusal — with `MAX_INFLIGHT_BYTES` effectively
   exhausted (the fixture can pre-charge `s.live_bytes`), `next` is refused, the worker
   still maps correctly, `prefetch_refused == 1`. RED first for (a) — without rotation the
   second window is a fresh submit and the counter stays 0.
   `test_window_prefix.py` stubs `window_remaining`/`submit_window` (`test_window_prefix.cpp:15-17`);
   keep those stubs compiling if you change signatures.

## Constraints

- Files you own: `bench/star-integrate/star_integrate.cpp`, `star_integrate.hpp`,
  `star_integrate_window.cpp`, `test_coordinator.{cpp,py}`, `test_window_prefix.cpp` (stub
  signatures only). Not `crates/`, not `make_star_integrate.py`, not `run_host.py`.
- **No decision in `lookup()`, `begin_map`, `handoff_read1`, or the candidate enumeration in
  `prepare_window` changes.** Parity has held ten times on that logic; this card changes
  *when* a window is prepared and *which* window is current, nothing about a window's
  contents. If you find you need to change what a frame contains, stop and say why.
- The `Window` pool from LAT 1a (per-worker free list, custom deleter releasing the charge)
  stays; two live windows per worker fit in its limit of 8.
- Gates to run and quote verbatim: `bash bench/star-integrate/test_abi.sh` (PASS line);
  `python3 -B bench/star-integrate/test_coordinator.py`, `test_source_patch.py`,
  `test_work.py`, `test_window_prefix.py`. The 20M strict gate and the timing are the
  orchestrator's on the Spark.
- clang-format before finishing. No commit.

## Finish with

Files touched; how the peek position is carried between windows (the exact fields); the
gate output; anything deferred and why.
