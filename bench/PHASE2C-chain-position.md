# P2C-CHAIN-POSITION — 76.40% initial-start gather coverage

**Decision: mechanism (a) alone goes into INTEGRATE-1. No grid20.**

On the full ERR188140_20M input, initial starts (`Lmapped == 0`) account for
**1,431,388,309 / 1,873,505,995 inner dependent gathers = 76.4015868%**.
This exceeds the user's 50% mechanism-selection threshold. Initial starts also
account for **88.7551404% of inner compared bytes**. This is observed work
coverage, not an achieved cache-hit rate or a measured accelerated-STAR gain.

## One counters-only run, stock parity intact

- Input: the same two checksummed ERR188140_20M FASTQs; 20,000,000 pairs, 20 workers.
- Private pinned STAR 2.7.11b build, default seedSearchLmax=0 and sparseD=1.
- One mapping run, no SSIR, no probe, no CUDA or new kernel work.
- Reused accepted stock output from `real-requests-host1/stock`; no stock rerun.
- Unchanged SPLIT checker: **PARITY_MATCH_COUNTERS_ONLY**, **53,710,530 SAM
  biological records**, including order and all tags; SJ.out.tab and non-timing
  Log.final.out fields match. Header normalization remains executable/prefix only.
- Inner request, gather and compared-byte totals reconcile exactly with the
  original SPLIT sidecar. Requests and bytes also equal the accepted full20M SPLIT
  totals: 188,903,274 requests and 12,615,582,076 bytes.

| Attribution | Inner requests | Dependent gathers | Compared bytes |
|---|---:|---:|---:|
| All inner work | 188,903,274 | 1,873,505,995 | 12,615,582,076 |
| Initial starts | 144,333,308 | 1,431,388,309 | 11,196,977,581 |
| Additional grid20 positions, observational | 1,956,346 | 19,360,308 | 40,210,012 |
| Initial + grid20 union, observational | 146,289,654 | 1,450,748,617 | 11,237,187,593 |
| Work outside that union | 42,613,620 | 422,757,378 | 1,378,394,483 |

The optional same-run position histogram puts the initial+grid20 union at
77.4349600% of inner gathers. Grid positions are not selected: initial starts
already clear the gate. This auxiliary membership count does not independently
execute hypothetical prefix queries or certify future speculative hits.

## Joint attribution requested by the card

`iDir` is STAR's original outer directional traversal value. A gather is one
logical dependent-SA comparison invocation, including endpoint, binary search
and expansion comparisons; it is not a hardware transaction counter.

| Lmapped == 0 | istart | iDir | Inner requests | Dependent gathers | Compared bytes |
|---|---:|---:|---:|---:|---:|
| false | 0 | 0 | 18,127,342 | 178,348,711 | 662,156,893 |
| false | 1 | 0 | 3,998,937 | 41,132,315 | 92,793,444 |
| false | 0 | 1 | 16,798,602 | 164,387,714 | 535,495,111 |
| false | 1 | 1 | 5,645,085 | 58,248,946 | 128,159,047 |
| true | 0 | 0 | 39,274,292 | 382,746,285 | 4,042,509,440 |
| true | 1 | 0 | 39,534,171 | 396,680,334 | 2,349,784,368 |
| true | 0 | 1 | 26,114,771 | 258,261,340 | 2,539,907,790 |
| true | 1 | 1 | 39,410,074 | 393,700,350 | 2,264,775,983 |

All eight distinct joint rows reconcile. Raw per-ip/iDir actual, opportunity and
suppression arrays are retained. STAR suppressed **13,435,368 reverse istart=0
chains**, out of **159,989,084 total directional chain opportunities**; no work
is invented for those suppressed chains. No offset histogram overflow occurred.

## Verification and provenance

Terra implemented; one Luna review identified the split-edge coordinate and
histogram-overflow issues. Parent corrections classify grid membership by
`istart*Lstart + Lmapped` before bucketing, preserve original SPLIT byte callbacks,
and reconcile work fields without mixing in tuple-check metadata. Actual C++
producer-to-Python-consumer and callback regressions went RED/GREEN; **7/7 tests
pass**. See `bench/CHAIN-POSITION-REVIEW.md` for the single review and disposition.

- Evidence: `bench/evidence/chain-position-host1/` — raw sidecars, summary,
  parity result, commands/exits/times, input/index/source identities and hook diff.
- Immutable host source: `/home/rborkows/uni-rnaseq-probe-lab/chain-source-v1/`.
- Immutable host evidence: `/home/rborkows/uni-rnaseq-probe-lab/chain-position-host1/`.
- `source-snapshot.tar` preserves the staged payload. All **70** staged file
  identities were independently compared to the local source snapshot; all match.
- Counter implementation SHA256:
  `86206070ab916c819bc621a6a2f033209dc128af9c0e9627631b58956949d4aa`.
- Unchanged parity checker SHA256:
  `37a4b369c44e9c3eb3be64d66a564a9e6f1abb40c0b19c112bd11fa893d792a6`.
- The staged README's old coordinate explanation is historical; the immutable
  staged code is corrected. Current `bench/chain-position/IMPLEMENTATION.md`
  supersedes that prose.

Counter mapping: **83.94 s wall, 1,242.32 s user + 30.64 s system = 1,272.96 CPU-s**,
peak RSS **31.18 GiB**. Instrumentation perturbs execution: these are not an
integration performance comparison. Prepare/build/parity took 0.24/26.48/20.40 s;
input/index/stock identity checks are outside those individual stage timers.

## Checkpoint

Coverage is in. Stop and report before starting INTEGRATE-1. The next mechanism
is initial-start speculation only, with the already chosen duplicate arrays,
exact-tuple lookup, stock fallback/oracle and coordinator batch policy. The next
check-in is strict integration parity **before** its three paired CPU timing runs.
No integration implementation or pipeline timing has been started by this card.
