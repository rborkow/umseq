# P2C-LAT-CONTRACT-CLOSE review and implementation handoff

## Current orchestrator disposition

**Local targeted repair accepted for host gating, not production acceptance.**
The orchestrator independently read the core/producer/header diff and surrounding
publication, shutdown, rotation and generated handoff paths. The stale-frame skip
uses the previous stock ordinal; handoff still requires exact identity. Terminal
markers are set only after successful rollback. CPU reason bits are published with
completion, and last queue owners are released outside the admission mutex.
No blocker found in these targeted changes; accounting is still not a partition.

Personally rerun, all exit zero: the 13 stream subprocess cases, coordinator suite,
prefix suite, eight source/ABI tests, and both normal/delayed shutdown cases with
ASan. The shutdown rows again report zero queued work and zero live requests.
`cargo fmt --all --check`, workspace clippy with `-D warnings`, and
`cargo test --workspace` also passed. Upstream C++ VLA warnings remain.
Logs and the frozen source manifest: `bench/evidence/lat-contract-close/`.

Spark gate launched under the resource lock with a 5,400-second outer timeout:
`~/uni-rnaseq-probe-lab/integrate-gate-lat-close-20260908`.
The source is a frozen archive of `89f005c` plus an explicit reviewed overlay,
not a commit or a live-tree sync. All 438 source-file digests were verified on
Spark, along with the unchanged accepted comparator. Manifest SHA-256:
`5286a11a90f34bc8706911e06d9376d9a5d18aaa5e6767445a61ac15dc94a32e`.
This first job rebuilds the CUDA backend/STAR and runs the strict 20M slice gate.
It explicitly keeps `STAR_INTEGRATE_THP=1` and historical
`STAR_INTEGRATE_DROP_INDEX_CACHE=1`; it does not settle cache policy.
Full-depth integrated parity, GPU Tier-0 and performance remain pending.

## Astra implementation handoff (historical review boundary below)

Reviewed Terra's uncommitted changes against accepted `89f005c`, the original
`docs/review-integrate-lat-prefetch.md`, and pinned STAR 2.7.11b source. This is
Astra's narrow contract review. **Terra's B1/S1 and portions of S2/S5 are verified;
Astra's residual repairs and new fixtures require orchestrator final review.**
This document does not independently approve its author's implementation.
No source/golden conflict was observed; no external golden was opened.

| Item | Terra / accepted behavior independently checked | Final local status / authorship |
|---|---|---|
| B1 | Terra drains the excluded whole-window queue entries, resolves them, and releases owners outside `State::mu`. No pool member reorder. | Verified. Strengthened the stopping fixture to be deterministic and added a backend blocked until `finish()` sets stopping, with all seven producer workers already closed. Both cases finish with empty queues/zero charges and ordinary process teardown, also under ASan. |
| B2 | Already fixed in accepted HEAD: unacquired admission charges are cleared. | Verified with current and next refusal tests; no new charge repair. |
| S1 | Terra records positions immediately after each admitted frame. | Verified with actual producer post-read byte-budget rejection, exact endpoint positions/ordinal, rotation, and every loaded record. Accepted producer fails the endpoint assertion. |
| S2 | Terra checks failed rollback and marks partial EOF/mismatched tails unusable. | Those changes verified. **Astra implementation:** refuse real-cursor restart while a terminal window remains live; remember an empty successor on its live predecessor after successful restoration. Both residual cases had executable RED before repair. |
| S3 | Terra skips frames strictly below the pre-`oneRead` ordinal. | Incomplete: equality reproduced a stranded cursor. **Astra implementation:** skip `<=` previous stock ordinal in both current and rotated windows. Actual generated sequence recovers on the very next loaded read; a forward gap still waits for its exact frame. |
| S4 | Existing “pool” test was rotation, and prefix test never called the producer. | **Astra tests:** separate exact-pointer reuse/reset and retained-owner regression; linked generated runtime contract fixture. Existing rotation remains separate. |
| S5 | Terra attributes retirement to the supplied window and charges whole-chain statistics only on first consumption. | Both independently verified by two-step and completed-next-window tests with selective pre-fix RED. **Astra implementation:** publish CPU-resolution/admission reason bits and report separate fallback reasons, retaining existing sidecar field names. |

## What the runtime fixture actually executes

