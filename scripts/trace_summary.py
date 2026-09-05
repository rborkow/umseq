#!/usr/bin/env python3
"""trace_summary.py — summarize a Nextflow trace.txt: wall share per process, CPU efficiency, peak RSS."""
import csv, re, sys

def secs(x):
    t = 0
    for v, u in re.findall(r"([\d.]+)\s*(ms|h|m|s)", x or ""):
        t += {"h": 3600, "m": 60, "s": 1, "ms": 0.001}[u] * float(v)
    return t

def pct(x):
    try: return float((x or "").rstrip("%"))
    except ValueError: return 0.0

def gb(x):
    m = re.match(r"([\d.]+)\s*(GB|MB|KB|B)", x or "0 B")
    return float(m[1]) * {"GB": 1, "MB": 1e-3, "KB": 1e-6, "B": 1e-9}[m[2]] if m else 0.0

rows = list(csv.DictReader(open(sys.argv[1]), delimiter="\t"))
n = int(sys.argv[2]) if len(sys.argv) > 2 else 15
tot = sum(secs(r["realtime"]) for r in rows)
cpu_min = sum(secs(r["realtime"]) * pct(r["%cpu"]) / 100 for r in rows) / 60
print(f"{len(rows)} tasks | sum realtime {tot/60:.1f} min | {cpu_min:.0f} cpu-min | peak RSS max {max(gb(r['peak_rss']) for r in rows):.1f} GB")
print(f"{'realtime':>9} {'%sum':>6} {'cpus':>4} {'%cpu':>7} {'eff':>5} {'rss GB':>7}  process")
for r in sorted(rows, key=lambda r: -secs(r["realtime"]))[:n]:
    rt = secs(r["realtime"]); cpus = int(r["cpus"]) if r["cpus"].isdigit() else 1; pct = pct(r["%cpu"])
    print(f"{rt:8.0f}s {100*rt/tot:5.1f}% {cpus:>4} {pct:6.0f}% {pct/cpus/100:4.0%} {gb(r['peak_rss']):7.1f}  {r['name'].split(':')[-1][:58]}")
