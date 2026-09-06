#!/usr/bin/env python3
"""Cost-curve model + figures for the uni-rnaseq memo.

Every number is from a measured run (bench/), except the ones marked ASSUMED, which are
parameters on the command line. Run: python3 scripts/cost_curve.py  ->  bench/fig/*.png
"""
import argparse, json, os
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

ap = argparse.ArgumentParser()
ap.add_argument("--batch-per-sample", type=float, default=12.0, help="ASSUMED: AWS Batch $/sample")
ap.add_argument("--spark-price", type=float, default=4000.0, help="ASSUMED: DGX Spark $")
ap.add_argument("--studio-price", type=float, default=9000.0, help="ASSUMED: M5 Ultra 256 GB $")
ap.add_argument("--life-months", type=float, default=36.0, help="ASSUMED: depreciation")
ap.add_argument("--kwh", type=float, default=0.30, help="ASSUMED: $/kWh")
ap.add_argument("--out", default="bench/fig")
A = ap.parse_args()
os.makedirs(A.out, exist_ok=True)

# ---------------------------------------------------------------------------------------
# MEASURED. Per-sample CPU-minutes, 78M-pair Geuvadis LCL, 20-core GB10.
# nf-core/rnaseq 3.26.0 stock: bench/trace-spark-tier2a.txt (6 samples, sum/6).
# 225 cpu-min/sample total; 101 task-wall-min/sample. Grouped:
NFCORE = {
    "STAR align":            64.6,
    "salmon quant":          51.3,
    "Qualimap":              22.6,
    "Picard markdup":        12.9,
    "samtools sort/stats":   22.7,   # sort 8.0 + sort_qualimap 9.5 + stats/flagstat/index 5.2
    "dupRadar":               7.7,
    "RSeQC (x7)":            17.2,   # 4.7+3.4+3.0+2.9+2.4+0.7+0.1
    "bedtools/featureCounts":  4.8,
    "trim/fastqc/fq":        19.1,   # trimgalore 6.6 + fastqc 4.4 + fq_lint 3.2 + subsample 1.4 + stringtie 4.4 (kept) - ...
}
# The BAM chain that umbam replaces (everything after STAR that reads the BAM, except salmon):
UMBAM_REPLACES = ["Qualimap", "Picard markdup", "samtools sort/stats", "dupRadar", "RSeQC (x7)", "bedtools/featureCounts"]
NFCORE_BAMCHAIN_CPU = sum(NFCORE[k] for k in UMBAM_REPLACES)          # 87.9 cpu-min
NFCORE_BAMCHAIN_WALL = 21.3+10.7+1.8+1.5+2.0+0.8+0.4+7.4+4.7+3.4+3.0+2.8+2.4+0.6+0.0+2.5+1.3  # 66.6 task-wall-min

# umbam chain --qc on the same sample, same box (/tmp/cost-timing.log, 2026-09-06):
UMBAM = {  # wall min, cpu min (user+sys), peak RSS GB
    "CPU (20 thr)":  (167.3/60, (764.3+44.9)/60, 42.9),
    "CPU + GPU":     (104.2/60, (688.0+35.2)/60, 40.0),
}
# nf-core wall on the Spark, 6 samples overlapped: 208 min -> 34.7 min/sample (bench/PHASE2-tier2a-throughput.md)
NFCORE_WALL_PER_SAMPLE_OVERLAPPED = 34.7
NFCORE_WALL_SERIAL = 105.0
# Power: measured ~150 W under nf-core load; GB10 GPU adds ~40 W when busy. ASSUMED idle 40 W.
WATTS_BUSY = 150.0

# ---------------------------------------------------------------------------------------
# Capacity model. A box is CPU-bound: samples/day = 24*60*cores*util / cpu-min-per-sample.
# That's the honest ceiling; it assumes a scheduler that keeps 20 cores busy (Nextflow's
# default overlap only reached 32% on Tier 2A). We show both the measured 32% and a
# realistic 80%.
CORES = 20
def samples_per_day(cpu_min_per_sample, util):
    return 24*60*CORES*util / cpu_min_per_sample