`test_window_contract.py` exports the generator from Git **89f005c**, SHA-256
`9375a851f6f9f30999bb02f10d9c94001053b95fd4c256ba8c95003a69d0be1f`.
It reads `/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source` without
modifying it. The compiled executable links these real paths:

- The accepted generated `mapChunk` statements `prepare_window(*this)` then
  `RA->oneRead()`, with a read-only observer between them. The fixture invokes
  this extracted iteration repeatedly; it does not run STAR's output-buffer loop.
- The generated `oneRead` prefix from entry through `readFileType`, including
  real per-mate `readLoad`, mate-status consistency, paired length checks,
  `handoff_read1`, and the original combine/complement/reverse fallback.
- The accepted generated `mapOneRead` entry through `begin_map`. The observer
  ends execution before seeding/alignment/output.
- The actual `star_integrate_window.cpp` and coordinator TU, STAR class layouts,
  `readLoad.cpp`, `ClipMate_clip.cpp`, `SequenceFuns.cpp` (including
  `qualitySplit`), and supporting STAR source objects.

Test constructors allocate input buffers instead of initializing a production
index/alignment engine. `exitWithError` preserves stock diagnostic text but
throws to the fixture boundary, which drains and returns 42 normally; this
intercepts fatal process control, not parsing. The backend stub rejects any
unexpected dispatch. Queued producer work is CPU-drained at ordinary fixture
shutdown. Separate coordinator tests use deterministic fake USI results for
multistep lookup and delayed transport. None of these are device/parity evidence.

No hand-built frames substitute for producer budget admission. `budget` feeds
1,800 paired chunk records, with distinct names/ordinals and four distinct
paired sequence patterns, using unchanged production byte/candidate limits and
no test window-length override. Observed admitted endpoints:

```text
budget window: frames=831 first=1 last=831 end=832
budget window: frames=831 first=832 last=1662 end=1663
```

Both mate seek positions must equal the end of the last admitted record, not
the later read which exceeded the byte budget. Every loaded ordinal in this
case must obtain its matching frame; at least two real prefetch rotations are
asserted. The fixture checks original sequence, quality, name, extra header,
filter, file index, lengths, all three numeric orientations, and the real stream
position/state at every generated pre-read handoff. It also runs a partial tail,
a full tail with empty successor, EOF and failbit independently on each mate,
termination mismatches in either direction, injected second-mate rollback
failure, admission refusal, and a second chunk on the same worker. Gap/overshoot
injection changes only the cursor of real producer frames; subsequent calls
follow the generated pre-read sequence, not a hand-called post-read ordinal.

The exact-pointer pool test removes real SPSC queue owners deterministically.
It holds the first window with both a queue reference and a retained shared
pointer, closes it, submits a distinct second window, then releases the second
and both first-window owners. The third submission must reacquire the exact
first pointer with fresh fields, job atomics, ranges/cursors, frame/candidate
pointers and live charges. This is separate from asynchronous rotation and the
delayed-backend lifetime test.

## Accounting meaning and remaining omissions

Existing sidecar keys remain available. `miss_reasons.cpu_admission` counts
lookup fallback events for jobs CPU-resolved because `submit_window` could not
publish. `miss_reasons.cpu_resolved` counts lookup fallback events for other CPU
resolution (including tails, shutdown, and disabled/faulted dispatch).
`device_stopped` now excludes these CPU publications; it still includes invalid
device results and exhausted/stopped device chains. Its historical values must
not be retroactively interpreted using the new definition. CPU publication
sets reason bits with the completion release operation and preserves an existing
retirement bit; no additional per-step locking or job-size increase is needed.
The status histogram remains device-only and may record both chain and step
status for one event. All miss reasons, including the two additions, contribute
to `key_misses` except `no_window`.

Two successful steps charge one chain's 123 bytes/17 gathers once. A completed,
unused next-window job contributes 77 bytes/9 gathers to that window at close,
even after current-window removal. **These counters are not a disjoint partition
of submitted work.** A not-ready miss may later also become unused; retirement
before publication has no unused backfill; rejected or exhausted chains do not
provide a full strict step/flag comparison. No expensive race-accounting scheme
was added. Host24's old status-zero events are not assigned exact new counts.

## Executable evidence

Evidence directory: **`/tmp/lat-contract-evidence/`** on this Mac. Saved Terra
snapshot: `/tmp/lat-contract-before/`; accepted implementations:
`/tmp/lat-contract-accepted/`. Source digests are in
`final-source-sha256.json`. This is a local correctness run, not a measurement.

