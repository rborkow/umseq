# Independent review: LAT and PREFETCH

Reviewed LAT **f6647ac** and PREFETCH **837c2a3**, against accepted HEAD **89f005c**. The eight requested integration/source-test files are unchanged between PREFETCH and accepted HEAD. All code and test reads used exported Git snapshots, not the concurrent generator changes. Read `AGENTS.md`, the latest rounds of `bench/PHASE2C-integrate-1.md`, and all three specified round-9 artifacts. No performance estimates or new performance claims are made here.

**LAT verdict: BLOCKED at its exact landing.** Admission refusal releases an unacquired charge (B2); the shutdown/pool lifetime defect B1 also applies. PREFETCH fixes B2, so it is not an outstanding charge bug at accepted HEAD.

**PREFETCH verdict: BLOCKED at its exact landing and accepted HEAD.** B1 reproduces locally. Normal rotation and refusal tests pass, but they do not cover shutdown with queued windows beyond the current batch. Other prefetch boundary findings below cause fallback/coverage problems or violate rewind handling; they are not evidence that Host24's compared output differs.

Unless explicitly marked LAT, repository citations below refer to **89f005c / 837c2a3**. Local upstream citations refer to the available fixture `/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source/`; that fixture was read, not modified.

## Blocking

### B1 — Shutdown can leave owning queue slots alive past pool destruction

`coordinator_main` takes whole windows only while they fit the remaining batch capacity (`bench/star-integrate/star_integrate.cpp:658`). If a partial fill cannot accommodate another queued window, the stopping branch CPU-resolves only the fill and **breaks**, without emptying the queues (`:709`). `finish()` joins this thread but does not drain the remaining slots (`:1385`). Joining therefore does not imply that all window owners are gone.

This becomes a destruction bug with LAT's raw queue pointer captured by the shared-pointer deleter (`:1064`). In `SpscQueue`, `slots` is declared before `pool_mu` and `free_windows` (`:157`). The destructor first deletes pooled objects (`:166`); member destruction then destroys the free list and mutex **before** destroying the owning slots. A leftover slot's deleter invokes `owner->recycle_window`, which locks the already-destroyed mutex (`:180`). Even a platform where that lock appears to work would then access a destroyed vector. The incomplete shutdown loop predates these landings; the pool makes the residual ownership particularly consequential.

**Reproduction:** seven producer threads each publish a 40,000-job fixture window and call `end_chunk`; coordinator execution is delayed until stopping is set. This models producers finishing while the coordinator is behind. Six windows fit (240,000 jobs), the seventh does not fit the 262,144 cap. Both snapshots returned from `finish()` with:

```text
after finish queued=40000 live_requests=40000 live_bytes=30720176 tails=240000
```

A second accepted-HEAD probe allowed ordinary process destruction instead of `_Exit`. Built with AddressSanitizer, it terminated with exit 134:

```text
libc++abi: terminating due to uncaught exception of type std::__1::system_error: mutex lock failed: Invalid argument
```

This is a reproduced libc++ teardown failure, not an ASan memory-error diagnosis or a GPU failure. Host24's zero live charges show that this particular backlog condition did not remain at its finish.

**Required correction/regression:** shutdown must resolve and relinquish every queued window, including windows excluded by whole-window batching, while the state and pools remain alive. Release last owners outside `State::mu`, since `Window::release()` acquires it. Retain this seven-window case and require empty queues, zero active charges, and successful ordinary process destruction. Also exercise a delayed backend with workers already closed, rather than settling every window before finish.

### B2 — LAT alone subtracts a charge that admission refused

At **f6647ac**, `submit_window` sets `charged_requests/charged_bytes` before checking admission (`bench/star-integrate/star_integrate.cpp:1035`). Both outright refusal and failed-ring-push rollback leave these fields nonzero (`:1055`, `:1063`, `:1072`). Last-owner `release()` consequently subtracts a charge which is not present. The `active` flag prevents a second release of the object, but does not prove that its first charge was acquired.

With the fixture's live-byte limit already full, a refused 40,000-job window produced:

```text
f6647ac: state=1 status=0 charged_requests=40000
after release: live_bytes=4264247120 live_requests=18446744073709511616
```

The request count wraps from zero. This violates the admission bound and can affect later admission decisions. **837c2a3 fixes this** by clearing both charge fields after refusal (`bench/star-integrate/star_integrate.cpp:1112`). The identical probe against accepted HEAD produced `charged_requests=0`, with `live_bytes=4294967296` and `live_requests=0` unchanged after release. Preserve a direct current-window-refusal regression as well as the existing next-window-refusal test. This finding concerns the exact LAT landing; no additional charge fix is requested at HEAD.

