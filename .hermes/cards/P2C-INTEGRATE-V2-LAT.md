# P2C-INTEGRATE-V2-LAT — window allocation churn (the GPU arm's +41 s sys) and the not_ready residue

## Context

Read `bench/PHASE2C-integrate-1.md` "Round 7" and "Round 7b" first. Numbers you are working
against, all measured on the Spark (20M reads, 20 threads, 3 rotated repeats):

| arm | user | sys | CPU-s |
|---|---|---|---|
| integrated binary, huge-page advice on, hooks bypassed (**the baseline**) | 643.9 | 20.8 | 664.8 |
| GPU on (round 7) | 592.7 | 62.1 | 654.7 (−1.5%) |

The GPU arm saves 51 user CPU-s and gives 41 back as sys. Two measurements this card acts on:

### A. Where the sys goes (`bench/evidence/integrate-1-host/gpu-sys.txt`, 8M, perf by DSO)

Kernel share 3.3% (bypass) → 9.0% (GPU), the excess spread across the 20 mapping threads
(~500 kernel samples each vs ~100). User-space frame under the kernel samples:

```
34.3%  __memcpy_sve < star_integrate::submit_window(...) < prepare_window(ReadAlignChunk&)
17.8%  __munmap    < cfree                              < Window::~Window()
 4.8%  submit_window itself
```
Kernel leaves: `_raw_spin_*`, `__pi_clear_page`, `page_counter_cancel`, `folio_remove_rmap_ptes`,
`do_page_fault`. That is **fresh-page faulting on every window and munmap on its death**:
`submit_window` (`star_integrate.cpp`, ~line 860) does `std::shared_ptr<Window> w(new Window);
w->frames = std::move(frames);` then `w->jobs.reserve(nj)` — a `jobs` vector of up to
262,144 × 544 B = 142 MB, above glibc's mmap threshold, so each window is a fresh `mmap`
that page-faults in and is `munmap`ed when the last owner drops it (~1000 windows per 20M
run, ~8k/s at 4M-read pace across 20 threads). The frames' own vectors (`a`, `b`,
`candidates` — reserved at `frames.reserve(min(window_limit(), MAX_WINDOW_READS))` in
`star_integrate_window.cpp` `prepare_window`) are built fresh per window too and die with it.

### B. Why 12% of chains still fall back (`gpu-sys.txt`, MISS counters, 20M)

```
submitted 159,989,084   gpu_consumed 141,201,259   key_misses 7,363,572   no_window 0
miss_reasons: not_ready 5,352,457 | chain_rejected_residue 2,011,115 | everything else 0
not_ready_where: queued 2,673,947 | filling 182,421 | draining 2,496,089
```
Zero key mismatches, zero positional exhaustion, zero device stops. **Every recoverable miss
is latency**: the mapping thread reached a read before its window's GPU result was back.
Half of those windows were still in the SPSC ring (the coordinator had not popped them —
it sleeps between batches now: 632 wakeups per 4M), half were dispatched and draining.
The residue (2.0M) is the later steps of chains whose first step was `not_ready`, so it
follows the fix for free.

## Deliverables, in value order — each lands independently

### 1. Window pool (item A) — target: sys back to the bypass arm's ~21 s

A per-worker free list of `Window` objects (and their `frames` vector with its per-frame
`a`/`b`/`candidates` capacity) so a window's memory is reused by that worker's next window
instead of being `mmap`ed and `munmap`ed per window. Shape that fits the current ownership:

- `Window` is held by `std::shared_ptr` in three places (producer's `current_window`, the
  SPSC ring slot, the coordinator's `owners`), and its **destructor releases the admission
  charge** (`Window::~Window()` — `live_bytes`/`live_requests`; that logic must survive).
  The cleanest pool is a custom deleter on the `shared_ptr` that runs the charge release
  and then returns the object to the *owning worker's* free list instead of deleting —
  `submit_window` knows `worker_queue` (thread_local), so the pool can hang off `SpscQueue`
  or a sibling `thread_local`. The coordinator thread drops the last reference in the common
  case (`owners.clear()` after drain), so the return-to-pool must be thread-safe: an
  `std::mutex`-guarded `std::vector<Window*>` per worker is fine (one lock per window, not
  per job). Bound the pool (e.g. 8 per worker); beyond that, delete.
- On reuse: `frames.clear()` keeps the vector's capacity but the `WindowRead` elements are
  destroyed with their `a`/`b`/`candidates` buffers. To keep *those* capacities too, the
  producer in `prepare_window` should take the pooled window's `frames` vector (moved out,
  capacity intact, elements still alive), reuse each element's buffers with `clear()`, and
  move it back. That touches `star_integrate_window.cpp`'s `prepare_window`
  (`std::vector<WindowRead> frames; frames.reserve(...)`) and `submit_window`'s signature
  (it currently takes `std::vector<WindowRead> &&`). Do the `Window`/`jobs` pool first
  (item 1a) and measure the frames part (1b) as a separate commit-sized step; if 1a alone
  removes the `munmap`/`clear_page` share, stop there and say so.
