# P2C-INTEGRATE-V2-T3 — Astra — SAindex prefix walk on the device: the GPU does STAR's whole `maxMappableLength2strands`

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md` (the one rule: byte-identical
or documented; STAR's source is the spec — verbatim in `docs/tool-src/` and at
`/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source/`), then
`.hermes/plans/2026-09-07_153800-star-integrate-v2.md` (Task 0 result at the end), then
`crates/umgpu/shim/seed_probe.cu`, `bench/star-integrate/seed_probe_abi.h`,
`bench/star-integrate/star_integrate_window.cpp:85-160` (`append_prefix_call`, our CPU
transcription of the prefix walk), `docs/STAR-INTEGRATE-DESIGN.md` §c.

Another worker (Terra) is concurrently editing `bench/star-integrate/star_integrate.cpp`,
`star_integrate_window.cpp`, `make_star_integrate.py` and the identity/setup part of
`crates/umgpu/ffi/star_integrate.rs`. **You own:** `crates/umgpu/shim/seed_probe.cu`,
`bench/star-integrate/seed_probe_abi.h` (+ its copy under `crates/umgpu/shim/` if present),
the probe request/response path in `crates/umgpu/ffi/star_integrate.rs` and
`crates/umgpu/src/cuda.rs`, `crates/umstar`, and tests for those. If you need a one-line change
in Terra's files (e.g. the window filling a new request field), write it as an exact request
in your report — do not edit them. No SSH, no commits.

## Why (measured)

Task 0: seed search is 35.0% of stock STAR's in-situ CPU. With 87–96% of *inner* searches on
the GPU, the seed bucket only drops 51%: `maxMappableLength2strands` self-time is untouched
(8.8 → 8.0 CPU-s at 4M, ~40 at 20M) because the CPU still does the outer work for every
call — building `ind1` from the read bytes, the `SAi` walk for `(Lind, iSA1, iSA2, L_in)`,
and the branch selection — before the generated `lookup`. This card moves that onto the
device so the CPU sends `(read arena offsets, S, N, dirR)` only.

## What the device must reproduce exactly (`ReadAlign_maxMappableLength2strands.cpp:4-100`)

Under the admitted profile only (`gSAsparseD == 1`, `seedSearchLmax == 0`, `iDist == 0`;
anything else → request rejected with a distinct status, CPU fallback):

1. `Lmax = min(gSAindexNbases, pieceLength)`; `ind1` from `Read1[0][pieceStart+ii]`
   forward or `3 - Read1[0][pieceStart-ii]` reverse, `Lmax` bases.
2. Walk down: `iSA1 = SAi[genomeSAindexStart[Lind-1] + ind1]`; while
   `(iSA1 & SAiMarkAbsentMaskC) != 0`: `--Lind; ind1 >>= 2`. `Lind == 0` → status (stock
   would index `[-1]`; our CPU copy returns false — match that: reject).
3. `iSA2`: if `genomeSAindexStart[Lind-1]+ind1+1 < genomeSAindexStart[Lind]` read
   `SAi[...+1]`; present → `(iSA2 & SAiMarkNmask) - 1`; else `nSA-1`, `iSA2good=false`.
4. `iSA1noN = (iSA1 & SAiMarkNmaskC) == 0`. Three branches:
   - `Lind < gSAindexNbases && iSA1noN && iSA2good` → **no SA search**: result is
     `(indStartEnd = [iSA1, iSA2], Nrep = iSA2-iSA1+1, maxL = Lind)`. Today the window
     *skips* these (`append_prefix_call` returns false, CPU does it). On device they're
     nearly free — serve them and report them separately (`prefix_only` count).
   - `iSA1 == iSA2 && iSA1noN && iSA2good` → unique: `maxL = compareSeqToGenome(...)`,
     `Nrep = 1`. Also skipped today; serve it (it's one compare against the resident genome).
   - else → `maxL = (iSA2good && iSA1noN) ? Lind : 0`; inner search from
     `(iSA1 & SAiMarkNmask, iSA2, prefix = maxL)` — today's kernel.
5. `SAi`, `genomeSAindexStart[]`, `SAiMarkAbsentMaskC`, `SAiMarkNmask`, `SAiMarkNmaskC`,
   `nSA`, `gSAindexNbases` are per-index constants: extend `ProbeConfig` (the index
   `SAi` bytes are already in the USI resident image — `ffi/star_integrate.rs:153,652`; expose
   the device pointer). Masks are derived in `Genome_genomeLoad.cpp` / `genomeSAindex.cpp` —
   take them from the loaded `Genome`, don't recompute.

`ProbeRequest` gains what it needs (`sai_mode`/version tag — keep the old request shape
working under the old tag so Terra's window keeps running unmodified until the switch);
`ProbeOutput` gains the branch taken. **ABI is `repr(C)` on both sides with static asserts —
extend `seed_probe_abi.h` and the Rust mirror together, bump the version constant.**

## The proof (unchanged, and it's the whole point)

Strict mode in the hooked STAR calls the **stock** `maxMappableLength2strands` (which does its
own prefix work on the CPU) and compares all four outputs — `L`, `Nrep`, `indStartEnd[0..1]`
— against the device's. Every branch above is exercised by the 20M gate. Additionally, before
any host run:

- **Unit oracle**: a Rust test in `crates/umstar` (or `umgpu` behind `--features cuda`, with a
  CPU reference in the same test so it also runs on Mac against the stub) that builds a small
  synthetic `SAi` + `SA` + `G` with absent prefixes, N-marked entries, and end-of-table cases,
  and asserts the device walk equals a straight transcription of the STAR source for **every**
  `(read, S, N, dir)` in a grid. Include the three branches and the `Lind` step-down.
- The existing `real-requests` corpus (`bench/evidence/seed-real-requests-host2/`, 999,914
  real requests with STAR's tuples) — extend the replay so requests carry read+S+N+dir and
  the device produces the tuples from scratch: **all 999,914 must match**. The orchestrator
  runs it on the Spark; you write it.

## Not in scope

Continuations (`Lmapped > 0`) — next card. The window/coordinator (Terra). `umem`. Parity
checker. Don't "improve" any of STAR's choices (e.g. the "safe, but can probably do better"
`nSA-1`): reproduce them.

## Finish

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace` (Mac, stub), clang-format on `.cu`/`.h`. `TDD.md` line. Report: what
the request/config ABI became (exact), the one-paragraph request for Terra's window (which
new fields to fill, which candidates to stop filtering), any STAR branch you could not
reproduce and why, and where you expect `maxMappableLength2strands` self-time to land.
