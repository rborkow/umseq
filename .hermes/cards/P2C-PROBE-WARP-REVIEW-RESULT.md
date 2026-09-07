# P2C-PROBE-WARP-REVIEW — Luna

Date: 2026-09-06. Scope: independent bounded review of the delivered
performance-only warp probe and its host4 measurement protocol. No code, commit,
SSH, network, install, or measurement action was performed. This is not SEED-CUDA,
STAR fidelity, biological correctness, or pipeline approval.

## Blocking

None found within this bounded probe scope.

The CUDA path uses a fixed 128-thread block and maps each request to a complete
warp (`crates/umgpu/shim/seed_probe.h:56-59`; `crates/umgpu/shim/seed_probe.cu:33-45`).
Inactive tail warps return together before request reads, and non-multiple-of-4
counts are covered by the mapping. In the comparator, all lanes perform both
ballots before a mismatch return; tail comparison lanes do not form or issue
sequence loads (`seed_probe.h:100-125`). Invalid-request and comparator-bound
returns occur before the lane-varying sequence loop and are uniform because the
request/state is warp-shared (`seed_probe.h:74-98`, `180-198`).

The lane-0 packed-SA load is broadcast with a full mask, and the eight-byte
unaligned load plus lease tail allowance covers the supported 33–54-bit packed
widths (`seed_probe.h:33-41`, `77-82`; `crates/umgpu/src/cuda.rs:989-1009`). The
same comparator feeds endpoint comparisons, midpoint search, and both expansion
paths (`seed_probe.h:154-178`, `191-228`). Its direction, reverse-coordinate,
and ordering expressions match the scalar implementation (`crates/umseed-probe/src/cpu.rs:59-111`).

Lease context/extent checks, writable-range disjointness, ATS/registration policy,
and request alignment are enforced before launch (`crates/umgpu/src/cuda.rs:982-1045`).
The shim drains the stream on every path before events are destroyed or leases
can be reclaimed (`crates/umgpu/shim/seed_probe.cu:52-85`), and Rust joins the
scoped CPU overlap worker after that drain (`cuda.rs:1056-1087`). The unchanged
thread entry point still selects variant Thread (`cuda.rs:937-963`), while the
stub remains explicitly unsupported without CUDA (`crates/umgpu/src/stub.rs:110-125`).

## Should fix

1. Correct the TSV names or values for request-byte fields. `row` fills
   `mean_request_bytes` and `max_request_bytes` from `ProbeStats.bytes`, which
   is compared/logical bytes, not request length (`crates/umseed-probe/src/main.rs:323-329`,
   `350-360`). The header still calls them `mean_request_bytes` and
   `max_request_bytes` (`main.rs:524-526`). Rename them to e.g.
   `mean_compared_bytes_per_request` / `max_compared_bytes_per_request`, or
   populate them from actual request lengths and retain separate compared-byte
   fields. The explanatory comments correctly say the logical metrics exclude
   warp speculative loads (`main.rs:497-505`), but that does not repair the
   schema ambiguity.

2. Tighten direct CLI provenance, or explicitly make the wrapper the only
   supported measurement entry point. `run` accepts any non-empty
   `--split-provenance` string (`crates/umseed-probe/src/main.rs:400-413`), so a
   direct invocation can self-label an unverified SPLIT decision. The actual
   host4 wrapper does preflight the archived successful run and agreement marker
   and embeds the boundary path/hash (`.hermes/cards/probe-warp-host4.sh:24-35`,
   `53-60`), so this is not a blocker for that wrapper-mediated protocol.

## Nits

- The warp comparator intentionally counts scalar-equivalent logical bytes while
  issuing additional in-range loads after a first mismatch. This is disclosed in
  both implementation comments and TSV comments (`seed_probe.h:126-132`;
  `main.rs:501-505`). Treat `logical_gpu_GBs` as a logical-work proxy, not traffic.
- `mean_warp_byte_imbalance` is computed by grouping result records in chunks of
  32 (`main.rs:330-343`) and is explicitly labeled a software group-of-32
  request-cost proxy, not hardware activity or warp divergence (`main.rs:503-505`).
  For warp-per-request it is not intra-warp imbalance; the label/disclaimer is
  adequate, but conclusions should not use it as a hardware metric.
- The host4 final verifier checks the expected row total and isolated repeat
  identities, but does not independently assert overlap repeat identities or
  denominators (`.hermes/cards/probe-warp-host4.sh:61-73`). The producer does use
  equal halves, records `half`, validates overlap outputs, and computes combined
  throughput as `2*n/combined` (`main.rs:568-604`, `350-376`), so this is a
  verification-hardening nit rather than a demonstrated fairness defect.
- Page provenance, no-overwrite behavior, supported counts/repeats, request/index
  parameter hash, and fail-closed cutoff checks are present (`main.rs:403-445`,
  `528-552`; `.hermes/cards/probe-warp-host4.sh:3-16`, `65-75`). Event time,
  device-call wall, and total wrapper/lease wall remain separately emitted
  (`main.rs:103-107`, `345-377`).

## Tests actually run

- `cargo test --offline -p umseed-probe -p umgpu`: passed; 3 `umgpu` unit tests,
  1 `umseed_probe` library test, 1 CLI protocol test, loader test, and all 6
  `probe_search` tests, including `probe_warp_collectives_and_tail_protocol`
  and `probe_host_kernel_transport_matches_cpu`.
- `cargo clippy --offline -p umseed-probe -p umgpu --all-targets -- -D warnings`:
  passed.

The focused odd-tail and direction/packed-width tests are host emulation, not
CUDA execution (`crates/umseed-probe/tests/probe_warp.cpp:1-2`; `probe_warp_host.h:1`).
I make no unseen host4 numerical or speedup claim. The orchestrator-provided
real CUDA build/SASS and host4 status are treated as supplied evidence, not
repeated here.

## Scope

Approve the bounded warp implementation and wrapper measurement protocol for
probe-only continuation, subject to the two schema/provenance fixes above being
tracked. Do not infer upstream correctness from CPU/GPU agreement on the same
synthetic `SA_SEARCH_FULL` inputs; the limitation is correctly stated in the
implementation and runner (`crates/umseed-probe/PROBE-WARP-IMPLEMENTATION.md:1-8`;
`.hermes/cards/probe-warp-host4.sh:57-60`).

Future correctness-ladder requirements remain future work: captured STAR request
distributions, an independent correctness oracle, biological parity/QC, and
SEED-CUDA integration. They are not defects in this bounded synthetic
performance screen and are not requested as approval gates here.
