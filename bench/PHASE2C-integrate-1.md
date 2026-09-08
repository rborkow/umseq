# P2C INTEGRATE-1 — gate (i) passed; gate (iii) failed on a fixed setup cost, then ~25% slower

Evidence: `~/uni-rnaseq-probe-lab/integrate-gate-host5/` (gate i), `integrate-timing-host1/`
(timing), `/tmp/integ-perf/` (profile). Source `dd6877d`.

## Gate (i): correctness — PASSED

Full ERR188140_20M, 20 threads, strict mode (the stock CPU function re-run on every consumed
GPU result, abort on mismatch):

| | |
|---|---|
| parity vs stock | **`PARITY_MATCH`** — 53,710,530 SAM records incl. order and all tags; SJ.out.tab; non-timing Log.final.out |
| requests submitted to GPU | 141,557,760 in 1,710 batches |
| **consumed** (GPU result used by STAR) | **129,552,266 (91.5%)** — 1.28 G dependent gathers served in place |
| suppressed-reverse unused | 12,005,494 (8.5%; CHAIN-POSITION predicted 13.4M) |
| key misses → CPU | 14,781,042 (frames turned over before consumption) |
| CPU tails (underfilled batches) | 16,153,620 |
| strict mismatches / faults / rejections | **0 / 0 / 0** |

STAR with its inner seed search on the GPU, reading the resident 30 GB index through ATS,
produces byte-identical alignments. That is the correctness result of the whole P2C lane.

## Gate (iii): throughput — FAILED as built

Three rotated repeats, same input, `%U+%S`:

| arm | wall | user+sys CPU-s | mapping phase | vs stock |
|---|---|---|---|---|
| stock (parity-accepted pinned build) | 54.8 s | 753 | 47 s | — |
| integrated, `STAR_INTEGRATE` unset (hooks compiled, bypassed) | 58.9 s | 805 | 50 s | **+7% CPU** — window/hook overhead |
| integrated, GPU on, strict off | **303 s** | **1,072** | 290 s | +42% CPU, 5.5× wall |

### Why: a 30 GB SHA-256 inside the mapping phase

`perf` on a 3M-read run: **44% of all samples in `ssir::Sha256::block`**, called from
`star_integrate::setup()` ← `prepare_window()` — i.e. on the *first window*, after "Started
mapping". `setup()` hashes Genome + SA + SAindex (30 GB) single-threaded to bind the
`UsiIdentityV1` to the loaded arrays. Timeline from the `umem` allocation reports (r2-gpu):
mapping started 13:25:38 → first `umem` allocation 13:28:15 → first GPU batch 13:29:24. **~225 s
of the 290 s mapping phase is setup**, during which all 20 workers are blocked. The contract
asked for identity binding "against hashes of the loaded resident bytes, not directory names";
the implementation did it in the worst possible place.

Subtracting setup: **~60–65 s of real mapping vs stock's 47 s** — still ~25–35% slower.
Candidates for the remainder, from the same profile (excluding SHA): `submit_window` 7.1%,
`complementSeqNumbers` + `convertNucleotidesToNumbers` 11% (the window re-prepares read bytes
STAR has already prepared), `__aarch64_cas4_acq` 4.8% (coordinator lock), `lookup` 2.7%,
`memcpy` 2.6%, `malloc` 2.2%. The 64k-minimum batch (1,481 of 1,657 batches) and the
synchronous drain are the structural part.

## What this means

- The GPU offload works and is correct; it saved ~55% of the seed-search CPU-s it touched
  (1.3 G gathers that stock would do on the CPU). The *measured* pipeline number is negative
  because of one setup bug and a first-cut coordinator.
- The decision rule stands: <5% with parity intact is an honest negative — **for v1 as
  built.** Two bounded fixes are cheap and were always part of the design's "measure then
  choose" plan; run them before calling the hypothesis.

## Next (one card, then re-time)

1. Move identity binding out of the mapping phase: hash in `setup()` called from STAR's
   `genomeLoad` completion (or accept the file-size + first/last-block + sampled-block
   identity the probe already validated; a full re-hash of 30 GB per STAR invocation is
   not a deployment-shaped cost either way). Target: setup < 5 s.
2. Stop re-preparing read bytes in the window: STAR's `Read1[0..1]` are already numeric
   and complemented at `mapOneRead` entry; borrow them. Target: `complementSeqNumbers` /
   `convertNucleotidesToNumbers` at stock share.
3. Re-time (same script, `run_timing_host.sh`). If bypass-arm overhead (+7%) is not also
   reduced, the ceiling is ~1.08× on STAR, not the 1.15× projected; report whichever it is.

## Round 2 (after INTEGRATE-1-SETUP, `efef1a1`): the honest number

Gate (i) again `PARITY_MATCH`, 129,971,534 consumed. Timing (`integrate-timing-host2`, 3 rotated repeats):

| arm | wall | startup | mapping | user+sys CPU-s | vs stock |
|---|---|---|---|---|---|
| stock | 56.8 s | 9 s | 47 s | **753** | — |
| integrated, hooks bypassed | 59.6 s | 9 s | 50 s | 800 | +6.3% |
| integrated, GPU on | 145 s | **75–91 s** | **62 s** | 894 | +18.8% |

