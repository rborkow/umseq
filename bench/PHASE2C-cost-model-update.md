# P2C cost model update — measured STAR ratios, projected pipeline scenarios

This bounded update replaces the obsolete projected `1.07–1.15x` kernel-capacity envelope
in `scripts/cost_curve.py`. It does not revise historical timings or claim full-pipeline
validation.

## Source and qualification

The STAR stage source is
`bench/evidence/integrate-1-host/timing-round9-raw.tsv`, three rotated repeats per arm,
20M-pair SAM input, 20 threads. The source discussion and independent-review qualification
are in `bench/PHASE2C-integrate-1.md`, Round 9: the accepted ordered-output comparison is
historical evidence, not lifecycle approval; the final candidate has a shutdown defect
under repair. This source is therefore labeled **PROVISIONAL HISTORICAL**, not approval of
final production software. New accepted timing rows replace it later.

The means are independently calculated from each raw row's `user_s + sys_s` using decimal
arithmetic:

| STAR arm | mean CPU-s |
|---|---:|
| stock | 753.9133333333333 |
| advised bypass | 661.5466666666667 |
| GPU | 618.6533333333333 |

Stage ratios used by the model:

| ratio | value |
|---|---:|
| advised bypass / stock | 0.8774837072342534 |
| GPU / stock | 0.82058945767418 |
| GPU / advised bypass (incremental comparator) | 0.9351620445017735 |

## Projected whole-pipeline model

Both scenarios start from the existing `umbam CPU replaces BAM chain` baseline of
148.48666666666665 CPU-min/sample. Only the existing 64.6 CPU-min STAR bucket changes:

`baseline - 64.6 + 64.6 * stage_ratio`

| scenario | projected CPU-min/sample |
|---|---:|
| umbam CPU + STAR advised bypass | 140.57211415399942 |
| umbam CPU + STAR GPU | 136.89674563241869 |

The existing `umbam CPU+GPU` scenario remains separate at 147.0533333333333 CPU-min/sample;
it is not combined with either STAR projection. Salmon and every other existing measured or
modeled baseline bucket are unchanged. No stage percentage was applied to the pipeline
total, and the projected samples/day values are model outputs, not measured throughput.

The charts mark both whole-pipeline scenarios as dashed/hatch PROJECTED series. The model
retains the existing per-volume hardware-cost mechanics, with AWS Batch flat pricing and
the existing Spark/Studio prices, depreciation, energy price, 20-core capacity, and 32%/80%
utilization assumptions explicitly retained in the script. No current-hardware or web claim
is made.

## Verification

Generated first in a temporary directory and inspected its JSON. Then regenerated the owned
artifacts under `bench/fig/`. The model test passed with the Miniconda interpreter:

`MPLCONFIGDIR=/private/tmp/uni-rnaseq-mpl python3 -m unittest scripts/tests_cost_curve.py -v`

Result: 1 test passed; all four generated PNGs and `cost-model.json` were nonempty.

Independent orchestrator review: `docs/review-cost-stages.md`. Raw means and
unchanged baseline buckets were verified independently; chart title clipping was
fixed and the generator/test rerun successfully (one test, 0.813s, no skips).
Accepted only with the provisional/projected qualifications above.
