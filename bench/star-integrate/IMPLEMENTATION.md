# INTEGRATE-1 implementation handoff

`crates/umstar` exports the frozen `usi.h` ABI as `libumstar.a`.  It duplicates
and validates `Genome`/`SA` through `umseed-probe::index::probe_load`; SAindex
is validated but dropped before the context is published.  The caller identity
is checked against hashes of the loaded resident bytes, not directory names.
The backend copies read/request/output storage into owned umem allocations and
uses the existing synchronous thread transport.  No-CUDA builds return
`USI_GPU`; they never synthesize GPU tuples.

The generated private source contains a dedicated coordinator, not a
lookup-false header stub. `submit_window` transfers producer-owned frames once;
the coordinator borrows their two byte arrays while synchronously calling USI.
Only batches of 65,536 through 262,144 candidates are dispatched. A queue that
remains underfilled for two milliseconds is completed CPU-only, so no mapping
thread waits for an all-worker rendezvous and no small GPU batch is issued.
`lookup` never queues or waits: it consumes exactly one completed candidate for
the current `iReadAll` frame after matching the STAR inner tuple, chain context,
and both byte arrays. Adaptive `Lmapped` calls remain stock CPU. End-of-chunk
and finish retire unused completed work before frame storage can turn over.

The generated hook records chain context immediately before the original outer
call and records STAR's existing reverse suppression predicate without changing
it. It writes the original `Nrep`, interval and `maxL` only on a usable slot;
fallback, oracle, and `storeAligns` stay stock. Strict mode invokes the CPU
oracle for every consumed result, emits the exact inner tuple and four compared
outputs on the first mismatch, then aborts.

Build the static library on the CUDA host:

```
cargo build -p umstar --release --features cuda
```

Prepare private sources (the source and tooling paths are explicit and pristine):

```
python3 bench/star-integrate/make_star_integrate.py \
  --source /private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source \
  --tooling /Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay \
  --private-root /private/tmp/star-integrate-private
```

The private hook is default-off (the normal STAR build has no
`STAR_INTEGRATE=1`). Build the staged enabled copy on Spark without replacing
upstream flags or HTSlib settings:

```
cd /private/tmp/star-integrate-private/integrated
make -j20 STAR \
  CXXFLAGSextra='-DSTAR_INTEGRATE=1 -I/Users/rborkows/projects/uni-rnaseq/bench/star-integrate -I/Users/rborkows/projects/uni-rnaseq/crates/umgpu/shim' \
  LDFLAGSextra='/Users/rborkows/projects/uni-rnaseq/target/release/libumstar.a -lcudart -ldl -lrt'
```

The generator adds `star_integrate.o` and the accepted replay `sha256.o` to the
private Makefile. Generate
`parametersDefault.xxd` with `xxd -i parametersDefault > parametersDefault.xxd`.
Do not replace global `CPPFLAGS`/`CXXFLAGS`.

Runtime switches: `STAR_INTEGRATE=0` is stock CPU bypass; `STAR_INTEGRATE=1`
requires the admitted static Full profile and a successful USI init; any profile
or init rejection visibly remains CPU-only and cannot satisfy gate (i).
`STAR_INTEGRATE_STRICT=1` is reserved for the STAR-side consumption hook: each
consumed success must invoke the original CPU call into independent storage and
fatal on a four-field mismatch.  It must not be represented as a timing mode.

The sidecar is JSONL, separate from `Log.final.out`. It reports submitted work,
actual GPU batch sizes, CPU tails, `gpu_consumed`, misses, separately partitioned
suppressed/other unused results, and hit/unused `ProbeStats` bytes and gathers.
CPU fallback is not labelled GPU-unused. The labels 13,435,368 and
159,989,084 are comparison-only reference values, never measured output.

Before timing, parent runs strict mode plus the unchanged split checker on the
20M command/artifacts. A small smoke recipe is the same command with the agreed
small FASTQ fixture and `STAR_INTEGRATE=1 STAR_INTEGRATE_STRICT=1`; it must show
nonzero `gpu_consumed`. The full gate uses the agreed 20M command with those two
variables, then `seed_split_parity.py` unchanged. CPU-only runs must be reported
as bypasses, not gates.

Local verification: `python3 -m unittest bench/star-integrate/test_source_patch.py`
and `python3 bench/star-integrate/test_coordinator.py`. The latter is a clearly
labelled deterministic backend fixture: two staggered 40K windows form one 64K
dispatch shape and an EOF CPU tail; it is not GPU evidence. CUDA execution and
the 20M gate remain the Spark-host step; no CPU-bypass result is gate evidence.
