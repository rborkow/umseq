# REAL-REQUESTS review — Luna

Date: 2026-09-06. Scope was one bounded, read-only review of the real INNER
capture adapter, the listed STAR 2.7.11b source, the unchanged SSIRv1 replay
writer/format, `umseed-probe`, and `seed_probe.{h,cu}`. No implementation edit,
SSH, delegation, commit, or held-work review was performed. The launched host
run is treated as pending evidence; no host result is inferred here.

## Blocking finding

### The real host run does not retain the existing overlap/permutation gate

`bench/real-requests/run_host.py:120-126` runs `umseed-probe` for `real`,
`synthetic`, and `real-4k`, but every invocation passes `--variant thread` and
none passes `--overlap`. The probe CLI’s existing disjoint-half gate is only
executed under that flag (`crates/umseed-probe/src/main.rs:719-755`); it checks
the isolated controls, concurrent execution, checksums, and CPU overlap result.
The probe implementation documentation and the established wrapper retain this
gate (`crates/umseed-probe/PROBE-IMPLEMENTATION.md:163-175`,
`crates/umseed-probe/scripts/probe-spark.sh:69`).

Reproduction: inspect the command recorded by the runner, or run
`rg -n -- '--overlap|--variant|for label' bench/real-requests/run_host.py`.
The only real-request command construction has no `--overlap`, so a successful
host run cannot establish the requested retained timing/permutation/overlap
gate. The host evidence therefore cannot receive scoped approval for the full
request as written until that existing gate is actually run and its raw output
is verified. No additional hardening is prescribed.

No separate request-order permutation check is present in the real-request
runner or the probe CLI. If “permutation” means the established disjoint-half
protocol, the missing `--overlap` is the concrete omission above; if it means a
distinct order/shuffle test, it is likewise not evidenced by this runner.

## Correctness review

No additional blocking defect was found in the bounded capture or projection.

- STAR source call order agrees with the adapter hooks. `ReadAlign_oneRead.cpp`
  calls `read_begin` after PE combination and complement construction, and
  `ReadAlign_mapOneRead.cpp`, `ReadAlign_maxMappableLength2strands.cpp`, and
  `SuffixArrayFuns.cpp` provide the outer, branch, INNER, phase, comparison,
  and read-end boundaries used by the adapter.
- The projection is wired to the actual STAR values: `S,N` and `L_in` are
  recorded at `inner_begin`, `i1,i2` are the initial SA bounds, `dirR` is
  recorded as 0/1, and `L_out,lo,hi,Nrep` are recorded at `inner_end`
  (`bench/real-requests/real_inner_capture_impl.hpp:298-332`). Both buffers are
  copied from the post-PE `Read1[0]/Read1[1]` arrays at the INNER boundary
  (`:317-318`), matching the existing PE tuple convention.
- Comparison calls and phase/byte attribution delegate to the approved SPLIT
  hooks and add exactly one local call/byte accounting update per observed
  comparison (`:357-374`).
- A read is saved only after `read_end`, with no open outer/INNER/direct state;
  a rejected or over-budget read is discarded and capture stops for that worker
  (`:342-355`). The estimates match the SSIR writer’s 176-byte read record and
  240-byte INNER payload plus both `length`-sized buffers. Per-worker record,
  read, INNER, outer, and 256 MiB file limits are checked before replay
  (`:135-143`, `:275-297`). The replay changes only the file-local expected-read
  count and assigns fresh file-local IDs (`:376-404`); the SSIR writer/parser
  caps are not raised.
- Startup binds the actual argv, effective parameters, active metadata, and
  active-array identities before capture; `verify_end` rechecks the active
  pointers and metadata before freeing the genome (`:155-218`). The binding
  tests also reconstruct the existing SSIR header and preserve PE STAR tuples.
- The converted real request file carries the exact captured buffers and STAR
  four-tuples (`bench/real-requests/ssir_to_umprobe.py:57-80`). For a real
  capture, the probe requires the tuple sidecar and the exact selected count
  (`crates/umseed-probe/src/main.rs:508-520`), compares STAR tuples against the
  CPU20 reference (`:538`, `:559-567`, `:663-667`), and checks the thread-GPU
  outputs and stats against that same reference for every selected request
  (`:274-295`, `:684-704`).

## Local checks run

All passed:

- `python3 -m unittest discover -s bench/real-requests -p 'tests_*.py' -v` — 8
  tests, including exact existing-header reconstruction, active-byte binding,
  PE tuple/buffer projection, and SSIR validation.
- `bash crates/umseed-probe/scripts/check-probe-cuda-syntax.sh` — syntax-only
  host/device check passed; this is not a CUDA execution.
- `cargo test -p umseed-probe` — all unit/integration tests passed.
- `python3 -B .hermes/cards/check_real_capture_syntax.py` — passed against the
  real STAR types.

## Scoped disposition and host gates

Disposition: **blocked for the requested full protocol by the missing host
overlap/permutation invocation**. The capture/projection implementation itself
is conditionally acceptable within the stated research scope, subject to raw
host verification.

The host evidence still must show, without invention:

1. the immutable source inventory and actual build inputs; 20-thread STAR
   capture startup binding; active metadata/array identity; actual argv; and
   no forbidden SSIR production directory;
2. normal exit and complete full 20M stock and capture STAR runs, with the
   existing stock-parity checker passing and all required STAR outputs present;
3. every emitted per-worker `.ssir` file passing unchanged SSIRv1 validation,
   complete-read boundaries, file-local counts/IDs, and aggregate selected
   request count at most 1,000,000;
4. conversion of all selected requests with exact source/index/runtime IDs,
   exact buffers, and a four-field STAR tuple sidecar whose count equals the
   request count;
5. CPU20/thread-GPU tuple-for-tuple equality for every selected real request,
   plus matching diagnostics against the 1M synthetic workload; and
6. the existing timing/repeat, disjoint-half overlap/permutation, huge-page,
   and 4K-page gates, with raw diagnostics and cutoff/clock evidence. The
   current runner visibly retains isolated timing, three repeats, huge pages,
   and real-4k pages, but does not run the overlap gate.

No host completion, parity result, throughput result, or biological/production
approval is claimed by this review.
