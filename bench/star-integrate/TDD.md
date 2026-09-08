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

## P2C-INTEGRATE-3-CONSUME local record

1. RED: the window's post-clip numeric read was prepared again by STAR's
   pair/complement/reverse block. GREEN: the generated `oneRead` hook retains
   `readLoad` side effects, then installs `Read1[0..2]` from the matching frame;
   `test_window_prefix.py` compares all three arrays for 1,000 random paired,
   randomly clipped reads. The pinned `readLoad` performs conversion internally,
   so that conversion cannot be skipped by this hook.
2. RED: the coordinator woke every 100 microseconds while idle. GREEN: it waits
   indefinitely without a fill and for `FILL_MAX_US` only while filling;
   producers notify on the aggregate submit-floor crossing and retirement.
   The real coordinator fixture bounds wakeups for its two-window fake backend.

## P2C-INTEGRATE-V2-T1 local record

1. GREEN: setup samples resident Genome/SA/SAindex bytes with the frozen sample
   positions and no STAR-side file opens; identity uses non-cryptographic FNV-1a.
2. GREEN: lookup consumes the current frame's dense candidate cursor, records a
   positional mismatch, and never searches a hash bucket.
3. GREEN: generated comparator code has no observer hook; enabled-only hooks and
   call-site accounting leave the disabled inner-search path as the stock call.

- **P2C-INTEGRATE-V2-T3 (2026-09-07):** Mac exhaustive prefix-grid oracle and workspace fmt/clippy/tests pass; V1 ABI retained, V2 prefix-only/unique/search and distinct rejections added. Exact 999,914 replay driver ready; CUDA, strict 20M and Tier 0 GPU gates pending Spark. Contract, Terra handoff and evidence: `bench/PHASE2C-integrate-v2-t3.md`.

- **P2C-INTEGRATE-V2-T3B (2026-09-07):** GREEN: V2 window requests contain
  only initial-start geometry (`tag=1`, zero prefix/interval/distance); the
  generator consumes successful V2 output above STAR's prefix block and keeps
  the full stock outer block as both fallback and strict oracle. Strict checks
  all four outer outputs plus handed-off `Read1` bytes; sidecar reports
  `prefix_only`, `unique`, and `searched` branch counters.

- **P2C-INTEGRATE-V2-T4 (2026-09-07):** PREREQUISITE FAILED: checked-in
  CHAIN-POSITION producer/evidence has no per-chain step-count distribution;
  UMPROBE1 also lacks chain identity/lmapped/istart. Stopped per AGENTS.md before
  choosing an unsupported percentile capacity. No V3 implementation or test-pass
  claim. Findings and Terra hold/handoff: `bench/PHASE2C-integrate-v2-t4.md`.

- **P2C-INTEGRATE-V2-T5 / Item A (2026-09-07):** GREEN by generated-source
  inspection: `maxMappableLength2strands` caches `STAR_INTEGRATE` mode in a
  function-local `static const bool`, dispatches its fully verbatim stock prefix
  loop before entering any `iDist` iteration, and returns from that arm. The V2
  frame/key lookup is consequently unreachable in bypass mode. `mapOneRead`
  likewise caches the mode before its seed loops for `set_chain` and
  `reverse_suppressed`. Spark timing remains the required acceptance check.

- **P2C-INTEGRATE-V2-T4R (2026-09-07):** measured chain histogram unblocks
  capacity 8. V3 thread/warp whole-chain transport and raw-host V2 launch sibling
  implemented alongside V2. Independent Rust chain transcription matches the
  shared host/device C++ body over the exhaustive synthetic grid, including
  seedMapMin termination, max_steps, exact capacity, overflow and flag outcomes;
  Mac fmt/clippy/workspace tests pass. CUDA grid, strict 20M per-step + Read1,
  GPU/CPU Tier 0 cmp and measured V2/thread/warp gathers/s remain pending Spark.
  Exact ABI, raw-pointer lifetime and Terra handoff: `bench/PHASE2C-integrate-v2-t4.md`.