## Should fix

### S1 — WindowEnd can describe a different boundary from its ordinal

The producer reads both mates before deciding that the next frame exceeds its byte/candidate budget (`bench/star-integrate/star_integrate_window.cpp:168`, `:217`). On refusal it breaks without admitting that frame (`:223`), then records the stream positions **after** it while setting `end.ordinal` to the last admitted ordinal plus one (`:276`). The next peek therefore skips that one record. `WindowEnd.ordinal` is copied but never used to validate the seek (`star_integrate.cpp:1031`; `star_integrate_window.cpp:137`). The same mismatch can occur at the post-read combined-length break (`:185`).

This does **not** consume the missing record from STAR: real positions are restored before submission (`:282`), and exact ordinal checks in `handoff_read1` and `begin_map` refuse the future frame (`star_integrate.cpp:1167`, `:1150`). STAR maps the gap on the CPU, then can match the later frame. The bug is a broken lookahead-boundary contract and an unnecessary hole in acceleration, not demonstrated SAM loss. The observed one `read1_fallback` per successful prefetch rotation is consistent with a gap, but those aggregates alone do not prove its cause.

Record the boundary after the last admitted frame (or rewind the rejected record before recording it). Add a real producer/consumer test forcing a budget break with at least three distinct paired records; verify every resulting frame ordinal, both seek positions, and the stock read sequence through rotation. A test maximum that only limits the loop count does not exercise this post-read budget break.

### S2 — EOF endpoints and failed rewinds are not handled consistently

Initial stream eligibility and restoration are per mate: `save_seekable` requires good state and tests seeking (`star_integrate_window.cpp:44`); normal restoration clears the peek's EOF/fail bits, seeks back, and restores the original state (`:63`). The producer also checks mate status consistency, matching upstream `ReadAlign_oneRead.cpp:13` rather than inventing a separate mate-ordinal rule. Upstream `readLoad.cpp:27` overwrites the shared ordinal from each mate, as does this producer.

At EOF, however, the producer saves unchecked `tellg()` results (`star_integrate_window.cpp:278`). The attempted next peek can seek to `-1`. Its failure branch calls `restore_streams` but ignores the return value (`:145`), whereas the final restoration correctly calls `fatal_restore` on failure (`:282`). A failed rollback can therefore return to stock with only part of the mate streams restored. Fix this branch to enforce the same restoration result. Do not publish an invalid endpoint as usable lookahead; mark a tail as having no successor to avoid repeated attempts while its current frames are consumed.

Concrete regressions: partial final windows; EOF/failbit independently on either mate; empty successor; mismatched mate termination; and a seekable fixture whose rollback fails on the second mate. Verify actual positions and states, not just a submission count. Stock's own malformed-input diagnostics remain the reference.

### S3 — Overshoot falls back safely but can strand the frame cursor

Rotation accepts a front ordinal greater than or equal to the ordinal supplied **before** `oneRead` (`star_integrate.cpp:1009`). That supplied value is the previously loaded ordinal: the generated hook precedes `oneRead` (`make_star_integrate.py:189`), and upstream `readLoad.cpp:27` supplies the next one. Equality is therefore not a sufficient proof that rotation starts at the next stock record.

The later exact checks prevent handing a mismatched frame to stock. But if stock's loaded ordinal overtakes the expected frame, `begin_map` returns without advancing or retiring it (`star_integrate.cpp:1149`), and `window_remaining` keeps returning true solely because frames remain (`:1012`). Acceleration stays stranded until `end_chunk`. Specify recovery for a stale ordinal and test both forward gaps and actual overshoot through the generated call order. No wrong-frame consumption was found on these mismatch paths.

### S4 — The accepted tests no longer prove pool reuse or the stream contract

LAT's `window_pool_reuse` really checks object reuse and selected reset fields. PREFETCH replaces those assertions with current/next rotation assertions, while retaining the function/test name (`test_coordinator.cpp:333`). The accepted test never reacquires the first object from the pool. Keep a third submission after both previous references have been released, and verify object identity, reset fields, rebuilt job/frame pointers, and charges. Include a separately retained shared pointer to prove that an object cannot be reacquired early.

