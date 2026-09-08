# INTEGRATE v2 T4R — device seed chains, ABI V3

2026-09-07. V3 implemented alongside V2; Mac host-oracle validation only.
**CUDA correctness and performance remain pending Spark.** No SSH, commits,
Spark jobs, or edits to Terra's window/coordinator/generator were performed.
Mac evidence: `bench/evidence/integrate-v2-t4-mac/`.

## Capacity and preserved findings

The initial T4 stop was correct: CHAIN-POSITION did not measure chain lengths,
and UMPROBE1/UMSTAR01 captures cannot recover chain identity. Requests contain
only tag/s0/s1/read_len/start/length/prefix/low/high/dir, not read ordinal,
piece, lmapped, istart, nstart, lstart or seedMapMin; inner-call capture also
omits prefix-only/unique outer calls. Real chain replay is therefore dropped,
as amended, without guessing groups or filtering the corpus. The existing
999,914-call replay is unchanged. The strict 20M per-step gate is the real-chain
proof, still pending for V3.

The new measured prerequisite is
`bench/evidence/integrate-1-host/chain-length-histogram-20M.json`: ERR188140 20M,
admitted profile, 146,553,716 executed chains / 201,733,339 outer calls,
including prefix-only/unique. Counts for lengths 1–6 are 113,648,373;
16,689,937; 11,612,145; 3,154,247; 1,442,415; 6,599. No observed chain exceeded
six; suppressed reverse opportunities are not in this denominator. Capacity
is **8**, covering the measured maximum plus two. This is not a claim about an
unmeasured population tail. Longer chains return `chain_overflow` and must be
counted by the consumer; the 20M overflow count has not been measured for V3.

## Exact ABI

`PROBE_ABI_VERSION=3`, `PROBE_CHAIN_CAPACITY=8`. V1/V2 records and callable
symbols retain their layouts and signatures. V3 reuses the unchanged 224-byte
`ProbeConfigV2` (including its loaded Genome masks/starts). Every V3 scalar is
u64; every record is aligned to 8 bytes. C/C++ and Rust assertions check sizes
and every V3 field offset.

| Record | Size | Fields at byte offsets |
|---|---:|---|
| `ProbeRequestV3` | 88 | s0 0, s1 8, read_len 16, piece_start 24, piece_length 32, istart 40, nstart 48, lstart 56, dir 64, seed_map_min 72, max_steps 80 |
| `ProbeStepV3` | 56 | shift 0, max_l 8, nrep 16, low 24, high 32, branch 40, status 48 |
| `ProbeOutputV3` | 472 | steps[8] 0 (stride 56), n_steps 448, flag_dir_map_cleared 456, status 464 |

`s0/s1` are read-arena offsets for STAR Read1[0]/Read1[1] (the latter is the
complement array, not a newly reversed array); `dir=1` means forward/iDir=0,
`dir=0` reverse/iDir=1. There is no request tag in V3. `nstart` validates
`istart<nstart`; the supplied `lstart` is used exactly, including zero. Bounds
and multiplication checks reject malformed coordinates without integer wrap.
Requests admit sparse=1 and seed_search_lmax=0 only, as before.

Per-step `max_l,nrep,low,high` are STAR's maxL/Nrep/indStartEnd[0..1]; under
sparse=1, maxLbest equals maxL. Branch codes stay 1 prefix_only, 2 unique,
3 searched (0 on early rejection). Inclusive intervals remain unchanged.
Top-level status zero alone permits consuming the chain. Any nonzero status
invalidates **every** step, including an otherwise completed prefix:

- 1–8 retain V2 meanings (direction, request/comparator/result bounds,
  unsupported profile, Lind exhausted, SAindex bounds, non-ACGT prefix).
- 9 `chain_overflow`: a ninth step is required; capacity takes precedence over
  max_steps when both are exhausted at eight.
- 10 `max_steps`: another step is required after the caller's guard is exhausted.
  `max_steps=0` accepts a zero-step chain, rejects a nonempty one; `max_steps>8`
  cannot enlarge the capacity.
- 11 `no_progress`: a successful per-step search returned L=0. Reject rather
  than looping forever; this is a defensive transport rejection, not a STAR
  behavioral substitute. The step status remains zero, the chain status is 11.

The loop checks STAR's strict seedMapMin bound before guards. Thus exactly
eight steps that finish succeed. On rejection `n_steps` counts attempted
stored steps (including a rejected per-step result); those records and the
flag bit are diagnostic only. Unused records are zeroed. `ProbeStats` stays
48 bytes and aggregates all executed steps: additive gathers/bytes/loops/
comparisons, maximum max_compare and OR of directions.

```c
int umgpu_seed_probe_v3(
    const uint8_t *g, const uint8_t *sa, const uint8_t *sai,
    const uint8_t *reads, size_t read_bytes,
    const ProbeRequestV3 *requests, size_t n, ProbeConfigV2 config,
    ProbeOutputV3 *out, ProbeStats *stats, unsigned variant,
    float *event_ms, struct CUstream_st *stream);

int32_t usi_search_batch_v3(
    UsiPrefixContext *, uint64_t epoch, const uint8_t *reads,
    uint64_t read_bytes, const ProbeRequestV3 *, uint64_t n,
    ProbeOutputV3 *, ProbeStats *, UsiErrorV1 *);
```

