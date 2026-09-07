# P2C-INTEGRATE-V2-T4 — Astra — the whole seed chain on the device: one request per (read, piece, dir, istart), the kernel walks the chain

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` ("Round 5b" — where numbers stand), `bench/PHASE2C-integrate-v2-t3.md`
(the V2 contract you built), `crates/umgpu/shim/seed_probe.cu`, `bench/star-integrate/seed_probe_abi.h`,
and STAR's `ReadAlign_mapOneRead.cpp:40-93` + `ReadAlign_maxMappableLength2strands.cpp` +
`ReadAlign_storeAligns.cpp` (source at `/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source/`).

Another worker (Terra) is concurrently editing `bench/star-integrate/star_integrate.cpp`,
`star_integrate_window.cpp`, `make_star_integrate.py`. **You own:** `crates/umgpu/shim/seed_probe.cu`,
`seed_probe.h`, `seed_probe_abi.h` (+ the `bench/star-integrate/` forwarding copy), `crates/umgpu/src/*`,
`crates/umgpu/ffi/star_prefix.rs`, `crates/umstar/**`, `bench/star-integrate/prefix_config.hpp`.
Handoff requests for Terra's files go in your report as exact text. No SSH, no commits.

## Why (measured, round 5b)

GPU arm mapping-phase is **−3.6%** vs stock with 88% of *submitted* requests served. Per run
63–67M `cpu_fallback`: every `Lmapped > 0` continuation (24% of all gathers per CHAIN-POSITION)
plus key misses. The gate (iii) bar is −8% (memo) / −12% (target). The continuations are the
last large block of seed-search CPU, and they are on the CPU only because the current contract
is one-request-one-search.

## The observation that makes this a kernel change, not a scheduler change

`ReadAlign_mapOneRead.cpp:62-75`: within one `(piece ip, direction iDir, istart)` chain,

```
Lmapped = 0
while (istart*Lstart + Lmapped + seedMapMin < splitR[1][ip]):
    Shift = dirR ? splitR[0][ip] + istart*Lstart + Lmapped
                 : splitR[0][ip] + splitR[1][ip] - istart*Lstart - 1 - Lmapped
    seedLength = splitR[1][ip] - Lmapped - istart*Lstart
    maxMappableLength2strands(Shift, seedLength, iDir, 0, nSA-1, L, iFrag)   // L = maxLbest out
    if (iDir==0 && istart==0 && Lmapped==0 && Shift+L == splitR[1][ip]) flagDirMap = false
    Lmapped += L
```

The next `Shift`/`seedLength` depend **only on the previous call's `L`** and constants of the
chain; `maxMappableLength2strands` has no other read-level state in (under the admitted profile
`gSAsparseD==1`, `seedSearchLmax==0`); `storeAligns` only *accumulates* (`ReadAlign_storeAligns.cpp`).
So the entire chain is a pure function of `(read bytes, splitR[0..2][ip], istart, Nstart, Lstart, dirR,
seedMapMin)`. **One request per chain; the device loops.** No rounds, no scheduler, no
inter-batch dependency. Each chain step is exactly what T3 already does per request (prefix walk
→ branch → inner search), repeated with the updated `Shift`.

## Deliverable

1. **ABI V3 alongside V2** (V2 stays callable — Terra's window keeps working until it switches).
   `ProbeRequestV3`: read arena offsets + `read_len`, `piece_start, piece_length, istart, nstart,
   lstart, dir, seed_map_min`, and a `max_steps` guard. Output: a fixed-capacity array of per-step
   `(shift, maxL, nrep, low, high, branch, status)` plus `n_steps` and a `flag_dir_map_cleared`
   bit — the consumer needs every step's tuple, in order, to replay `storeAligns` exactly. Pick the
   capacity from data: CHAIN-POSITION's evidence has the per-chain length distribution; size for the
   99.99th percentile and **reject** (distinct status) longer chains to CPU — count them.
2. **Kernel**: per-thread chain loop over the existing per-step body. Watch divergence: chains
   differ in length; at 1 thread/request the warp stalls on the longest. Measure both 1-thread
   and warp-per-request (T3's `probe_warp_kernel` exists) and report gathers/s vs the V2 kernel
   on the same requests. Don't optimize past what the numbers require.
3. **The exact-transcription oracle, extended**: the synthetic-grid test (`prefix_walk.rs`)
   gains chains — every `(read, piece, istart, dir)` on the small synthetic index, device chain
   output vs an independent Rust transcription of the `while` loop above calling the existing
   per-step oracle. Include chains that terminate on the `seedMapMin` bound at step 1, chains
   that hit `max_steps`, and the `flagDirMap` condition both ways.
4. **Real replay, extended**: `prefix_replay` gains a chain mode. The real corpus
   (`bench/evidence/seed-real-requests-host2/real-requests.bin` + `.star-tuples.bin`, 999,914
   captured inner calls with STAR's tuples) contains continuations as separate captured calls;
   group them by `(read ordinal, piece, dir, istart)` using the captured `lmapped`/`istart`
   fields (check `crates/umseed-probe/src/requests.rs` for what was captured; if chain grouping
   isn't recoverable from the capture, say so — that's a finding, and the strict 20M gate remains
   the proof), and assert the device chain reproduces **every** step tuple in order.
5. **Handoff to Terra** (exact text in your report): what the window fills per chain (it already
   has `piece/istart/nstart/lstart` from `set_chain`), and how the generated hook consumes step
   *k* at the k-th `maxMappableLength2strands` call of that chain, with strict mode running
   stock's body for **every** step and comparing all four outputs — the oracle is unchanged in
   kind, just per step. State precisely how `flagDirMap` is handled: the hook must set it from
   the device's bit *only* if the stock condition would have (compute it on the CPU from the
   returned `L`; don't trust the device's bit as the source of truth — it's a cross-check).

## Not in scope

The window/coordinator/generator (Terra). Reverse-direction suppression logic (stays as is).
`seedSearchLmax > 0`, sparse > 1 (reject as now). The second index copy (separate card).

## Finish

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, clang-format on `.cu/.h/.hpp`. `TDD.md` line. Report as for T3:
ABI exact, Terra handoff exact, any STAR behaviour you could not reproduce and why, and the
kernel's measured (not predicted) gathers/s vs V2 on the Mac stub is meaningless — say
"pending Spark" and give the command.
