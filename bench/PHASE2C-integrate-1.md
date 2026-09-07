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