`test_coordinator.py:20` compiles a generated outer hook to an object file, but that object is **not linked into the executable** built at `:26`. The executable includes the actual coordinator TU and manually constructs frames/calls (`test_coordinator.cpp:50`); this is useful coordinator coverage, not execution of the generated producer/consumer contract. `test_window_prefix.cpp:15` stubs window state/submission and never calls `prepare_window`; its randomized clipping checks construct the frame from the already-combined stock bytes (`:89`). `test_source_patch.py` supplies useful patch/compilation checks, not runtime stream traversal.

The narrow missing test is a generated `mapChunk`/`oneRead` handoff driving the actual producer and coordinator with small paired chunk streams. Cover S1–S3, admission refusal, rotation, and a second chunk reusing the worker. These are targeted regressions for this contract, not a request for a broad hardening matrix.

### S5 — Counter names do not establish disjoint accounting

Host24 has positive GPU consumption (**145,590,472 chains**), strict shift/flag/step-count mismatches **0/0/0**, `batch_faults=0`, and `rejected=0`. It also records **877,083** `device_stopped` misses with status **0**. There is a concrete non-device-error explanation:

- A new `Job` zero-initializes its output (`star_integrate.cpp:66`). `resolve_cpu` publishes `COMPLETE` without `VALID`, leaving that zero output (`:510`).
- Refused **current** windows remain available for CPU/frame handoff, with all jobs CPU-resolved (`:1108`, `:1125`). At lookup, completed-but-invalid jobs increment `device_stopped` and status zero (`:1325`). They were never submitted to the device, so neither `rejected` nor `batch_faults` need increment.
- CPU resolution also occurs for shutdown/disabled batches (`:693`, `:709`). Separately, a valid result whose `n_steps` is exhausted can take the same miss path (`:1339`); `note_device_stopped` records the chain status, and may additionally record a step status (`:372`). Thus the status histogram itself is not inherently one count per miss.

The refusal scratch probe reproduced the precise `COMPLETE`, not-`VALID`, status-zero state. Host24's zero faults/tails/rejections and its admission refusals make CPU-resolved admission fallback a supported explanation. The artifacts do **not** distinguish how many of its 877,083 events came from that path versus exhausted valid chains; claiming an exact attribution requires a reason counter or trace. An exhausted chain can set `chain_rejected`, after which later steps fall back and `finish_active_chain` skips its count/flag comparison (`:1277`, `:338`). Therefore strict 0/0/0 does not by itself exclude this second path. The generated fallback executes the full stock outer body (`make_star_integrate.py:143`); status zero here is not evidence of a CUDA fault.

Unused counters also overlap **fallback events** and omit some work:

- `retire` counts only a first retirement of a completed valid, unconsumed job (`star_integrate.cpp:521`). A job missed as `not_ready` can complete later and be counted as `other_unused` at close. Those are two descriptions of the same chain, not separate populations.
- A job retired before completion gets no unused count; later publication preserves `RETIRED` but does not backfill unused accounting (`:620`).
- Closing `next_window` calls `retire`, which writes through `current_window`, rather than the window being closed (`:758`, `:525`). Normal `end_chunk` clears current first (`:1216`), so completed unconsumed next-window jobs receive no unused attribution.
- A successful first-step CAS increments `chains_consumed` once (`:1359`), but whole-chain `j.stats` are added on **every consumed step** (`:1379`). Hence consumed gathers/bytes can exceed submitted totals; these stats are not a partition of device work.

For illustration from the actual artifacts, submitted minus consumed minus the two unused counts leaves **7,277** chains in Host24 and **67,721** in timing round-9 repeat 1. These arithmetic residuals are not newly identified categories. Timing repeat 1 has `not_ready=157664` and `other_unused=104621`; the latter cannot be read as “STAR never asked for these chains.” Separate CPU-resolved admission from actual device stops, attribute retirement to the supplied window, and count whole-chain stats once before presenting partition-style accounting. None of these observations invalidates the external output comparator.

## Nits

`WindowEnd.ordinal` currently looks like an enforced boundary check but is unused; either enforce it with its stream positions or describe its diagnostic-only role. Rename the accepted “window pool” regression to rotation if a separate reuse test is added. No other cosmetic changes are requested.

## Verified tests and lifetime checks

All runs were local, with synthetic fixture data and fake USI transport; none is GPU parity evidence.

