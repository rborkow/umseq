# INTEGRATE v2 T4 — capacity evidence prerequisite failed

2026-09-07. Source/evidence inspection only. **V3 is not implemented.** No SSH,
commits, device measurements, or changes to Terra's files. Existing evidence:
`bench/evidence/chain-position-host1/`. No new measurement evidence directory.

The card's hypothesis is that CHAIN-POSITION contains a per-chain length
distribution from which the 99.99th-percentile output capacity can be selected.
The checked-in evidence and its producer contradict that hypothesis. Following
AGENTS.md's instruction to stop when a card assumption is wrong, implementation
stops before freezing an arbitrary capacity into the ABI.

`counters/chain-position-counters.json` contains joint initial/istart/iDir counts,
directional offsets, work partitions, and reverse suppression counts. Its 61
offset rows are positions, not chain lengths. `summary.json` adds work shares,
not a chain histogram. `bench/chain-position/chain_position_impl.hpp`'s `Totals`
and `finish()` contain no per-chain step-count accumulator or serialization;
`outer_end()` merely clears the outer-call flag. Therefore the 188,903,274 inner
calls and 144,333,308 initial inner calls cannot determine a step-count percentile.
They also omit prefix-only/unique calls, which must count as V3 steps.

To resume the requested data-sized ABI, supply a histogram of **all outer calls
per completed chain**, including prefix-only/unique steps, with the input/profile
identity and denominator. Distinguish executed zero-step chains and suppressed
reverse opportunities. For histogram counts h[k], select the smallest capacity C
with `10000 * sum(h[k], k <= C) >= 9999 * sum(h[k])`; retain the exact tail count.
A new capture is needed if no separate artifact already has this information.
An explicitly authorized provisional capacity would be a changed requirement,
not a measured percentile.

The second finding is independent: `crates/umseed-probe/src/requests.rs` encodes
UMPROBE1 requests as ten u64 words: tag, s0, s1, read_len, start, length, prefix,
low, high, dir. Neither lmapped, istart, piece identity, nstart, lstart,
seedMapMin nor read ordinal is serialized. UMSTAR01 adds only length/low/high/count.
Arena offsets do not recover the missing chain identity. Moreover the capture
contains inner calls only, so prefix-only/unique intervening outer steps are
absent. Grouping neighboring compatible tuples would be a guess, not exact replay.
The original 999,914 per-call replay remains useful; the strict 20M per-step
integration gate remains the real-chain proof, as allowed by the card.

STAR source inspection confirms the proposed pure chain dependency for sparse=1
and seedSearchLmax=0. storeAligns mutates accumulated read state, including sorted
insertion, duplicate suppression and limits, but does not feed the next search.
Its effects must remain in the original CPU order. The flag condition really is
`Shift + L == splitR[1][ip]`, **not** piece_start + piece_length; preserve it even
for pieces with nonzero start. No source/golden disagreement was found.

## Exact handoff to Terra

V3 is not available yet; keep the existing V2 calls and slots. Do not switch the
window until the step-capacity evidence has been supplied and V3 has landed.
The intended chain request will carry s0/s1/read_len, piece_start/piece_length,
istart/nstart/lstart, dirR (1 forward, 0 reverse), seed_map_min and max_steps.
Keep piece/iFrag and chain identity in CPU metadata. Consume step k only at the
k-th outer maxMappableLength2strands call for that chain, validating its Shift.
A rejected chain must fall back in its entirety before any storeAligns effect;
count capacity rejection separately. Strict mode must run stock's full outer
body for EVERY consumed step and compare maxL, Nrep, low and high, then execute
the original storeAligns exactly once. On the CPU compute
`iDir==0 && istart==0 && Lmapped==0 && Shift+L==splitR[1][ip]` from the returned L;
only that condition may clear flagDirMap. The device bit is a cross-check and
must never independently clear it. Reverse suppression remains unchanged.

## Validation status

No implementation was changed, so fmt/clippy/workspace tests and clang-format
were not run for this inspection-only stop. No new ABI sizes or symbols are
claimed. Independent implementation review, synthetic chain grid, V3 real-chain
replay, strict 20M parity and Tier 0 GPU/CPU cmp remain outstanding.

Kernel gathers/s versus V2: **pending Spark**. There is no V3 executable yet, so
an executable V3 benchmark command cannot honestly be provided. The eventual
benchmark must flatten the successful chain steps into V2 requests, verify every
tuple, and report measured event times and logical gathers for V2, V3 thread and
V3 warp on those same searches, with repeats and raw rows. Mac stub timings would
not establish kernel performance.

Files touched: this report and one appended entry in `bench/star-integrate/TDD.md`.