Low-level variant 0 is one thread/chain; variant 1 is one warp/chain with
cooperative comparisons. Four whole warps/block, lane zero writes outputs;
all lanes follow the same chain and search decisions. Both paths use the
same templated V2 per-step prefix/branch/search body. No scheduler rounds or
inter-batch dependency were added. Event timing encloses the kernel launch;
all shim paths attempt a synchronous stream drain. Safe Rust `seed_probe_v3`
checks all seven leases, extents, alignment, writable overlap and ATS domain.
The owning C V3 batch currently selects the thread variant. Use existing
`usi_init_v2` and `usi_destroy_v2`; V3 shares that context's single resident
index and reusable batch buffers. There is no separately initialized V3 index.
`PrefixSession::search_chains(epoch, reads, requests, variant)` returns
`(Vec<ProbeOutputV3>, Vec<ProbeStats>, event_ms)`. `search_timed` exposes V2
kernel timing while existing `search` keeps its original pair return type.

## Raw-host-pointer sibling for T5 B

Exported by `umgpu` on CUDA and stub builds (stub returns Unsupported):

```rust
pub unsafe fn seed_probe_v2_raw_host(
    ctx: &Context,
    genome: (*const u8, usize),
    sa: (*const u8, usize),
    sai: (*const u8, usize),
    reads: &GpuLease<umem::Ro>,
    read_bytes: usize,
    requests: &GpuLease<umem::Ro>,
    output: &GpuLease<umem::Rw>,
    stats: &GpuLease<umem::Rw>,
    config: ProbeConfigV2,
    n: usize,
) -> Result<f32, Error>;
```

Caller owns immutable ATS-accessible G/SA/SAi allocations throughout the call
and drain. **genome.0 must be STAR G-200**, with at least nGenome+400 readable
bytes; the kernel adds 200. SA must cover the final unaligned packed load:
`((n_sa-1)*(strand_bit+1)/8)+8` bytes. For STAR's packed SAi payload, set
`sai_offset=0` and `sai_bytes` to that payload's actual readable extent; for the
resident file representation retain its header offset. Pass actual lengths,
not reconstructed allocations. The function validates extents, null/overflow,
ATS availability and all writable overlaps, and retains checked umem leases
for batch buffers. It does not register, copy or allocate the index. On an
error, caller must confirm drain/device teardown before releasing its index;
retain the index if completion is uncertain. The unsafe FFI call's SAFETY
comment explicitly names this caller-owned index lifetime. This is a Rust FFI
boundary sibling; a borrowed owning C context for T5 is not implemented here.

## Exact handoff to Terra

Use `seed_probe_abi.h` and the unchanged loaded `probe_config_v2` helper.
Keep one `UsiPrefixContext` initialized/destroyed by `usi_init_v2` /
`usi_destroy_v2`; switch the chain batch call to `usi_search_batch_v3`.
Allocate 88-byte request slots, 472-byte output slots and 48-byte stats slots.
At `set_chain`, fill exactly:

```cpp
ProbeRequestV3 q{};
q.s0 = s0; q.s1 = s1; q.read_len = read_len;
q.piece_start = splitR[0][ip]; q.piece_length = splitR[1][ip];
q.istart = istart; q.nstart = Nstart; q.lstart = Lstart;
q.dir = dirR; q.seed_map_min = P.seedMapMin; q.max_steps = 8;
```

Retain read ordinal, piece/ip, iFrag and chain identity in CPU metadata.
Submit one request for each otherwise admitted chain. Keep reverse suppression
unchanged; do not use speculative results to create or suppress additional
chains. Unsupported sparse/Lmax profiles stay on CPU. On transport failure
consume no output. Before the first storeAligns effect, inspect the whole
chain status: any nonzero value falls back for the whole chain. Count status 9
as `chain_overflow` separately, and report status 10/11 and other rejections
separately. Do not consume partial prefixes from rejected chains.

At the k-th `maxMappableLength2strands` call of that chain, consume only
`steps[k]`, require `k<n_steps`, `step.status==0`, and verify `step.shift`
against stock's current Shift. Set `maxL,Nrep,indStartEnd[0],indStartEnd[1]`
from `max_l,nrep,low,high`. Keep the original maxLbest and storeAligns flow
exactly once per call, in original CPU order; verify the final consumed count
matches n_steps. Strict mode must run stock's full outer prefix/branch/search
body for **EVERY step**, comparing all four outputs and Read1 bytes, including
continuations, prefix-only and unique steps. An inner-only strict oracle is
insufficient. Keep the original CPU effects exactly once after comparison.

In mapOneRead, compute the original condition on the CPU from returned L:

