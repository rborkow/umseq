# INTEGRATE-1-FIX TDD record

Local-only RED/GREEN record; no Spark, CUDA, or STAR host gate was run.

1. RED: a third initial start could be produced/consumed by the coordinator.
   GREEN: producer bounds `istart` with `min(nstart,2)` and real-coordinator
   fixture rejects `istart == 2` at lookup.
2. RED: consumption scanned every job in the current frame. GREEN:
   `lookup_index` selects a bucket and exact field comparison resolves it;
   `test_coordinator.cpp` asserts the real TU's visit counters.
3. RED: malformed successful tuples could fall back. GREEN: fake backend's
   out-of-request-range success aborts in strict mode; Rust validates success
   against `q.low..q.high` before publishing.
4. RED: generation/index were insufficient. GREEN: full frozen key is hashed
   only to select a bucket, then field-compared; fixture covers changed bytes,
   generation, epoch, call field, and repeat consumption.
5. RED: emitted accounting used `gpu_hits`/mixed unused work. GREEN: sidecar
   emits `gpu_consumed`, consumed/suppressed/other aggregates and separate
   CPU-work observer fields; the coordinator fixture parses the emitted row
   with the gate-(i) assertions.
6. RED: queue admission had no aggregate cap. GREEN: window and live
   request/byte caps wait before ownership transfer; fixture has two workers,
   a full 65,536 batch and an underfilled CPU tail under a 30-second bound.
7. RED: gate expected `gpu_consumed`. GREEN: fixture parses the actual final
   JSON row and applies `gpu_consumed > 0`, zero fault/rejection checks.
8. RED: the generated inner hook constructed a partial positional key, so a
   real producer candidate missed. GREEN: `test_coordinator.py` compiles the
   real coordinator fixture; its generated-hook-shaped construction calls the
   shared builder and prints `generated key hook: consumed=1 key_misses=0`.
   `test_work.py` separately compiles the generated upstream hook itself.

Should-fix checks: stream state is captured before `tellg`, lookahead is bypassed
unless every stream is seekable, restoration is checked before publication; ABI
overlap regression verifies `usi_destroy_v1` rejects overlap without mutation.

## Visit counters (real coordinator fixture)

| Shape | W | J dispatched | frame cursor visits | frame-offset creations | candidate visits |
|---|---:|---:|---:|---:|---:|
| two staggered frames | 2 | 65,536 | 2 | ≤2 | 4 |

The fixture's `lookup_jobs == 4` (one hit and one deliberately repeated lookup
per frame) is the regression guard against the prior
per-frame 40,000-job consumption scan.

## P2C-INTEGRATE-2-WINDOW local record

1. GREEN: `STAR_INTEGRATE_COUNTERS=0` makes the comparator observer, inner-call
   observer, and observer scopes inline no-ops. The observer fixture remains a
   counters-on build by default; timing builds must pass
   `-DSTAR_INTEGRATE_COUNTERS=0`.
2. GREEN: lookahead scratch arrays are no longer value-initialized per record;
   the split and clipping scratch containers are retained for the whole window.
   `test_window_prefix.py` proves the published prefix record remains identical.
