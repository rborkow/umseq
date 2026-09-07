# P2C PROBE measurement review — Luna

Date: 2026-09-06. Scope: one local, read-only review pass of the PROBE crate and
the `umgpu` PROBE additions. No GPU run, benchmark, SSH, network access, source
edit, or commit was performed.

## Findings

### [CAVEAT] Divergence TSV field is mislabeled

Path:line: `crates/umseed-probe/src/main.rs:313-345`, especially the format row at
`main.rs:496`.

`mean_request_bytes` and `max_request_bytes` are populated from per-request
`ProbeStats.bytes`, i.e. bytes compared by the search, not request size. The
separate `mean_compare_bytes` is also computed from total compared bytes divided
by comparator calls. Thus the output is usable for compared-byte divergence, but
the schema says it contains request bytes. A reader could incorrectly treat it as
transport/query-size evidence.

Necessary fix: rename those two columns to `mean_compared_bytes_per_request` and
`max_compared_bytes_per_request`, or populate them from each request's actual
`length`/read span and retain separate compared-byte columns. Do not use the
current names in a divergence conclusion until corrected.

### [CAVEAT] SPLIT provenance is enforced by the supplied wrapper, not by the CLI

Path:line: `crates/umseed-probe/src/main.rs:400-403`; wrapper checks at
`crates/umseed-probe/scripts/probe-spark.sh:15-18,36-37,67`.

`run` only requires a non-empty `--split-provenance` string. A direct invocation
can therefore label a run `inner-only` without proving that the referenced
evidence exists or matches a hash. The supplied Spark wrapper does check an
absolute evidence file and embeds its SHA256, so this is not a blocker for that
wrapper-mediated run; it is a provenance weakness for the documented underlying
command.

Necessary fix: make the CLI accept/require an evidence file, verify it exists,
is absolute, and record its hash; or explicitly restrict the underlying command
to an orchestrator-only interface and remove the appearance that a free-form
string is an attestation.

### [CAVEAT] No median is emitted

Path:line: `crates/umseed-probe/src/main.rs:518-537`, TSV schema at
`main.rs:496`.

The program emits three individual repeats per batch, but does not calculate a
per-batch median. This does not invalidate the raw timings, and an orchestrator
can calculate the median from the rows, but the result file itself does not meet
a literal “per-batch medians/repeats” reporting requirement.

Necessary fix: either emit a clearly identified median row/summary per mode and
batch, or document that the orchestrator must compute medians and record the
selection rule (including whether warmup is excluded).

## Bounded conclusion

No blocker was found at the intentionally performance-only PROBE scope for
faithful CPU control, same-algorithm CPU/GPU tuple and stats consumption, valid
request bounds, packed-SA extent handling, padded genome addressing, lease
lifetime, CUDA-event versus host-wall separation, disjoint-half overlap setup,
or the huge/4K allocator checks. The implementation’s same-algorithm agreement
remains only a smoke test, as disclosed; it is not an independent STAR oracle.

The synthetic `SA_SEARCH_FULL` grid, position-wise complement, absent scheduler
capture, lack of GPU throughput, lack of warp variant, and allocation-time page
observations are already disclosed in the implementation report and are not
reclassified here as kernel blockers. No throughput or boundary claim follows
from the local review.