- `jobs` must be `clear()`ed, not reallocated; `ranges`/`cursors` likewise. Reset every
  counter field (`next_frame`, `consumed`, …, the `miss_reasons` array, `*_stats`) — write a
  `Window::reset()` and make the constructor call it so the two cannot drift.
- Cheap disqualifier **before** writing the pool, and put its numbers in your summary:
  `test_coordinator` fixtures with `mallopt(M_MMAP_THRESHOLD, 1<<30)` + `M_TRIM_THRESHOLD`
  set at startup are not a measurement of the real thing; the real disqualifier is the
  orchestrator's to run on the Spark (`MALLOC_MMAP_THRESHOLD_=1073741824
  MALLOC_TRIM_THRESHOLD_=1073741824` env on the GPU arm). Ask for it in your summary if you
  want it before committing to 1b; do 1a regardless — it is right on its own.

### 2. Prefetch one window ahead (item B) — target: `not_ready` → near zero

Today `prepare_window` runs when the current window is exhausted (`window_remaining()`
false) and immediately submits; the producer then starts consuming the *same* window it
just submitted, so the first reads always race the GPU. Change: keep **two** windows per
worker — when the current one is exhausted, the *next* one (prepared and submitted one
window earlier) becomes current, and a new next is prepared and submitted. The first
window of a chunk is the only one that races. Concretely:

- `thread_local std::shared_ptr<Window> current_window` gains a sibling `next_window`;
  `window_remaining()` / `close_window()` rotate `next → current` and `prepare_window`
  fills `next`. `prepare_window` already peeks ahead in the read stream and rewinds
  (`save_seekable` / `restore`); check that peeking *two* windows ahead stays within what
  that rewind supports (the stream position saved must be the one STAR resumes from — i.e.
  the start of `current`, not `next`). If `readInStream` rewinding cannot cover two windows,
  say so and stop item 2 with the measurement of how far ahead it *can* peek.
- Admission: two windows per worker doubles the in-flight charge; `MAX_INFLIGHT_BYTES`
  (4 GB) has room (round 7 census: 1.3–3.8 GB live). Keep the bound; if admission refuses
  the second window it degrades to today's behaviour, never to a wait.
- The coordinator's fill rule (`submit_floor` 16384 after a drain, `FILL_MAX_US` age-out)
  is unchanged. Do **not** shrink `cap`/`submit_floor` to chase latency in this card; if
  after prefetch `not_ready_where.queued` is still large, report it as the next lever.

### 3. Tests

- `test_coordinator.cpp`: a pool test — submit, drain, drop, submit again on the same
  worker; assert the second `Window*` is the first one reused and every counter/`jobs`
  is reset (RED first: without the pool the pointer differs). A prefetch test — two
  windows submitted before the first is consumed; `map_and_check` on both in order; assert
  `not_ready` stayed 0 for the second (the fixture backend completes synchronously, so this
  checks the rotation, not the race).
- `test_window_prefix.py` / `test_work.py` must stay green untouched.

## Constraints

- Files you own: `bench/star-integrate/star_integrate.cpp`, `star_integrate.hpp`,
  `star_integrate_window.cpp`, `test_coordinator.{cpp,py}`. Nothing under `crates/`; not
  `make_star_integrate.py`; not `run_host.py`/`run_timing_host.sh`.
- Every `return false` in `lookup()` and every decision in `prepare_window` stays as is —
  parity has held nine times on this logic. Item 2 changes *when* a window is prepared,
  not *what* it contains.
- The sidecar's existing field names and meanings stay (parsed by `run_host.py` and the
  bench doc).
- Local gates to run and quote verbatim: `bash bench/star-integrate/test_abi.sh` (PASS
  line), `python3 -B bench/star-integrate/test_coordinator.py`, `test_source_patch.py`,
  `test_work.py`, `test_window_prefix.py`. The Spark runs (sys attribution rerun, MISS
  counters, the 3×3 timing) are the orchestrator's; do not estimate them.
- clang-format before finishing. No commit.

## Finish with

Files touched (exact); what landed of 1a / 1b / 2 and what did not, with the reason; the
gate output verbatim; what you want measured on the box first.
