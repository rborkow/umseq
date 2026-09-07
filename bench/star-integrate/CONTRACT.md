# INTEGRATE-1 frozen contract

Terra implements next; Astra performs one consumption-order review after delivery.
Mechanism (a) only: bounded initial starts, no grid or continuations. Existing
thread kernel/transport accepted at 6.60×, 999,914 tuples on both arms; no search
algorithm/tuning changes. This contract supersedes the historical proposed ABI
and Log.final.out telemetry suggestion in `docs/STAR-INTEGRATE-DESIGN.md`.

## Boundary and ownership

Final declarations are in `usi.h`; transport records and the existing
`umgpu_seed_probe` symbol are in `../../crates/umgpu/shim/seed_probe_abi.h`.
`seed_probe.h` contains the separate C++ search implementation. The four records
remain ProbeRequest80 / ProbeOutput40 / ProbeStats48 / ProbeConfig24, with every
field offset asserted. No parallel request/result schema.

- `usi_init_v1(index_dir, identity, epoch, out, error)` owns a duplicate immutable
  G/SA umem context; CPU fallback continues using STAR's original arrays.
- `usi_search_batch_v1(ctx, epoch, reads, read_bytes, requests, n, results, stats,
  error)` synchronously materializes inputs, leases internal buffers, executes
  the existing thread transport, drains, then copies n results/stats back.
- `usi_destroy_v1(ctx**, error)` drains and destroys; null handle is a no-op.

One coordinator serializes these calls, including destruction. All caller
arrays/path/identity/error storage are borrowed only during each call; no caller
pointer escapes. Caller keeps inputs immutable and storage live until return.
Writable arrays/error/handle storage must be disjoint from all other arguments;
backend checks lengths, conversions to size_t, products, additions and overlap.
Required error storage receives code=return, reserved=0 and a NUL-terminated
message (empty on success). Catch backend exceptions/panics at the C boundary.

Init requires *out initially null, sets it null before work, and publishes only a
fully validated context. Epoch is nonzero and checked on every batch, including
n=0; n=0 otherwise touches no arrays. n>0 requires all arrays; n<=262144. The
backend may reuse owned capacities only after completion, never retain caller
pointers or use caller output memory as GPU storage. Uncertain completion must
quarantine internal allocations, disable the context, and report code5, never
free memory a device might touch. Destroy nulls the handle even on quarantine.

Identity is the actual STAR-loaded immutable snapshot: byte lengths plus a
64-bit FNV-1a digest (stored in the ABI-reserved 32-byte digest slots) over each
array's ordered first and last 1 MiB and 64 evenly spaced 64 KiB samples (the
first/last samples are retained even when they overlap), plus nSA/strand
bit/sparse. This is a consistency check between two in-process resident copies,
not a security boundary. STAR samples its loaded bytes directly (including the
reconstructed SAindex header); the backend computes the same scheme from its
resident bytes. This sampled identity is bound immediately after `genomeLoad`,
before mapping workers or frames can start, and its wall time is emitted as
`setup_wall_s` in the sidecar. Validate both snapshots against it, metadata and
packed extents; a directory name or epoch alone is not identity. No third
full-size temporary or retained duplicate SAindex. Genome includes 200 sentinel bytes on each side;
packed SA includes the tail needed by the existing eight-byte load. Validate
actual huge-page coverage and ATS/no-registration requirements. v1 admits the
pinned 2.7.11b Full, strand32, sparse1, static index/profile (including the pinned
chromosome/SJ metadata), NoSharedMemory, no transform, twopass None, default
seedSearchLmax=0. Reject active sequence/SJ insertion, not merely saved
`sjdbInsertSave Basic` metadata. Any mutation requires CPU bypass and table
invalidation; no in-place context epoch updates.

## Admission, identity and consumption

After stock read preparation and qualitySplit, enumerate each actual piece
(ip,b,f,iFrag), istart in {0,1} AND istart<Nstart, iDir in {0,1}, Lmapped=0.
Compute stock Nstart/Lstart and enforce `istart*Lstart+seedMapMin < f`; higher
starts and adaptive continuations remain CPU. Respect combined mate/spacer and
split structure; do not assume two clean 75-base pieces. Prepare prefixes and
branch choice on CPU exactly as stock; short/direct paths stay CPU. Admit only
the actual inner request, with post-prefix S,N,L_in,masked i1,i2 and dirR.

ProbeRequest uses tag=0, dir=dirR (1 forward, 0 reverse), start=S, length=N,
prefix=L_in, low=i1, high=i2. s0/s1 offset complete prepared Read1[0]/Read1[1]
into the arena; Read1[1] is the position-wise complement, NOT Read1[2]. Keep
numeric bytes/sentinels exact. Enforce 0<read_len<=4096, N>0, prefix<=N,
low<=high<nSA, both read extents, and directional S/N bounds without clamping.

