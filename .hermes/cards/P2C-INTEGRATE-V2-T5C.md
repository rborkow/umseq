# P2C-INTEGRATE-V2-T5C — Terra — wire the borrowed index into the integrated STAR (part 2 of T5B, now measured)

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` **"T5B part 1"** (the measurement that gates this card: borrowed +
`MADV_HUGEPAGE`-before-first-touch = 1.00× the `umem` rate, all tuples identical),
`bench/PHASE2C-integrate-v2-t5b.md` (your part 1 report and extent arithmetic),
`crates/umseed-probe/src/bin/probe_borrowed.rs` (`alloc_advised` — the ordering that works:
advise the reserved capacity, *then* first touch, *then* read in place), and
`bench/PHASE2C-integrate-v2-t4.md` §"Raw-host-pointer sibling". You are the only worker; you
own `bench/star-integrate/**`, `crates/umgpu/ffi/star_integrate.rs`, `crates/umgpu/ffi/star_prefix.rs`
(context/session side), `crates/umseed-probe/**`. Kernel/ABI untouched; the raw-host launch
siblings (V2 and V3) exist — use them, don't modify them. No SSH, no commits.

## Deliverable

1. **`usi_init_v2` borrowed mode.** A new entry `usi_init_v2_borrowed(g_minus_200, g_len, sa,
   sa_len, sai_payload, sai_len, identity, config, epoch, out, err)` builds the `UsiPrefixContext`
   over caller-owned pointers: no `probe_load`, no `umem` for the index, batch buffers still
   `umem`. Extents per your part-1 arithmetic and Astra's rules (`G-200` with ≥ nGenome+400
   readable; SA ≥ `((n_sa-1)*(strand_bit+1)/8)+8`; SAi payload with `sai_offset = 0`). The
   coordinator's `dispatch` uses `seed_probe_v3_raw_host` (V2 sibling for the V2 path if you
   keep it compiling). Identity: sample the borrowed bytes with the existing sampled scheme;
   both sides now hash the *same* memory — record in `CONTRACT.md` that the identity check is
   a self-consistency check of extents/lengths, not a cross-copy comparison.
2. **STAR `genomeLoad` patch via the generator** (`Genome_genomeLoad.cpp:272-289`,
   `PackedArray.cpp:31-33` — read them; the `new char[]` for G, SA (`allocateArray`), SAi):
   immediately after each allocation and **before any read/fill**, `madvise(ptr, len,
   MADV_HUGEPAGE)` (page-align the range as `alloc_advised` does). After STAR's reads of the
   three index files complete, `posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)` on each (find
   where STAR opens them; if it uses `ifstream`, get the fd via `open()` on the same path
   after the read — the advice is by file, not by descriptor). Both hooks are behind
   `STAR_INTEGRATE` and are no-ops for output; parity proves it. Generator tests assert the
   hooks land where stated and precede the reads.
3. **`setup()`**: pass `mapGen.G - 200` (check what `G` points to relative to `G1` — STAR
   sets `G = G1 + L` with `L = 200`; `Genome_genomeLoad.cpp` has it), `mapGen.SA.charArray`,
   `mapGen.SAi.charArray + header` to the borrowed init. Remove the index-directory path from
   the enabled build. `sidecar`: `index_mode: "borrowed"`, `index_anon_huge_bytes` read from
   `/proc/self/smaps` for the three ranges at setup end (fix the parse that printed 0 in part
   1 — sum `AnonHugePages:` of every mapping overlapping each range), `setup_wall_s`.
4. **Memory accounting:** `MAX_INFLIGHT_BYTES` and `CANDIDATE_BUDGET_BYTES` are unchanged;
   this card removes ~30 GB, it doesn't spend it.

Tests: existing suites + `test_abi.sh` (the enabled-coordinator syntax check against
`usi.h` — add the new declaration there first); generator tests for the two hooks; a
`prefix_replay`-style Rust test that `usi_init_v2_borrowed` over a small synthetic index
reproduces the grid oracle's outputs (stub on Mac: assert the Unsupported path). Local gates:
`cargo fmt/clippy/test --workspace`, `python3 -B bench/star-integrate/test_*.py`, clang-format.

Report: the exact pointer/extent expressions used for the three ranges, where the two
generator hooks landed (file:line in the private tree shape), and anything you could not do as
written.
