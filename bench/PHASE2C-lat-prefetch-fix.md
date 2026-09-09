# P2C LAT/PREFETCH contract-fix handoff

The local contract gap now has executable regressions and targeted repairs.
This supersedes the earlier partial Terra handoff. Independent assessment and
implementation ownership are recorded in
[`docs/review-integrate-lat-prefetch-fix.md`](../docs/review-integrate-lat-prefetch-fix.md).
Astra verifies Terra's fixes but does not independently approve Astra's own
residual implementation; orchestrator final review is still required.

Evidence directory: `/tmp/lat-contract-evidence/` on the Mac. No timing,
throughput, CUDA parity, or ordered-output-parity claim is made.

| Item | Final local result |
|---|---|
| B1 | Terra's all-queues drain verified. Seven closed producers × 40,000 jobs; normal teardown and delayed backend both pass, including ASan. |
| B2 | Accepted HEAD's refused-charge clearing retained and verified for current and next windows. |
| S1 | Actual `prepare_window` hits the production post-read byte budget; endpoint ordinal and both stream positions match the last admitted frame. |
| S2 | Terra rollback/EOF fixes verified. Astra prevents duplicate live-tail lookahead and records an empty successor on its live predecessor. |
| S3 | Terra's `<` skip was incomplete. Astra's `<=` skip recovers equality with the previous stock ordinal through actual generated pre-`oneRead` order. Forward gaps remain exact-only. |
| S4 | Separate exact-pointer pool reuse/reset/retained-owner test; runtime generated stream fixture now linked and executed. |
| S5 | Terra's once-per-chain stats and supplied-window retirement verified. Astra separately counts CPU admission/other CPU-resolution misses and actual device stops. No partition claim. |

The stream fixture exports the **89f005c generator**, SHA-256
`9375a851f6f9f30999bb02f10d9c94001053b95fd4c256ba8c95003a69d0be1f`, and reads
pinned STAR 2.7.11b source without changing it. It executes the generated
pre-`oneRead` statements, generated loading/handoff prefix, generated
`mapOneRead` entry, real `readLoad`/clipping/quality splitting, and actual
producer/coordinator. Alignment and output code are outside this fixture;
test constructors allocate input buffers, fatal diagnostics are intercepted at
the subprocess boundary, and backend/index work is stubbed. Full details and
limits are in the review.

The production-budget fixture consumes every one of 1,800 paired records and
checks original records, metadata, all three numeric orientations, frame
identity, and stream state/positions through rotation. Raw admitted endpoints:

```text
budget window: frames=831 first=1 last=831 end=832
budget window: frames=831 first=832 last=1662 end=1663
```

The 13 stream subprocess cases cover budget rejection, partial tail, empty
successor, forward gap, stale overshoot, admission refusal, each mate's EOF and
failbit, termination mismatch in either direction, and failed second-mate
rollback. Tail/refusal also exercise another chunk on the same worker.

Counter meanings: `miss_reasons.cpu_admission` counts lookup fallback after
failed window publication; `cpu_resolved` covers other CPU publication.
`device_stopped` excludes those events while keeping the existing field name.
Historical counts cannot be relabeled exactly. Whole-chain stats are charged
once, step counts remain per continuation, and completed next-window unused
work is attributed to that window. Not-ready/unused overlap and
retire-before-publication omissions remain documented; no disjoint accounting
of device work is claimed.

Commands and results:

| Command | Result / evidence log |
|---|---|
| `python3 bench/star-integrate/test_window_contract.py` | Pass, 13 cases; `green-stream.log` |
| `python3 bench/star-integrate/test_coordinator.py` | Pass, existing cases plus reuse, two-step accounting, delayed shutdown; `green-coordinator.log` |
| `python3 bench/star-integrate/test_window_prefix.py` | Pass helper/orientation fixture; `green-prefix.log` |
| `python3 bench/star-integrate/test_source_patch.py` | Pass, 8 tests, no skips; `green-source-abi.log` |
| `python3 bench/star-integrate/test_coordinator.py --asan --case shutdown-drain --case shutdown-delayed` | Pass, ordinary teardown in both cases; `green-asan.log` |

Raw shutdown rows, identical in ordinary and ASan runs:

```text
shutdown drain: queued=0 live_requests=0 tails=280000
shutdown delayed: queued=0 live_requests=0 tails=40000
```

Executable RED was observed for accepted producer endpoint/rollback behavior,
saved Terra stale-equality/tail behavior, and the empty-successor behavior
before Astra's marker repair. Selective pre-fix reversions separately fail the
shutdown, per-step statistics, next-window attribution, and CPU-reason tests.
These are runtime assertion failures, not compile failures. The review lists
exact snapshots, variant construction, replay arguments and `red-*.log` files.
Source hashes are saved in `final-source-sha256.json` in the evidence directory.
C++ was clang-formatted and Python Black-formatted; scoped diff/formatter checks
pass. Existing upstream VLA-extension warnings are unchanged. No Rust changes.

Files touched: the seven pre-existing owned integration/test files
(`star_integrate.cpp`, `.hpp`, `star_integrate_window.cpp`,
`test_coordinator.cpp`, `.py`, `test_window_prefix.cpp`, `.py`), new
`test_window_contract.cpp`/`.py`, the new review, and this handoff. No generator,
source-patch test, measurement/cache/perf/full-depth runner, comparator,
board/card, or host artifact was changed by Astra.

**Remaining gates:** orchestrator independent review of Astra's implementation,
strict host slice and full-depth ordered-output parity on the final candidate,
Spark GPU/CPU Tier-0, and performance. These are **NOT RUN** here, as are
repository-wide Rust gates and CUDA error-drain testing. Per orchestrator, the
stock full-depth golden is already complete at **78,619,701 pairs**; it was not
accessed or regenerated. No SSH, GPU, production inputs, credentials,
commit, or push.