scenarios = {
    # name: cpu-min/sample
    "nf-core stock (measured)":      sum(NFCORE.values()),                                 # 225
    "umbam CPU replaces BAM chain":  sum(NFCORE.values()) - NFCORE_BAMCHAIN_CPU + UMBAM["CPU (20 thr)"][1],
    "umbam CPU+GPU":                 sum(NFCORE.values()) - NFCORE_BAMCHAIN_CPU + UMBAM["CPU + GPU"][1],
}
for k, v in scenarios.items():
    print(f"{k:34s} {v:6.1f} cpu-min/sample  -> {samples_per_day(v,0.8):5.1f}/day @80%  {samples_per_day(v,0.32):5.1f}/day @32%")

# Measured nf-core throughput as the anchor: 41.5/day at 32% util. Scale scenarios by cpu-min ratio.
MEASURED_NFCORE_PER_DAY = 41.5

def per_sample_cost(price, life_months, monthly_volume, cpu_min, watts=WATTS_BUSY, cap_per_day=None):
    """$/sample at a given monthly volume for one box. Above capacity, add boxes."""
    cap_month = (cap_per_day or samples_per_day(cpu_min, 0.8)) * 30
    boxes = np.ceil(monthly_volume / cap_month)
    capex_month = boxes * price / life_months
    energy = (cpu_min/60/CORES) * watts/1000 * A.kwh  # kWh per sample at full-core equiv
    return capex_month / monthly_volume + energy, boxes

# ---------------------------------------------------------------------------------------
# Figure 1: the cost curve. Below a box's capacity every scenario is the same line
# (capex/volume); throughput only matters at the step where you need another box.
vol = np.logspace(0, 4.3, 600)  # 1 .. 20,000 samples/month
UTIL = 0.8
fig, (ax, ax2) = plt.subplots(2, 1, figsize=(9.5, 7.5), gridspec_kw={"height_ratios": [3, 1.3]}, sharex=True)
ax.axhline(A.batch_per_sample, color="#c0392b", lw=2.5, label=f"AWS Batch, ${A.batch_per_sample:.0f}/sample (flat, ASSUMED)")
styles = {
    "nf-core stock (measured)": ("#7f8c8d", "--"),
    "umbam CPU replaces BAM chain": ("#2980b9", "-"),
    "umbam CPU+GPU": ("#27ae60", "-"),
}
caps = {}
for name, cpu in scenarios.items():
    c, ls = styles[name]
    cap = samples_per_day(cpu, UTIL) * 30
    caps[name] = cap
    cost, boxes = per_sample_cost(A.spark_price, A.life_months, vol, cpu)
    ax.plot(vol, cost, color=c, ls=ls, lw=2, label=f"DGX Spark — {name}: one box ≤ {cap:,.0f}/mo")
    ax2.step(vol, boxes, where="post", color=c, ls=ls, lw=2)
    ax.axvline(cap, color=c, ls=":", lw=1, alpha=.7)
cost_ms, boxes_ms = per_sample_cost(A.studio_price, A.life_months, vol, scenarios["umbam CPU replaces BAM chain"]*20/32)
ax.plot(vol, cost_ms, color="#8e44ad", ls=":", lw=2, label=f"Mac Studio M5 Ultra (ASSUMED ${A.studio_price/1000:.0f}k, 32 cores, untested)")
ax2.step(vol, boxes_ms, where="post", color="#8e44ad", ls=":", lw=2)
ax.set_xscale("log"); ax.set_yscale("log")
ax.set_ylabel("$ / sample (hardware over %.0f mo + energy)" % A.life_months)
ax.set_title("Cost curve: $/sample vs monthly volume. Below capacity every box is capex ÷ volume;\nthroughput decides when you need the next box", fontsize=11)
ax.grid(True, which="both", alpha=.25)
ax.set_ylim(0.05, 300); ax.set_xlim(1, 20000)
ax.legend(fontsize=8, loc="upper right")
be = A.spark_price / A.life_months / A.batch_per_sample
ax.annotate(f"break-even {be:.0f} samples/mo\n(${A.spark_price:,.0f} over {A.life_months:.0f} mo)", (be, A.batch_per_sample),
            xytext=(-10, 30), textcoords="offset points", fontsize=8.5, ha="right",
            arrowprops=dict(arrowstyle="->", color="#555"))