| Snapshot | Test/probe | Result |
|---|---|---|
| f6647ac | `python3 bench/star-integrate/test_coordinator.py` in scratch export | Passed, including original pool reuse/reset regression |
| 89f005c (requested files identical to 837c2a3) | `test_coordinator.py` | Passed: generated-hook compile, generated-key fixture, whole-window batching, rotation, chunk boundary, prefetch refusal, shuffled positions, sidecar assertions |
| 89f005c | `test_window_prefix.py` | Passed helper admission and 1,000 constructed clipping/orientation cases |
| 89f005c | `test_source_patch.py` | 8 tests passed; no skips |
| Both snapshots | Refused-current-window scratch probe | LAT charge subtraction/wrap reproduced; accepted HEAD preserves counters |
| Both snapshots | Seven-window stopping scratch probe | 40,000 requests remain queued after finish |
| 89f005c | Same shutdown with ordinary destruction, ASan build | Exit 134; destroyed-mutex failure reproduced |

Generated STAR compilation emitted existing variable-length-array extension warnings. The scratch wrapper renames the fixture's `main`, which emits a missing-return warning for that uncalled renamed function. Scratch C++ was formatted with `/opt/homebrew/opt/llvm/bin/clang-format`; no illustrative C++ or Rust is embedded in this report. Scratch evidence: `/tmp/lat-review-probe.cpp`, `/tmp/lat-review-destruction.cpp`, their executables, and exports `/tmp/lat-prefetch-review-{lat,pinned}`. The first shutdown probe deliberately uses `_Exit` after printing state; the second deliberately permits teardown.

Apart from B1/B2, the normal pool lifecycle is coherent: last shared ownership delays release until ring/batch/worker users are finished; `release` is idempotent through `active`; the pool mutex serializes recycling and reacquisition; reset clears frame/job/range/cursor contents and all per-window counters, including the new endpoint/refusal fields; reconstructed jobs get fresh atomics and pointers (`star_integrate.cpp:129`, `:170`, `:378`, `:1076`). Retained allocation capacity is not an active admission charge, so zero live bytes does not mean zero retained pool memory.

For the borrowed index, the generated `finish` precedes `genomeMain.freeMemory` (`make_star_integrate.py:36`). Upstream `mapThreadsSpawn.cpp:21` joins mapping workers; generated chunk exit drops both window references (`make_star_integrate.py:191`; `star_integrate.cpp:1216`). Coordinator batch owners protect C++ frames/jobs through synchronous dispatch (`:646`, `:668`, `:704`). Rust stores raw borrowed G/SA/SAi ranges without taking allocation ownership (`crates/umgpu/ffi/star_prefix.rs:82`), copies batch reads/requests into leased buffers, and reclaims their leases after the call (`:417`). The CUDA raw-host contract requires index lifetime through successful drain (`crates/umgpu/src/cuda.rs:1563`); reclamation waits on the submission (`:1170`). `usi_destroy_v2` drops its context after coordinator join (`star_prefix.rs:792`; `star_integrate.cpp:1396`). B1's leftover queue has never-dispatched host jobs, not a demonstrated post-free GPU read. It still violates the required ownership teardown.

## Unverified host requirements

The latest bench record and user-provided comparator description take precedence over older board labels. The external comparator checks **every ordered SAM record** (20M pairs, **53,710,530 records**), with only executable/output-prefix header argv normalized, plus SJ bytes and non-timing logs. Its legacy `PARITY_MATCH_COUNTERS_ONLY` label names historical instrumentation; it is **not** a counts-only comparison. The local Host24 JSON is a checkpoint requiring orchestrator verification and records the unchanged comparator hash `37a4b369c44e9c3eb3be64d66a564a9e6f1abb40c0b19c112bd11fa893d792a6`. I did not independently reopen the host SAMs or execute that external comparator. There is no observed source-versus-golden disagreement in this review.

The round-9 timing TSV's nine rows and repeat-1 sidecar were read directly. Repeat 1 has `device_stopped=0`, unlike Host24; timing and strict-gate counters must not be substituted for one another. Performance is not used to accept either correctness verdict.

After fixes, the orchestrator still owns full ordered-output parity on the pinned generated source, strict positive-consumption evidence, and the required Spark GPU/CPU Tier-0 comparison. CUDA drain-error/borrowed-index teardown behavior was not exercised locally. No new host runs, SSH, production-input access, repository-wide gates, commits, or pushes were performed.

**Repository file touched:** only `docs/review-integrate-lat-prefetch.md`. Concurrent generator, runner, cards, board, and host-evidence work was left untouched.
