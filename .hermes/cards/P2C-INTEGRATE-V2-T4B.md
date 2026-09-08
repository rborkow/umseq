# P2C-INTEGRATE-V2-T4B — Terra — switch the window and consumption to the V3 (whole-chain) contract

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-v2-t4.md` **in full — "Exact handoff to Terra" (lines ~136–188) is
your spec, follow it literally**, then `bench/PHASE2C-integrate-1.md` "Round 5b",
`bench/evidence/integrate-1-host/chain-length-histogram-20M.json` (why capacity is 8),
`seed_probe_abi.h` (V3 records), `make_star_integrate.py`, `star_integrate.cpp`,
`star_integrate_window.cpp`. You are the only worker; you own everything under
`bench/star-integrate/` and the V1/V2/V3 context side of `crates/umgpu/ffi/star_integrate.rs`.
**Do not edit the kernel, ABI, `star_prefix.rs`, `crates/umgpu/src/*`, `crates/umstar/**`.** No
SSH, no commits.

## Where we are

Round 5b: GPU arm mapping-phase −3.6% vs stock with initial starts on the device (V2). Per
run 63–67M `cpu_fallback` — every continuation step (27.4% of outer calls; chains up to 6
steps, none longer). V3 serves the **whole chain** from one request: the window submits one
`ProbeRequestV3` per `(read, piece, dir, istart)` chain; the device returns up to 8 per-step
tuples; the hook consumes step *k* at the k-th `maxMappableLength2strands` call of that chain.

## Deliverable (Astra's handoff, made concrete)

1. **Generation.** At the window's candidate enumeration (which already mirrors `mapOneRead`'s
   `(ip, iDir, istart)` order), emit one V3 request per chain with exactly the fields in the
   handoff (`piece_start/length` from `splitR`, `istart/nstart/lstart`, `dir`, `seed_map_min =
   P.seedMapMin`, `max_steps = 8`). Retain chain identity in the frame's CPU metadata. Reverse
   suppression unchanged. 88-byte request / 472-byte output / 48-byte stats slots;
   `usi_search_batch_v3`.
2. **Consumption.** Positional cursor moves from per-candidate to per-chain + step index. At the
   generated hook: require `k < n_steps` and `steps[k].status == 0`, **verify `steps[k].shift ==
   Shift`** (stock's current value), then assign `maxL, Nrep, indStartEnd[0..1]` and fall to the
   single original `storeAligns` flow. Before the chain's first `storeAligns` effect inspect the
   whole-chain status: any nonzero → the entire chain falls back to stock (count `chain_overflow`
   for status 9, others separately); never consume a partial prefix of a rejected chain. At chain
   end verify consumed count == `n_steps` (mismatch → `strict_fail` in strict, counter otherwise).
3. **`flagDirMap`.** Computed on the CPU from the returned `L` with stock's exact condition
   (`iDir==0 && istart==0 && Lmapped==0 && Shift+L == splitR[1][ip]`); the device's
   `flag_dir_map_cleared` bit is compared against it (mismatch → `strict_fail` / counter), never
   assigned from.
4. **Strict mode.** For **every** step — continuations, prefix-only, unique — run stock's full
   outer body on the CPU and compare all four outputs + `Read1` bytes before consuming. The
   existing V2 strict path does this for initial starts; extend it to step k.
5. **Sidecar.** Add `chains_submitted`, `chains_consumed`, `steps_consumed`, `chain_overflow`,
   `chain_rejected_other`, `shift_mismatch`, `flag_mismatch`, `step_count_mismatch`. Keep
   `chain_length_histogram`.
6. **Coordinator.** Batch sizing is now in chains; outputs are 472 B — check the high-water-mark
   buffer arithmetic and `read_cap`. Keep async double-buffering as is.

Tests: coordinator suite on V3 records (positional per chain+step; shuffled completion;
linearity); generator test proving the hook consumes at step k, cross-checks `Shift`, and that
strict runs the stock body per step; a synthetic multi-step chain through the real coordinator
with a fake backend returning 3 steps, asserting three consumptions in order and the
`flagDirMap` cross-check both ways. Local gates: `cargo fmt/clippy/test --workspace`,
`python3 -B bench/star-integrate/test_*.py`, clang-format. `TDD.md` lines.

Report: files touched, new counters, and anything in the handoff you could not do as written.