ax2.set_ylabel("boxes needed\n(%.0f%% util)" % (UTIL*100)); ax2.set_ylim(0, 8); ax2.grid(True, which="both", alpha=.25)
ax2.set_xlabel("samples / month (78M-pair, full nf-core star_salmon)")
fig.tight_layout(); fig.savefig(f"{A.out}/cost-curve.png", dpi=150)

# ---------------------------------------------------------------------------------------
# Figure 2: where the CPU-minutes go — nf-core stock vs after umbam (stacked, per sample)
fig, ax = plt.subplots(figsize=(9.5, 5.2))
order = ["STAR align", "salmon quant", "trim/fastqc/fq", "Qualimap", "samtools sort/stats", "RSeQC (x7)", "Picard markdup", "dupRadar", "bedtools/featureCounts"]
colors = {"STAR align":"#34495e","salmon quant":"#5d6d7e","trim/fastqc/fq":"#85929e",
          "Qualimap":"#e74c3c","samtools sort/stats":"#e67e22","RSeQC (x7)":"#f1c40f","Picard markdup":"#c0392b","dupRadar":"#d35400","bedtools/featureCounts":"#f39c12"}
bars = {
    "nf-core stock": {k: NFCORE[k] for k in order},
    "umbam CPU":     {**{k: NFCORE[k] for k in order if k not in UMBAM_REPLACES}, "umbam chain (all of the above, one pass)": UMBAM["CPU (20 thr)"][1]},
    "umbam CPU+GPU": {**{k: NFCORE[k] for k in order if k not in UMBAM_REPLACES}, "umbam chain (all of the above, one pass)": UMBAM["CPU + GPU"][1]},
}
colors["umbam chain (all of the above, one pass)"] = "#27ae60"
ys = list(bars.keys())
for yi, y in enumerate(ys):
    left = 0
    for k, v in bars[y].items():
        ax.barh(yi, v, left=left, color=colors[k], edgecolor="white", label=k if yi == 0 or k.startswith("umbam") and yi == 1 else None)
        if v > 8: ax.text(left + v/2, yi, f"{v:.0f}", ha="center", va="center", fontsize=8, color="white")
        left += v
    ax.text(left + 3, yi, f"{left:.0f} cpu-min", va="center", fontsize=9)
ax.set_yticks(range(len(ys))); ax.set_yticklabels(ys); ax.invert_yaxis()
ax.set_xlabel("CPU-minutes per 78M-pair sample (DGX Spark, 20 cores)")
ax.set_title("Where the CPU-minutes go: the BAM chain (warm colours) collapses to one resident pass (green)", fontsize=10.5)
ax.legend(fontsize=7.5, ncol=3, loc="upper center", bbox_to_anchor=(0.5, -0.22)); ax.grid(axis="x", alpha=.25)
ax.set_xlim(0, 260)
fig.tight_layout(); fig.savefig(f"{A.out}/cpu-minutes.png", dpi=150)

# ---------------------------------------------------------------------------------------
# Figure 3: umbam chain wall per stage, CPU vs GPU — what the GPU actually bought
stages = [("decode", 10.7, 11.6), ("sort+index", 3.8, 3.8), ("BGZF writes ×2", 32.4, 32.8), ("markdup", 9.3, 2.5),
          ("featureCounts+genomecov", 11.4, 11.7), ("seq/pos DupRate", 62.8, 5.1), ("dupRadar", 7.4, 7.5), ("Qualimap", 10.9, 11.4),
          ("RSeQC rest + BED", 6.1, 5.7)]
