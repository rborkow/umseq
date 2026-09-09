# Independent review — collapse disqualifier

Reviewer: orchestrator, independent of Terra's implementation.
Scope: `make_star_integrate.py`, generated helper/call sites and
`test_collapse_patch.py`.

## Disposition

**Accepted for a bounded collapse-enabled host correctness gate, not for
production, residency acceptance or a performance claim.** No memory-management
or content-correctness blocker found in the targeted source review.

Traced the actual pinned STAR allocation branches in `Genome_genomeLoad.cpp`:
G1 allocation with/without sequence insertion and pass1/pass2 reserve;
SA payload ownership/interior pointers; SAi allocation; all three reads and
cache-drop hooks before collapse, and loader return before GPU setup. Collapse
is private-load only and rounds inward to complete 2-MiB extents. Null, empty,
small and pointer-overflow cases skip; failures retain errno, elapsed time and
fall through. No expansion of packed arrays, registration or allocator change.
The helper also emits skipped-policy diagnostics and reads its clock when off;
“default off” means no collapse syscall, not literally zero diagnostic work.

## Verification and corrections

Personally reran the delivered three collapse tests, eight source-patch tests
and complete generated window-contract suite on Mac successfully. Added mock
coverage for explicit off/invalid values, null, both overflow checks and exact
aligned bounds. These tests exercise the extracted generated helper.

Corrected test-only portability defects:
- Source path can be supplied with `STAR_SOURCE_DIR`; compiler with `CXX`.
- Diagnostic scratch path is private to each test, not a shared `/tmp` log.
- The mocked madvise macro is applied after system headers. Applying it before
  glibc's declaration caused a real GCC exception-specifier mismatch.
- Host guard compilation explicitly undefines `__linux__`; this does not stand
  in for the full actual Linux build, which the host gate must perform.

Linux's first compiler choice (clang++) could not find omp.h. Using STAR's
actual compiler (`CXX=g++`) exposed the mock exception-specifier issue above.
After fixing the mock, **all three tests passed on both Mac and Spark** with
no skips. The real collapse syscall has not yet been exercised by these mocks.

The gate-diagnostic checker also passed three synthetic fixture cases:
success, missing required call and failed call. Synthetic fixtures are tooling
validation only, not fabricated workload evidence.

## Frozen candidate and pending acceptance

New source root:
`~/uni-rnaseq-probe-lab/integrate-source-collapse-20260908`.
Verified all 438 files from the previous accepted snapshot before copying it.
Among those files, only `bench/star-integrate/make_star_integrate.py` changed;
the focused test is added. Copied generator/test hashes were compared with the
reviewed local files. No dirty-tree rsync or commit was used.

Host gate:
`~/uni-rnaseq-probe-lab/integrate-gate-collapse-20260908`;
monitor `proc_54591917881f`. Explicit collapse=1, THP=1, eviction=1, strict oracle.
Rebuild, backend tests and ordered 20M output comparison are pending. The wrapper
also requires three successful positive-length collapse calls, recording all
raw results even on failure. This is not a coverage assertion: actual smaps
residency and same-binary CPU/GPU collapse-off/on tests remain required. Full-depth
approval belongs to the prior binary until the new one is itself gated.
