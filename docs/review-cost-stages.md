# Independent review — P2C-COST-STAGES

Reviewer: orchestrator, independent of Luna's implementation.
Scope: uncommitted `scripts/cost_curve.py`, `scripts/tests_cost_curve.py`,
`bench/fig/cost-model.json` and the four generated PNGs.

## Verdict

Accepted as a **provisional historical-stage-based pipeline projection**, not a
measured whole-pipeline result or approval of the repaired STAR candidate.
No correctness blocker found in the stage substitution.

## Checks actually executed

- Read the code/test diff and generated JSON. Independently parsed all nine raw
  round-9 timing rows: three each for stock, bypass and GPU; every exit zero.
  Computed means with `statistics.mean(user_s + sys_s)` independently of the
  model's Decimal implementation. All three match the generated metadata.
- Compared the generated JSON with `git show HEAD:bench/fig/cost-model.json`:
  `nfcore`, `umbam` and all three existing non-PROJECTED scenario values are
  unchanged. Only the STAR bucket changes in the two new projections; neither
  incorporates the separate umbam CPU+GPU scenario.
- Verified the pipeline substitution and incremental comparator. Projections are
  140.57211415399942 and 136.89674563241869 CPU-min/sample, versus the existing
  umbam CPU baseline of 148.48666666666665. GPU increment over the advised-pipeline
  projection is 3.6753685215807366 CPU-min, or 2.614578676361301%.
- Ran `MPLCONFIGDIR=/private/tmp/uni-rnaseq-mpl
  /Users/rborkows/miniconda3/bin/python -m unittest scripts/tests_cost_curve.py -v`:
  one test passed, no skips. This test executes the generator and checks model
  formulas, metadata and all four nonempty PNGs.
- Visually inspected generated charts. Found the CPU-minutes title clipped by
  the added long labels; wrapped projected row labels and shortened/wrapped that
  title in `scripts/cost_curve.py`. Regenerated artifacts and inspected the new
  CPU-minutes image: title, labels and totals visible.
- Reran the generator and regression test after the layout correction: exit 0;
  one test passed in 0.813s. `git diff --check` passed.

## Limits

The source remains the historical 20M-pair round-9 candidate, whose lifecycle
repair is not yet accepted here. Scaling its STAR ratio into the full nf-core
stage remains a projection. The fixed hardware, utilization and AWS Batch cost
assumptions were preserved, not independently remeasured. No Spark benchmark,
full-depth integrated parity, salmon improvement or deployment validation is
claimed by this card.