| Regression | RED implementation / result | GREEN |
|---|---|---|
| Budget endpoint | Accepted coordinator/producer verbatim, with additive current header fields for assertions; fails mate endpoint equality | Pass |
| Failed second-mate rollback | Same accepted implementation; reaches fixture's “rollback must abort” assertion instead of required fatal diagnostic | Pass: SIGABRT and `input lookahead rewind failed` |
| Stale equality | Saved Terra implementation; generated next read has no current frame | Pass |
| Live terminal restart | Saved Terra implementation; duplicate next window appears | Pass |
| Empty successor | Current implementation before the exhaustion-marker repair; live tail still advertises successor | Pass |
| B1 queue drain | Selectively restore accepted stopping branch; nonzero live charges after finish | Pass, ordinary + delayed backend + ASan |
| Whole-chain stats | Selectively restore pre-fix per-step charging; two-step stats assertion fails | Pass |
| Next-window retirement | Selectively restore pre-fix current-window attribution; next unused stats assertion fails | Pass |
| CPU admission reason | Selectively restore pre-fix device-stop classification; reason assertion fails | Pass |

Selective reversions preserve new declarations required to compile the new
fixture; they are not claimed to be byte-for-byte historical binaries.
`make_red_variants.py` in the evidence directory constructs those variants.
Pool reuse, already-fixed B2, normal tails/stream states and successful rotation
are coverage tests; no historical defect is asserted for each of those cases.
All RED failures above are runtime assertions/diagnostic mismatches, not compile
failures. Logs are `red-{budget,rollback,overshoot,tail,empty,shutdown,stats,retirement,cpu-reason}.log`.

GREEN commands (all exit 0):

```sh
python3 bench/star-integrate/test_window_contract.py
python3 bench/star-integrate/test_coordinator.py
python3 bench/star-integrate/test_window_prefix.py
python3 bench/star-integrate/test_source_patch.py
python3 bench/star-integrate/test_coordinator.py --asan --case shutdown-drain --case shutdown-delayed
```

The contract runner passes 13 subprocess cases, including a rerun with inherited
`STAR_INTEGRATE_WINDOW_TEST_MAX=1` (the fixture clears it before selecting cases,
so the budget test still uses production limits). The source/ABI test passes all 8
tests with no skips. Existing generated STAR VLA-extension warnings remain.
ASan logs contain both `queued=0 live_requests=0` rows, with CPU tails 280,000
for stopped-before-dispatch and 40,000 for the delayed 240,000-job dispatch.
No `_Exit`, pool-layout workaround, or suppressed sanitizer failure is used.
C++ files were clang-formatted; Python fixtures were Black-formatted. No Rust
files were changed. Scoped `git diff --check` and formatter checks pass.

For reproduction, both runners accept `--source-dir`; the contract runner also
accepts repeated `--case`, e.g. `--source-dir /tmp/lat-contract-accepted --case
budget`. Coordinator selective reversions use `--source-dir
/tmp/lat-contract-red-stats --case chain-accounting`, or `red-shutdown` with
`--case shutdown-drain`. Expected RED runners exit nonzero.

## Final-review boundary and files

Astra changed only these authorized files (some retain Terra's earlier edits):

- `bench/star-integrate/star_integrate.cpp`
- `bench/star-integrate/star_integrate.hpp`
- `bench/star-integrate/star_integrate_window.cpp`
- `bench/star-integrate/test_coordinator.cpp`
- `bench/star-integrate/test_coordinator.py`
- `bench/star-integrate/test_window_prefix.cpp`
- `bench/star-integrate/test_window_prefix.py`
- `bench/star-integrate/test_window_contract.cpp` (new)
- `bench/star-integrate/test_window_contract.py` (new)
- `docs/review-integrate-lat-prefetch-fix.md` (new)
- `bench/PHASE2C-lat-prefetch-fix.md`

All requested local contract cases are implemented. **NOT RUN:** strict host
slice/full-depth ordered-output parity, Spark GPU/CPU Tier-0, performance,
CUDA error-drain behavior, or repository-wide Rust gates. The orchestrator
reports the stock full-depth golden complete at **78,619,701 pairs**; it was
not accessed or regenerated. Final independent review of Astra's repairs and
candidate/runner host gates remain with the orchestrator. No SSH, GPU,
production data, credentials, generator/runner edits, comparator changes,
board/card updates, commit, or push were performed.
