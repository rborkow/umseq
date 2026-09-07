# P2C chain position — private counters only

`make_chain_position.py` makes a private STAR source copy, validates the full
tagged-source inventory through the accepted `real_index_oracle.py`, and applies
only exact, one-occurrence source patches. It imports the accepted SPLIT counter
implementation unchanged and writes its existing
`seed-split-counters.json` unchanged. The extension emits only the bounded
`chain-position-counters.json`; it neither captures SSIR nor changes STAR's
mapping logic.

The supported profile is `seedSearchLmax=0`, `gSAsparseD=1`. The process exits
before mapping for another profile. This is intentional: STAR's second,
fixed-length `maxMappableLength2strands` call is active only for
`seedSearchLmax>0` and has no initialized adaptive `Lmapped`. It is not
attributed as an adaptive chain.

## Prepare and build

```sh
python3 bench/chain-position/make_chain_position.py \
  --tooling /Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay \
  --source /private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source \
  --private-root /private/tmp/star-chain-position/build

cd /private/tmp/star-chain-position/build/counters
xxd -i parametersDefault > parametersDefault.xxd
/opt/homebrew/opt/llvm/bin/clang++ -c -O3 -std=c++11 -fopenmp \
  -I/opt/homebrew/opt/libomp/include \
  -D'COMPILATION_TIME_PLACE="chain-position"' \
  -D'GIT_BRANCH_COMMIT_DIFF=""' STAR.cpp
```

For a normal complete build, preserve the same compiler/OpenMP choices:

```sh
make -j20 CXX=/opt/homebrew/opt/llvm/bin/clang++ \
  CXXFLAGSextra='-I/opt/homebrew/opt/libomp/include -L/opt/homebrew/opt/libomp/lib -lomp'
```

Run the resulting binary with the already accepted SPLIT command and exactly:

```sh
SSIR_COUNTERS_ONLY=1 \
SSIR_COUNTERS_DIRECTORY=/absolute/new/empty/counters-output \
STAR ...existing accepted SPLIT arguments...
```

`SSIR_DIRECTORY` must be unset. The output directory must not contain either
sidecar because creation is exclusive. Follow the mapping with the unchanged
accepted `seed_split_parity.py` command.

## Summarize

```sh
python3 bench/chain-position/summarize_chain_position.py \
  --chain /absolute/new/empty/counters-output/chain-position-counters.json \
  --split /absolute/new/empty/counters-output/seed-split-counters.json \
  --output /absolute/new/empty/counters-output/chain-position-summary.json
```

The primary result is `initial_gather_share`: gathers at `Lmapped==0` divided by
all actual inner gathers. It also reports bytes and inner requests for initial,
grid-only additions, their non-overlapping union, and adaptive tail. `grid20` is
recommended only when the initial gather share is below 0.5.

The grid classification is observational, not speculative execution. The full
directional split-edge offset is `istart*Lstart + Lmapped`; the union predicate
is `Lmapped == 0 || full_offset % 20 == 0`. Work is classified into initial,
grid-only, and tail **before** histogram bucketing, including large offsets.
The hook checks the original `Shift` and remaining-length formulas against
`(b,f,istart,Lstart,Lmapped)` and the observed inner piece start/length/direction.
Grid membership counts actual stock-prefix-narrowed calls at candidate positions;
it does not independently execute hypothetical prefix queries or prove a future
cache hit. No extra prefix preparation or search is performed. Exact-tuple
lookup, unused candidates and achieved coverage remain INTEGRATE-1 gates.

Reverse suppression is accounted separately at the literal source gate
`flagDirMap || istart>0`; no request or work is invented for a suppressed chain.
The JSON includes per-`ip,iDir` actual/opportunity/suppression bounded arrays and
the offset table, in addition to the requested `(Lmapped==0,istart,iDir)` joint
gather/byte rows. Counters are fixed-size per worker; only worker registration
uses a mutex.

## Tests

```sh
python3 -B -m unittest discover -s bench/chain-position -p 'test_*.py' -v
```

These include actual pinned-source GREEN patches and a source-drift RED case;
portable aggregation cases cover multi-piece attribution through `ip`, both
directions, reverse suppression, offsets at/on/off 20, initial/grid overlap,
and zero-work denominators. The summarizer also refuses to report a result if
the chain sidecar cannot reconcile to the existing SPLIT inner requests,
gathers, and compared bytes.

Parent regressions compile the actual counter producer and feed its JSON to the
actual summarizer. They cover nonzero istart with non-grid Lstart, both directions,
initial/grid overlap and grid membership beyond the histogram limit. A source-hook
regression preserves all eight original SPLIT byte-return callbacks. All seven
tests pass; RED evidence is `.hermes/cards/P2C-CHAIN-COUNTER-RED.log`.
