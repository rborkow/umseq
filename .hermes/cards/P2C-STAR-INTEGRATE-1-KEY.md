# P2C-STAR-INTEGRATE-1-KEY — Terra — the frozen identity key is asserted but never populated

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then this card, then
`bench/star-integrate/star_integrate_window.cpp` (producer), `star_integrate.cpp` (consumer,
`lookup` at ~605–650, `same_call`, `call_hash`), and the `ReadAlign_maxMappableLength2strands.cpp`
patch text in `make_star_integrate.py` (~line 26, the `InnerCall starIntegrateCall={...}` literal).

## What the Spark said (gate host4, `~/uni-rnaseq-probe-lab/integrate-gate-host4/`)

The integrated STAR built, ran the full 20M in strict mode, **parity passed** — and
`integrate-stats.jsonl`:

```
batches            959        gpu_batch_sizes  65536 ×4 then 262144 …
submitted          152,502,272
gpu_consumed       0
key_misses         144,333,308   (= exactly CHAIN-POSITION's initial-start inner request count)
cpu_fallback       0             (misses aren't even counted as fallback)
other_unused       139,567,817   suppressed_unused 12,934,455
```

The GPU did all the work (1.5 G gathers submitted), and the CPU then redid all of it because
**every lookup missed on the identity key**. Parity is therefore evidence about the stock
fallback path only. The gate rightly refused it (`gpu_consumed > 0` required).

## Why

`lookup()` requires equality on all 22 `InnerCall` fields plus `chain` context. The two sides
populate them differently:

| field | producer (`star_integrate_window.cpp:295–302`) | STAR call site (`make_star_integrate.py` literal) |
|---|---|---|
| `generation` | `0` (comment: "producer fills the frame identity immediately after" — it doesn't) | `current_generation()` |
| `index_epoch` | `0` | `current_epoch()` |
| `read_id` | `ordinal` | `iReadAll` |
| `worker`, `chunk`, `mate_context`, `split_context` | never set (zero from `= {}`) | positional zeros / not passed |

Also verify `read_id`: `ordinal` (window-local) vs `iReadAll` (STAR global) — these must be the
same quantity, and `lookup` additionally checks `c.read_id != current_frame->ordinal`.

Finding 4 of the review asked for the complete key to be *compared*; it was, but the producer
was never taught to *fill* it, and no test drove a real candidate through a real lookup with the
generated call site. `test_coordinator.cpp`'s toy passes because it constructs both sides itself.

## Deliverable

1. **One source of truth for the key.** A single function (in `star_integrate.hpp`) that
   builds the `InnerCall` identity fields from `(frame, chain, epoch)` — used by the producer
   when it appends a candidate *and* by the generated call site (emit a call to it from
   `make_star_integrate.py` instead of a 22-value positional literal). Positional literals of 22
   `uint64_t` are how this happened; ban them.
2. **Frame identity assigned once, before candidates.** `generation` (non-reused, per
   `begin_map`), `index_epoch` (`S().epoch`), `read_id`, `worker`, `chunk` set on the frame,
   copied into every candidate. Define `mate_context`/`split_context` concretely (mate lengths
   + `Nsplit`/split index is the design's intent) and derive them identically on both sides,
   or drop them from the key with a one-line rationale — don't leave zero-vs-unset.
3. **The test that would have caught this:** compile the *generated* patched
   `ReadAlign_maxMappableLength2strands.cpp` hook (as `test_work.py` already does for the
   work hooks) against a fixture where the producer has appended candidates for a read, then
   call the real `lookup` with the real call-site construction; assert `consumed == 1`,
   `key_misses == 0`. RED on current code (misses), GREEN after. Record in `TDD.md`.
4. **Count misses honestly:** a key miss that falls to the stock function *is* a CPU fallback
   for attribution purposes; `cpu_fallback` must not read 0 when 144M requests ran on the CPU.
   Keep `key_misses` as its own counter alongside.
5. Local: `cargo clippy/test --workspace`, `python3 -B bench/star-integrate/test_*.py`,
   `cargo fmt`, clang-format. No kernel/transport/umem/parity-checker changes. No SSH/commits.

Finish with: the key-builder signature, where both sides call it, the RED/GREEN output, and
anything about `mate_context`/`split_context` you had to decide.