fig, ax = plt.subplots(figsize=(9.5, 4.8))
x = np.arange(len(stages)); w = 0.38
ax.bar(x - w/2, [s[1] for s in stages], w, color="#2980b9", label="CPU, 20 threads")
ax.bar(x + w/2, [s[2] for s in stages], w, color="#27ae60", label="CPU + GB10 GPU (--gpu)")
for i, s in enumerate(stages):
    if s[1] / max(s[2], .1) > 1.8:
        ax.text(i + w/2, s[2] + 1.5, f"{s[1]/s[2]:.1f}×", ha="center", va="bottom", fontsize=9, color="#1e8449", fontweight="bold")
ax.set_xticks(x); ax.set_xticklabels([s[0] for s in stages], rotation=25, ha="right", fontsize=9)
ax.set_ylabel("seconds (76M records)")
ax.set_title(f"umbam chain --qc per stage, one 78M-pair sample: wall {UMBAM['CPU (20 thr)'][0]*60:.0f} s → {UMBAM['CPU + GPU'][0]*60:.0f} s\n"
             "GPU wins the sort-shaped stages; BGZF stays on the CPU (nvCOMP was 7.5× slower at equal ratio)", fontsize=10)
ax.legend(); ax.grid(axis="y", alpha=.25)
fig.tight_layout(); fig.savefig(f"{A.out}/umbam-stages.png", dpi=150)

# ---------------------------------------------------------------------------------------
# Figure 3b: per-sample WALL for the BAM chain — nf-core's 17 serial tasks vs one umbam run.
# This is what the GPU actually buys: latency, not cores.
fig, ax = plt.subplots(figsize=(9, 3.2))
wall = [("nf-core BAM chain\n(17 tasks, task-wall sum)", NFCORE_BAMCHAIN_WALL, "#e67e22"),
        ("umbam --qc, CPU 20 thr", UMBAM["CPU (20 thr)"][0], "#2980b9"),
        ("umbam --qc, CPU + GPU", UMBAM["CPU + GPU"][0], "#27ae60")]
for i, (n, v, c) in enumerate(wall):
    ax.barh(i, v, color=c)
    ax.text(v + 0.8, i, f"{v:.1f} min" + (f"  ({wall[0][1]/v:.0f}×)" if i else ""), va="center", fontsize=10)
ax.set_yticks(range(3)); ax.set_yticklabels([w[0] for w in wall], fontsize=9); ax.invert_yaxis()
ax.set_xlabel("minutes per 78M-pair sample, same box"); ax.set_xlim(0, 80); ax.grid(axis="x", alpha=.25)
ax.set_title("Post-alignment BAM chain, wall time: every output byte-identical to the nf-core tools", fontsize=10.5)
fig.tight_layout(); fig.savefig(f"{A.out}/bamchain-wall.png", dpi=150)

# ---------------------------------------------------------------------------------------
# Figure 4: samples/day and break-even table
rows = []
for name, cpu in scenarios.items():
    for util in (0.32, 0.8):
        spd = samples_per_day(cpu, util)
        be_samples = A.spark_price / (A.batch_per_sample - 0.05)
        rows.append((name, util, spd, be_samples / spd))
print("\nscenario, util, samples/day, days to break even vs Batch (Spark $%.0f):" % A.spark_price)
for r in rows:
    print(f"  {r[0]:34s} {r[1]:.0%}  {r[2]:6.1f}/day  {r[3]:5.1f} days")
json.dump({"scenarios_cpu_min": scenarios, "umbam": UMBAM, "nfcore": NFCORE, "rows": rows,
           "assumed": vars(A)}, open(f"{A.out}/cost-model.json", "w"), indent=1)
print("wrote", A.out)
