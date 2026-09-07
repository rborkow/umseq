# CHAIN-POSITION independent review

## Parent disposition after this single review

Both findings below were corrected before the host run: grid membership uses
`istart*Lstart+Lmapped`, and work classes are counted before histogram bucketing.
Parent verification also found and repaired lost original SPLIT byte callbacks
and a JSON reconciliation that compared three work fields against four fields.
The actual C++ producer-to-Python-consumer regression and original byte-hook test
failed on the delivered code and pass after correction; the full suite is 7/7.
Patched translation units compile against the real pinned STAR source. The review
below is retained as the historical delivery review; there was no second pass.
The staged source's old IMPLEMENTATION.md coordinate prose is superseded by the
corrected local document; the immutable staged **code** contains all corrections.

Review scope: `bench/chain-position/`, the pinned STAR 2.7.11b source under
`/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source`, and the accepted SPLIT
counter tooling. No host mapping was run.

## Decision

The counters-only instrumentation is close, but it is not ready for the parent’s
single measurement. There is one real measurement blocker and one conditional
coverage blocker:

1. **Blocker: grid20 is classified in the wrong coordinate system.**
   `chain_position_impl.hpp:60,62` tests `x.lmapped % 20`, and
   `IMPLEMENTATION.md:68–73` makes the same claim. The requested grid is measured
   from each directional split edge. For a chain starting at `istart`, the full
   forward/reverse edge offset is `istart * Lstart + Lmapped`; `Lmapped` alone
   incorrectly admits (or rejects) positions whenever `istart > 0` and `Lstart`
   is not a multiple of 20. This affects `grid_only_added`, the union, adaptive
   tail, and the decision threshold, though it does not affect the primary
   initial share (`Lmapped == 0`).

   Minimal correction: compute `full_split_offset = istart*lstart + lmapped`
   at `inner_begin`; retain `initial = (lmapped == 0)`, and classify the union
   as `initial || full_split_offset % 20 == 0`. Use the same full offset for the
   offset rows (or add a separate full-offset table), and update the README and
   fixture with `istart > 0`, non-multiple `Lstart`, and an on-grid full offset.
   The existing test only exercises fabricated `offset` rows and cannot catch
   this source-coordinate error (`test_chain_position.py:18–29`).

2. **Conditional blocker: overflow buckets lose grid membership.**
   `chain_position_impl.hpp:14,33,63` caps all `Lmapped > 4096` values into one
   offset bucket. The summarizer treats the emitted `offset: -1` bucket as tail
   (`summarize_chain_position.py:20–27`), so a real full offset above 4096 that
   is divisible by 20 is silently excluded from the grid union. The joint
   `istart` overflow at `:17,34` is acceptable as a bounded marginal bucket,
   but the offset overflow is not acceptable if it occurs in the admitted input.
   Minimal correction: classify initial/grid/tail before capping, and retain
   separate overflow totals for those classes (or emit the full offset modulo
   20 alongside the overflow bucket). The parent should fail the measurement if
   any offset-overflow row is nonzero unless this correction is made; report the
   joint overflow count separately rather than interpreting it as a precise
   `istart` distribution.

## What is correct

- The primary measure is implemented on actual inner comparator calls:
  `chain_position_impl.hpp:67–68` increments gathers and compared bytes, while
  `:60–62` records the request classification. The hook is reached from all
  `compareSeqToGenome` mismatch and exact-return paths in the patched
  `SuffixArrayFuns.cpp`, including endpoint comparisons, binary-loop
  comparisons, and `findMultRange` expansion comparisons. The phase hooks at
  `make_chain_position.py:100–103` preserve those three categories. The total
  gather reconciliation at `chain_position_impl.hpp:76` is therefore the right
  primary denominator, subject to the grid correction above.
- `Lmapped` is captured at the actual adaptive call site, not reconstructed:
  `make_chain_position.py:60–67` wraps the call at the source
  `ReadAlign_mapOneRead.cpp:65–77`; `chain_position_impl.hpp:49–56` checks the
  directional edge, `Shift`, and remaining `N`. The source’s independent
  `istart`, `iDir`, and `Lmapped += L` chain state is preserved.
- The requested joint `(Lmapped == 0, istart, iDir)` request/gather/byte rows are
  emitted by `chain_position_impl.hpp:61,70–73`. `ip` attribution is retained in
  the bounded actual array at `:19,61,75–77`.
- Reverse suppression is attached to the literal existing gate. The patch adds
  the opportunity before `if (flagDirMap || istart>0)` and records suppression in
  its `else` (`make_chain_position.py:55–59`); `reverse_suppressed` also checks
  `iDir==1 && istart==0` (`chain_position_impl.hpp:48`). It invents no inner work
  for a suppressed reverse chain.
- The default guard rejects `seedSearchLmax>0` and non-sparse1 at
  `chain_position_impl.hpp:37–40`, before mapping. This avoids attributing the
  separate fixed-length call at `ReadAlign_mapOneRead.cpp:81–87` with an
  uninitialized adaptive `Lmapped`. No stock mapping expression is changed; the
  patch only adds calls around it. SSIR is prohibited by the combined startup
  guards in the accepted SPLIT implementation and `chain_position_impl.hpp:41–43`.
- Existing SPLIT counters remain present and are emitted by the copied accepted
  implementation. `summarize_chain_position.py:30–34` reconciles inner calls,
  compared bytes, and inner requests against that sidecar. The parent runner
  reuses the accepted stock artifacts and invokes the unchanged
  `seed_split_parity.py` (`.hermes/cards/run_chain_position_host.py:55–61,89–92`);
  it also rejects generated SSIR files at `:87–88`.

## Tuple/proof limitation

The hook’s tuple checks (`chain_position_impl.hpp:58–60`) validate the actual
`pieceStart`, `pieceLength`, `dirR`, `L_in`, and SA bounds passed to the inner
call, and the preceding checks validate the source formulas. For the admitted
default sparse1 profile, that is sufficient to identify an *observed* request’s
read/index inputs at its actual position; sparse1 has no `iDist` alternatives.
It is not an execution or exact-tuple-matching proof for a hypothetical grid
candidate, because this counters run never generates or compares a candidate
request against a second `(S,N,L_in,i1,i2,dirR)` tuple. The report must therefore
describe grid20 as classification of actual requests, not proof that unused grid
candidates would be admitted or consumed. After correcting the full-offset
classification, this is sufficient for the requested coverage estimate; no
speculative search is needed.

## Verification performed and remaining tests

`python3 -m unittest -v bench/chain-position/test_chain_position.py` passes all
5 tests. A prepared private copy of the pinned source also passes syntax-only
compilation for `STAR.cpp`, `ReadAlign_oneRead.cpp`,
`ReadAlign_mapOneRead.cpp`, `ReadAlign_maxMappableLength2strands.cpp`, and
`SuffixArrayFuns.cpp` with the Homebrew libomp include path; only pre-existing
STAR extension/deprecation warnings appeared. The current RED/GREEN source
tests (`test_chain_position.py:40–47`) do exercise exact patch anchors and drift,
but do not test full split-edge grid offsets or offset-overflow classification.

Required before the one host run: fix the two classification points above,
add the missing multi-`istart` full-offset and `>4096` cases, rerun the portable
tests and syntax checks, then let the parent run its existing full20M mapping and
unchanged parity check. No owned/cap/fault hardening work is a blocker for this
review.