```cpp
bool stock_clear = iDir == 0 && istart == 0 && Lmapped == 0
                   && Shift + L == splitR[1][ip];
if (stock_clear) flagDirMap = false;
```

Do not replace the RHS with piece_start+piece_length. Accumulate stock_clear
for the chain and compare it to `flag_dir_map_cleared` (or compare it at step
zero, since only step zero can satisfy it). The device bit is a cross-check,
never the source of truth or an independent flag assignment. No device-bit
consumption is allowed on a rejected chain.

For T5 B, use the Rust raw-host sibling above at your borrowed-index boundary;
pass G-200, the actual padded SA allocation and actual SAi representation,
and keep the caller's loaded index alive through confirmed completion. No
change to Terra's files was made to perform this switch.

## Validation and pending measurements

Mac `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, and `cargo test --workspace` passed. A final targeted prefix grid
was rerun after expanding zero-stride chain cases. The six owned CUDA/C/C++
files passed clang-format 23.1.0 dry-run checks; the public ABI passed C11
static assertions. Logs and a tracked command/status summary live in the
Mac evidence directory. Independent adversarial review is recorded in
`docs/review-integrate-v2-t4.md`; no open blocking source finding.

The synthetic test enumerates all 256 four-base ACGT reads, every nonempty
piece, both directions, every istart for Nstart 1 through piece_length+1,
every seedMapMin 0 through piece_length, and max_steps 0/1/8/9. It compares
all 59 output words, including unused steps, against an independent Rust
transcription of mapOneRead calling the existing per-step oracle. Extra
24-base reads cover exactly-eight success and ninth-step overflow. Assertions
require both flag outcomes, zero-step termination, step-one seedMapMin
termination, continuation, step guard, and capacity overflow. Missing-prefix
and unsupported-profile tables remain included; malformed offsets,
direction and multiplication overflow are rejected. With CUDA enabled the
same full grid checks both thread and warp device outputs, without skipping
rejections. This is pre-Spark source-transcription proof, not stock-STAR real
chain parity.

No new admitted STAR behavior was intentionally changed. As in V2, Lind==0
rejects instead of accessing STAR's negative prefix-table index; non-ACGT
prefixes and unsupported profiles reject. L=0 gets a no_progress rejection.
storeAligns accumulations are intentionally left on the CPU. No source/golden
disagreement was found. Raw-pointer execution has not run on CUDA.

Kernel gathers/s versus V2: **pending Spark**. Mac stub timings are meaningless
for this comparison, and none are reported. No thread/warp winner is selected
from unmeasured performance. On Spark, under the existing one-heavy-job lock
and an absolute timeout, run from this checkout:

```sh
nohup timeout 1800 flock "$HOME/.cache/uni-rnaseq-resource.lock" sh -c '
  mkdir -p bench/evidence/integrate-v2-t4-spark &&
  UMSTAR_CHAIN_BENCH=1 cargo test -p umstar --release --features cuda \
    --test prefix_walk -- --nocapture \
    > bench/evidence/integrate-v2-t4-spark/chain-grid-and-bench.log 2>&1
' > /tmp/integrate-v2-t4-launch.log 2>&1 &
```

This command runs actual device correctness before benchmarking, then flattens
**every executed step** into equivalent V2 requests with the same residual
length/shift, including prefixes of rejected chains. No request is filtered
out of the correctness gate. Per-step outputs and aggregate logical
ProbeStats must match across V2, V3 thread and V3 warp before timing rows are
accepted. A warm-up precedes five rotated repeats; CSV rows include dataset,
repeat, arm, chains, steps, overflow and guard counts, CUDA event ms, gathers
and gathers/s. Ordinary and missing-A datasets are labeled; unsupported
profiles execute no steps and have no timing row. These are synthetic-grid
microbenchmarks, not an ERR188140 throughput measurement or the −8%/−12%
integration acceptance gate. Archive raw rows and a tracked summary on Spark.

Still required after Terra switches: nvcc build, device grid, strict 20M
four-output + Read1 parity with rejection counts, Tier 0 --gpu/CPU output cmp,
and integrated mapping/cost measurements against the 20-thread Rust CPU
control. The kernel change is not GPU-gated done until those checks pass.

## Files touched

- `crates/umgpu/shim/seed_probe.cu`, `seed_probe.h`, `seed_probe_abi.h`
- `crates/umgpu/src/cuda.rs`, `stub.rs`, `seed_probe.rs`
- `crates/umgpu/ffi/star_prefix.rs`
- `crates/umstar/src/prefix_oracle.rs`, `tests/prefix_walk.rs`, `tests/prefix_host.cpp`
- `bench/star-integrate/seed_probe_abi.h`, `TDD.md`
- This report, `docs/review-integrate-v2-t4.md`, `bench/evidence/integrate-v2-t4-mac/validation.json`

`prefix_config.hpp` was formatted/checked but has no content diff. Terra's
`star_integrate.cpp`, `star_integrate_window.cpp`, `make_star_integrate.py`
and the shared `ffi/star_integrate.rs` were not edited.
