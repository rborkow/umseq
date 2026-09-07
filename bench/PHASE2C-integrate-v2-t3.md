# INTEGRATE v2 T3 — device prefix walk implementation

2026-09-07. Implementation and Mac validation; **GPU correctness and performance are not yet established**. Evidence directory: `bench/evidence/integrate-v2-t3-mac/`. No SSH, host measurements, or commits were performed by this task.

The device body transcribes STAR 2.7.11b `ReadAlign_maxMappableLength2strands.cpp:17–97`: forward/reverse `Read1[0]` key, absent-prefix step-down, exact successor or `nSA-1` fallback, prefix-only branch, unique comparison beginning at Lind, and the existing inner search. No admitted STAR branch is omitted. `Lind==0` is rejected as requested, instead of reproducing STAR's negative table index. Non-ACGT prefix bytes are rejected to CPU; quality-split admitted spans are ACGT.

V1 remains unchanged and callable. V2 uses separate records and symbols so Terra's current window continues to work until it switches. `PROBE_ABI_VERSION=2`; C/C++ static assertions and Rust const assertions check the layouts. All fields below are u64, all records have alignment 8, and offsets are bytes:

| Record | Size | Fields and offsets |
|---|---:|---|
| `ProbeRequestV2` | 88 | `inner: ProbeRequest` at 0 (unchanged 80 bytes), `distance` at 80 |
| `ProbeOutputV2` | 48 | `inner: ProbeOutput` at 0 (unchanged 40 bytes), `branch` at 40 |
| `ProbeConfigV2` | 224 | `inner: ProbeConfig` at 0 (24 bytes); `index_bases` 24, `sai_width` 32, `absent_mask` 40, `n_mask` 48, `n_mask_c` 56, `sparse` 64, `seed_search_lmax` 72, `sai_offset` 80, `sai_bytes` 88, `starts[16]` 96 |

The nested request is exactly `tag,s0,s1,read_len,start,length,prefix,low,high,dir`, offsets 0 through 72. Tag 0 runs the old inner search; tag 1 derives prefix and interval itself, ignoring `prefix/low/high`. `dir=1` is forward (`iDir==0` in STAR), `dir=0` reverse. `distance=0`, sparse=1, and seed_search_lmax=0 are the admitted outer profile. The nested output is `length,low,high,count,status`, offsets 0 through 32. Branch codes: 0 legacy, 1 prefix_only, 2 unique, 3 searched. Count prefix-only results separately by `branch==1`; the replay prints all four branch counts. Only status 0 is consumable.

Old statuses 1–4 retain their meanings. New statuses: 5 unsupported profile/distance; 6 Lind exhausted; 7 SAindex/config/interval bounds; 8 non-ACGT prefix. Transport failure means consume no slot; unsupported individual requests return per-slot status. `starts` contains the terminal entry at `index_bases`; unused entries are zero. `sai_offset` skips the on-disk header (8*(index_bases+2)); `sai_bytes` is the full resident file extent. Masks and packed width are copied from the **loaded Genome**, using `probe_config_v2(g, P.seedSearchLmax, identity.sai_file_bytes)` in `prefix_config.hpp`; masks are not reconstructed from strand width.

`umgpu_seed_probe_v2(g,sa,sai,reads,read_bytes,requests,n,config,out,stats,event_ms,stream)` launches one thread per request and drains synchronously. The safe Rust `seed_probe_v2` requires live, checked G/SA/SAi/read/request/output/stats leases. SAindex is accessed directly from that resident lease. The separate owning `usi_init_v2`/`usi_search_batch_v2`/`usi_destroy_v2` context loads one index, retains its SAindex, reuses batch buffers, and shares V1's identity comparison helpers. It does not coexist with a separately initialized V1 context in the intended integration: use one version's context. V1 setup code was not edited; only a module declaration was added to `star_integrate.rs`.

## Exact handoff to Terra

