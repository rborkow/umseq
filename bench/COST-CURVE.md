# Cost curve — first full draft (2026-09-06)

Model: `scripts/cost_curve.py` → `bench/fig/*.png`, `bench/fig/cost-model.json`. Every input
is a measured number from `bench/` except those marked **ASSUMED** (CLI flags on the script).

## Inputs

**Measured, DGX Spark GB10 (20 cores, 121 GB), 78M-pair Geuvadis LCL:**

| | CPU-min / sample | wall / sample |
|---|---|---|
| nf-core/rnaseq 3.26 stock, 39 processes (Tier 2A trace, 6 samples) | **223** | 101 task-wall-min; 34.7 min amortized with Nextflow's default overlap (32% core utilization) |
| of which the post-alignment BAM chain (Qualimap, Picard, samtools ×5, dupRadar, RSeQC ×7, bedtools, featureCounts) | 88 | 66.6 |
| of which STAR + salmon | 116 | 21 |
| `umbam chain --qc` replacing the BAM chain, CPU 20 thr | **13.5** | **2.8 min** |
| `umbam chain --qc --gpu` | 12.1 | **1.7 min** |

Every `umbam` output is byte-identical to the tool it replaces at full depth (dupRadar's
multimapper columns ±1 on 17/78,900 genes, Qualimap's gene/ambiguous split — documented
residuals). Power ~150 W under load.

**ASSUMED:** Batch $12/sample flat; Spark $4,000; Mac Studio M5 Ultra $9,000 (not yet
measured — dotted line only); 36-month depreciation; $0.30/kWh; 80% achievable utilization
with a scheduler that keeps 20 cores busy (Nextflow's stock overlap reached 32%).

## Results

| scenario | CPU-min/sample | samples/day @32% (measured regime) | @80% | days to pay off $4k vs Batch |
|---|---|---|---|---|
| nf-core stock | 223 | 41 (measured 41.5 ✔) | 103 | 8 |
| + umbam CPU | 148 | 62 | 155 | 5 |
| + umbam CPU+GPU | 147 | 63 | 157 | 5 |

The model reproduces the measured Tier 2A throughput (41.3 predicted vs 41.5 observed), so
the other rows are extrapolations of a calibrated model, not guesses.

**Break-even is ~9 samples/month** ($4,000 / 36 mo / $12). Below one box's capacity the $/sample
curve is just capex ÷ volume and is identical for every software scenario; the software only
moves the *step* — the volume at which a second box is needed (~3,100/mo stock, ~4,700/mo
with umbam at 80%). Energy is $0.01–0.02/sample and never matters.

## What the figures say

1. `cost-curve.png` — Batch is a flat $12; one Spark falls below it at ~9 samples/month and
   reaches ~$0.30/sample at 400/mo. The team's actual monthly volume is the only number
   missing from this chart. **The cost-curve result is a workstation result**, not a UM one.
2. `cpu-minutes.png` — nf-core spends 223 cpu-min/sample; 88 of those are the BAM chain and
   umbam does that work in 13.5. What remains (STAR 65, salmon 51, trimming/QC 19) is 90% of
   the new total and is untouched. **The next target is alignment, not more BAM stages.**
3. `bamchain-wall.png` — 66.6 → 2.8 → 1.7 min per sample for the same outputs. This is the
   engineering result: one resident pass over a 6.7 GB BAM beats 17 serial single-threaded
   tools by 24×; the GPU takes it to 38×.
4. `umbam-stages.png` — where the GPU helped (sort-shaped stages, 3.7–12×) and where it
   didn't (BGZF compression, nvCOMP 7.5× slower at equal ratio; decode/dupRadar/Qualimap
   unchanged). Post-GPU, BGZF is 31% of the chain and is the ceiling on further wall gains.

## Honest framing for the memo

- **Fixed asset: yes, decisively, and it doesn't need the GPU.** At any volume above ~10/mo,
  a $4k box beats $12/sample; at 100/mo it is ~$1.20/sample; the box pays for itself in a
  week of use.
- **What the Rust chain changed:** 223 → 148 cpu-min (1.5× more samples per box-day), and per-
  sample latency of the BAM chain 67 → 3 min. That moves the second-box step, not the
  break-even.
- **What the GPU changed on the cost curve: almost nothing** (148 → 147 cpu-min). It bought
  latency (2.8 → 1.7 min), correctness-for-free (no staging, byte-identical), and a proven
  pattern. The pipeline is CPU-bound in STAR/salmon; until the GPU touches alignment, the
  UM-specific thesis has no cost number attached.
- **Unified memory specifically:** on this box it removed the staging step the old PoC died
  on, and kernels read the resident table at ~145 GB/s. That's necessary for the alignment
  experiment, not sufficient; it's why STAR is the next card and why the Mac Studio is
  interesting rather than justified.
- **Not counted:** storage, egress, orchestration labour, the second box for redundancy,
  Batch's own fixed costs. Batch's $12 is the team's optimized number and is treated as flat.

## Open inputs

1. Team's monthly sample volume (sets where on the curve we are — and whether one box or two).
2. Whether the Batch $12 includes storage/egress (moves the flat line).
3. Mac Studio row: everything is ASSUMED until the box arrives.
