# P2C-PIPELINE-RUN — six Tier 2A samples, measured CPU-min/sample (re-gated 2026-09-08)

Owner: orchestrator (Spark, serial under the lock). **Gate: P2C-STAR-PRODUCTION-GATE green
(cmp-identical under nf-core argv, gpu_consumed>0 or GPU off — see below).**

## Re-gating

Old rule: "only if INTEGRATE-1 ≥ 8%" (GPU). Superseded: the advised-bypass path alone is
−11.9% CPU-s (ablated, same binary, `bench/PHASE2C-integrate-1.md` round 7b) and is the
robust result; the GPU adds −5.6% on top with +6% wall. The pipeline number has been gated
on the wrong variable since round 7. This run uses **integrated STAR, THP advice on, GPU
off** (`STAR_INTEGRATE_THP=1`, no `--gpu`, eviction as the cache-policy card left it) plus
umbam CPU, so the measured number is the one that could ship on Batch if P2C-THP-X86-PORT
holds. A second pass with GPU on is optional and separate.

## Protocol

- nf-core/rnaseq at the pinned revision used for `runs/tier1-full`, `-profile docker`,
  with the STAR container replaced by a local image built from the frozen integrated source
  root (the P2C-STAR-PRODUCTION-GATE accepted root) and the BAM-chain modules replaced by
  umbam per the existing Tier-1 harness (`scripts/tier1_validate.sh` — read how it swaps
  them). Everything else stock.
- Six Tier 2A samples from `data/samples/MANIFEST.tsv` (ERR188081, ERR188152, ERR188217,
  ERR188339, ERR188354, ERR204897 — confirm from the manifest, do not trust this list).
- One sample at a time (saturated box: `--max_cpus 20`, the trace's `%cpu` × realtime is
  the CPU-min). Trace to `bench/evidence/pipeline-run/<sample>/trace.txt`.
- Per sample: `cmp` STAR outputs vs a stock-STAR run of the same sample under the same argv
  (this doubles STAR cost; do it for two of six and state so), umbam outputs vs its Tier-1
  goldens (existing harness).

## Deliverable

`bench/PHASE2E-pipeline-run.md`: per-sample CPU-min by stage, mean ± range across six; the
row `umbam CPU + STAR advised (MEASURED)` replaces `(PROJECTED)` in `bench/fig/cost-model.json`
via `scripts/cost_curve.py`; wall reported separately. Comparator named on every ratio.