Include `prefix_config.hpp`; replace the coordinator's context and calls with `UsiPrefixContext` and `usi_init_v2(index_dir,&identity,&config,epoch,&ctx,&error)`, `usi_search_batch_v2(ctx,epoch,reads,read_bytes,requests,n,outputs,stats,&error)`, and `usi_destroy_v2(&ctx,&error)`, where `config=probe_config_v2(map_gen,p.seedSearchLmax,identity.sai_file_bytes)` is built once from the loaded Genome. Use 88-byte request slots and 48-byte output slots. For each initial candidate fill `q.inner={1,s0,s1,read_len,piece_start,piece_length,0,0,0,dir_r}; q.distance=0`; stop building `ind1` or reading SAi on the CPU and stop filtering prefix-only and unique candidates. Keep sparse/seedSearchLmax/distance admission and continuation fallback. Consume the candidate at the **outer** prefix-block boundary, assigning `maxL,Nrep,indStartEnd[0],indStartEnd[1]` from `o.inner`, then retain original maxLbest/storeAligns flow exactly once. Strict mode must execute the stock outer prefix/branch/search code and compare those four outputs before the single original storeAligns effect; the currently generated strict check calls only `maxMappableLength` and is insufficient for T3. Count `o.branch==1` as prefix_only (and unique/searched separately). To supply replay configuration, add `write_probe_config_v2(config_dump_path,config)` once after config construction; remove the dump from timed runs. These are requests only: Terra's window, coordinator, generator, and setup code were not edited here.

## Validation and Spark command

Mac `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed. All six owned C/C++/CUDA files passed clang-format 23.1.0's dry-run check. The C11 public header passed its static assertions. Compiling the helper against full local STAR headers was blocked by missing `omp.h`; no full STAR or CUDA build is claimed.

The Rust test enumerates every four-base ACGT read, every valid start/length/direction, under ordinary and missing-first-prefix tables plus unsupported sparse/Lmax profiles. It compares the **actual shared host/device C++ body** with an independent Rust outer transcription, reaching prefix-only, unique, searched, N-marked, absent-successor, end-table, Lind step-down, and Lind==0 cases. Legacy tag 0 and distance rejection are also checked. With `--features cuda`, the same test additionally compares the actual device results. The Rust oracle delegates only the already gated inner comparison/search to the existing CPU probe; this is not a replacement for stock-STAR goldens.

Run on Spark under the orchestrator's required lock/deadline policy, without altering the corpus:

```sh
cargo test -p umstar --release --features cuda --test prefix_walk
cargo run -p umstar --release --features cuda --bin prefix_replay -- \
  /absolute/index \
  bench/evidence/seed-real-requests-host2/real-requests.bin \
  /absolute/loaded-prefix-config.bin
```

Replay requires exactly 999,914 requests and their mandatory STAR tuple sidecar, checks the CPU prefix transcription against every stock tuple, poisons captured prefix/interval fields, and checks every GPU result against all four original tuple values. It neither skips nor normalizes any request. A failure stops replay. The real corpus contains captured inner calls; new prefix-only/unique branches are covered by the synthetic grid and must also be covered by the strict 20M outer gate.

Pending: nvcc build, actual CUDA grid, 999,914-request replay, strict 20M parity, Tier 0 GPU/CPU `cmp`, and performance measurements. No speedup or device-identical claim follows from the Mac oracle. Until Terra moves both window generation and consumption to the outer boundary, expect the outer CPU self-time to remain. After that switch, prefix work for served initial starts should disappear from the CPU, leaving consumption, stock side effects, and fallback/continuation work. There is no measured numerical self-time prediction from this task; Task 0's 8.8→8.0 CPU-s at 4M is the comparison baseline.

## Files touched

- `crates/umgpu/shim/seed_probe.cu`, `seed_probe.h`, `seed_probe_abi.h`
- `crates/umgpu/src/cuda.rs`, `stub.rs`, `seed_probe.rs`
- `crates/umgpu/ffi/star_integrate.rs` (module declaration only), new `star_prefix.rs`
- `crates/umstar/src/lib.rs`, new `prefix_oracle.rs`, `bin/prefix_replay.rs`
- New `crates/umstar/tests/prefix_walk.rs`, `prefix_host.cpp`
- New `bench/star-integrate/seed_probe_abi.h` forwarding/public V2 header, `prefix_config.hpp`
- `bench/star-integrate/TDD.md` (one appended task entry)
- This report, `docs/review-integrate-v2-t3.md`, and `bench/evidence/integrate-v2-t3-mac/`