Setup is now *before* mapping and separable: 65–91 s (`setup_wall_s`), because the sampled
identity scheme still *reads the index files from disk* to compare against resident bytes —
the cost is 30 GB of file I/O, not hashing. Fixable (compare resident-to-resident; the file
identity is already established by STAR's own load), and not part of the mapping number.

**Subtracting setup, the GPU arm is 800 CPU-s — identical to the hooks-bypassed arm.**
The GPU served 1.3 G dependent gathers (~130 CPU-s at stock's measured per-gather rate) and
the coordinator spent exactly that much: mapping wall 62 s vs stock 47 s, with workers waiting
on synchronous 64k batches; read re-preparation, `submit_window`, the coordinator lock, and
copies (round-1 profile) account for it.

**INTEGRATE-1 verdict: net zero CPU, −30% wall. The kernel's 6.6× is real and the alignments
are byte-identical; mechanism (a) with a synchronous coordinator gives the whole gain back.**
This is design risk #3 ("replay can overstate realizable batching"), measured. It is not a bug
to fix in this card: the +6.3% lookahead floor and the synchronous drain are the mechanism.

What a v2 would have to do differently (not authorized here): asynchronous double-buffered
batches so workers never block on the drain; borrow STAR's already-prepared `Read1` bytes
instead of re-preparing; per-worker queues with a lock-free hand-off. Whether that recovers
the ~130 CPU-s is the open question; the ceiling on STAR is the design's 1.15×, and the
measured floor for the lookahead alone is −6%. Expected value of a v2 is positive but small.

## Round 3 (INTEGRATE-2, `d1b3c1d`): async coordinator — wall recovered, CPU-s not

Evidence: `integrate-gate-host7/` (gate i), `integrate-timing-host3/`,
`bench/evidence/integrate-1-host/timing-round3-raw.tsv`.

Gate (i) again `PARITY_MATCH` (53,710,530 records, strict oracle). Consumed 134.7–136.5M of
141.6M submitted (**95–96%**, up from 91.5%); **cpu_tails 0** (was 14.5M); 710–721 batches
of mostly 230k–262k (was 1,657 at 64k).

| arm (20M, 20 thr, 3 rotated repeats) | startup | mapping wall | user+sys CPU-s |
|---|---|---|---|
| stock | 7–9 s | 48–49 s | **755** |
| integrated, hooks bypassed | 8–9 s | 50–51 s | 803 (+6.4%) |
| integrated, GPU on | 79–105 s | **52–53 s** | **912** (+20.8%) |

Read against round 2 (GPU arm: mapping 62 s, 894 CPU-s):

- **Blocking is gone.** GPU-arm mapping wall 62 → 52 s, now within 3–4 s of stock (48 s).
  The async double buffer, per-worker queues, and drain-gated 256k batches did exactly what
  was asked. Workers no longer wait on the GPU.
- **CPU-s went the other way: 894 → 912.** With the coordinator no longer parking workers,
  the producer-side duplicated work (read re-preparation, prefix recomputation, per-read
  window build) runs at full concurrency — it was there in round 2 too, partly hidden as
  idle time. `sys` rose 28 → 64–75 s: the coordinator thread now spins/polls between fills
  (`cv.wait_for(100 µs)` loop) rather than sleeping on a full batch.
- **Startup 79–105 s** is still the sampled identity re-reading 30 GB of index files
  (WINDOW item 1, not landed — needs coordinator-side access). Zero user-CPU; it is I/O
  wait and would vanish resident-to-resident. Excluded from the mapping number, included
  in the honesty column.

### Where the ~157 CPU-s over stock now sits (round-2 profile, still applicable)

| source | est. CPU-s | fix | status |
|---|---|---|---|
| read re-preparation (`convertNucleotides`, `complementSeq` ×2) | ~75 | hand `Read1` from the frame (WINDOW item 3) | not landed — needs `oneRead` hook + frame access |
| `prepare_window` self (qualitySplit + prefix + candidate build) | ~70 | prefix-skip on hit (WINDOW item 5) | not landed — riskiest |
| coordinator poll/`sys` | ~40 | sleep on a fill-or-drain event instead of 100 µs `wait_for` | small, next |
| hook floor (bypass arm) | ~48 | counters now compile out; residual is `set_chain`/`inner_call` on the disabled path | partially landed |
| GPU-served seed search | **−130** | — | landed, working |

Sum of the not-landed rows ≈ 185 CPU-s against a GPU credit of 130. **The integration
pays for the GPU's work roughly 1.4× over in duplicated CPU work**, all of it enumerated,
none of it in the kernel or the memory path.

### Verdict after three rounds

- Correctness: settled. Three parity passes at 91–96% GPU coverage, strict oracle, byte-
  identical output, zero faults.
- Scheduling: settled. Async coordinator holds mapping wall to within ~8% of stock.
- Throughput: **not achieved.** Gate (iii) target was ≥12% STAR CPU-s reduction (≥8%
  memo-grade); measured **+20.8%**, i.e. −29 points from the bar. The remaining gap is
  the lookahead computing what STAR computes again. Closing it means the hooked STAR must
  *consume* the window's prep (reads, prefix) rather than merely check its answers — a
  deeper source patch than INTEGRATE-1's "insert a lookup at the call site" contract, and
  the item the design flagged as riskiest for parity.

The P2C thesis stands where round 2 left it, sharper: unified memory makes the GPU a
correct in-place seed-search engine (6.6× at the boundary, in production STAR); the
realizable throughput depends on how much of STAR's own per-read work the lookahead can
*replace* rather than duplicate. That is a STAR-refactoring question, not a memory one.

## Round 4 (INTEGRATE-3, `3f80471`): Read1 hand-off — small gain; a correction to round 3's attribution

Evidence: `integrate-gate-host8/`, `integrate-timing-host4/`,
`bench/evidence/integrate-1-host/timing-round4-raw.tsv`, `timing-round4-r*-gpu-stats.jsonl`.

Gate (i): `PARITY_MATCH`, fourth time, with STAR now consuming the window's `Read1[0..2]`
bytes for every read (`read1_fallback = 0`); 134–136M consumed (96%), 0 CPU tails.

| arm (20M, 20 thr, 3 rotated repeats) | mapping wall | user | sys | user+sys |
|---|---|---|---|---|
| stock | 47–48 s | 726 | 27 | **753** |
| integrated, hooks bypassed | 50–51 s | 775 | 27 | 802 (+6.5%) |
| integrated, GPU on | 52–53 s | 829 | 69 | **898 (+19.3%)** |

Item 1 (`Read1` hand-off) removed **14 user-s**, not the ~75 estimated from the round-2
profile. The estimate double-counted: the profile's `convertNucleotides`/`complementSeq`
share included the window's *own* conversion, which still has to happen once; only STAR's
second copy was removable, and it is cheaper than its symbol share suggested.

**Correction to round 3:** the excess `sys` (~40 s) is not coordinator polling. Across the
three repeats `sys` tracks `setup_wall_s` (76.6/80.2, 66.8/69.1, 64.0/70.3): it is the
sampled identity check reading 30 GB of index files through the page cache
(`copy_to_user`). Item 2 (event-driven coordinator) therefore had nothing to recover there
and measured nothing; the poll was cheap. Round 3's "~40 CPU-s coordinator poll" row was
a wrong hypothesis and is withdrawn. Binding the identity resident-to-resident would
remove that 42 s of `sys` and ~70 s of startup wall; it is separable from mapping and
was never landed (needs the coordinator's view of the loaded index).

Mapping-phase CPU-s with the setup `sys` excess excluded: **856, +13.7%** over stock. That
is the number to carry: what the hooked STAR costs per sample once the identity bug is
fixed, with 96% of seed search on the GPU.

### Final accounting after four rounds

| | CPU-s vs stock |
|---|---|
| hook floor (lookahead: `readLoad`+combine, `qualitySplit`, prefix, candidate build; bypass arm) | +49 |
| coordinator + consumption on top of the floor | +54 |
| GPU-served seed search | −(what the +103 net leaves unrecovered) |
| **net, mapping phase** | **+103 (+13.7%)** |
| identity setup (separable, fixable) | +42 sys |

The GPU takes ~39% of stock's seed-search CPU off the host (probe: 6.6× on real requests)
and the lookahead puts ~1.8× that back. The biggest remaining single item, STAR's prefix
recomputation on hit (item 3, ~70 CPU-s by the same profile that overestimated item 1
by 5×), was not attempted; even at face value it would land at ~+4%, not −8%.

### Verdict

Gate (iii) **failed** after four rounds: best mapping-phase result +13.7% CPU-s vs the
≥−8% memo bar; mapping wall within 10% of stock. Gate (i) passed four times.

What is established, each with raw evidence:
1. The seed-search kernel over the resident 30 GB index is correct and fast: 6.6× on 1M
   real requests, byte-identical tuples, and now byte-identical alignments from
   production STAR at 96% coverage.
2. A lookahead that re-derives STAR's per-read state to build GPU requests costs more
   than the GPU saves, because STAR's seed search is ~39% of a read's CPU and the
   lookahead's re-derivation plus consumption is ~a third of a read's CPU on its own.
3. The only remaining path to a net gain is inverting the integration: STAR's own
   `ReadAlign` loop produces requests as a side effect of the work it already does,
   and a *deferred* second pass consumes results — i.e. restructuring STAR's read loop,
   not hooking it. That is a different, larger project, and this evidence is what
   would justify or kill it.

PIPELINE-RUN is not funded under the ≥8% rule. Deployment recommendation unchanged from
`bench/COST-CURVE.md`: the umbam chain is the product; the STAR seed lane is a proven
kernel awaiting an integration architecture.

## Round 5a (INTEGRATE v2 T1, `91981c8`): floor to zero, GPU arm +2.3% mapping-phase

Evidence: `integrate-gate-host10/`, `integrate-timing-host5/`,
`bench/evidence/integrate-1-host/timing-round5a-raw.tsv`, `timing-round5a-r*-gpu-stats.jsonl`.

Gate (i) `PARITY_MATCH` (fifth). 134–136M consumed (96%), 0 CPU tails, positional consumption
(`key_misses` 8–10M are the frames whose candidate the CPU fell back on; no hash).

| arm (20M, 20 thr, 3 rotated repeats) | startup | mapping wall | user | sys | user+sys |
|---|---|---|---|---|---|
| stock | 8–10 s | 47–48 s | 726 | 28 | **754** |
| integrated, hooks bypassed | 7–8 s | 46–47 s | 728 | 26 | **753 (−0.2%)** |
| integrated, GPU on | 74–90 s | **47–49 s** | 743 | 74 | 818 (+8.4%) |

- **Bypass floor gone**: +6.5% → −0.2%. `compared()` out of the inner loop and `enabled_fast()`
  gating did what Task 0 said they would.
- **GPU-arm user CPU 829 → 743 (−86 CPU-s)**: positional consumption removed `lookup`'s hash
  (Task 0 said ~45 at 20M) plus the floor (~48). Mapping wall now equals stock.
- **Mapping-phase CPU-s: 772, +2.3%** over stock (user 743 + stock's sys 28). The GPU arm
  is now within noise of stock on the mapping phase, with 96% of inner seed searches on the
  device. Gate (iii) bar is −8%; T3 (prefix on device) is the next measured step.
- **Setup did *not* improve** (74–90 s wall, ~46 sys CPU-s). Item A removed STAR-side file
  sampling, but the dominant cost was never that: `usi_init` → `probe_load`
  (`crates/umseed-probe/src/index.rs:91-213`) reads Genome/SA/SAindex **from disk into a
  second 30 GB resident copy** on every run. The identity check was the small part. Fix
  is below; it's the last fixed cost and it is not a hashing problem.

### The second index copy

T6 (`docs/design-phase2b.md:54-60`) established that on GB10 a plain host `mmap` buffer is
device-accessible through HMM/ATS with no registration. STAR's own `Genome`/`SA`/`SAindex`
are `new char[]` (`Genome_genomeLoad.cpp:272-289`, `PackedArray.cpp:32`) — also plain host
memory, also device-accessible. The kernel could gather from **STAR's arrays directly**: no
second copy, no 74–90 s load, 30 GB less RSS. The open question is page size: T6's rate was
measured on THP-backed `umem`; the probe's 4K-page control ran at 0.07× (P2C-SEED-GPU-PROBE).
`new char[30 GB]` under glibc is `mmap`'d and gets THP only if `transparent_hugepage=always`
or STAR calls `madvise` (it doesn't). Measured next, not assumed: the same probe over STAR's
arrays (a) as-is, (b) after `madvise(MADV_HUGEPAGE)` + a touch pass, vs the `umem` copy.

## Round 5b (INTEGRATE v2 T3 + T3B, `ce2b514` + `5cb4e5b`): the GPU arm is below stock

Evidence: `integrate-gate-host11/` (gate i, V2 contract, strict outer oracle),
`integrate-gate-host11.replay.log` (999,914-request from-scratch replay),
`integrate-gate-host11.prefix-config.bin` (the config the integrated STAR loaded),
`integrate-timing-host6/`, `bench/evidence/integrate-1-host/timing-round5b-*`.

**Gate (i)**: `PARITY_MATCH` (sixth), now with the SAindex prefix walk and branch selection on
the device and strict mode running stock's *full outer body* (`ind1` → walk → branch →
search) on the CPU and comparing `(maxL, Nrep, indStartEnd[0], indStartEnd[1])` + `Read1`
bytes for every consumed request. Zero mismatches, zero rejections.

**From-scratch replay** (`prefix_replay`, CUDA): device given only `(read bytes, S, N, dir)` and
the loaded config → **999,914 / 999,914 STAR tuples matched**, all via the `searched` branch
(the corpus is captured *inner* calls, so no prefix-only/unique cases by construction; those
are covered by the synthetic grid oracle and by the 20M strict gate: 830–858k prefix-only
and 1.2M unique consumed per run, all strict-verified).

| arm (20M, 20 thr, 3 rotated repeats) | startup | mapping wall | user | sys | user+sys |
|---|---|---|---|---|---|
| stock | 7 s | 48 s | 726 | 27 | **753** |
| integrated, hooks bypassed | 7–10 s | 48–50 s | 740 | 27 | 767 (+1.8%) |
| integrated, GPU on | 68–71 s | **45–47 s** | **700** | 63 | 763 (+1.3%) |

- **Mapping-phase CPU-s: 727, −3.6% vs stock** (user 700 + stock's sys 27; the 36 s of sys
  excess is the second index copy at setup, unchanged, addressed below). First round on the
  right side of zero. Mapping wall 45–47 s vs stock 48.
- Round 5a → 5b: GPU-arm user 743 → 700 (−43 CPU-s) — the outer work leaving the CPU, close
  to Task 0's ~40 estimate for `maxMappableLength2strands` self at 20M.
- Bypass floor rose −0.2% → +1.8%: the V2 hook sits above STAR's prefix block and
  its disabled-path branch is inside the per-call path now. ~14 CPU-s; a Terra item.
- Coverage 88% of a larger submitted set (152.5M vs 141.6M: the window no longer filters
  prefix-only/unique candidates, ~2.1M served); `cpu_fallback` 63–67M are continuations and
  key misses — T4's target.

### Accounting, round 5b

| | CPU-s vs stock (20M) |
|---|---|
| GPU-served outer+inner seed search (96% initial starts) | ≈ −(56×0.35×4.7…) — measured net below |
| bypass floor | +14 |
| consumption + coordinator | small; not separable without a new paired profile |
| **net, mapping phase** | **−26 (−3.6%)** |
| identity/second-index setup (separable) | +36 sys, ~62 s wall |

### Against the gates

- Gate (i): passed six times, oracle strengthened twice.
- Gate (iii): target ≥ −12% (memo-grade ≥ −8%). **−3.6% measured**; bar not met. What's
  left is enumerated and each item has a mechanism: continuations (T4, the 24% of gathers
  and the 63–67M `cpu_fallback`), the +1.8% hook floor, and the second index copy (setup
  wall, not mapping CPU — but it is 60+ s per sample of a 55 s run, so it gates any
  wall-time claim).

## Round 6 (INTEGRATE v2 T4 + T4B, `b2dcabe`..`bd925f9`): gate (i) passed with the whole chain on the device; timing invalid — the box is out of memory at setup

Evidence: `integrate-gate-host17/` (gate i), `integrate-timing-host7/` (aborted after r2),
`bench/evidence/integrate-1-host/v3-kernel-bench.txt`, `v3-perf-4M.txt`, `v3-mem-4M.log`.

**Gate (i): `PARITY_MATCH`** (seventh), strict running stock's full outer body **per step** for
every consumed chain: 38.8M chains / 53.6M steps consumed, zero shift/flag/step-count
mismatches, zero rejections, zero overflow (capacity 8 vs measured max 6). Three consumer
bookkeeping defects were caught by strict before this pass, each a chain-state invariant
(unconsumed chain not comparable; a chain is all-device or all-CPU; hook must set every key
field the window sets) — all in `git log`, none in the kernel.

**Timing: not a valid measurement.** GPU arm r1 mapping wall 79 s, user 923, sys 183, RSS 86 GB;
r2 sys 598 (killed). Diagnosis, in order:

1. *Kernel isolated* (`prefix_replay` bench mode, same 999,914 real requests, identical gather
   work): V2 thread 10.4 ms / 9.5e8 gathers/s; **V3 thread 18.9 ms / 5.2e8** (1.8× per step —
   the 472 B output write); V3 warp 146 ms (14×, unusable; coordinator uses thread). At 20M
   that is ~1–2 s of GPU time. Not the regression.
2. *Host profile* (4M, enabled): 33.6% `libcuda`, 35% kernel — `queued_spin_lock_slowpath`,
   `try_to_free_pages`, `swapin_readahead`, `folio_check` **under `compareSeqToGenome`**. Page
   reclaim during mapping.
3. *Memory timeline* (4M, 3 s samples): STAR loads its index 0→25 s (RSS → 25 GB); `usi_init`
   allocates the second copy at 24 s; **`MemFree` 34 → 5 → 0 GB and stays at 0 for 50 s**
   (27→78 s) while every worker is blocked in reclaim; swap touched at 87–90 s; mapping itself
   (76→96 s) is a normal 20 s.

**Root cause: two resident copies of a 32 GB index plus ~32 GB of page cache from reading it,
on a box with ~89 GB after the HugeTLB reserve.** The box is at zero free memory for the
whole setup phase in *every* round since INTEGRATE-1; V3 is worse only because its host-side
job records are 50% larger (768 vs 512 B per candidate), tipping reclaim into swap. This is
what "setup 60–90 s / sys +40" has been all along — not hashing, not file reads, not
polling. Two of my earlier attributions of it were wrong; this one has the timeline.

**Fix is T5 item B, now unblocked** (Astra exposed the raw-host launch in T4): gather from
STAR's own arrays, no second copy; drop the index files' page cache after STAR's load
(`posix_fadvise(DONTNEED)`). Expected: RSS ~66 → ~34 GB, setup → seconds, and the GPU arm's
mapping-phase number becomes measurable with V3 for the first time. Until then round 6's
mapping-phase CPU-s cannot be quoted.

## T5B part 1 — the borrowed index (`probe_borrowed`, three arms, real corpus)

Evidence: `bench/evidence/integrate-1-host/probe-borrowed-{umem,borrowed,borrowed-madvise}.txt`.
Same 999,914 real requests, V2 kernel, **every tuple identical to the STAR sidecar in all
three arms**. Index loaded STAR-style (plain anonymous allocation, file read into it) in the
two borrowed arms; today's `probe_load` second copy in the control.

| arm | gathers/s | vs umem | load wall | sys CPU-s |
|---|---|---|---|---|
| `umem` (second resident copy, today) | 7.83e8 | — | 73 s | 16.4 |
| borrowed, 4K pages | 9.06e6 | 0.012× | 42 s | 16.5 |
| **borrowed + `MADV_HUGEPAGE` before first touch** | **7.81e8** | **1.00×** | **25 s** | **3.7** |

- The GPU gathers from the aligner's own arrays at the full `umem` rate. No copy, no
  registration, no staging — T6's HMM/ATS finding applied to memory we don't allocate.
- Ordering is the whole trick: `madvise` must precede population. The first run advised
  *after* `fs::read` and got the 4K rate (0.012×, matching P2C-SEED-GPU-PROBE's 4K control).
  STAR's `new char[]` + `fread` has the same shape, so the `genomeLoad` patch is: advise
  right after `new`, before the read.
- `index_anon_huge_bytes` printed 0 in the madvise arm — a smaps-parse defect in the probe
  (the 86× rate is the evidence it took); to fix, not to argue about.
- This removes the second 30 GB copy that put the box at `MemFree 0` for 50 s in every round
  (round 6 timeline). Expected in the integrated STAR: RSS ~66 → ~34 GB, setup → STAR's own
  load, and a valid V3 mapping-phase measurement for the first time.

## Round 7 (INTEGRATE v2 T5C + coordinator fixes, `92471bf`..HEAD): borrowed index, gate passed, first valid whole-chain timing

Evidence: `integrate-gate-host22/` (gate i, V3 whole-chain contract, strict per-step oracle,
borrowed index), `integrate-timing-host10/` (3 rotated repeats × 3 arms),
`bench/evidence/integrate-1-host/{timing-round7-raw.tsv,gate-round7-host22.json,
timing-round7-r1-gpu-stats.jsonl,round7-*.log}`.

**Gate (i)**: `PARITY_MATCH` (ninth; 53,710,530 records), whole chain on the device, GPU
gathering from **STAR's own `G`/`SA`/`SAi` arrays** — no second index copy, `setup_wall_s`
0.34 (was 60–90), `index_anon_huge_bytes` 30.2 GB inside STAR's process. 160.0M chains
submitted, **140.7M consumed (88%)**, 193.8M steps strict-verified, zero shift/flag/step-count
mismatches, `cpu_tails` 0, 8.0M key misses (5%).

| arm (20M, 20 thr, 3 rotated repeats) | startup | mapping wall | user | sys | user+sys | max RSS |
|---|---|---|---|---|---|---|
| stock | 7–24 s | 47–49 s | 723 | 31.5 | **754.6** | 31.2 GB |
| integrated, hooks bypassed (= stock + `MADV_HUGEPAGE` on the index) | 2–25 s | 40–41 s | 643 | 18.9 | **661.9 (−12.3%)** | 31.2 GB |
| integrated, GPU on | 24–30 s | **40 s** | **593** | 62.1 | **654.7 (−13.2%)** | 33.6 GB |

Raw rows (`timing-round7-raw.tsv`): stock 725.20/27.09, 722.15/35.20, 721.91/32.16; bypass
643.75/18.43, 643.69/17.36, 641.55/20.92; gpu 591.86/62.34, 591.53/64.59, 594.62/59.24.

### Two results, and the comparator matters

1. **`MADV_HUGEPAGE` on STAR's index alone is −12.3% CPU-s** (user −80, sys −12.6), with no
   GPU involved. The bypass arm is the T5A verbatim-stock loop (round 5b: +1.8% vs stock) plus
   T5C's generator patch: `madvise` after each `new char[]` in `genomeLoad`, before the read.
   Three repeats each, 641–644 vs 722–725 user. STAR's seed search is TLB-bound on a 30 GB
   index at 4K pages; 2 MB pages fix that on the CPU as much as they did for the GPU (T5B:
   0.012× → 1.00×). A one-line patch to stock STAR, portable, needs nothing from this lane.
2. **The GPU seed search on top of that: −1.1% CPU-s vs the huge-page baseline** (−7.8% user,
   +43 s sys). Against stock it reads −13.2%, but stock is the wrong comparator now: the
   memo-grade number for the GPU is the increment over (1), and that is marginal today.
   If the 43 s of sys excess is removable (it is not the index copy any more — setup is
   0.34 s; not profiled yet), the GPU arm is 611.6 = −7.6% vs the huge-page baseline,
   −19.0% vs stock. Profile before believing that.

Against the gates: gate (iii) target ≥ −12% STAR CPU-s. **Met against stock (−13.2%)**; the
GPU's own share of it is −1.1%. Both numbers go in the memo, labelled.

### What round 7 took (four host-side bugs, none in the kernel)

The strict oracle had verified every chain step since round 6; every failure between round 6
and this table was plumbing between STAR and the GPU, and all four were invisible until the
borrowed index removed the memory noise that had been masking them.

| bug | symptom | fix |
|---|---|---|
| `finish()` after `genomeMain.freeMemory()` | CUDA 700 at 20M, sanitizer-clean at 200k (the drain won the race) | `finish()` moved before `freeMemory()` in the generated `STAR.cpp`; order pinned by test |
| window split across the 262,144 batch cap | tail never dispatched nor CPU-resolved: 76% of chains missed | `pop_if_fits`: a window is popped whole or waits; RED/GREEN test |
| admission charge released at `close_window()` | producer closed windows the coordinator still held: 146 live windows / 18.8M queued against a 0.5 GB "charge"; RSS +2.7 GB per M reads (91 GB, `earlyoom`) | charge released by the `Window` destructor (last owner); `STAR_INTEGRATE_MEMLOG` census |
| coordinator 450 ms/batch host-side vs ~3 ms kernel | one thread could not keep 20 producers fed: 40% consumed at 4M | exact-fit batch buffers re-allocated per batch, each `probe_allocate` walking `/proc/self/smaps` (19% of the thread) and `munmap`ing its predecessor (14%); geometric growth + reused dispatch scratch + `search_chains_into` (no `to_vec`) |

Census at 4M reads, 20 threads (`round7-v3-census*.log`): batches 88 → 158 → 200; chains
consumed 5.9M → 14.3M → 28.3M (88%); key misses 32.2M → 1.4M; queued at end 11M → 0;
coordinator wakeups 3 → 632 (it sleeps now; the producers are the bottleneck); peak RSS
42.7 → 35.1 GB; CPU-s 167.8 → 135.2.

Disqualified on the way: glibc arena retention (`MALLOC_ARENA_MAX=1` made peak RSS *worse*,
72.5 GB) and the strict oracle (strict on/off identical at 47.9/47.6 GB).

### Open

- The GPU arm's +43 s sys over the huge-page baseline: profile (`perf` per-thread, as for the
  coordinator). Candidates: the remaining per-batch `smaps` reads at buffer growth (few),
  GPU-driver page-table work on ATS access to 2 MB host pages, `cuStreamSynchronize` spin.
- 12% of chains still fall back (8M key misses at 20M) — not the coordinator any more; a
  per-miss reason count is the next sidecar field.
- `posix_fadvise(DONTNEED)` after load evicts the index from page cache, so the *next* run's
  startup re-reads 30 GB (the 24–30 s startups above). Correct for a one-shot; for the
  timing harness it penalises whichever arm follows. Wall-time claims wait on that.

### Miss classification (P2C-INTEGRATE-V2-MISS)

The sidecar now keeps `key_misses` as the sum of its nine previously conflated
consuming lookup failures and reports `no_window` separately, including in
`miss_reasons`. `not_ready_where` records the coordinator phase observed with
relaxed diagnostic atomics; `device_stop_status` records both the chain output
status and the stopping step status as numeric ABI codes.

| reason | implication for a fix | 20M host count |
|---|---|---:|
| `read_bytes` | Frame/read hand-off identity or lifetime defect; inspect producer framing, not scheduling. | TBD |
| `positional_exhausted` | STAR requested more eligible candidates than the frame retained; inspect candidate enumeration/retirement. | TBD |
| `chain_rejected_residue` | Expected all-CPU remainder after an earlier fallback; fix its first cause, not this residue. | TBD |
| `no_job` | Chain bookkeeping lost its selected job; inspect initial-chain selection. | TBD |
| `key_mismatch` | Frozen identity differs from STAR's call; compare key construction and hook context. | TBD |
| `not_ready` | Producer won the race; `queued`/`filling`/`draining` distinguishes coordinator admission from dispatch/drain latency. | TBD |
| `device_stopped` | Device terminated the chain; use `device_stop_status` (8 non-ACGT, 9 overflow, 10 max steps, 11 no progress) to choose capacity/input work. | TBD |
| `shift` | Device result violates the strict shift contract; diagnostic only in non-strict mode, investigate before reuse. | TBD |
| `cas_lost` | Another path retired or consumed the selected job; inspect competing ownership/retirement. | TBD |
| `no_window` | Lookup was never admitted/prepared (or is outside its supported start); bounds recoverable coverage before any miss fix. | TBD |

## Round 7b — the huge-page ablation (review Blocking 1–2 closed)

Evidence: `bench/evidence/integrate-1-host/thp-ablation-{raw.tsv,env.txt}`,
`thp-residency.txt`, `round7-followup.txt`; review `docs/review-integrate-v2-huge.md`.
Scripts: `bench/star-integrate/thp_ablation.sh`, `thp_residency.sh`.

The review's blocking demand: same build, advice on vs off, `fadvise` kept in both, equal
cache state before every run, page backing verified per arm from the real STAR pid. Done:
the generator's `starIntegrateAdviseHuge` is now switched at run time by
`STAR_INTEGRATE_THP=0` (same binary, sha `ee8ce7d4…`); every run is preceded by a `cat` of
the three index files (so each arm's own `posix_fadvise(DONTNEED)` is neutralised for the
next); `STAR_INTEGRATE` unset in all three arms; 3 rotated repeats × 20M × 20 threads.

| arm | wall | user | sys | user+sys | vs stock |
|---|---|---|---|---|---|
| stock (`f84493ef…`) | 55.7 s | 726.8 | 27.8 | **754.6** | — |
| integrated binary, advice **off** (`STAR_INTEGRATE_THP=0`) | 56.4 s | 727.0 | 27.5 | **754.5** | **−0.0%** |
| integrated binary, advice **on** | 45.4 s | 643.9 | 20.8 | **664.8** | **−11.9%** |

Raw rows (`thp-ablation-raw.tsv`): stock 725.26/27.94, 727.22/25.92, 727.96/29.48; off
726.90/27.45, 726.08/27.32, 728.01/27.73; on 643.50/18.00, 644.81/23.22, 643.45/21.27.
All three arms' `Aligned.out.sam` bodies are `cmp`-identical (53,710,727 lines).

**Page backing, real STAR pid, sampled during mapping** (`thp-residency.txt`, 4M reads):

| arm | index VMAs | `AnonHugePages` | `THPeligible` / `VmFlags` |
|---|---|---|---|
| stock | one merged 30.46 GB anon VMA | **0** | 0 / no `hg` |
| advice off | one merged 30.46 GB anon VMA | **0** | 0 / no `hg` |
| advice on | SA 23.61 GB, Genome 3.04, SAi 1.46 (separate VMAs) | **28.0 GB** (23.54 + 3.04 + 1.42) | 1 / `hg` on all three |

Box: kernel 6.17.0-1031-nvidia, 4 KB base page, THP `[madvise]`, `hugepages-2048kB`
`[inherit]`, glibc 2.39, `GLIBC_TUNABLES`/`LD_PRELOAD` unset.

### What this settles

- **The −12% is the `madvise` and nothing else.** Advice off reproduces stock to 0.1 CPU-s
  (the hook floor is gone with T5A; the review's "don't subtract +1.8%" is moot). `fadvise`
  moves no CPU-s. Equal cache state also removed the sys skew the review flagged in round
  7's rotation (stock sys 26–29 in every position now, vs 27–35 there).
- **Stock STAR 2.7.11b on this box runs its whole 30 GB index at 4 KB pages.** Not glibc's
  hugetlb tunable, not `[always]` — a plain `new char[]` under `[madvise]` policy gets
  nothing. The user-time saving is 83 CPU-s (11.4%) on 20 threads, sys 6.7.
- The review's mechanism arithmetic (TLB reach 8 MB → 4 GB with a 2048-entry L2 TLB;
  ~25 ns saved per gather over 3.19 G consumed gathers) is consistent with 83 s; it is not
  a PMU measurement and is not claimed as one. "TLB-bound" stays a hypothesis with a
  measured effect; a walk-event PMU profile (review Suggested 5) would make it a mechanism.
- **Portability is still open** and is the memo question: whether the team's x86 Batch
  nodes run THP `[always]` (in which case stock already has this and the patch is a
  workstation fix) or `[madvise]` (in which case it is a free 12% for the fleet). Review §4
  gives the cheap check — one stock run on the Batch AMI reading the real pid's index
  `smaps`. Needs the user's AWS access; not a Spark measurement.

### The GPU arm, re-read

Against the corrected baseline (advice on, 664.8) the round-7 GPU arm (654.7) is **−1.5%**
CPU-s: user −51, sys +41. The +43 s sys is now the only thing between the GPU and a
memo-grade increment (611 CPU-s ≈ −8% over the huge-page baseline if the sys excess is
ours to remove). `round7-followup.txt` has a first per-thread profile of the GPU arm at 8M:
29% of samples in kernel mode, top user-space callers into the kernel `stitchPieces` (19%)
and `Transcript` copy-construction (11%) — STAR's own per-read allocation faulting — then
`submit_window` (13%); the review correctly notes the substring classifier is not evidence
(28.8% of kernel leaves were `[unknown]`). Next: `perf` with the privilege field, GPU vs
advice-on bypass, same 8M, per thread.

## Round 7c — sys attribution, miss classification, and the window pool (LAT 1a)

Evidence: `bench/evidence/integrate-1-host/gpu-sys.txt` (perf by DSO, GPU vs advised-bypass,
8M), `malloc-disq.txt`; cards `P2C-INTEGRATE-V2-MISS`, `-LAT`.

**Where the GPU arm's +41 s sys goes** (8M, 20 thr, classified by perf's DSO field — the
review's objection to the substring classifier stands and was acted on): kernel share
3.3% (bypass) → 9.0% (GPU), evenly across the 20 mapping threads. The user-space frame
beneath the kernel samples:

| share of GPU-arm kernel samples | user frame | meaning |
|---|---|---|
| 34.3% | `__memcpy_sve` ← `submit_window` ← `prepare_window` | first-touch faulting of a fresh window |
| 17.8% | `__munmap` ← `cfree` ← `Window::~Window` | that window's death |
| 15.7% + 7.6% + 6.9% | `writev` / `read` ← `basic_filebuf` | STAR's own SAM/FASTQ I/O (present in bypass too) |
| 4.8% | `submit_window` self | — |

Kernel leaves `_raw_spin_*`, `__pi_clear_page`, `page_counter_cancel`, `folio_remove_rmap_ptes`,
`do_page_fault`. Half the excess is a window being born and dying: `jobs` is up to 262,144 ×
544 B = 142 MB, above glibc's mmap threshold, so every window is a fresh mapping (~1,000 per
20M run).

**Disqualifier before the fix** (`malloc-disq.txt`): GPU arm at 8M with
`MALLOC_MMAP_THRESHOLD_=MALLOC_TRIM_THRESHOLD_=1 GB` (keep the windows inside the heap, no
mmap/munmap) — sys 37.4/32.0 → 32.2/30.5 s, user unchanged. **A few seconds, not the 20 the
mmap story predicted.** So the cost is the first touch of ~140 MB of fresh pages per
window (`clear_page`, the rmap/memcg bookkeeping), which the heap pays as well as `mmap`
does; only *reusing* the object avoids it. That is LAT item 1a, landed (Terra): a per-worker
pool of up to 8 `Window`s behind a `shared_ptr` custom deleter; `jobs`/`ranges`/`cursors`
keep their capacity across windows; the admission charge is still released exactly once
(an `active` flag, since the object now outlives its charge). Round 8 measures it.

**Why 12% of chains fall back** (`gpu-sys.txt`, MISS counters at 20M, strict off):

| reason | chains |
|---|---|
| `not_ready` — mapping thread reached the read before its GPU result existed | **5,352,457** |
| ↳ window still in the SPSC ring (`queued`) | 2,673,947 |
| ↳ popped, batch filling | 182,421 |
| ↳ dispatched, draining | 2,496,089 |
| `chain_rejected_residue` — later steps of the chains above | 2,011,115 |
| `key_mismatch`, `positional_exhausted`, `device_stopped`, `read_bytes`, `no_job`, `shift`, `cas_lost`, `no_window` | **0** |

The identity contract is exact and the device never stops a chain early. Every recoverable
miss is the producer racing the coordinator on the window it just submitted: the first
reads of each window are consumed before the batch has been popped, let alone drained.
Two levers, in order of cost: submit at a lower floor / age-out sooner when a queue is
non-empty (coordinator-only), or have each producer consume one window behind the one it
last submitted (`prepare_window` would peek two windows ahead of STAR's stream position;
the frame↔read match is by ordinal so the bookkeeping is already safe; the rewind
distance is the thing to verify — Terra declined it without a stream fixture, correctly).
Round 8's `not_ready_where` decides which.

## Round 8 (LAT 1a, `f6647ac`): window pool — sys excess gone, GPU arm −4.9% vs the huge-page baseline

Evidence: `integrate-gate-host23/` (gate i, tenth `PARITY_MATCH`), `integrate-timing-host11/`,
`bench/evidence/integrate-1-host/{timing-round8-raw.tsv,timing-round8-r1-gpu-stats.jsonl,gate-round8-host23.json}`.

| arm (20M, 20 thr, 3 rotated repeats) | user | sys | user+sys | vs bypass | vs stock |
|---|---|---|---|---|---|
| stock | 724.2 | 31.3 | 755.5 | — | — |
| integrated, hooks bypassed (huge-page baseline) | 643.1 | 19.0 | 662.1 | — | −12.4% |
| integrated, GPU on, window pool | **600.3** | **29.1** | **629.4** | **−4.9%** | **−16.7%** |

Raw rows: stock 725.40/28.05, 723.39/31.35, 723.95/34.42; bypass 643.51/18.78,
643.52/18.33, 642.12/19.98; gpu 599.87/29.03, 601.39/29.56, 599.76/28.61. Round 7 → 8, GPU
arm: sys 62.1 → **29.1** (−33 s; the pool removed the per-window first-touch), user 592.7 →
600.3 (+8; three repeats within 1.6 s of each other, so real — the pool's reset/lock or
the retained 140 MB `jobs` staying cache-cold; small, not chased yet). Net 654.7 → 629.4.
Peak RSS 33.6 → 34.5 GB (8 pooled windows per worker).

The GPU arm's sys is now 10 s above bypass, of which STAR's own SAM/FASTQ I/O and the
coordinator's remaining per-batch work are the candidates; the 41 s question is closed.

**Misses are unchanged, as expected** (the pool changes memory, not timing): 8.0M of 160M,
`not_ready` 5.86M (queued 3.11M / filling 0.15M / draining 2.59M), residue 2.20M, all other
reasons 0. Batch shape from the histogram: 1,001 batches, 1,000 of them 120–175k jobs — one
window per batch; fill wait p50 590 µs, p90 776 µs. So the coordinator pops each window
within a millisecond of its arrival; the "queued" half of `not_ready` is the previous
window's dispatch+drain (~40 ms per batch at 1,001 batches over a 40 s mapping phase),
and the "draining" half is the window's own. A producer consumes a window in ~800 ms
(20 workers × 40 ms); the first ~5% of each window's reads race the ~40 ms round trip.
Prefetching one window per worker removes the race at the cost of one window's memory
per worker — that is LAT item 2, now with the mechanism measured rather than guessed.

Against the gates: −12% STAR CPU-s vs stock **met** (−16.7% with the GPU; −12.4% without
it). The GPU's own increment over the huge-page baseline is −4.9%: past the "<5% honest
negative" line, short of −8% memo-grade. Recovering the 8M `not_ready` chains is worth at
most their CPU cost (5% of chains → ≈ 2–3% of mapping user time) — enough to reach −7 to
−8% if the prefetch is clean. That is the last cheap lever; after it the accounting is
what it is.
