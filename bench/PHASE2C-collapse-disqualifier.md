# Phase 2C collapse disqualifier

## Status

The implementation is independently reviewed and the frozen collapse-enabled
candidate has passed rebuild/backend tests and strict ordered 20M parity. All
three collapse calls succeeded; their combined wall duration was 5.248563158 s
in that diagnostic run. The subsequent 12-run matrix is verified: collapse
stabilized residency and saved about 2% CPU in both CPU/GPU paths, including
collapse cost. It remains opt-in, not an accepted production default. The prior
binary retains its separate approval, and the new binary's collapse-enabled
full-depth gate has also passed; see `bench/PHASE2C-full-depth.md`.

Gate evidence is at `~/uni-rnaseq-probe-lab/integrate-gate-collapse-20260908`.
See `bench/PHASE2C-collapse-matrix.md` for the verified same-binary CPU/GPU ×
collapse-off/on matrix, narrow 8.0831% on/on point estimate, per-repeat spread,
and exact protocol. Passing syscall diagnostics alone is not proof of residency
or performance. Expanded tests and Linux mock/compiler corrections are recorded
in `docs/review-collapse-disqualifier.md`.

The generated STAR integration now has a Linux-only experiment switch:

```text
STAR_INTEGRATE_COLLAPSE_INDEX=1
```

It has no effect unless THP advice is also enabled. `STAR_INTEGRATE_THP=0` remains
the genuine no-advice/no-collapse arm. Any unset value, `0`, or value other than
exactly `1` for the collapse flag does not issue `MADV_COLLAPSE`.

## Intervention boundary

For private (`NoSharedMemory`) STAR index loads only, the generated loader:

1. Reads Genome, SA, and SAindex normally.
2. Runs the existing optional per-file `POSIX_FADV_DONTNEED` calls.
3. Optionally calls `MADV_COLLAPSE` before `Genome::genomeLoad()` returns, and
   therefore before generated GPU setup/mapping begins.

The three spans are derived from STAR's actual loaded storage: `G1` and its private
allocation extent (including STAR's sentinel bytes), plus `SA.charArray` /
`SA.lengthByte` and `SAi.charArray` / `SAi.lengthByte`. The helper rounds inward to
complete 2 MiB ranges; partial endpoint ranges, null spans, empty/small spans, and
overflow-risk spans are skipped. It does not change allocation, packed data, cache
policy defaults, ABI, timing boundaries, or mapping behavior.

Every helper invocation writes a `STAR_INTEGRATE_COLLAPSE` stderr diagnostic with
label, state, attempted flag, syscall result, errno, in-range bytes, and elapsed
monotonic wall time. A failed syscall is reported as failed and execution continues;
it is not treated as successful coverage. Successful `madvise` is not evidence of
full huge-page coverage: acceptance must use the same-binary on-host `smaps`
measurement.

## Required experiment

Use one frozen, accepted binary and compare collapse-off/on in both advised CPU and
GPU arms. Include reported collapse time in the target invocation cost, rotate
repeats, preserve comparator symmetry, and reject an intended collapse-on arm when
any collapse diagnostic has a failed syscall. Collect per-VMA `smaps` evidence for
the actual backing; do not infer it from successful diagnostics or mapping rate.

No retries, fallback allocator, HugeTLB reservation, `cudaHostRegister`, global
cache drop, or output normalization is part of this experiment.