Worker-local lookup identity includes worker/chunk/read ordinal, a nonreused
mapOneRead generation, piece ip/b/f/iFrag and mate/split context, Nstart/Lstart,
istart/iDir/iDist, call kind and Lmapped=0; immutable index identity/epoch;
complete original inner inputs (S,N,L_in,i1,i2,dirR), combined read length and
exact s0/s1 read bytes. Arena offsets or pointers alone are never identity.
A hash can select a bucket but byte/field equality must establish the match.
Remap/overlap/WASP or modified bytes get a new generation and CPU bypass in v1.
Original ReadAlign/read/chunk/RNG ownership and consumption order remain stock.

Look up only at the original inner `maxMappableLength` call in
`ReadAlign_maxMappableLength2strands.cpp`, after stock prefix preparation. Result
j corresponds to request j regardless of GPU completion order. status=0 exposes
(L_out, inclusive low, inclusive high, count=high-low+1); validate containment
within input bounds and prefix<=L_out<=N before consumption. Assign precisely
the stock outputs there; preserve original sparse winner traversal, suppression
test (`Shift+L == splitR[1][ip]`), Lmapped advancement and storeAligns untouched.
Never apply speculative results directly to alignment state.

Call return 0 defines every slot/status. Slot statuses retain 0 success,
1 unsupported tag/direction, 2 request bounds, 3 comparator bounds, 4 result
bounds; nonzero status has no usable tuple. Nonzero call return invalidates the
entire batch: 1 bad input, 2 identity/profile/epoch, 3 allocation/placement,
4 GPU, 5 uncertain completion. Any miss/rejection/failure calls the original
stock function with untouched original inputs before stock side effects.
Disable submissions on transport/index faults; report invalid GPU results as
defects, not ordinary misses. Initialization failure leaves stock CPU running.

**Strict parity mode runs the original CPU oracle on every consumed GPU tuple**
with saved original L_in and independent result storage, compares all four tuple
fields, and fails fatally on any mismatch even if fallback could produce the
same SAM. Bounds defects in returned successes also fail parity. CPU fallback
must never hide a GPU/CPU mismatch.

## Window, progress and gate

Bound each worker's initial-start window within its already assigned chunk;
retain compact prepared read bytes once per read, piece/key metadata and slot
results, not a replicated ReadAlign/downstream workspace per read. Worker owns
its windows through submission and stock ordered consumption; coordinator owns
its aggregate materialization. Retire after last consumption/unused accounting;
never recycle generation or overwrite bytes/slots while queued or in flight.

After stock `readLoad` has preserved stream, name, quality, length and clipping
side effects, a matching frame may install its post-clip `Read1[0..2]` bytes in
place of STAR's pair-combine/complement/reverse preparation. Ordinal or length
mismatch remains stock and increments `read1_fallback`. The pinned loader's
numeric conversion is required by clipping and remains stock-owned.

One synchronous coordinator targets 65,536 aggregated requests; 262,144 only if
existing chunk/window capacity allows. CPU underfilled tails. Progress must not
require every worker to reach a barrier: when no full batch is presently ready,
resolve pending underfilled windows to CPU and release workers; EOF, zero-input,
zero-candidate, small chunks and worker retirement cannot strand waiters. Bound
queues and bytes as well as request counts; backpressure may choose CPU. Do not
change stock chunk assignment or consume any read/RNG downstream work early.

All telemetry goes in a separate integration sidecar, never extra Log.final.out
fields. Count submitted/unique/consumed/unused, actual calls, misses/key mismatch,
slot rejection reasons, batch faults and CPU fallbacks; record prefix,
materialization, lookup and device/total-call costs. Use existing ProbeStats:
loops=main+expansion, bytes=logical examined bytes, gathers/comparisons=comparator
invocations, max_compare=max examined bytes, directions=bit mask. Sum additive
counters, max max_compare, OR directions. No invented main-only counter. Attribute
unused suppressed-reverse work using its actual stats; report against observed
13,435,368 suppressed chains / 159,989,084 directional chain opportunities,
without treating opportunities as GPU requests or N sums as traffic.

Gate (i), before ANY timing mode/runs: strict oracle plus unchanged SPLIT checker
on the agreed 20M input, exact SAM biological records/order/all tags, SJ and
non-timing Log.final.out. Retain existing executable/prefix-only header and
approved timing normalization. Report gate evidence before paired timing runs.

Minimal Terra tests: backend init identity/epoch mismatch and failure cleanup;
zero/max/overflow/overlap inputs and slot order/status/inclusive bounds; caller
storage reuse after return and fault/quarantine paths; key collision/stale
generation/read-byte/index mismatch; split/mate/Nstart eligibility and suppressed
reverse accounting; injected wrong tuple fatal despite identical fallback SAM;
EOF/small/empty tails and bounded progress with retiring workers; complete 20M
gate (i). ABI compile checks here establish declarations/layouts only, not GPU
execution, backend correctness or integrated parity.
