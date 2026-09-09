#!/usr/bin/env python3
from __future__ import annotations
import argparse, csv, statistics
from collections import defaultdict
def main():
    p = argparse.ArgumentParser(); p.add_argument("tsv"); path = p.parse_args().tsv
    rows = list(csv.DictReader(open(path), delimiter="\t")); by = defaultdict(list)
    for r in rows:
        r["cpu"] = float(r["user_s"]) + float(r["sys_s"])
        r["ahp"] = float(r["anonhuge_pages_kib"]) if r["anonhuge_pages_kib"] != "NA" else float("nan")
        by[r["arm"]].append(r)
    print("arm\tn\tmean_cpu_s\trange_cpu_s\tmean_AnonHugePages_KiB")
    for arm in ("stock", "patched-off", "patched-on"):
        vals=by[arm]; cpu=[r["cpu"] for r in vals]; ahp=[r["ahp"] for r in vals if r["ahp"] == r["ahp"]]
        print(f"{arm}\t{len(vals)}\t{statistics.mean(cpu):.3f}\t{min(cpu):.3f}-{max(cpu):.3f}\t" + (f"{statistics.mean(ahp):.1f}" if ahp else "NA"))
    bad = [r for r in rows if r["cmp_sam"] != "0" or r["cmp_sj"] != "0"]
    if bad:
        print(f"cmp: DIFFERED ({len(bad)} rows); ratios withheld"); return 1
    print("cmp: Aligned.out.sam identical; SJ.out.tab identical")
    stock=statistics.mean(r["cpu"] for r in by["stock"]); off=statistics.mean(r["cpu"] for r in by["patched-off"]); on=statistics.mean(r["cpu"] for r in by["patched-on"])
    print(f"patched-on vs stock: {(on/stock-1)*100:.2f}% (vs stock)")
    print(f"patched-on vs patched-off: {(on/off-1)*100:.2f}% (madvise alone, same binary)")
if __name__ == "__main__": raise SystemExit(main())

