# Real prefix-narrowed requests

## Current verified result

Completed: **6.60× CPU20 on 999,914 real requests**, both replay arms match every captured STAR tuple, zero skipped. Original capture/stock/parity artifacts were reused; corrected execution resumed from convert in fresh `real-requests-host2`. All three repeats, overlap and4K controls passed. The only parser-domain change permits a fully-known reverse prefix (`L_in=N=S+1`); wire format and caps remain unchanged. See `PHASE2C-real-requests.md` for the authoritative evidence and stop point.

## Original implementation handoff (historical)

The staging commands and “unchanged format.cpp” statements below describe the pre-boundary-fix handoff, not a pending capture or the final measured source snapshot.

This card measures the existing `umseed-probe` CPU20 control against the
thread-per-request GPU kernel on observed STAR inner calls. It is a
representativeness measurement only: no pipeline or RNA-seq throughput gain is
claimed.

The request artifact remains `UMPROBE1`, with ten little-endian u64 request
fields. A real artifact has `source_kind=STAR_INNER_CAPTURE` in its adjacent
`.probe-provenance.txt` and a mandatory `*.star-tuples.bin` sidecar (`UMSTAR01`,
count, then `(length,low,high,count)` u64 tuples). The Rust loader preserves
both observed buffers as distinct arena regions; it accepts STAR's `4`/`11`
outside eligible spans and never creates a reverse complement.

`ssir_to_umprobe.py` does not claim to parse SSIR. It first executes
`ssir_validate`, which is compiled directly with the approved, unchanged
`format.cpp`/`format.hpp`/`sha256.cpp`. Only after that structural validation
(identities, footer, lifecycle and the fixed v1 caps) does it project validated
INNER fields: `(S,N,L_in,i1,i2,dir,b0,b1)` and STAR's `(L_out,lo,hi,Nrep)`.
The converter takes whole `READ_END` batches in input-file order. It has a
predeclared hard upper bound of 1,000,000, stops before the next whole read,
reports its actual count, and makes no global-first-N claim in a multi-thread
run. Capture selection is per-worker/file order and must be described in the
manifest; it is not scheduler-global order.

The new capture adapter belongs on the verified private counters-only build,
not installed STAR. It must retain the exact 20M argv from
`run_seed_split_host1.sh`, retain source/read/thread attribution, write only
complete SSIR v1 files at read boundaries (100,000 records, 10,000 reads,
256MiB each), and on cap only disable capture—STAR mapping runs to normal
completion. It must use actual maxMappableLength inner buffers/directions and
emit no zero-padded or invented calls. The existing single-thread/tiny-index
`make_capture.py` is specifically not suitable for this host task.

Fresh host staging (not run locally):

```bash
cd /home/rborkows/uni-rnaseq
bash bench/real-requests/run_real_requests_host.sh
```

Before the final command, the host runner builds the validator from the exact
`tooling-seed-split-v3` snapshot, validates every batch against the external
source/index/runtime hashes, converts it, and invokes `umseed-probe` with the
actual count and three repeats. The probe fails on the first STAR-vs-CPU tuple,
CPU-vs-GPU tuple, or logical-counter mismatch. Parsing/materialization is
before timing; index loading is excluded as in the original PROBE and reported
in the TSV header. The runner stages `make_real_inner_capture.py` from this
repository alongside approved tooling, creating fresh private baseline/capture
snapshots. `L_in=0` is retained because it is an actual STAR inner state.
`--diagnostics FILE` writes an untimed JSON sidecar for N, L_in, interval
width, scalar main+expand loop trips, and logical dependent gathers; it does
not change timed CPU/GPU aggregate counters.

For interpretation, report real and unchanged synthetic distributions
separately: `N`, `L_in`, interval width, logical loop trips, and logical
dependent gathers (histogram, mean, median, p90, p99, max). “Loop” means the
existing scalar search/expand loop counter, while derived logical gathers are
the existing comparator/SA-gather counter; neither is physical DRAM traffic.
Use the same aggregate counters in timed CPU and GPU work. At least 5x real
CPU20/GPU supports further rigor; about 2x requires an explicit decision. It
does not establish a pipeline gain.
