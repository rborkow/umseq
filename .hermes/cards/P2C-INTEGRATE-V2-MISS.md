# P2C-INTEGRATE-V2-MISS — classify the 12% of chains that fall back to the CPU

## Context

Read `bench/PHASE2C-integrate-1.md` "Round 7" first. The integrated STAR (borrowed index,
whole chain on the device) passes the strict gate and consumes 88% of submitted chains
(140.7M of 160.0M at 20M reads). The remaining 12% (8.0M `key_misses`, all counted as
`cpu_fallback`) is no longer the coordinator — it kept up (0 queued at end, 632 wakeups).
Every one of those misses is a chain that STAR ran on the CPU; each is roughly the CPU cost
the GPU exists to remove. We don't know why they miss. That is this card.

`lookup()` in `bench/star-integrate/star_integrate.cpp` (line ~1035) has **nine** distinct
non-consuming return paths that all increment the single counter `current_window->misses`:

1. read-bytes/length mismatch against the frame (`mate_length != len` / `memcmp`)
2. positional exhaustion: `cursor == range.last` (all candidates for this frame retired/used)
3. `chain_rejected` (a previous step of this chain already fell back — expected residue, not a miss to fix)
4. `!jp` (no chain job)
5. the big identity/quick-match compare (`!quick_match || …`) — a key mismatch proper
6. job state not `COMPLETE|VALID` or already `RETIRED` (GPU result not there yet: **the producer got ahead of the coordinator** — timing miss, not a key miss)
7. `chain_cursor >= n_steps` or `steps[cursor].status != 0` (device stopped the chain early: capacity/overflow/non-ACGT)
8. `step.shift != shift` (strict-fail in strict mode; counted only when strict is off)
9. CAS lost: `state & (CONSUMED|RETIRED)` at first consumption

Plus the early `return false` at the top (line ~1050: `!current_window || !current_frame ||
… chain.istart >= 2 || !same_index || !admitted`) which is **not counted at all** today —
these are the calls STAR makes that never had a window (window admission refused, frame not
prepared, `istart >= 2`). Those need their own counter: they are the "never submitted" set
and bound what any fix can recover.

## Deliverables

1. `star_integrate.cpp`: replace the single `misses` increment with a per-reason counter
   array on `Window` (and `Totals`), merged in `merge_window`, emitted in the sidecar JSON as
   `"miss_reasons": {"read_bytes": n, "positional_exhausted": n, "chain_rejected_residue": n,
   "no_job": n, "key_mismatch": n, "not_ready": n, "device_stopped": n, "shift": n,
   "cas_lost": n, "no_window": n}`. Keep `key_misses` as the sum of the first nine (so every
   existing consumer of the sidecar sees the same number) and add `no_window` as a separate
   top-level field too. For `device_stopped`, also split by `j.out.status` and the step's
   `status` (a small histogram `"device_stop_status": {"<code>": n}`) — statuses 8 (non-ACGT
   → CPU), 9 (overflow), 10 (max steps), 11 (no progress) are defined in
   `crates/umgpu/ffi/seed_probe_abi.h` / `usi.h`; read them, don't guess.
2. For `not_ready` (reason 6): this is the one we can act on. Record, at the miss, the age of
   the job's window in the coordinator: was it still queued (not yet popped), popped but not
   dispatched, or dispatched and draining? A three-bucket counter `"not_ready_where":
   {"queued": n, "filling": n, "draining": n}` — the Job has no such field today; the
   cheapest way is a `uint8_t phase` on `Job` set by the coordinator at pop (`FILLING`) and
   at `dispatch` entry (`DRAINING`), default `QUEUED`. Relaxed atomics are fine; it is
   diagnostic.
3. `test_coordinator.cpp`: extend the existing fixtures so at least `key_mismatch`,
   `positional_exhausted`, `not_ready` and `no_window` are each exercised once with the
   expected reason asserted (the fixture already produces a key miss in `map_and_check` —
   "wrong" ReadAlign — and a positional miss on the repeated key). `test_coordinator.py`
   drives it; keep it green. RED before GREEN for the new asserts.
4. A short section appended to `bench/PHASE2C-integrate-1.md` under a new heading
   `### Miss classification (P2C-INTEGRATE-V2-MISS)` describing the counters and *what each
   reason implies for a fix* (one line each). Leave a `TBD` table for the host numbers —
   the orchestrator runs the 20M pass and fills it in.

## Constraints

- Files you own: `bench/star-integrate/star_integrate.cpp`, `star_integrate.hpp`,
  `test_coordinator.cpp`, `test_coordinator.py`, `bench/PHASE2C-integrate-1.md` (append only).
- Do **not** touch `star_integrate_window.cpp`, `make_star_integrate.py`, anything under
  `crates/`, `run_host.py`, `run_timing_host.sh`. If you need something from the window
  side, write a one-paragraph request in your summary instead of editing.
- Do not change any decision in `lookup()` — this card classifies, it does not fix. Every
  `return false` stays where it is with the same condition. Parity depends on that.
- The sidecar is parsed by `run_host.py` (`stats['gpu_consumed']`, `batch_faults`,
  `rejected`) — keep every existing field name and meaning.
- Local gates you must run and report: `bash bench/star-integrate/test_abi.sh` (it
  syntax-checks the enabled TU against the real `usi.h` — must print PASS), and
  `python3 -B bench/star-integrate/test_coordinator.py`, `test_source_patch.py`,
  `test_work.py`, `test_window_prefix.py`. The real 20M run is the orchestrator's on the
  Spark; you cannot run it. Say so rather than estimating numbers.
- Format with clang-format (`/opt/homebrew/opt/llvm/bin/clang-format -i`) before finishing.
  No commit.

## Finish with

A summary: files touched (exact), the reason→counter mapping as implemented, which fixture
covers which reason, gate results verbatim, anything deferred and why.
